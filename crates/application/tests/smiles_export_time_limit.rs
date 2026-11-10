//! SMILES export, `document.export` with `format: "smiles"`, runs under a time limit and a cancel,
//! as the four read-only inspections do, but takes no compute slot. The limit is read before the
//! document is. A stop replies with no result, the document's revision, the style's model and no
//! losses, and it is decided before any loss or consent. The other export formats keep no limit.

use std::sync::atomic::{AtomicUsize, Ordering};

use chemifree_application::{Application, CancelFlag, Interrupt, Interrupted};
use chemifree_providers::interchange_codec::LocalInterchangeCodec;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities, OpenSmilesCodec, V2000Codec};
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
    .with_ctfile_codec(V2000Codec)
    .with_interchange_codec(LocalInterchangeCodec)
    .with_smiles_codec(OpenSmilesCodec)
}

fn request(request_id: &str, operation: &str, parameters: Value) -> Vec<u8> {
    json!({"contract": 1, "request_id": request_id, "profile": "preserve-semantics", "operation": operation, "parameters": parameters})
        .to_string()
        .into_bytes()
}

fn call(app: &App, request_id: &str, operation: &str, parameters: Value) -> Value {
    serde_json::to_value(app.execute(&request(request_id, operation, parameters))).unwrap()
}

