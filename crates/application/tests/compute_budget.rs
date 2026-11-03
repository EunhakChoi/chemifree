//! `chem.aromaticity_convert` computes outside the session lock, under a time limit and a
//! cancel, and applies its result only to the revision it was computed for.

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use chemifree_application::{Application, ComputeLimits, Interrupt, Interrupted, ListQuery};
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};

type App = Application<LocalCapabilities, FileDocumentRepository>;

/// Long enough for any request that is not blocked; short enough to end a failing run.
const WAIT: Duration = Duration::from_secs(30);

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

fn add_atom(app: &App, id: &str, request_id: &str, revision: u64) -> Value {
    call(
        app,
        request_id,
        "document.transaction",
        json!({"document_id": id, "expected_revision": revision, "commands": [
            {"op":"add_atom", "element":"N", "position":{"x":500,"y":0}}
        ]}),
    )
}

fn convert_request(id: &str, request_id: &str, revision: u64) -> Vec<u8> {
    request(
        request_id,
        "chem.aromaticity_convert",
        json!({"document_id": id, "expected_revision": revision, "to": "aromatic"}),
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
            let _ = self.release.lock().unwrap().recv();
        }
        Ok(())
    }
}

/// Runs `work` on its own thread and fails if it does not finish: a request that waits for
/// the session lock would wait for as long as the computation is stopped. The computation is
/// released first, so that the failing run ends instead of waiting for the lock for good.
fn finishes<T: Send>(label: &str, release: &Release, work: impl FnOnce() -> T + Send) -> T {
    thread::scope(|scope| {
        let (done, result) = mpsc::channel();
        scope.spawn(move || {
            let _ = done.send(work());
        });
        result.recv_timeout(WAIT).unwrap_or_else(|_| {
            release.now();
            panic!("{label} did not finish while a conversion was computing")
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

#[test]
fn convert_does_not_hold_the_session_lock() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let converting = benzene_document(&app, "converting");
    let other = app
        .create_document(None, Some("other"))
        .unwrap()
        .document
        .id;
    let (gate, reached, release) = gate();
    thread::scope(|scope| {
        // Owned by this closure, so a failed assertion below releases the computation.
        let release = release;
        let converted =
            scope.spawn(|| app.execute_with(&convert_request(&converting, "convert", 1), &gate));
        reached
            .recv_timeout(WAIT)
            .expect("the conversion started computing");

        // Another document can be edited, read and listed while the conversion computes.
        let edited = finishes("an edit to another document", &release, || {
            add_atom(&app, &other, "edit-other", 0)
        });
        assert_eq!(edited["outcome"], "ok", "{edited}");
        // So can the same document.
        let edited = finishes("an edit to the same document", &release, || {
            add_atom(&app, &converting, "edit-same", 1)
        });
        assert_eq!(edited["outcome"], "ok", "{edited}");
        assert_eq!(edited["document_revision"], 2);
        finishes("reading the document", &release, || {
            app.document(&converting).unwrap()
        });
        finishes("listing documents", &release, || {
            app.list_documents(&ListQuery::default()).unwrap()
        });

        release.now();
        let _ = converted.join().unwrap();
    });
}

#[test]
fn stale_conversion_is_refused_not_applied() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let id = benzene_document(&app, "stale");
    let (gate, reached, release) = gate();
    let refused = thread::scope(|scope| {
        // Owned by this closure, so a failed assertion below releases the computation.
        let release = release;
        let converted =
            scope.spawn(|| app.execute_with(&convert_request(&id, "convert", 1), &gate));
        reached
            .recv_timeout(WAIT)
            .expect("the conversion started computing");
        let edited = finishes("an edit to the same document", &release, || {
            add_atom(&app, &id, "edit", 1)
        });
        assert_eq!(edited["outcome"], "ok", "{edited}");
        release.now();
        serde_json::to_value(converted.join().unwrap()).unwrap()
    });
    assert_eq!(refused["outcome"], "error", "{refused}");
    assert_eq!(refused["diagnostics"][0]["code"], "revision_conflict");
    assert_eq!(
        refused["diagnostics"][0]["details"]["current_revision"], 2,
        "{refused}"
    );
    let document = serde_json::to_value(app.document(&id).unwrap()).unwrap();
    assert_eq!(document["document"]["revision"], 2);
    assert_eq!(
        document["session"]["undo_depth"], 2,
        "only the drawing and the edit"
    );
    let bonds = document["document"]["bonds"].as_array().unwrap();
    assert!(
        bonds.iter().all(|bond| bond["order"] != "aromatic"),
        "the stale result must not reach the document: {bonds:?}"
    );
    // Asked again for the revision it is at, the conversion is computed and applied.
    let retried = call(
        &app,
        "convert-again",
        "chem.aromaticity_convert",
        json!({"document_id": id, "expected_revision": 2, "to": "aromatic"}),
    );
    assert_eq!(retried["outcome"], "ok", "{retried}");
    assert_eq!(retried["document_revision"], 3);
}

#[test]
fn a_conversion_computed_before_a_discarded_edit_and_reopen_is_still_stale() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let id = benzene_document(&app, "reopened");
    app.save(&id, Some("save"), 1).unwrap();
    let (gate, reached, release) = gate();
    let refused = thread::scope(|scope| {
        // Owned by this closure, so a failed assertion below releases the computation.
        let release = release;
        let converted =
            scope.spawn(|| app.execute_with(&convert_request(&id, "convert", 1), &gate));
        reached
            .recv_timeout(WAIT)
            .expect("the conversion started computing");
        let edited = finishes("an edit to the same document", &release, || {
            add_atom(&app, &id, "edit", 1)
        });
        assert_eq!(edited["outcome"], "ok", "{edited}");
        finishes("closing the document", &release, || {
            app.close(&id, Some("close"), true, Some(2)).unwrap()
        });
        let reopened = finishes("reopening the document", &release, || {
            app.document(&id).unwrap()
        });
        assert_ne!(
            reopened.document.revision, 1,
            "a reopened copy must not reuse the number the discarded edit moved past"
        );
        release.now();
        serde_json::to_value(converted.join().unwrap()).unwrap()
    });
    assert_eq!(
        refused["diagnostics"][0]["code"], "revision_conflict",
        "{refused}"
    );
    let document = serde_json::to_value(app.document(&id).unwrap()).unwrap();
    assert!(
        document["document"]["bonds"]
            .as_array()
            .unwrap()
            .iter()
            .all(|bond| bond["order"] != "aromatic"),
        "{document}"
    );
}

#[test]
fn same_request_id_while_running_is_in_progress() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let id = benzene_document(&app, "running");
    let (gate, reached, release) = gate();
    thread::scope(|scope| {
        // Owned by this closure, so a failed assertion below releases the computation.
        let release = release;
        let converted =
            scope.spawn(|| app.execute_with(&convert_request(&id, "convert", 1), &gate));
        reached
            .recv_timeout(WAIT)
            .expect("the conversion started computing");
        let again = finishes("the same request again", &release, || {
            serde_json::to_value(app.execute(&convert_request(&id, "convert", 1))).unwrap()
        });
        assert_eq!(again["outcome"], "error", "{again}");
        assert_eq!(
            again["diagnostics"][0]["code"], "request_in_progress",
            "{again}"
        );
        release.now();
        let done = serde_json::to_value(converted.join().unwrap()).unwrap();
        assert_eq!(done["outcome"], "ok", "{done}");
    });
}

#[test]
fn cancelled_conversion_changes_nothing_and_is_computed_again_when_retried() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let id = benzene_document(&app, "cancelled");
    let before = serde_json::to_value(app.document(&id).unwrap()).unwrap();
    // The entry is checkpoint 1 and the benzene system is checkpoint 2.
    let cancel = CancelAt::new(2, Interrupted::Cancelled);
    let reply =
        serde_json::to_value(app.execute_with(&convert_request(&id, "convert", 1), &cancel))
            .unwrap();
    assert_eq!(reply["outcome"], "error", "{reply}");
    assert_eq!(reply["diagnostics"][0]["code"], "cancelled", "{reply}");
    assert!(reply.get("result").is_none_or(Value::is_null), "{reply}");
    assert_eq!(
        serde_json::to_value(app.document(&id).unwrap()).unwrap(),
        before,
        "revision, content, history depths, dirtiness and draft state are all as before"
    );
    // A cancelled request was not recorded, so the same request ID computes it afresh.
    let retried = serde_json::to_value(app.execute(&convert_request(&id, "convert", 1))).unwrap();
    assert_eq!(retried["outcome"], "ok", "{retried}");
    assert_eq!(retried["document_revision"], 2);
}

