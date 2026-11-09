//! The computation gate of `chem.aromaticity_convert` and `chem.cip`. A computation takes one of
//! the slots that `Application::with_compute_concurrency` sets, and waits for a free one within
//! its own time limit. `Application::cancel_computations` stops the running and the waiting
//! computations and refuses new ones.
//!
//! Every test sets the number of slots itself. A computation that must hold its slot is latched
//! at its first checkpoint, which the gate reaches only after it has taken the slot. A stopped
//! holder keeps its slot until the test releases it, so each outcome is decided by the latch, a
//! cancel or a time limit, never by how long anything takes. `WAIT` bounds every wait. A waiter
//! that the test releases later runs under the default limit of five seconds, the one wall-clock
//! bound a test leaves to a request.

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use chemifree_application::{Application, Interrupt, Interrupted};
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

/// Waits until `interrupt` has been asked. A request asks only once it has passed its running
/// guard and is waiting for a slot, so this is the point at which a test may act. Fails if
/// that does not happen within `WAIT`.
fn until_asked(interrupt: &CancelAt) {
    let deadline = Instant::now() + WAIT;
    while interrupt.calls.load(Ordering::SeqCst) == 0 {
        assert!(
            Instant::now() < deadline,
            "the waiter must ask its interrupt within WAIT"
        );
        thread::yield_now();
    }
}

#[test]
fn a_conversion_waits_for_a_slot_until_its_own_deadline_and_is_answered_time_limit() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory).with_compute_concurrency(1);
    let holding = benzene_document(&app, "holding");
    let waiting = benzene_document(&app, "waiting");
    let before = serde_json::to_value(app.document(&waiting).unwrap()).unwrap();
    let (gate, reached, release) = gate();
    thread::scope(|scope| {
        // Owned by this closure, so a failed assertion below releases the computation.
        let release = release;
        let holder =
            scope.spawn(|| app.execute_with(&convert_request(&holding, "holder", 1), &gate));
        reached
            .recv_timeout(WAIT)
            .expect("the holder started computing");
        // The holder keeps the only slot until it is released, so this conversion can end only
        // at its own limit.
        let reply = finishes("the conversion that waits for a slot", &release, || {
            call(
                &app,
                "waiter",
                "chem.aromaticity_convert",
                json!({"document_id": waiting, "expected_revision": 1, "to": "aromatic", "time_limit_ms": 1000}),
            )
        });
        assert_eq!(reply["outcome"], "error", "{reply}");
        assert_eq!(reply["diagnostics"][0]["code"], "time_limit", "{reply}");
        assert_eq!(
            reply["diagnostics"][0]["details"],
            json!({"limit_ms": 1000}),
            "only the limit is reported, never an elapsed time: {reply}"
        );
        assert_eq!(
            serde_json::to_value(app.document(&waiting).unwrap()).unwrap(),
            before,
            "the waiting document is unchanged: {reply}"
        );
        release.now();
        let holder_reply = serde_json::to_value(holder.join().unwrap()).unwrap();
        assert_eq!(holder_reply["outcome"], "ok", "{holder_reply}");
    });
}

#[test]
fn chem_cip_waits_at_the_same_gate() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory).with_compute_concurrency(1);
    let holding = benzene_document(&app, "holding");
    let (gate, reached, release) = gate();
    thread::scope(|scope| {
        // Owned by this closure, so a failed assertion below releases the computation.
        let release = release;
        let holder =
            scope.spawn(|| app.execute_with(&convert_request(&holding, "holder", 1), &gate));
        reached
            .recv_timeout(WAIT)
            .expect("the holder started computing");
        let reply = finishes("chem.cip", &release, || {
            call(
                &app,
                "cip",
                "chem.cip",
                json!({"document_id": holding, "time_limit_ms": 1000}),
            )
        });
        assert_eq!(reply["outcome"], "error", "{reply}");
        assert_eq!(reply["model_id"], "chemifree-cip-1", "{reply}");
        assert_eq!(reply["diagnostics"][0]["code"], "time_limit", "{reply}");
        assert_eq!(
            reply["diagnostics"][0]["details"],
            json!({"limit_ms": 1000}),
            "{reply}"
        );
        release.now();
        let holder_reply = serde_json::to_value(holder.join().unwrap()).unwrap();
        assert_eq!(holder_reply["outcome"], "ok", "{holder_reply}");
    });
}

