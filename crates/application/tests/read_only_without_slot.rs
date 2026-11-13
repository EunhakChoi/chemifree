//! The read-only operations that run under a time limit take no compute slot: `chem.aromaticity`,
//! `chem.identity` (recorded and chemical), `chem.substructure`, `chem.properties`, `chem.stereo`
//! and SMILES export (`document.export` with `format: "smiles"`, in both styles). Unlike
//! `chem.aromaticity_convert` and `chem.cip`, they never wait for a slot of
//! `Application::with_compute_concurrency`, even when every slot is taken. Each runs under its
//! request's time limit and the host's cancel, and `Application::cancel_computations` refuses it.
//!
//! Every test sets one slot and holds it with a conversion that is latched at its first
//! checkpoint. It reaches that checkpoint only after it has taken the slot, so it keeps the only
//! slot until the test releases it. Each outcome is decided by the latch, a cancel or the closed
//! gate, never by how long anything takes. `WAIT` bounds every wait.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use chemifree_application::{Application, Interrupt, Interrupted};
use chemifree_providers::{FileDocumentRepository, LocalCapabilities, OpenSmilesCodec};
use serde_json::{Value, json};

type App = Application<LocalCapabilities, FileDocumentRepository>;

/// Long enough for any request that is not blocked; short enough to end a failing run.
const WAIT: Duration = Duration::from_secs(30);

/// The largest limit a request may set, in milliseconds. A holder takes it, so that its own limit
/// cannot run out while a test keeps it latched.
const HOLDER_LIMIT_MS: u64 = 8_000;

fn new_app(directory: &tempfile::TempDir) -> App {
    Application::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    )
    .with_smiles_codec(OpenSmilesCodec)
}

fn request(id: &str, operation: &str, parameters: Value) -> Vec<u8> {
    json!({"contract": 1, "request_id": id, "profile": "preserve-semantics", "operation": operation, "parameters": parameters})
        .to_string()
        .into_bytes()
}

fn call(app: &App, id: &str, operation: &str, parameters: Value) -> Value {
    serde_json::to_value(app.execute(&request(id, operation, parameters))).unwrap()
}

fn call_with(
    app: &App,
    id: &str,
    operation: &str,
    parameters: Value,
    cancel: &dyn Interrupt,
) -> Value {
    serde_json::to_value(app.execute_with(&request(id, operation, parameters), cancel)).unwrap()
}

fn document(app: &App, id: &str) -> Value {
    serde_json::to_value(app.document(id).unwrap()).unwrap()
}

fn kekule_benzene() -> Vec<Value> {
    let mut commands: Vec<Value> = (0..6)
        .map(|i| json!({"op":"add_atom", "element":"C", "position":{"x":i * 20,"y":0}}))
        .collect();
    for i in 0..6 {
        let order = if i % 2 == 0 { "double" } else { "single" };
        commands.push(json!({"op":"add_bond", "begin":format!("${i}"), "end":format!("${}",(i+1)%6), "order":order}));
    }
    commands
}

/// A document holding a Kekulé benzene at revision 1.
fn benzene_document(app: &App, title: &str) -> String {
    let id = app.create_document(None, Some(title)).unwrap().document.id;
    let drawn = call(
        app,
        &format!("draw-{title}"),
        "document.transaction",
        json!({"document_id": id, "expected_revision": 0, "commands": kekule_benzene()}),
    );
    assert_eq!(drawn["outcome"], "ok", "{drawn}");
    id
}

/// A conversion to aromatic of `id` at `revision`, with the largest limit a request may set.
fn holder_request(id: &str, request_id: &str, revision: u64) -> Vec<u8> {
    request(
        request_id,
        "chem.aromaticity_convert",
        json!({"document_id": id, "expected_revision": revision, "to": "aromatic", "time_limit_ms": HOLDER_LIMIT_MS}),
    )
}

/// Stops the computation at its first checkpoint until released, saying so when it gets there.
struct Gate {
    reached: Sender<()>,
    release: Mutex<Receiver<()>>,
    first: AtomicUsize,
}

/// Lets the stopped computation go on when released or dropped, also when the test fails.
struct Release(Mutex<Option<Sender<()>>>);

impl Release {
    fn now(&self) {
        self.0.lock().unwrap().take();
    }
}

fn gate() -> (Gate, Receiver<()>, Release) {
    let (reached, reached_rx) = mpsc::channel();
    let (release_tx, release) = mpsc::channel();
    (
        Gate {
            reached,
            release: Mutex::new(release),
            first: AtomicUsize::new(0),
        },
        reached_rx,
        Release(Mutex::new(Some(release_tx))),
    )
}

