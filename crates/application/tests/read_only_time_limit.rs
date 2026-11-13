//! `chem.aromaticity`, `chem.identity`, `chem.substructure`, `chem.properties` and `chem.stereo`
//! run under a time limit and a cancel, as `chem.cip` does, but take no compute slot. The limit is
//! read before the document is, and `chem.stereo` reads it before its instance too. A stop replies
//! with no result, the analyzed snapshot's revision and the operation's model, and it changes
//! nothing.

use std::sync::atomic::{AtomicUsize, Ordering};

use chemifree_application::{Application, CancelFlag, Interrupt, Interrupted};
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};

type App = Application<LocalCapabilities, FileDocumentRepository>;

/// The limit of a request that sets none, in milliseconds.
const DEFAULT_LIMIT_MS: u64 = 5_000;
/// The largest limit a request may set, in milliseconds.
const MAXIMUM_LIMIT_MS: u64 = 8_000;

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

/// A document whose `chem.stereo` plan holds one stored center and one stored double bond, so that
/// the comparison asks its entry checkpoints. The center is `a1` with F `a2`, Cl `a3` and Br `a4`
/// (clockwise over them, the hydrogen implicit). The double bond is `b4` between `a5` and `a6`,
/// with `a7` on `a5` and `a8` on `a6` on opposite sides.
fn stereo_document(app: &App, title: &str) -> String {
    let id = app.create_document(None, Some(title)).unwrap().document.id;
    let commands = json!([
        {"op":"add_atom","element":"C","position":{"x":100,"y":100}},
        {"op":"add_atom","element":"F","position":{"x":100,"y":70},"bond_to":{"atom":"$0","order":"single"}},
        {"op":"add_atom","element":"Cl","position":{"x":126,"y":115},"bond_to":{"atom":"$0","order":"single"}},
        {"op":"add_atom","element":"Br","position":{"x":74,"y":115},"bond_to":{"atom":"$0","order":"single"}},
        {"op":"add_atom","element":"C","position":{"x":300,"y":100}},
        {"op":"add_atom","element":"C","position":{"x":330,"y":100},"bond_to":{"atom":"$4","order":"double"}},
        {"op":"add_atom","element":"C","position":{"x":270,"y":115},"bond_to":{"atom":"$4","order":"single"}},
        {"op":"add_atom","element":"C","position":{"x":360,"y":85},"bond_to":{"atom":"$5","order":"single"}},
        {"op":"set_atom_configuration","atom":"a1","configuration":{"kind":"tetrahedral","neighbors":["a2","a3","a4","implicit_hydrogen"],"turn":"clockwise"}},
        {"op":"set_bond_configuration","bond":"b4","configuration":{"kind":"relative","begin_neighbor":"a7","end_neighbor":"a8","relation":"opposite_sides"}}
    ]);
    let drawn = call(
        app,
        &format!("draw-{title}"),
        "document.transaction",
        json!({"document_id": id, "expected_revision": 0, "commands": commands}),
    );
    assert_eq!(drawn["outcome"], "ok", "{drawn}");
    id
}