#[test]
fn a_waiter_whose_host_cancels_leaves_and_the_next_request_runs() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory).with_compute_concurrency(1);
    let holding = benzene_document(&app, "holding");
    let waiting = benzene_document(&app, "waiting");
    let before = serde_json::to_value(app.document(&waiting).unwrap()).unwrap();
    let (gate, reached, release) = gate();
    thread::scope(|scope| {
        // Owned by this closure, so a failed assertion below releases the computation.
        let release = release;
        let holder =
            scope.spawn(|| app.execute_with(&convert_request(&holding, "holder", 1), &gate));
        reached
            .recv_timeout(WAIT)
            .expect("the holder started computing");
        // The host cancels the waiter at its first question, which the waiter asks only once
        // it is waiting for the slot that the holder keeps.
        let host = CancelAt::new(1, Interrupted::Cancelled);
        let cancelled = finishes("the waiter whose host cancels", &release, || {
            serde_json::to_value(app.execute_with(&convert_request(&waiting, "waiter", 1), &host))
                .unwrap()
        });
        assert_eq!(cancelled["outcome"], "error", "{cancelled}");
        assert_eq!(
            cancelled["diagnostics"][0]["code"], "cancelled",
            "{cancelled}"
        );
        assert_eq!(
            serde_json::to_value(app.document(&waiting).unwrap()).unwrap(),
            before,
            "the cancelled waiter changed nothing: {cancelled}"
        );
        release.now();
        let holder_reply = serde_json::to_value(holder.join().unwrap()).unwrap();
        assert_eq!(holder_reply["outcome"], "ok", "{holder_reply}");
        // The cancelled waiter took no slot, so the slot the holder gave back serves the next
        // request.
        let next = finishes("the next conversion", &release, || {
            call(
                &app,
                "next",
                "chem.aromaticity_convert",
                json!({"document_id": waiting, "expected_revision": 1, "to": "aromatic"}),
            )
        });
        assert_eq!(next["outcome"], "ok", "{next}");
        assert_eq!(next["document_revision"], 2, "{next}");
    });
}

#[test]
fn the_same_request_id_while_waiting_is_request_in_progress() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory).with_compute_concurrency(1);
    let holding = benzene_document(&app, "holding");
    let waiting = benzene_document(&app, "waiting");
    let (gate, reached, release) = gate();
    // Never reaches `usize::MAX` questions, so it only counts them and never cancels.
    let counted = CancelAt::new(usize::MAX, Interrupted::Cancelled);
    thread::scope(|scope| {
        // Owned by this closure, so a failed assertion below releases the computation.
        let release = release;
        let holder =
            scope.spawn(|| app.execute_with(&convert_request(&holding, "holder", 1), &gate));
        reached
            .recv_timeout(WAIT)
            .expect("the holder started computing");
        let waiter =
            scope.spawn(|| app.execute_with(&convert_request(&waiting, "convert", 1), &counted));
        until_asked(&counted);
        let again = finishes("the same request again", &release, || {
            serde_json::to_value(app.execute(&convert_request(&waiting, "convert", 1))).unwrap()
        });
        assert_eq!(again["outcome"], "error", "{again}");
        assert_eq!(
            again["diagnostics"][0]["code"], "request_in_progress",
            "{again}"
        );
        release.now();
        let holder_reply = serde_json::to_value(holder.join().unwrap()).unwrap();
        assert_eq!(holder_reply["outcome"], "ok", "{holder_reply}");
        let waiter_reply = serde_json::to_value(waiter.join().unwrap()).unwrap();
        assert_eq!(
            waiter_reply["outcome"], "ok",
            "the waiting request runs once the slot is given back: {waiter_reply}"
        );
    });
}

#[test]
fn cancel_computations_stops_a_running_conversion_and_changes_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory).with_compute_concurrency(1);
    let id = benzene_document(&app, "stopping");
    let before = serde_json::to_value(app.document(&id).unwrap()).unwrap();
    let (gate, reached, release) = gate();
    let reply = thread::scope(|scope| {
        // Owned by this closure, so a failed assertion below releases the computation.
        let release = release;
        let converted =
            scope.spawn(|| app.execute_with(&convert_request(&id, "convert", 1), &gate));
        reached
            .recv_timeout(WAIT)
            .expect("the conversion started computing");
        app.cancel_computations();
        release.now();
        serde_json::to_value(converted.join().unwrap()).unwrap()
    });
    assert_eq!(reply["outcome"], "error", "{reply}");
    assert_eq!(reply["diagnostics"][0]["code"], "cancelled", "{reply}");
    assert!(reply.get("result").is_none_or(Value::is_null), "{reply}");
    let after = serde_json::to_value(app.document(&id).unwrap()).unwrap();
    assert_eq!(after["document"]["revision"], 1, "{after}");
    assert_eq!(
        after["session"]["undo_depth"], 1,
        "only the drawing: {after}"
    );
    assert_eq!(
        after, before,
        "revision, content, history depths, dirtiness and draft state are all as before"
    );
}