impl Interrupt for Gate {
    fn check(&self) -> Result<(), Interrupted> {
        if self.first.fetch_add(1, Ordering::SeqCst) == 0 {
            self.reached.send(()).unwrap();
            // The sender is dropped to release; nothing is ever sent.
            self.release.lock().unwrap().recv().ok();
        }
        Ok(())
    }
}

/// Runs `work` on its own thread and fails if it does not finish within `WAIT`. A stopped holder
/// is released first, so that a failing run ends instead of keeping the slot for good.
fn finishes<T: Send>(label: &str, release: &Release, work: impl FnOnce() -> T + Send) -> T {
    thread::scope(|scope| {
        let (done, result) = mpsc::channel();
        scope.spawn(move || {
            // The receiver is gone only when the test has already failed.
            done.send(work()).ok();
        });
        result.recv_timeout(WAIT).unwrap_or_else(|_| {
            release.now();
            panic!("{label} did not finish within the wait bound")
        })
    })
}

/// Counts checkpoints and stops, for `reason`, at the `cancel_at`-th.
struct CancelAt {
    cancel_at: usize,
    reason: Interrupted,
    calls: AtomicUsize,
}

impl CancelAt {
    fn new(cancel_at: usize, reason: Interrupted) -> Self {
        Self {
            cancel_at,
            reason,
            calls: AtomicUsize::new(0),
        }
    }
}

impl Interrupt for CancelAt {
    fn check(&self) -> Result<(), Interrupted> {
        if self.calls.fetch_add(1, Ordering::SeqCst) + 1 == self.cancel_at {
            Err(self.reason)
        } else {
            Ok(())
        }
    }
}

/// The documents of a test. `benzene` and `empty` are the subjects a request is about, and
/// `other` is the second document of the operations that take one.
struct Documents {
    benzene: String,
    empty: String,
    other: String,
}

impl Documents {
    fn new(app: &App) -> Self {
        Self {
            benzene: benzene_document(app, "benzene"),
            empty: app
                .create_document(None, Some("empty"))
                .unwrap()
                .document
                .id,
            other: benzene_document(app, "other"),
        }
    }