fn call_with(
    app: &App,
    request_id: &str,
    operation: &str,
    parameters: Value,
    cancel: &dyn Interrupt,
) -> Value {
    serde_json::to_value(app.execute_with(&request(request_id, operation, parameters), cancel))
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

/// One `document.export` request with a fresh request ID.
fn export(app: &App, ids: &mut RequestIds, parameters: Value) -> Value {
    call(app, &ids.fresh(), "document.export", parameters)
}

/// [`export`] whose computation also asks `cancel`.
fn export_with(
    app: &App,
    ids: &mut RequestIds,
    parameters: Value,
    cancel: &dyn Interrupt,
) -> Value {
    call_with(app, &ids.fresh(), "document.export", parameters, cancel)
}

fn document(app: &App, id: &str) -> Value {
    serde_json::to_value(app.document(id).unwrap()).unwrap()
}

fn revision_of(app: &App, id: &str) -> u64 {
    document(app, id)["document"]["revision"].as_u64().unwrap()
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

/// The drawing of the conformance case `stereo-smiles-export-needs-consent`: a tetrahedral carbon
/// with a wedge on one bond, which the recorded text omits, so its export needs consent.
fn stereo_commands() -> Vec<Value> {
    vec![
        json!({"op":"add_atom","element":"C","position":{"x":100,"y":100}}),
        json!({"op":"add_atom","element":"F","position":{"x":100,"y":70},"bond_to":{"atom":"$0","order":"single"}}),
        json!({"op":"add_atom","element":"Cl","position":{"x":126,"y":115},"bond_to":{"atom":"$0","order":"single"}}),
        json!({"op":"add_atom","element":"Br","position":{"x":74,"y":115},"bond_to":{"atom":"$0","order":"single"}}),
        json!({"op":"set_bond_mark","bond":"$2","mark":{"kind":"wedge","narrow":"begin"}}),
        json!({"op":"set_atom_configuration","atom":"a1","configuration":{"kind":"tetrahedral","neighbors":["a2","a3","a4","implicit_hydrogen"],"turn":"clockwise"}}),
    ]
}

/// A document with `commands` drawn in one transaction at expected revision 0, so that it is at
/// revision 1.
fn drawn_document(app: &App, title: &str, commands: Vec<Value>) -> String {
    let id = app.create_document(None, Some(title)).unwrap().document.id;
    let drawn = call(
        app,
        &format!("draw-{title}"),
        "document.transaction",
        json!({"document_id": id, "expected_revision": 0, "commands": commands}),
    );
    assert_eq!(drawn["outcome"], "ok", "{title}: {drawn}");
    id
}

/// The documents of the tests: a Kekulé benzene at revision 1, an empty drawing at revision 0, and
/// the stereo drawing at revision 1, whose recorded and canonical exports need consent.
struct Documents {
    benzene: String,
    empty: String,
    consent: String,
}

impl Documents {
    fn new(app: &App) -> Self {
        Self {
            benzene: drawn_document(app, "benzene", kekule_benzene()),
            empty: app
                .create_document(None, Some("empty"))
                .unwrap()
                .document
                .id,
            consent: drawn_document(app, "consent", stereo_commands()),
        }
    }

    /// The documents, each with the name that a failure message gives it.
    fn subjects(&self) -> [(&'static str, &str); 3] {
        [
            ("benzene", self.benzene.as_str()),
            ("empty", self.empty.as_str()),
            ("consent", self.consent.as_str()),
        ]
    }

    /// Every document as it is now, to show that a request changed none of them.
    fn snapshots(&self, app: &App) -> Vec<Value> {
        [&self.benzene, &self.empty, &self.consent]
            .into_iter()
            .map(|id| document(app, id))
            .collect()
    }
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

/// One style of the export, with the model that its replies name.
struct Row {
    style: &'static str,
    model_id: &'static str,
}

fn rows() -> [Row; 2] {
    [
        Row {
            style: "recorded",
            model_id: "chemifree-smiles-1",
        },
        Row {
            style: "canonical",
            model_id: "chemifree-smiles-canonical-1",
        },
    ]
}

impl Row {
    /// The export parameters of `document`, with `time_limit_ms` and `accepted_losses` when given.
    fn parameters(&self, document: &str, limit: Option<Value>, accepted: Option<&str>) -> Value {
        let mut parameters =
            json!({"format": "smiles", "document_id": document, "style": self.style});
        if let Some(limit) = limit {
            parameters["time_limit_ms"] = limit;
        }
        if let Some(digest) = accepted {
            parameters["accepted_losses"] = json!(digest);
        }
        parameters
    }
}

/// The reply fields that a limit which is not reached, or a cancel that never comes, must leave
/// as they are.
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

/// Asserts that `reply` is the stop of `row`'s export with `code`: an error whose one diagnostic has
/// that code, with no result, the document's `revision`, the style's model and no losses.
fn assert_stopped(reply: &Value, row: &Row, code: &str, revision: u64, label: &str) {
    assert_eq!(reply["outcome"], "error", "{label}: {reply}");
    let diagnostics = reply["diagnostics"].as_array().unwrap();
    assert_eq!(diagnostics.len(), 1, "{label}: {reply}");
    assert_eq!(diagnostics[0]["code"], code, "{label}: {reply}");
    assert!(reply["result"].is_null(), "{label}: {reply}");
    assert_eq!(reply["document_revision"], revision, "{label}: {reply}");
    assert_eq!(reply["model_id"], row.model_id, "{label}: {reply}");
    assert_eq!(
        reply["loss_report"],
        json!([]),
        "{label}: a stop carries no losses: {reply}"
    );
}

/// The `loss_digest` of a consent refusal.
fn digest_of(refusal: &Value) -> &str {
    refusal["diagnostics"][0]["details"]["loss_digest"]
        .as_str()
        .unwrap()
}

/// The reply without its request ID, the one field that differs between two calls of a request.
fn without_request_id(reply: &Value) -> Value {
    let mut reply = reply.clone();
    reply.as_object_mut().unwrap().remove("request_id");
    reply
}

#[test]
fn results_below_the_limit_are_unchanged() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let documents = Documents::new(&app);
    let mut ids = RequestIds::default();
    for row in rows() {
        for (name, document) in documents.subjects() {
            let label = format!("smiles {} on {name}", row.style);
            let plain = export(&app, &mut ids, row.parameters(document, None, None));
            let limited = export(
                &app,
                &mut ids,
                row.parameters(document, Some(json!(MAXIMUM_LIMIT_MS)), None),
            );
            let never = export_with(
                &app,
                &mut ids,
                row.parameters(document, None, None),
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
            match name {
                "benzene" => {
                    assert_eq!(plain["outcome"], "partial", "{label}: {plain}");
                    assert_eq!(plain["document_revision"], 1, "{label}: {plain}");
                    assert_eq!(plain["model_id"], row.model_id, "{label}: {plain}");
                }
                "empty" => {
                    assert_eq!(plain["outcome"], "error", "{label}: {plain}");
                    assert!(plain["result"].is_null(), "{label}: {plain}");
                }
                _ => {
                    assert_eq!(
                        plain["diagnostics"][0]["code"], "loss_consent_required",
                        "{label}: {plain}"
                    );
                    let digest = digest_of(&plain);
                    let accepted =
                        export(&app, &mut ids, row.parameters(document, None, Some(digest)));
                    let accepted_limited = export(
                        &app,
                        &mut ids,
                        row.parameters(document, Some(json!(MAXIMUM_LIMIT_MS)), Some(digest)),
                    );
                    let accepted_never = export_with(
                        &app,
                        &mut ids,
                        row.parameters(document, None, Some(digest)),
                        &CancelAt::new(usize::MAX, Interrupted::Cancelled),
                    );
                    assert_eq!(accepted["outcome"], "partial", "{label}: {accepted}");
                    assert_eq!(
                        observed(&accepted_limited),
                        observed(&accepted),
                        "{label}: a limit changed the accepted export"
                    );
                    assert_eq!(
                        observed(&accepted_never),
                        observed(&accepted),
                        "{label}: a cancel that never comes changed the accepted export"
                    );
                }
            }
        }
    }
}

#[test]
fn a_zero_limit_stops_the_export_and_changes_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let documents = Documents::new(&app);
    let mut ids = RequestIds::default();
    for row in rows() {
        for (name, document) in documents.subjects() {
            let label = format!("smiles {} on {name} with time_limit_ms 0", row.style);
            let before = documents.snapshots(&app);
            let revision = revision_of(&app, document);
            let stopped = export(
                &app,
                &mut ids,
                row.parameters(document, Some(json!(0)), None),
            );
            assert_stopped(&stopped, &row, "time_limit", revision, &label);
            assert_eq!(
                stopped["diagnostics"][0]["details"],
                json!({"limit_ms": 0}),
                "{label}: only the limit is reported: {stopped}"
            );
            if name == "consent" {
                // The stop wins over a correct digest: the limit is read first, and the digest is
                // never looked at.
                let refusal = export(&app, &mut ids, row.parameters(document, None, None));
                let digest = digest_of(&refusal);
                let accepted = export(
                    &app,
                    &mut ids,
                    row.parameters(document, Some(json!(0)), Some(digest)),
                );
                let label_accepted = format!("{label} with the correct accepted_losses");
                assert_stopped(&accepted, &row, "time_limit", revision, &label_accepted);
                assert_eq!(
                    accepted["diagnostics"][0]["details"],
                    json!({"limit_ms": 0}),
                    "{label_accepted}: no loss_digest is reported: {accepted}"
                );
            }
            assert_eq!(
                documents.snapshots(&app),
                before,
                "{label}: a document changed"
            );
        }
    }
}

#[test]
fn an_over_maximum_limit_is_refused_before_the_document_is_read() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let mut ids = RequestIds::default();
    for row in rows() {
        let label = format!(
            "smiles {} with time_limit_ms 8001 on an unknown document",
            row.style
        );
        let reply = export(
            &app,
            &mut ids,
            row.parameters("no-such-document", Some(json!(8001)), None),
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
            reply["diagnostics"][0]["details"],
            json!({"maximum_ms": MAXIMUM_LIMIT_MS}),
            "{label}: {reply}"
        );
        assert_eq!(reply["model_id"], row.model_id, "{label}: {reply}");
        assert!(reply["document_revision"].is_null(), "{label}: {reply}");
    }
}

#[test]
fn an_unreadable_limit_is_refused_without_naming_a_model() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let documents = Documents::new(&app);
    let mut ids = RequestIds::default();
    for row in rows() {
        for limit in [json!("5"), json!(-1)] {
            let label = format!("smiles {} with time_limit_ms {limit}", row.style);
            let reply = export(
                &app,
                &mut ids,
                row.parameters(&documents.benzene, Some(limit), None),
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
            assert!(reply["document_revision"].is_null(), "{label}: {reply}");
        }
    }
}

#[test]
fn an_unknown_style_is_refused_before_the_limit_is_read() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let documents = Documents::new(&app);
    let mut ids = RequestIds::default();
    let reply = export(
        &app,
        &mut ids,
        json!({"format": "smiles", "document_id": documents.benzene, "style": "fancy", "time_limit_ms": 8001}),
    );
    assert_eq!(reply["outcome"], "error", "{reply}");
    assert_eq!(reply["diagnostics"].as_array().unwrap().len(), 1, "{reply}");
    assert_eq!(
        reply["diagnostics"][0]["code"], "invalid_request",
        "{reply}"
    );
    assert_eq!(reply["diagnostics"][0]["path"], "parameters", "{reply}");
    assert!(
        reply["model_id"].is_null(),
        "an unknown style names no model: {reply}"
    );
    assert!(reply["document_revision"].is_null(), "{reply}");
}

/// A SMILES export of a document that does not exist, with `time_limit_ms` when one is given.
fn unknown_document_export(limit: Option<Value>) -> Value {
    let mut parameters =
        json!({"format": "smiles", "document_id": "no-such-document", "style": "recorded"});
    if let Some(limit) = limit {
        parameters["time_limit_ms"] = limit;
    }
    parameters
}

#[test]
fn without_a_smiles_codec_the_limit_is_never_read() {
    let bare_directory = tempfile::tempdir().unwrap();
    let bare = Application::new(
        LocalCapabilities,
        FileDocumentRepository::new(bare_directory.path()),
    );
    let ctfile_directory = tempfile::tempdir().unwrap();
    let ctfile_only = Application::new(
        LocalCapabilities,
        FileDocumentRepository::new(ctfile_directory.path()),
    )
    .with_ctfile_codec(V2000Codec);
    let mut ids = RequestIds::default();
    // With no format codec, document.export is not running; with other codecs, SMILES is not
    // connected. Either reply comes before the limit is read.
    for (name, app, code) in [
        ("no format codec", &bare, "operation_not_implemented"),
        ("no SMILES codec", &ctfile_only, "unsupported_format"),
    ] {
        let without = without_request_id(&export(app, &mut ids, unknown_document_export(None)));
        let with = without_request_id(&export(
            app,
            &mut ids,
            unknown_document_export(Some(json!(8001))),
        ));
        assert_eq!(without["diagnostics"][0]["code"], code, "{name}: {without}");
        assert_eq!(
            with, without,
            "{name}: a limit above the maximum changed the reply"
        );
    }
}

#[test]
fn a_cancel_wins_over_an_expired_limit() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let documents = Documents::new(&app);
    let mut ids = RequestIds::default();
    for row in rows() {
        for (name, document) in documents.subjects() {
            let label = format!(
                "smiles {} on {name} with time_limit_ms 0 and a cancel",
                row.style
            );
            let before = documents.snapshots(&app);
            let revision = revision_of(&app, document);
            let cancel = CancelFlag::new();
            cancel.cancel();
            let reply = export_with(
                &app,
                &mut ids,
                row.parameters(document, Some(json!(0)), None),
                &cancel,
            );
            assert_stopped(&reply, &row, "cancelled", revision, &label);
            assert_eq!(
                reply["diagnostics"][0]["details"],
                json!({}),
                "{label}: {reply}"
            );
            assert_eq!(
                documents.snapshots(&app),
                before,
                "{label}: a document changed"
            );
        }
    }
}