#[test]
fn cancel_computations_wakes_waiters_and_refuses_new_computations_but_not_other_operations() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory).with_compute_concurrency(1);
    let holding = benzene_document(&app, "holding");
    let waiting = benzene_document(&app, "waiting");
    let before = serde_json::to_value(app.document(&waiting).unwrap()).unwrap();
    let other = app
        .create_document(None, Some("other"))
        .unwrap()
        .document
        .id;
    let (gate, reached, release) = gate();
    // Never reaches `usize::MAX` questions, so it only counts them and never cancels.
    let counted = CancelAt::new(usize::MAX, Interrupted::Cancelled);
    thread::scope(|scope| {
        // Owned by this closure, so a failed assertion below releases the computation.
        let release = release;
        let holder =
            scope.spawn(|| app.execute_with(&convert_request(&holding, "holder", 1), &gate));
        reached
            .recv_timeout(WAIT)
            .expect("the holder started computing");
        let waiter =
            scope.spawn(|| app.execute_with(&convert_request(&waiting, "waiter", 1), &counted));
        until_asked(&counted);
        app.cancel_computations();
        // The waiter leaves at once, though the holder still keeps the only slot.
        let woken = finishes("the waiter", &release, move || {
            serde_json::to_value(waiter.join().unwrap()).unwrap()
        });
        assert_eq!(woken["outcome"], "error", "{woken}");
        assert_eq!(woken["diagnostics"][0]["code"], "cancelled", "{woken}");
        // The running conversion stops at its next checkpoint.
        release.now();
        let stopped = finishes("the running conversion", &release, move || {
            serde_json::to_value(holder.join().unwrap()).unwrap()
        });
        assert_eq!(stopped["diagnostics"][0]["code"], "cancelled", "{stopped}");
        // Its slot is free now, and new computations are still refused.
        let converted = finishes("a new conversion", &release, || {
            call(
                &app,
                "new-convert",
                "chem.aromaticity_convert",
                json!({"document_id": waiting, "expected_revision": 1, "to": "aromatic"}),
            )
        });
        assert_eq!(converted["outcome"], "error", "{converted}");
        assert_eq!(
            converted["diagnostics"][0]["code"], "cancelled",
            "{converted}"
        );
        let cip = finishes("a new chem.cip", &release, || {
            call(&app, "new-cip", "chem.cip", json!({"document_id": waiting}))
        });
        assert_eq!(cip["outcome"], "error", "{cip}");
        assert_eq!(cip["diagnostics"][0]["code"], "cancelled", "{cip}");
        assert_eq!(cip["model_id"], "chemifree-cip-1", "{cip}");
        // Other operations go on: a read and an edit of another document.
        let snapshot = call(
            &app,
            "snapshot",
            "document.snapshot",
            json!({"document_id": other}),
        );
        assert_eq!(snapshot["outcome"], "ok", "{snapshot}");
        let edited = add_atom(&app, &other, "edit-other", 0);
        assert_eq!(edited["outcome"], "ok", "{edited}");
        assert_eq!(edited["document_revision"], 1, "{edited}");
        assert_eq!(
            serde_json::to_value(app.document(&waiting).unwrap()).unwrap(),
            before,
            "the cancelled and the refused requests changed nothing"
        );
    });
}

#[test]
fn a_concurrency_of_zero_still_runs_requests() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory).with_compute_concurrency(0);
    let (_gate, _reached, release) = gate();
    // Each conversion gives its slot back, so the second one runs as well.
    for title in ["first", "second"] {
        let id = benzene_document(&app, title);
        let reply = finishes("a conversion", &release, || {
            call(
                &app,
                title,
                "chem.aromaticity_convert",
                json!({"document_id": id, "expected_revision": 1, "to": "aromatic"}),
            )
        });
        assert_eq!(reply["outcome"], "ok", "{title}: {reply}");
        assert_eq!(reply["document_revision"], 2, "{title}: {reply}");
    }
}
