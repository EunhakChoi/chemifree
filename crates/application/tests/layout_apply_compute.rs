//! `layout.structure` in mode `"apply"` under the compute gate. An edit made while the layout is
//! computed refuses the apply. A stop changes nothing and is not recorded for replay. An apply
//! waits for a compute slot, where a preview does not. A host cancel stops a running apply and
//! refuses new ones. The same request ID while an apply runs is `request_in_progress`.
//!
//! A computation that a test holds is latched at its first question, which the gate reaches only
//! after it has taken the slot, and it is released before the test ends. Each outcome is therefore
//! decided by the test, never by how long anything takes.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use chemifree_application::{Application, Interrupt, Interrupted};
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};

type App = Application<LocalCapabilities, FileDocumentRepository>;

/// Long enough for any request that is not blocked; short enough to end a failing run.
const WAIT: Duration = Duration::from_secs(30);

/// The model that every reply of `layout.structure` names.
const MODEL_ID: &str = "chemifree-chain-layout-1";

/// The questions that a successful apply of butane asks, counted in a passing run: one once the
/// compute slot is taken, the layout's own checkpoints, and the last one, asked under the session
/// lock just before the edit is committed. A change to the layout's checkpoints changes this
/// count; then count again and check that the last question is still the one under the lock.
const APPLY_QUESTIONS: usize = 11;

fn new_app(directory: &tempfile::TempDir) -> App {
    Application::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    )
}

fn request(id: &str, operation: &str, parameters: Value) -> Vec<u8> {
    json!({"contract": 1, "request_id": id, "profile": "preserve-semantics", "operation": operation, "parameters": parameters})
        .to_string()
        .into_bytes()
}

fn call(app: &App, id: &str, operation: &str, parameters: Value) -> Value {
    serde_json::to_value(app.execute(&request(id, operation, parameters))).unwrap()
}

/// The JSON of document `id` as a reader sees it.
fn document(app: &App, id: &str) -> Value {
    serde_json::to_value(app.document(id).unwrap()).unwrap()
}

/// Carbons a1 to a4 in a row 30 pt apart, bonded in turn, so that bonds b1 to b3 join them.
fn butane() -> Value {
    json!([
        {"op": "add_atom", "element": "C", "position": {"x": 0, "y": 0}},
        {"op": "add_atom", "element": "C", "position": {"x": 30, "y": 0}, "bond_to": {"atom": "$0", "order": "single"}},
        {"op": "add_atom", "element": "C", "position": {"x": 60, "y": 0}, "bond_to": {"atom": "$1", "order": "single"}},
        {"op": "add_atom", "element": "C", "position": {"x": 90, "y": 0}, "bond_to": {"atom": "$2", "order": "single"}}
    ])
}

/// A document titled `title` that holds butane at revision 1.
fn butane_document(app: &App, title: &str) -> String {
    let id = app.create_document(None, Some(title)).unwrap().document.id;
    let drawn = call(
        app,
        &format!("draw-{title}"),
        "document.transaction",
        json!({"document_id": id, "expected_revision": 0, "commands": butane()}),
    );
    assert_eq!(drawn["outcome"], "ok", "{drawn}");
    id
}

/// Adds a loose nitrogen, far from the butane, to document `id` at `expected_revision`.
fn add_nitrogen(app: &App, id: &str, request_id: &str, expected_revision: u64) -> Value {
    call(
        app,
        request_id,
        "document.transaction",
        json!({"document_id": id, "expected_revision": expected_revision, "commands": [
            {"op": "add_atom", "element": "N", "position": {"x": 500, "y": 0}}
        ]}),
    )
}

/// The parameters of a preview of `atoms` in document `id`.
fn preview_parameters(id: &str, atoms: &[&str]) -> Value {
    json!({"document_id": id, "mode": "preview", "selection": {"atoms": atoms}})
}

/// The parameters of an apply of `atoms` in document `id` at `expected_revision`, with no limit.
fn apply_parameters(id: &str, expected_revision: u64, atoms: &[&str]) -> Value {
    json!({"document_id": id, "mode": "apply", "expected_revision": expected_revision, "selection": {"atoms": atoms}})
}

/// The bytes of an apply request with `request_id`.
fn apply_request(request_id: &str, id: &str, expected_revision: u64, atoms: &[&str]) -> Vec<u8> {
    request(
        request_id,
        "layout.structure",
        apply_parameters(id, expected_revision, atoms),
    )
}