#[test]
fn a_stop_inside_the_write_names_the_limit_or_the_cancel() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let documents = Documents::new(&app);
    let revision = revision_of(&app, &documents.benzene);
    let mut ids = RequestIds::default();
    for row in rows() {
        let label = format!("smiles {} on benzene", row.style);
        let parameters = || row.parameters(&documents.benzene, None, None);
        let plain = export(&app, &mut ids, parameters());

        // Question 1 is the check made before the write.
        let first = CancelAt::new(1, Interrupted::TimeLimit);
        let reply = export_with(&app, &mut ids, parameters(), &first);
        let label_first = format!("{label}, time limit at question 1");
        assert_stopped(&reply, &row, "time_limit", revision, &label_first);
        assert_eq!(first.asked(), 1, "{label_first}");
        assert_eq!(
            reply["diagnostics"][0]["details"],
            json!({"limit_ms": DEFAULT_LIMIT_MS}),
            "{label_first}: {reply}"
        );

        // Question 2 is asked inside the write, and a stop there replies the same way.
        let inside = CancelAt::new(2, Interrupted::TimeLimit);
        let reply = export_with(&app, &mut ids, parameters(), &inside);
        let label_inside = format!("{label}, time limit at question 2");
        assert_stopped(&reply, &row, "time_limit", revision, &label_inside);
        assert_eq!(inside.asked(), 2, "{label_inside}");
        assert_eq!(
            reply["diagnostics"][0]["details"],
            json!({"limit_ms": DEFAULT_LIMIT_MS}),
            "{label_inside}: {reply}"
        );

        let cancelled = CancelAt::new(2, Interrupted::Cancelled);
        let reply = export_with(&app, &mut ids, parameters(), &cancelled);
        let label_cancelled = format!("{label}, cancelled at question 2");
        assert_stopped(&reply, &row, "cancelled", revision, &label_cancelled);
        assert_eq!(cancelled.asked(), 2, "{label_cancelled}");
        assert_eq!(
            reply["diagnostics"][0]["details"],
            json!({}),
            "{label_cancelled}: {reply}"
        );

        // Without a stop the write asks more than once, so question 2 is inside it.
        let counting = CancelAt::new(usize::MAX, Interrupted::Cancelled);
        let reply = export_with(&app, &mut ids, parameters(), &counting);
        assert_eq!(
            observed(&reply),
            observed(&plain),
            "{label}: a count without a stop changed the reply"
        );
        assert!(
            counting.asked() >= 2,
            "{label}: the write asked {} questions",
            counting.asked()
        );
    }
}