#[test]
fn a_time_limit_of_zero_is_refused_the_same_way_every_time_and_changes_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let id = benzene_document(&app, "zero");
    let before = serde_json::to_value(app.document(&id).unwrap()).unwrap();
    let parameters =
        json!({"document_id": id, "expected_revision": 1, "to": "aromatic", "time_limit_ms": 0});
    for attempt in 0..3 {
        let reply = call(&app, "zero", "chem.aromaticity_convert", parameters.clone());
        assert_eq!(reply["outcome"], "error", "attempt {attempt}: {reply}");
        let diagnostic = &reply["diagnostics"][0];
        assert_eq!(diagnostic["code"], "time_limit", "{reply}");
        assert_eq!(
            diagnostic["details"],
            json!({"limit_ms": 0}),
            "only the limit is reported, never an elapsed time: {reply}"
        );
        assert_eq!(
            serde_json::to_value(app.document(&id).unwrap()).unwrap(),
            before
        );
    }
}

#[test]
fn the_server_default_applies_without_a_request_limit_and_a_request_may_only_lower_it() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory).with_compute_limits(ComputeLimits {
        default_time_ms: 0,
        max_time_ms: 6_000,
    });
    let id = benzene_document(&app, "default");
    let refused = call(
        &app,
        "default",
        "chem.aromaticity_convert",
        json!({"document_id": id, "expected_revision": 1, "to": "aromatic"}),
    );
    assert_eq!(refused["diagnostics"][0]["code"], "time_limit", "{refused}");
    assert_eq!(refused["diagnostics"][0]["details"]["limit_ms"], 0);
    // A request may ask for more than the default, up to the maximum ...
    let allowed = call(
        &app,
        "raised",
        "chem.aromaticity_convert",
        json!({"document_id": id, "expected_revision": 1, "to": "aromatic", "time_limit_ms": 6000}),
    );
    assert_eq!(allowed["outcome"], "ok", "{allowed}");
    // ... and no more than that.
    let above = call(
        &app,
        "above",
        "chem.aromaticity_convert",
        json!({"document_id": id, "expected_revision": 2, "to": "kekule", "time_limit_ms": 6001}),
    );
    assert_eq!(above["outcome"], "error", "{above}");
    assert_eq!(
        above["diagnostics"][0]["code"], "invalid_request",
        "{above}"
    );
    assert_eq!(above["diagnostics"][0]["path"], "parameters.time_limit_ms");
    assert_eq!(above["diagnostics"][0]["details"]["maximum_ms"], 6_000);
}
#[test]
fn the_same_request_id_with_another_time_limit_is_a_different_request() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let id = benzene_document(&app, "fingerprint");
    let with_limit = |limit: u64| {
        request(
            "limited",
            "chem.aromaticity_convert",
            json!({"document_id": id, "expected_revision": 1, "to": "aromatic", "time_limit_ms": limit}),
        )
    };
    let first = serde_json::to_value(app.execute(&with_limit(1_000))).unwrap();
    assert_eq!(first["outcome"], "ok", "{first}");
    let replay = serde_json::to_value(app.execute(&with_limit(1_000))).unwrap();
    assert_eq!(replay, first, "the same request replays");
    let other = serde_json::to_value(app.execute(&with_limit(2_000))).unwrap();
    assert_eq!(other["diagnostics"][0]["code"], "request_id_reused", "{other}");
    let none = call(
        &app,
        "limited",
        "chem.aromaticity_convert",
        json!({"document_id": id, "expected_revision": 1, "to": "aromatic"}),
    );
    assert_eq!(none["diagnostics"][0]["code"], "request_id_reused", "{none}");
}