/// `parameters` with a time limit of `limit_ms` added.
fn with_limit(mut parameters: Value, limit_ms: u64) -> Value {
    parameters["time_limit_ms"] = json!(limit_ms);
    parameters
}

/// Every atom of `snapshot` by atom ID, with its position as bit patterns. Bit patterns compare
/// exactly, and tell `-0.0` from `0.0`.
fn atom_bits(snapshot: &Value) -> BTreeMap<String, (u64, u64)> {
    snapshot["document"]["atoms"]
        .as_array()
        .unwrap()
        .iter()
        .map(|atom| {
            let position = &atom["position"];
            (
                atom["id"].as_str().unwrap().to_owned(),
                (
                    position["x"].as_f64().unwrap().to_bits(),
                    position["y"].as_f64().unwrap().to_bits(),
                ),
            )
        })
        .collect()
}

/// Asserts that `reply` is an apply that was stopped with `code`: an error with that one
/// diagnostic, no result, no revision and the layout's model.
fn assert_stopped(reply: &Value, code: &str) {
    assert_eq!(reply["outcome"], "error", "{reply}");
    let diagnostics = reply["diagnostics"].as_array().unwrap();
    assert_eq!(diagnostics.len(), 1, "{reply}");
    assert_eq!(diagnostics[0]["code"], code, "{reply}");
    assert!(reply.get("result").is_none_or(Value::is_null), "{reply}");
    assert!(reply["document_revision"].is_null(), "{reply}");
    assert_eq!(reply["model_id"], MODEL_ID, "{reply}");
}

/// Stops the computation at its first question until released, and says so when it gets there.
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
            let _ = self.release.lock().unwrap().recv();
        }
        Ok(())
    }
}

/// Runs `work` on its own thread and fails if it does not finish: a request that waits for the
/// session lock would wait for as long as the computation is stopped. The computation is released
/// first, so that the failing run ends instead of waiting for the lock for good.
fn finishes<T: Send>(label: &str, release: &Release, work: impl FnOnce() -> T + Send) -> T {
    thread::scope(|scope| {
        let (done, result) = mpsc::channel();
        scope.spawn(move || {
            let _ = done.send(work());
        });
        result.recv_timeout(WAIT).unwrap_or_else(|_| {
            release.now();
            panic!("{label} did not finish while the layout was computing")
        })
    })
}

/// The reply to `request` when `interrupt` is asked at the computation's checkpoints.
fn reply_with(app: &App, request: &[u8], interrupt: &dyn Interrupt) -> Value {
    serde_json::to_value(app.execute_with(request, interrupt)).unwrap()
}

/// Counts the questions that an apply asks, and stops question `stop.0` (numbered from 1) with
/// reason `stop.1`. No other question is stopped.
struct Questions {
    stop: Option<(usize, Interrupted)>,
    count: AtomicUsize,
}

impl Questions {
    fn new(stop: Option<(usize, Interrupted)>) -> Self {
        Self {
            stop,
            count: AtomicUsize::new(0),
        }
    }

    fn asked(&self) -> usize {
        self.count.load(Ordering::SeqCst)
    }
}

impl Interrupt for Questions {
    fn check(&self) -> Result<(), Interrupted> {
        let number = self.count.fetch_add(1, Ordering::SeqCst) + 1;
        match self.stop {
            Some((stop_at, reason)) if stop_at == number => Err(reason),
            _ => Ok(()),
        }
    }
}