    /// The subjects, each with the name that a failure message gives it.
    fn subjects(&self) -> [(&'static str, &str); 2] {
        [
            ("benzene", self.benzene.as_str()),
            ("empty", self.empty.as_str()),
        ]
    }
}

/// One operation of the table: the model its replies name, the outcome of its plain reply for the
/// drawn benzene subject, and the parameters of its request for a subject document and an other
/// document. None of them sets a time limit.
struct Row {
    operation: &'static str,
    model_id: &'static str,
    outcome: &'static str,
    parameters: fn(&str, &str) -> Value,
}

/// The five inspections, with the identity operation in both of its modes, and SMILES export in
/// both of its styles. A drawing's export is `partial`: atom positions have no SMILES form.
fn rows() -> Vec<Row> {
    vec![
        Row {
            operation: "chem.aromaticity",
            model_id: "chemifree-aromaticity-2",
            outcome: "ok",
            parameters: |subject, _| json!({"document_id": subject}),
        },
        Row {
            operation: "chem.identity",
            model_id: "chemifree-recorded-graph-1",
            outcome: "ok",
            parameters: |subject, _| json!({"document_id": subject, "mode": "recorded"}),
        },
        Row {
            operation: "chem.identity",
            model_id: "chemifree-chemical-graph-2",
            outcome: "ok",
            parameters: |subject, other| json!({"document_id": subject, "mode": "chemical", "other_document_id": other}),
        },
        Row {
            operation: "chem.substructure",
            model_id: "chemifree-substructure-1",
            outcome: "ok",
            parameters: |subject, other| json!({"query_document_id": subject, "target_document_id": other}),
        },
        Row {
            operation: "chem.properties",
            model_id: "chemifree-properties-1",
            outcome: "ok",
            parameters: |subject, _| json!({"document_id": subject, "scope": "molecule", "property": "formula"}),
        },
        Row {
            operation: "chem.stereo",
            model_id: "chemifree-stereo-1",
            outcome: "ok",
            parameters: |subject, _| json!({"document_id": subject}),
        },
        Row {
            operation: "document.export",
            model_id: "chemifree-smiles-1",
            outcome: "partial",
            parameters: |subject, _| json!({"format": "smiles", "document_id": subject, "style": "recorded"}),
        },
        Row {
            operation: "document.export",
            model_id: "chemifree-smiles-canonical-1",
            outcome: "partial",
            parameters: |subject, _| json!({"format": "smiles", "document_id": subject, "style": "canonical"}),
        },
    ]
}

/// Request IDs that no two calls of a test share, so that no call answers from another's record.
#[derive(Default)]
struct RequestIds(usize);

impl RequestIds {
    fn fresh(&mut self) -> String {
        self.0 += 1;
        format!("request-{}", self.0)
    }
}

/// The reply fields that a request gives the same way whether or not the only slot is held.
fn observed(reply: &Value) -> Value {
    json!({
        "outcome": reply["outcome"],
        "document_revision": reply["document_revision"],
        "result": reply["result"],
        "diagnostics": reply["diagnostics"],
        "loss_report": reply["loss_report"],
        "model_id": reply["model_id"],
    })
}

/// Asserts that `reply` is the stop of `row`'s request with `code`: an error with no result, the
/// analyzed `revision`, the operation's model and exactly one diagnostic.
fn assert_stopped(reply: &Value, row: &Row, code: &str, revision: u64, label: &str) {
    assert_eq!(reply["outcome"], "error", "{label}: {reply}");
    assert!(reply["result"].is_null(), "{label}: {reply}");
    assert_eq!(reply["document_revision"], revision, "{label}: {reply}");
    assert_eq!(reply["model_id"], row.model_id, "{label}: {reply}");
    let diagnostics = reply["diagnostics"].as_array().unwrap();
    assert_eq!(diagnostics.len(), 1, "{label}: {reply}");
    assert_eq!(diagnostics[0]["code"], code, "{label}: {reply}");
}

/// Asserts that every row, on every subject, is answered `cancelled` with no result and no
/// details, and that the request changes nothing. `phase` names the moment in a failure's label.
fn assert_every_row_is_cancelled(
    app: &App,
    ids: &mut RequestIds,
    documents: &Documents,
    release: &Release,
    phase: &str,
) {
    for row in rows() {
        for (name, subject) in documents.subjects() {
            let label = format!("{} on {name} {phase}", row.operation);
            let before = document(app, subject);
            let revision = before["document"]["revision"].as_u64().unwrap();
            let id = ids.fresh();
            let parameters = (row.parameters)(subject, &documents.other);
            let reply = finishes(&label, release, || {
                call(app, &id, row.operation, parameters)
            });
            assert_stopped(&reply, &row, "cancelled", revision, &label);
            assert_eq!(
                reply["diagnostics"][0]["details"],
                json!({}),
                "{label}: {reply}"
            );
            assert_eq!(
                document(app, subject),
                before,
                "{label}: the refused request changed the document: {reply}"
            );
        }
    }
}

/// Asks every row, on every subject, with a host that cancels at its first question. Each request
/// must be stopped with `cancelled` and no details, and must change nothing. `phase` names the
/// moment in a failure's label.
fn assert_host_cancels_every_row(
    app: &App,
    ids: &mut RequestIds,
    documents: &Documents,
    release: &Release,
    phase: &str,
) {
    for row in rows() {
        for (name, subject) in documents.subjects() {
            let label = format!("{} on {name} {phase}, host cancel", row.operation);
            let before = document(app, subject);
            let revision = before["document"]["revision"].as_u64().unwrap();
            let id = ids.fresh();
            let parameters = (row.parameters)(subject, &documents.other);
            let host = CancelAt::new(1, Interrupted::Cancelled);
            let reply = finishes(&label, release, || {
                call_with(app, &id, row.operation, parameters, &host)
            });
            assert_stopped(&reply, &row, "cancelled", revision, &label);
            assert_eq!(
                reply["diagnostics"][0]["details"],
                json!({}),
                "{label}: {reply}"
            );
            assert_eq!(
                document(app, subject),
                before,
                "{label}: the cancelled request changed the document: {reply}"
            );
        }
    }
}

#[test]
fn each_operation_runs_while_the_only_slot_is_held() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory).with_compute_concurrency(1);
    let documents = Documents::new(&app);
    let mut ids = RequestIds::default();
    // The reply of each request on the idle app, taken before anything holds the slot.
    let mut idle = BTreeMap::new();
    for row in rows() {
        for (name, subject) in documents.subjects() {
            let parameters = (row.parameters)(subject, &documents.other);
            let reply = call(&app, &ids.fresh(), row.operation, parameters);
            idle.insert((row.model_id, name), observed(&reply));
        }
    }
    let holding = benzene_document(&app, "holding");
    let holder_id = ids.fresh();
    let (gate, reached, release) = gate();
    thread::scope(|scope| {
        // Owned by this closure, so a failed assertion below releases the computation.
        let release = release;
        let holder =
            scope.spawn(|| app.execute_with(&holder_request(&holding, &holder_id, 1), &gate));
        reached
            .recv_timeout(WAIT)
            .expect("the holder started computing");
        // The holder keeps the only slot until it is released. Each request still completes, with
        // no limit and no wait, and gives the reply that it gives on the idle app. The empty
        // subject is not asserted to be ok: the substructure row refuses an empty query on the
        // idle app too, so its reply is compared with the idle reply only.
        for row in rows() {
            for (name, subject) in documents.subjects() {
                let label = format!("{} on {name} while the only slot is held", row.operation);
                let id = ids.fresh();
                let parameters = (row.parameters)(subject, &documents.other);
                let reply = finishes(&label, &release, || {
                    call(&app, &id, row.operation, parameters)
                });
                if name == "benzene" {
                    assert_eq!(reply["outcome"], row.outcome, "{label}: {reply}");
                    assert_eq!(reply["model_id"], row.model_id, "{label}: {reply}");
                }
                let key = (row.model_id, name);
                assert_eq!(observed(&reply), idle[&key], "{label}: {reply}");
            }
        }
        release.now();
        let holder_reply = serde_json::to_value(holder.join().unwrap()).unwrap();
        assert_eq!(holder_reply["outcome"], "ok", "holder: {holder_reply}");
    });
}