#[test]
fn a_stopped_attempt_is_not_recorded_so_the_request_id_may_ask_for_another_limit() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let id = benzene_document(&app, "retry-limit");
    let ask = |limit: u64| {
        call(
            &app,
            "retry",
            "chem.aromaticity_convert",
            json!({"document_id": id, "expected_revision": 1, "to": "aromatic", "time_limit_ms": limit}),
        )
    };
    assert_eq!(ask(0)["diagnostics"][0]["code"], "time_limit");
    let retried = ask(1_000);
    assert_eq!(retried["outcome"], "ok", "{retried}");
}
#[test]
fn configured_limits_are_capped_so_a_limit_never_means_no_deadline() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory).with_compute_limits(ComputeLimits {
        default_time_ms: u64::MAX,
        max_time_ms: u64::MAX,
    });
    let id = benzene_document(&app, "capped");
    let hour = 3_600_000;
    assert_eq!(ComputeLimits::CAP_MS, hour);
    let above = call(
        &app,
        "above",
        "chem.aromaticity_convert",
        json!({"document_id": id, "expected_revision": 1, "to": "aromatic", "time_limit_ms": u64::MAX}),
    );
    assert_eq!(above["diagnostics"][0]["code"], "invalid_request", "{above}");
    assert_eq!(above["diagnostics"][0]["details"]["maximum_ms"], hour);
    // The default is capped as well: a request that sets no limit still runs, and with a real
    // deadline.
    let ok = call(
        &app,
        "no-limit",
        "chem.aromaticity_convert",
        json!({"document_id": id, "expected_revision": 1, "to": "aromatic"}),
    );
    assert_eq!(ok["outcome"], "ok", "{ok}");
}