#[test]
fn an_edit_during_the_computation_refuses_the_apply_with_revision_conflict() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let id = butane_document(&app, "edited");
    let before = document(&app, &id);
    let (gate, reached, release) = gate();
    let reply = thread::scope(|scope| {
        // Owned by this closure, so a failed assertion below releases the computation.
        let release = release;
        let applying =
            scope.spawn(|| app.execute_with(&apply_request("apply", &id, 1, &["a2"]), &gate));
        reached
            .recv_timeout(WAIT)
            .expect("the apply started computing");
        // The session lock is free while the layout is computed, so an edit at revision 1 goes
        // through and makes the apply stale.
        let edited = finishes("an edit to the same document", &release, || {
            add_nitrogen(&app, &id, "edit", 1)
        });
        assert_eq!(edited["outcome"], "ok", "{edited}");
        assert_eq!(edited["document_revision"], 2, "{edited}");
        release.now();
        serde_json::to_value(applying.join().unwrap()).unwrap()
    });
    assert_eq!(reply["outcome"], "error", "{reply}");
    assert_eq!(
        reply["diagnostics"][0]["code"], "revision_conflict",
        "{reply}"
    );
    assert_eq!(
        reply["diagnostics"][0]["details"]["current_revision"], 2,
        "{reply}"
    );
    assert_eq!(reply["document_revision"], 2, "{reply}");
    assert_eq!(reply["model_id"], MODEL_ID, "{reply}");
    let after = document(&app, &id);
    assert_eq!(after["document"]["revision"], 2, "{after}");
    assert_eq!(
        after["session"]["undo_depth"], 2,
        "only the drawing and the edit: {after}"
    );
    assert_eq!(
        after["document"]["atoms"].as_array().unwrap().len(),
        5,
        "the edit survives: {after}"
    );
    let (before_bits, after_bits) = (atom_bits(&before), atom_bits(&after));
    assert_eq!(before_bits.len(), 4, "{before}");
    for atom in ["a1", "a2", "a3", "a4"] {
        assert_eq!(
            after_bits.get(atom),
            before_bits.get(atom),
            "{atom} moved though the apply was refused: {after}"
        );
    }
}

#[test]
fn a_zero_limit_changes_nothing_and_a_retry_computes() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let id = butane_document(&app, "zero");
    let before = document(&app, &id);
    let stopped = call(
        &app,
        "zero",
        "layout.structure",
        with_limit(apply_parameters(&id, 1, &["a2"]), 0),
    );
    assert_stopped(&stopped, "time_limit");
    assert_eq!(
        stopped["diagnostics"][0]["details"],
        json!({"limit_ms": 0}),
        "only the limit is reported, never an elapsed time: {stopped}"
    );
    assert_eq!(
        document(&app, &id),
        before,
        "the stopped apply changed the document"
    );
    // A stop is not recorded, so the same request ID without a limit computes and applies.
    let retried = call(
        &app,
        "zero",
        "layout.structure",
        apply_parameters(&id, 1, &["a2"]),
    );
    assert_eq!(retried["outcome"], "ok", "{retried}");
    assert_eq!(retried["document_revision"], 2, "{retried}");
    assert_eq!(retried["model_id"], MODEL_ID, "{retried}");
}

#[test]
fn a_stop_at_the_last_question_under_the_lock_changes_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    // The count comes from a successful apply of the same fixture. The stop is at that same
    // question number, so it lands on the last question of an apply, the one under the lock.
    let counted = butane_document(&app, "counted");
    let counting = Questions::new(None);
    let applied = reply_with(
        &app,
        &apply_request("counted", &counted, 1, &["a2"]),
        &counting,
    );
    assert_eq!(applied["outcome"], "ok", "{applied}");
    assert_eq!(
        counting.asked(),
        APPLY_QUESTIONS,
        "a successful apply asks this many questions"
    );
    for (reason, code) in [
        (Interrupted::Cancelled, "cancelled"),
        (Interrupted::TimeLimit, "time_limit"),
    ] {
        let id = butane_document(&app, &format!("last-{code}"));
        let before = document(&app, &id);
        let stop = Questions::new(Some((APPLY_QUESTIONS, reason)));
        let reply = reply_with(&app, &apply_request("last", &id, 1, &["a2"]), &stop);
        assert_eq!(
            stop.asked(),
            APPLY_QUESTIONS,
            "{code}: the apply asked a different number of questions"
        );
        assert_stopped(&reply, code);
        let after = document(&app, &id);
        assert_eq!(after["document"]["revision"], 1, "{code}: {after}");
        assert_eq!(
            atom_bits(&after),
            atom_bits(&before),
            "{code}: a stopped apply moved an atom: {after}"
        );
        assert_eq!(after, before, "{code}: no revision, no undo entry");
        // Nothing was recorded for replay, so the same request ID computes and applies.
        let retried = call(
            &app,
            "last",
            "layout.structure",
            apply_parameters(&id, 1, &["a2"]),
        );
        assert_eq!(retried["outcome"], "ok", "{code}: {retried}");
        assert_eq!(retried["document_revision"], 2, "{code}: {retried}");
    }
}