/// Counts the questions a computation asks, and stops at the `cancel_at`-th with `reason`.
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

    fn asked(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
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

/// One operation of the table: the model its replies name, and its parameters without a time
/// limit for a subject document and an other document.
struct Row {
    operation: &'static str,
    model_id: &'static str,
    without_limit: fn(&str, &str) -> Value,
}

impl Row {
    /// The request's parameters, with `time_limit_ms` set to `limit` when one is given.
    fn parameters(&self, subject: &str, other: &str, limit: Option<Value>) -> Value {
        let mut parameters = (self.without_limit)(subject, other);
        if let Some(limit) = limit {
            parameters["time_limit_ms"] = limit;
        }
        parameters
    }
}

/// The five operations, with the identity operation in both of its modes.
fn rows() -> Vec<Row> {
    vec![
        Row {
            operation: "chem.aromaticity",
            model_id: "chemifree-aromaticity-2",
            without_limit: |subject, _| json!({"document_id": subject}),
        },
        Row {
            operation: "chem.identity",
            model_id: "chemifree-recorded-graph-1",
            without_limit: |subject, _| json!({"document_id": subject, "mode": "recorded"}),
        },
        Row {
            operation: "chem.identity",
            model_id: "chemifree-chemical-graph-2",
            without_limit: |subject, other| json!({"document_id": subject, "mode": "chemical", "other_document_id": other}),
        },
        Row {
            operation: "chem.substructure",
            model_id: "chemifree-substructure-1",
            without_limit: |subject, other| json!({"query_document_id": subject, "target_document_id": other}),
        },
        Row {
            operation: "chem.properties",
            model_id: "chemifree-properties-1",
            without_limit: |subject, _| json!({"document_id": subject, "scope": "molecule", "property": "formula"}),
        },
        Row {
            operation: "chem.stereo",
            model_id: "chemifree-stereo-1",
            without_limit: |subject, _| json!({"document_id": subject}),
        },
    ]
}

/// The `chem.stereo` row of [`rows`].
fn stereo_row() -> Row {
    rows()
        .into_iter()
        .find(|row| row.operation == "chem.stereo")
        .unwrap()
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

/// The reply fields that a limit which is not reached must leave as they are.
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

/// Asserts that `reply` is the stop of `row`'s computation with `code`: an error with no result,
/// the analyzed `revision`, the operation's model and exactly one diagnostic.
fn assert_stopped(reply: &Value, row: &Row, code: &str, revision: u64, label: &str) {
    assert_eq!(reply["outcome"], "error", "{label}: {reply}");
    assert!(reply["result"].is_null(), "{label}: {reply}");
    assert_eq!(reply["document_revision"], revision, "{label}: {reply}");
    assert_eq!(reply["model_id"], row.model_id, "{label}: {reply}");
    let diagnostics = reply["diagnostics"].as_array().unwrap();
    assert_eq!(diagnostics.len(), 1, "{label}: {reply}");
    assert_eq!(diagnostics[0]["code"], code, "{label}: {reply}");
}

#[test]
fn results_below_the_limit_are_unchanged() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let documents = Documents::new(&app);
    let mut ids = RequestIds::default();
    for row in rows() {
        for (name, subject) in documents.subjects() {
            let label = format!("{} on {name}", row.operation);
            let parameters = row.parameters(subject, &documents.other, None);
            let plain = call(&app, &ids.fresh(), row.operation, parameters.clone());
            let limited = call(
                &app,
                &ids.fresh(),
                row.operation,
                row.parameters(subject, &documents.other, Some(json!(MAXIMUM_LIMIT_MS))),
            );
            let never = call_with(
                &app,
                &ids.fresh(),
                row.operation,
                parameters,
                &CancelAt::new(usize::MAX, Interrupted::Cancelled),
            );
            assert_eq!(
                observed(&limited),
                observed(&plain),
                "{label}: a limit of {MAXIMUM_LIMIT_MS} ms changed the reply"
            );
            assert_eq!(
                observed(&never),
                observed(&plain),
                "{label}: a cancel that never comes changed the reply"
            );
            if name == "benzene" {
                assert_eq!(plain["outcome"], "ok", "{label}: {plain}");
                assert_eq!(plain["model_id"], row.model_id, "{label}: {plain}");
                assert_eq!(
                    plain["document_revision"],
                    document(&app, subject)["document"]["revision"],
                    "{label}: the ok reply names the subject's revision: {plain}"
                );
            }
        }
    }
}

#[test]
fn a_zero_limit_stops_before_the_computation_and_changes_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let documents = Documents::new(&app);
    let mut ids = RequestIds::default();
    for row in rows() {
        for (name, subject) in documents.subjects() {
            let label = format!("{} on {name} with time_limit_ms 0", row.operation);
            let before = document(&app, subject);
            let other_before = document(&app, &documents.other);
            let revision = before["document"]["revision"].as_u64().unwrap();
            let stopped = call(
                &app,
                &ids.fresh(),
                row.operation,
                row.parameters(subject, &documents.other, Some(json!(0))),
            );
            assert_stopped(&stopped, &row, "time_limit", revision, &label);
            assert_eq!(
                stopped["diagnostics"][0]["details"],
                json!({"limit_ms": 0}),
                "{label}: only the limit is reported: {stopped}"
            );
            assert_eq!(
                document(&app, subject),
                before,
                "{label}: the document changed"
            );
            assert_eq!(
                document(&app, &documents.other),
                other_before,
                "{label}: the other document changed"
            );
        }
    }
}

#[test]
fn an_unknown_document_is_refused_by_the_limit_before_it_is_read() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let documents = Documents::new(&app);
    let mut ids = RequestIds::default();
    for row in rows() {
        let label = format!(
            "{} with time_limit_ms 8001 on an unknown document",
            row.operation
        );
        let reply = call(
            &app,
            &ids.fresh(),
            row.operation,
            row.parameters("no-such-document", &documents.other, Some(json!(8001))),
        );
        assert_eq!(reply["outcome"], "error", "{label}: {reply}");
        assert_eq!(
            reply["diagnostics"].as_array().unwrap().len(),
            1,
            "{label}: {reply}"
        );
        assert_eq!(
            reply["diagnostics"][0]["code"], "invalid_request",
            "{label}: {reply}"
        );
        assert_eq!(
            reply["diagnostics"][0]["path"], "parameters.time_limit_ms",
            "{label}: {reply}"
        );
        assert_eq!(
            reply["diagnostics"][0]["details"]["maximum_ms"], MAXIMUM_LIMIT_MS,
            "{label}: {reply}"
        );
        assert_eq!(reply["model_id"], row.model_id, "{label}: {reply}");
    }
}