#[test]
fn a_host_cancel_stops_each_operation_while_the_slot_is_held_and_while_it_is_free() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory).with_compute_concurrency(1);
    let documents = Documents::new(&app);
    let mut ids = RequestIds::default();
    let holding = benzene_document(&app, "holding");
    let holder_id = ids.fresh();
    let (gate, reached, release) = gate();
    thread::scope(|scope| {
        // Owned by this closure, so a failed assertion below releases the computation.
        let release = release;
        let holder =
            scope.spawn(|| app.execute_with(&holder_request(&holding, &holder_id, 1), &gate));
        reached
            .recv_timeout(WAIT)
            .expect("the holder started computing");
        assert_host_cancels_every_row(
            &app,
            &mut ids,
            &documents,
            &release,
            "while the only slot is held",
        );
        release.now();
        let holder_reply = serde_json::to_value(holder.join().unwrap()).unwrap();
        assert_eq!(holder_reply["outcome"], "ok", "holder: {holder_reply}");
        assert_host_cancels_every_row(
            &app,
            &mut ids,
            &documents,
            &release,
            "after the slot is given back",
        );
    });
}

#[test]
fn cancel_computations_refuses_every_operation_while_the_slot_is_held_and_after_it_is_free() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory).with_compute_concurrency(1);
    let documents = Documents::new(&app);
    let mut ids = RequestIds::default();
    let holding = benzene_document(&app, "holding");
    let holding_before = document(&app, &holding);
    let holder_id = ids.fresh();
    let (gate, reached, release) = gate();
    thread::scope(|scope| {
        // Owned by this closure, so a failed assertion below releases the computation.
        let release = release;
        let holder =
            scope.spawn(|| app.execute_with(&holder_request(&holding, &holder_id, 1), &gate));
        reached
            .recv_timeout(WAIT)
            .expect("the holder started computing");
        app.cancel_computations();
        // The holder still keeps the only slot. No read-only request takes one, so the closed
        // gate refuses each of them at once.
        assert_every_row_is_cancelled(
            &app,
            &mut ids,
            &documents,
            &release,
            "after cancel_computations, while the only slot is still held",
        );
        // Other operations go on: validation answers from the same documents without a slot.
        for (name, subject) in documents.subjects() {
            let id = ids.fresh();
            let validated = call(&app, &id, "chem.validate", json!({"document_id": subject}));
            assert_eq!(
                validated["outcome"], "ok",
                "chem.validate on {name} after cancel_computations: {validated}"
            );
        }
        release.now();
        let holder_reply = serde_json::to_value(holder.join().unwrap()).unwrap();
        assert_eq!(holder_reply["outcome"], "error", "{holder_reply}");
        assert_eq!(
            holder_reply["diagnostics"][0]["code"], "cancelled",
            "{holder_reply}"
        );
        assert!(holder_reply["result"].is_null(), "{holder_reply}");
        assert_eq!(
            document(&app, &holding),
            holding_before,
            "the stopped conversion changed nothing: {holder_reply}"
        );
        // The slot is free now, and the closed gate still refuses every new computation.
        assert_every_row_is_cancelled(
            &app,
            &mut ids,
            &documents,
            &release,
            "after the slot is given back",
        );
    });
}