#[test]
fn cancel_computations_stops_a_running_apply_and_refuses_new_ones() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let id = butane_document(&app, "stopping");
    let before = document(&app, &id);
    let (gate, reached, release) = gate();
    let reply = thread::scope(|scope| {
        // Owned by this closure, so a failed assertion below releases the computation.
        let release = release;
        let applying =
            scope.spawn(|| app.execute_with(&apply_request("apply", &id, 1, &["a2"]), &gate));
        reached
            .recv_timeout(WAIT)
            .expect("the apply started computing");
        app.cancel_computations();
        release.now();
        serde_json::to_value(applying.join().unwrap()).unwrap()
    });
    assert_stopped(&reply, "cancelled");
    assert_eq!(
        document(&app, &id),
        before,
        "the cancelled apply changed the document"
    );
    // The gate is closed for good, so an apply of another document is refused as well.
    let other = butane_document(&app, "refused");
    let other_before = document(&app, &other);
    let refused = call(
        &app,
        "refused",
        "layout.structure",
        apply_parameters(&other, 1, &["a2"]),
    );
    assert_stopped(&refused, "cancelled");
    assert_eq!(
        document(&app, &other),
        other_before,
        "the refused apply changed the document"
    );
}

#[test]
fn an_apply_waits_for_a_slot_and_a_preview_does_not() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory).with_compute_concurrency(1);
    let holding = butane_document(&app, "holding");
    let waiting = butane_document(&app, "waiting");
    let before = document(&app, &waiting);
    let (gate, reached, release) = gate();
    thread::scope(|scope| {
        // Owned by this closure, so a failed assertion below releases the computation.
        let release = release;
        let holder = scope.spawn(|| {
            let parameters = with_limit(apply_parameters(&holding, 1, &["a2"]), 8_000);
            app.execute_with(&request("holder", "layout.structure", parameters), &gate)
        });
        reached
            .recv_timeout(WAIT)
            .expect("the holder started computing");
        // The holder keeps the only slot until it is released, so this apply can end only at its
        // own limit.
        let waited = finishes("the apply that waits for a slot", &release, || {
            call(
                &app,
                "waiter",
                "layout.structure",
                with_limit(apply_parameters(&waiting, 1, &["a2"]), 1_000),
            )
        });
        assert_stopped(&waited, "time_limit");
        assert_eq!(
            waited["diagnostics"][0]["details"],
            json!({"limit_ms": 1_000}),
            "{waited}"
        );
        assert_eq!(
            document(&app, &waiting),
            before,
            "the waiting document changed: {waited}"
        );
        // A preview takes no slot, so it runs while the holder keeps the only one.
        let previewed = finishes("a preview while the holder computes", &release, || {
            call(
                &app,
                "preview",
                "layout.structure",
                preview_parameters(&waiting, &["a2"]),
            )
        });
        assert_eq!(previewed["outcome"], "ok", "{previewed}");
        release.now();
        let holder_reply = serde_json::to_value(holder.join().unwrap()).unwrap();
        assert_eq!(holder_reply["outcome"], "ok", "{holder_reply}");
        assert_eq!(holder_reply["document_revision"], 2, "{holder_reply}");
    });
}

#[test]
fn the_same_request_id_while_running_is_request_in_progress() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let id = butane_document(&app, "running");
    let (gate, reached, release) = gate();
    thread::scope(|scope| {
        // Owned by this closure, so a failed assertion below releases the computation.
        let release = release;
        let applying =
            scope.spawn(|| app.execute_with(&apply_request("same", &id, 1, &["a2"]), &gate));
        reached
            .recv_timeout(WAIT)
            .expect("the apply started computing");
        let again = finishes("the same request again", &release, || {
            serde_json::to_value(app.execute(&apply_request("same", &id, 1, &["a2"]))).unwrap()
        });
        assert_eq!(again["outcome"], "error", "{again}");
        assert_eq!(
            again["diagnostics"][0]["code"], "request_in_progress",
            "{again}"
        );
        assert_eq!(again["model_id"], MODEL_ID, "{again}");
        release.now();
        let done = serde_json::to_value(applying.join().unwrap()).unwrap();
        assert_eq!(done["outcome"], "ok", "{done}");
        assert_eq!(done["document_revision"], 2, "{done}");
    });
}