#[test]
fn a_cancel_wins_over_an_expired_limit() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let documents = Documents::new(&app);
    let mut ids = RequestIds::default();
    for row in rows() {
        for (name, subject) in documents.subjects() {
            let label = format!(
                "{} on {name} with time_limit_ms 0 and a cancel",
                row.operation
            );
            let before = document(&app, subject);
            let revision = before["document"]["revision"].as_u64().unwrap();
            let cancel = CancelFlag::new();
            cancel.cancel();
            let reply = call_with(
                &app,
                &ids.fresh(),
                row.operation,
                row.parameters(subject, &documents.other, Some(json!(0))),
                &cancel,
            );
            assert_stopped(&reply, &row, "cancelled", revision, &label);
            assert_eq!(
                reply["diagnostics"][0]["details"],
                json!({}),
                "{label}: {reply}"
            );
            assert_eq!(
                document(&app, subject),
                before,
                "{label}: the document changed"
            );
        }
    }
}

#[test]
fn a_stop_inside_the_computation_names_the_limit_or_the_cancel() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let documents = Documents::new(&app);
    let revision = document(&app, &documents.benzene)["document"]["revision"]
        .as_u64()
        .unwrap();
    let mut ids = RequestIds::default();
    for row in rows() {
        let label = format!("{} on benzene", row.operation);
        let parameters = || row.parameters(&documents.benzene, &documents.other, None);

        // Question 1 is the check made before any work.
        let first = CancelAt::new(1, Interrupted::TimeLimit);
        let reply = call_with(&app, &ids.fresh(), row.operation, parameters(), &first);
        let label_first = format!("{label}, stopped at question 1");
        assert_stopped(&reply, &row, "time_limit", revision, &label_first);
        assert_eq!(first.asked(), 1, "{label_first}");
        assert_eq!(
            reply["diagnostics"][0]["details"],
            json!({"limit_ms": DEFAULT_LIMIT_MS}),
            "{label_first}: {reply}"
        );

        // Question 2 is asked inside the computation, and a stop there replies the same way.
        let inside = CancelAt::new(2, Interrupted::TimeLimit);
        let reply = call_with(&app, &ids.fresh(), row.operation, parameters(), &inside);
        let label_inside = format!("{label}, stopped at question 2");
        assert_stopped(&reply, &row, "time_limit", revision, &label_inside);
        assert_eq!(inside.asked(), 2, "{label_inside}");
        assert_eq!(
            reply["diagnostics"][0]["details"],
            json!({"limit_ms": DEFAULT_LIMIT_MS}),
            "{label_inside}: {reply}"
        );

        let cancelled = CancelAt::new(2, Interrupted::Cancelled);
        let reply = call_with(&app, &ids.fresh(), row.operation, parameters(), &cancelled);
        let label_cancelled = format!("{label}, cancelled at question 2");
        assert_stopped(&reply, &row, "cancelled", revision, &label_cancelled);
        assert_eq!(
            reply["diagnostics"][0]["details"],
            json!({}),
            "{label_cancelled}: {reply}"
        );

        // Without a stop the computation asks more than once, so question 2 is inside it.
        let counting = CancelAt::new(usize::MAX, Interrupted::Cancelled);
        let reply = call_with(&app, &ids.fresh(), row.operation, parameters(), &counting);
        assert_eq!(reply["outcome"], "ok", "{label}: {reply}");
        assert!(
            counting.asked() >= 2,
            "{label}: the computation asked {} questions",
            counting.asked()
        );
    }
}

#[test]
fn an_unreadable_limit_is_refused_without_naming_a_model() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let documents = Documents::new(&app);
    let mut ids = RequestIds::default();
    for row in rows() {
        let label = format!("{} with time_limit_ms \"5\"", row.operation);
        let reply = call(
            &app,
            &ids.fresh(),
            row.operation,
            row.parameters(&documents.benzene, &documents.other, Some(json!("5"))),
        );
        assert_eq!(reply["outcome"], "error", "{label}: {reply}");
        assert_eq!(
            reply["diagnostics"].as_array().unwrap().len(),
            1,
            "{label}: {reply}"
        );
        assert_eq!(
            reply["diagnostics"][0]["code"], "invalid_request",
            "{label}: {reply}"
        );
        assert_eq!(
            reply["diagnostics"][0]["path"], "parameters",
            "{label}: {reply}"
        );
        assert!(
            reply["model_id"].is_null(),
            "{label}: unreadable parameters name no model: {reply}"
        );
    }
}