#[test]
fn a_stop_before_consent_carries_no_loss_or_digest() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let documents = Documents::new(&app);
    let revision = revision_of(&app, &documents.consent);
    let mut ids = RequestIds::default();
    // A digest that no consent refusal has: a correct digest and this one must both be ignored by
    // a stop.
    let wrong_digest = "0000";
    for row in rows() {
        let label = format!("smiles {} on the consent document", row.style);
        let refusal = export(
            &app,
            &mut ids,
            row.parameters(&documents.consent, None, None),
        );
        assert_eq!(
            refusal["diagnostics"][0]["code"], "loss_consent_required",
            "{label}: {refusal}"
        );
        let digest = digest_of(&refusal).to_owned();

        // Count the questions of the export when nothing stops it.
        let counting = CancelAt::new(usize::MAX, Interrupted::Cancelled);
        let counted = export_with(
            &app,
            &mut ids,
            row.parameters(&documents.consent, None, None),
            &counting,
        );
        let total = counting.asked();
        assert!(
            total >= 2,
            "{label}: {total} questions asked when nothing stops it, but a stop inside the write needs at least 2"
        );
        assert_eq!(
            observed(&counted),
            observed(&refusal),
            "{label}: counting changed the consent refusal"
        );

        for question in 1..=total {
            for reason in [Interrupted::TimeLimit, Interrupted::Cancelled] {
                let (code, details) = match reason {
                    Interrupted::TimeLimit => ("time_limit", json!({"limit_ms": DEFAULT_LIMIT_MS})),
                    Interrupted::Cancelled => ("cancelled", json!({})),
                };
                for (variant, accepted) in [
                    ("no digest", None),
                    ("correct digest", Some(digest.as_str())),
                    ("wrong digest", Some(wrong_digest)),
                ] {
                    let cancel = CancelAt::new(question, reason);
                    let reply = export_with(
                        &app,
                        &mut ids,
                        row.parameters(&documents.consent, None, accepted),
                        &cancel,
                    );
                    let stop = format!(
                        "{label}, {variant}, stopped by {code} at question {question} of {total}"
                    );
                    assert_stopped(&reply, &row, code, revision, &stop);
                    assert_eq!(
                        reply["diagnostics"][0]["details"], details,
                        "{stop}: {reply}"
                    );
                    assert_eq!(cancel.asked(), question, "{stop}");
                }
            }
        }

        // The question after the last one is never asked, so the consent refusal comes back.
        let after = CancelAt::new(total + 1, Interrupted::TimeLimit);
        let reply = export_with(
            &app,
            &mut ids,
            row.parameters(&documents.consent, None, None),
            &after,
        );
        assert_eq!(
            observed(&reply),
            observed(&refusal),
            "{label}: the question after the last one changed the reply"
        );
        assert_eq!(after.asked(), total, "{label}");
    }
}