#[test]
fn a_stop_at_any_checkpoint_of_the_stereo_entries_replies_the_stop() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let stereo = stereo_document(&app, "stereo");
    let before = document(&app, &stereo);
    let revision = before["document"]["revision"].as_u64().unwrap();
    let row = stereo_row();
    let parameters = || row.parameters(&stereo, &stereo, None);
    let mut ids = RequestIds::default();

    // The questions of a comparison without a stop: the computation's own check, one before the
    // instance, one before the center, one before the double bond and one before the closing
    // findings. Those before the center and the double bond are inside the entry loops, so a
    // count of two or fewer would mean that the loops ask nothing.
    let counting = CancelAt::new(usize::MAX, Interrupted::Cancelled);
    let plain = call_with(&app, &ids.fresh(), row.operation, parameters(), &counting);
    let questions = counting.asked();
    assert_eq!(plain["outcome"], "ok", "{plain}");
    assert_eq!(
        plain["result"]["atoms"].as_array().unwrap().len(),
        1,
        "{plain}"
    );
    assert_eq!(
        plain["result"]["bonds"].as_array().unwrap().len(),
        1,
        "{plain}"
    );
    assert!(
        questions > 2,
        "the comparison asked {questions} questions; its entry checkpoints ask nothing"
    );

    // A stop at each question replies the stop of the reason it names, as the first two do.
    for stop_at in 1..=questions {
        for (reason, code) in [
            (Interrupted::TimeLimit, "time_limit"),
            (Interrupted::Cancelled, "cancelled"),
        ] {
            let label = format!(
                "chem.stereo on a stored center and double bond, {code} at question {stop_at} of {questions}"
            );
            let stopper = CancelAt::new(stop_at, reason);
            let reply = call_with(&app, &ids.fresh(), row.operation, parameters(), &stopper);
            assert_stopped(&reply, &row, code, revision, &label);
            assert_eq!(stopper.asked(), stop_at, "{label}");
            let details = match reason {
                Interrupted::TimeLimit => json!({"limit_ms": DEFAULT_LIMIT_MS}),
                Interrupted::Cancelled => json!({}),
            };
            assert_eq!(
                reply["diagnostics"][0]["details"], details,
                "{label}: {reply}"
            );
            assert_eq!(
                document(&app, &stereo),
                before,
                "{label}: the document changed"
            );
        }
    }
}

#[test]
fn stereo_refuses_the_limit_before_it_reads_the_instance() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let documents = Documents::new(&app);
    let row = stereo_row();
    let mut ids = RequestIds::default();
    for instance in [json!(7), json!("not an identifier")] {
        let label = format!("chem.stereo with instance {instance}");

        // The control: with a limit in range the instance alone is refused.
        let mut in_range = row.parameters(
            &documents.benzene,
            &documents.other,
            Some(json!(MAXIMUM_LIMIT_MS)),
        );
        in_range["instance"] = instance.clone();
        let control = call(&app, &ids.fresh(), row.operation, in_range);
        assert_eq!(
            control["diagnostics"][0]["path"], "parameters.instance",
            "{label}: {control}"
        );

        let mut above = row.parameters(&documents.benzene, &documents.other, Some(json!(9000)));
        above["instance"] = instance;
        let reply = call(&app, &ids.fresh(), row.operation, above);
        assert_eq!(reply["outcome"], "error", "{label}: {reply}");
        assert!(reply["result"].is_null(), "{label}: {reply}");
        assert!(reply["document_revision"].is_null(), "{label}: {reply}");
        assert_eq!(reply["model_id"], row.model_id, "{label}: {reply}");
        assert_eq!(
            reply["diagnostics"].as_array().unwrap().len(),
            1,
            "{label}: {reply}"
        );
        assert_eq!(
            reply["diagnostics"][0]["code"], "invalid_request",
            "{label}: {reply}"
        );
        assert_eq!(
            reply["diagnostics"][0]["path"], "parameters.time_limit_ms",
            "{label} with time_limit_ms 9000: the limit is read before the instance: {reply}"
        );
        assert_eq!(
            reply["diagnostics"][0]["details"]["maximum_ms"], MAXIMUM_LIMIT_MS,
            "{label}: {reply}"
        );
    }
}