#[test]
fn the_other_export_formats_keep_no_limit() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let documents = Documents::new(&app);
    let mut ids = RequestIds::default();
    let benzene = documents.benzene.as_str();
    for (format, parameters) in [
        (
            "mol",
            json!({"format": "mol", "records": [{"document_id": benzene}]}),
        ),
        (
            "sdf",
            json!({"format": "sdf", "records": [{"document_id": benzene}]}),
        ),
        ("cdxml", json!({"format": "cdxml", "document_id": benzene})),
        ("cdx", json!({"format": "cdx", "document_id": benzene})),
        ("svg", json!({"format": "svg", "document_id": benzene})),
    ] {
        let label = format!("{format} export");
        let plain = export(&app, &mut ids, parameters.clone());
        assert_ne!(
            plain["outcome"], "error",
            "{label} without the key: {plain}"
        );
        assert!(
            !plain["result"].is_null(),
            "{label} without the key: {plain}"
        );
        let mut keyed = parameters;
        keyed["time_limit_ms"] = json!(0);
        let refused = export(&app, &mut ids, keyed);
        assert_eq!(refused["outcome"], "error", "{label}: {refused}");
        assert_eq!(
            refused["diagnostics"][0]["code"], "invalid_request",
            "{label}: {refused}"
        );
        assert_eq!(
            refused["diagnostics"][0]["path"], "parameters",
            "{label}: {refused}"
        );
        assert!(refused["result"].is_null(), "{label}: {refused}");
    }
}
