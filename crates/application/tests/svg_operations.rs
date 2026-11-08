//! SVG drawing export through `document.export` (P38a2): the consent round trip, a digest that an
//! edit has made stale, the request and document errors, the import refusal, the read-only rule, and
//! the drawing of an atom-less document. The expected text is the committed P38a1 golden under
//! `tests/fixtures/svg`, never the output of the renderer under test.

use std::fs;
use std::path::Path;

use chemifree_application::Application;
use chemifree_providers::interchange_codec::LocalInterchangeCodec;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities, OpenSmilesCodec, V2000Codec};
use serde_json::{Value, json};
use tempfile::TempDir;

type App = Application<LocalCapabilities, FileDocumentRepository>;

/// The model an SVG export names.
const SVG_MODEL: &str = "chemifree-svg-1";
/// A document id that no document in these tests has.
const MISSING_DOCUMENT: &str = "doc-ffffffffffffffff";
/// A small SVG text. Its format alone makes `document.import` refuse it.
const SVG_SOURCE: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="10pt" height="10pt" viewBox="0 0 10 10"><g/></svg>"#;

/// An application with the format codecs that `svg_loss_drift.rs` attaches. Without one of them,
/// `document.export` and `document.import` are not executable and every call answers
/// `operation_not_implemented`. The image decoder is left out: no test here draws an image.
fn app(path: &Path) -> App {
    Application::new(LocalCapabilities, FileDocumentRepository::new(path))
        .with_ctfile_codec(V2000Codec)
        .with_interchange_codec(LocalInterchangeCodec)
        .with_smiles_codec(OpenSmilesCodec)
}

fn call(app: &App, request_id: &str, operation: &str, parameters: Value) -> Value {
    let request = json!({"contract":1,"request_id":request_id,"profile":"preserve-semantics","operation":operation,"parameters":parameters});
    serde_json::to_value(app.execute(request.to_string().as_bytes())).unwrap()
}

fn setup() -> (App, TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    let application = app(dir.path());
    let created = call(&application, "create", "document.create", json!({}));
    let id = created["result"]["document"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    (application, dir, id)
}

/// The snapshot `document.snapshot` returns: the document, and the session's revision, history and
/// unsaved-change state.
fn snapshot(app: &App, id: &str) -> Value {
    call(
        app,
        "snapshot",
        "document.snapshot",
        json!({"document_id":id}),
    )["result"]
        .clone()
}

/// The committed golden `name` under `tests/fixtures/svg`: a P38a1 drawing, byte for byte.
fn golden(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/svg")
        .join(name);
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} cannot be read: {error}", path.display()))
}

/// The commands of the `labels` golden, as `label_commands` in `svg_render.rs` writes them: stated
/// and unknown hydrogens, a +1 nitrogen, a 13C isotope, a 2- oxygen, a list query of four elements
/// (drawn truncated) and a negated list of two elements.
fn labels_commands() -> Value {
    json!([
        {"op":"add_atom","element":"N","position":{"x":0,"y":0},"hydrogens":{"mode":"specified","count":3}},
        {"op":"update_atom","atom":"a1","charge":1},
        {"op":"add_atom","element":"C","position":{"x":30,"y":0},"hydrogens":{"mode":"specified","count":3}},
        {"op":"add_atom","element":"C","position":{"x":60,"y":0},"bond_to":{"atom":"a2","order":"single"}},
        {"op":"add_atom","element":"C","position":{"x":0,"y":40}},
        {"op":"update_atom","atom":"a4","isotope":13},
        {"op":"add_atom","element":"O","position":{"x":0,"y":80}},
        {"op":"update_atom","atom":"a5","charge":-2},
        {"op":"add_atom","element":"C","position":{"x":30,"y":80}},
        {"op":"update_atom","atom":"a6","hydrogens":{"mode":"unknown"}},
        {"op":"add_atom","query":{"kind":"list","elements":["N","O","S","P"]},"position":{"x":0,"y":120}},
        {"op":"add_atom","query":{"kind":"list","elements":["N","O"],"negated":true},"position":{"x":30,"y":120}},
        {"op":"update_settings","carbon_labels":"all"}
    ])
}

/// Draws the labels document on the empty document `id`, in one transaction that must succeed.
fn draw_labels(application: &App, id: &str) {
    let edit = call(
        application,
        "draw",
        "document.transaction",
        json!({"document_id":id,"expected_revision":0,"commands":labels_commands()}),
    );
    assert_eq!(edit["outcome"], "ok", "{edit}");
}

/// An SVG export of `id`. It accepts the losses named by `accepted` when that is given.
fn export(app: &App, request_id: &str, id: &str, accepted: Option<&str>) -> Value {
    let mut parameters = json!({"format":"svg","document_id":id});
    if let Some(digest) = accepted {
        parameters["accepted_losses"] = json!(digest);
    }
    call(app, request_id, "document.export", parameters)
}

/// The digest that a consent refusal asks its caller to repeat as `accepted_losses`.
fn loss_digest(refusal: &Value) -> String {
    refusal["diagnostics"][0]["details"]["loss_digest"]
        .as_str()
        .unwrap_or_else(|| panic!("no loss_digest in the refusal: {refusal}"))
        .to_owned()
}

/// The SVG text of an export.
fn content(reply: &Value) -> &str {
    reply["result"]["content"]
        .as_str()
        .unwrap_or_else(|| panic!("no export content: {reply}"))
}

/// The feature of each loss a reply reports, in the order the reply lists them.
fn loss_features(reply: &Value) -> Vec<&str> {
    reply["loss_report"]
        .as_array()
        .unwrap()
        .iter()
        .map(|loss| loss["feature"].as_str().unwrap())
        .collect()
}

#[test]
fn a_consent_refusal_names_its_features_and_digest_and_accepting_it_writes_the_labels_golden() {
    let (application, _directory, id) = setup();
    draw_labels(&application, &id);
    let refused = export(&application, "refused", &id, None);
    assert_eq!(refused["outcome"], "error", "{refused}");
    assert!(refused["result"].is_null(), "{refused}");
    let diagnostic = &refused["diagnostics"][0];
    assert_eq!(diagnostic["code"], "loss_consent_required", "{refused}");
    // The chemical graph is a loss that needs no consent; only the truncated query does.
    assert_eq!(
        diagnostic["details"]["features"],
        json!(["query_label_truncated"]),
        "{refused}"
    );
    let digest = loss_digest(&refused);
    assert_eq!(
        digest.len(),
        64,
        "a SHA-256 digest is 64 hex digits: {digest}"
    );

    let accepted = export(&application, "accepted", &id, Some(&digest));
    assert_eq!(accepted["outcome"], "partial", "{accepted}");
    assert_eq!(accepted["model_id"], SVG_MODEL, "{accepted}");
    assert_eq!(
        accepted["result"]["media_type"], "image/svg+xml",
        "{accepted}"
    );
    assert_eq!(
        loss_features(&accepted),
        ["chemical_graph", "query_label_truncated"],
        "{accepted}"
    );
    let golden_text = golden("labels.svg");
    assert_eq!(
        content(&accepted),
        golden_text.as_str(),
        "the accepted export is the labels golden"
    );
}

#[test]
fn a_digest_from_before_an_edit_is_refused_as_changed_and_the_new_digest_is_accepted() {
    let (application, _directory, id) = setup();
    draw_labels(&application, &id);
    let first = export(&application, "first", &id, None);
    assert_eq!(
        first["diagnostics"][0]["code"], "loss_consent_required",
        "{first}"
    );
    let stale = loss_digest(&first);
    // The new carbon carries an attachment point: the drawing gains a carbon and a consent loss.
    let edit = call(
        &application,
        "edit",
        "document.transaction",
        json!({"document_id":id,"expected_revision":1,"commands":[
            {"op":"add_atom","element":"C","position":{"x":90,"y":0},"attachment_points":[1]}
        ]}),
    );
    assert_eq!(edit["outcome"], "ok", "{edit}");
    let refused = export(&application, "stale", &id, Some(&stale));
    assert_eq!(refused["outcome"], "error", "{refused}");
    assert!(refused["result"].is_null(), "{refused}");
    assert_eq!(
        refused["diagnostics"][0]["code"], "loss_consent_changed",
        "{refused}"
    );
    let fresh = export(&application, "fresh", &id, None);
    assert_eq!(
        fresh["diagnostics"][0]["code"], "loss_consent_required",
        "{fresh}"
    );
    assert_eq!(
        fresh["diagnostics"][0]["details"]["features"],
        json!(["attachment_point_mark", "query_label_truncated"]),
        "{fresh}"
    );
    let current = loss_digest(&fresh);
    assert_ne!(current, stale, "the edit changes the digest");
    let accepted = export(&application, "accepted", &id, Some(&current));
    assert_eq!(accepted["outcome"], "partial", "{accepted}");
}

#[test]
fn an_unknown_parameter_is_an_invalid_request() {
    let (application, _directory, id) = setup();
    draw_labels(&application, &id);
    // A misspelt consent parameter, and a SMILES parameter sent to an SVG export. Each must be
    // refused as a bad request, not ignored, which would leave the export to fail for want of consent.
    for (request_id, parameters) in [
        (
            "misspelt",
            json!({"format":"svg","document_id":id,"accepted_loss":"0".repeat(64)}),
        ),
        (
            "smiles-style",
            json!({"format":"svg","document_id":id,"style":"recorded"}),
        ),
    ] {
        let reply = call(&application, request_id, "document.export", parameters);
        assert_eq!(reply["outcome"], "error", "{request_id}: {reply}");
        assert_eq!(
            reply["diagnostics"][0]["code"], "invalid_request",
            "{request_id}: {reply}"
        );
        assert!(reply["result"].is_null(), "{request_id}: {reply}");
    }
}

#[test]
fn a_missing_document_is_document_not_found() {
    let (application, _directory, _id) = setup();
    let reply = export(&application, "missing", MISSING_DOCUMENT, None);
    assert_eq!(reply["outcome"], "error", "{reply}");
    assert_eq!(
        reply["diagnostics"][0]["code"], "document_not_found",
        "{reply}"
    );
    assert!(reply["result"].is_null(), "{reply}");
}

#[test]
fn importing_svg_is_unsupported_and_leaves_the_revision_unchanged() {
    let (application, _directory, id) = setup();
    draw_labels(&application, &id);
    let before = snapshot(&application, &id);
    assert_eq!(before["document"]["revision"], 1, "{before}");
    // The expected revision is the current one, so only the format can refuse the import.
    let reply = call(
        &application,
        "import",
        "document.import",
        json!({"document_id":id,"expected_revision":1,"format":"svg","source":SVG_SOURCE,"preview":true}),
    );
    assert_eq!(reply["outcome"], "error", "{reply}");
    assert_eq!(
        reply["diagnostics"][0]["code"], "unsupported_format",
        "{reply}"
    );
    assert!(reply["result"].is_null(), "{reply}");
    assert_eq!(
        snapshot(&application, &id),
        before,
        "a refused import changes nothing"
    );
}

#[test]
fn export_changes_no_revision_history_or_dirty_state() {
    let (application, _directory, id) = setup();
    draw_labels(&application, &id);
    let unsaved = snapshot(&application, &id);
    assert_eq!(unsaved["document"]["revision"], 1, "{unsaved}");
    assert_eq!(unsaved["session"]["undo_depth"], 1, "{unsaved}");
    assert_eq!(unsaved["session"]["redo_depth"], 0, "{unsaved}");
    assert_eq!(
        unsaved["session"]["unsaved_changes"],
        json!(true),
        "{unsaved}"
    );

    let refused = export(&application, "refused", &id, None);
    assert_eq!(
        refused["diagnostics"][0]["code"], "loss_consent_required",
        "{refused}"
    );
    let digest = loss_digest(&refused);
    let accepted = export(&application, "accepted", &id, Some(&digest));
    assert_eq!(accepted["outcome"], "partial", "{accepted}");
    assert_eq!(accepted["document_revision"], 1, "{accepted}");
    assert_eq!(
        snapshot(&application, &id),
        unsaved,
        "an export changes nothing in an unsaved document"
    );

    let saved = call(
        &application,
        "save",
        "document.save",
        json!({"document_id":id,"expected_revision":1}),
    );
    assert_eq!(saved["outcome"], "ok", "{saved}");
    let clean = snapshot(&application, &id);
    assert_eq!(clean["session"]["unsaved_changes"], json!(false), "{clean}");
    assert_eq!(clean["session"]["saved_revision"], 1, "{clean}");
    let again = export(&application, "again", &id, Some(&digest));
    assert_eq!(again["outcome"], "partial", "{again}");
    assert_eq!(
        snapshot(&application, &id),
        clean,
        "an export leaves a saved document saved"
    );

    // The labelling step is still the one undo step. Undo is a new revision: the domain's `undo`
    // adds one, so the undone drawing is at revision 2, not back at revision 0.
    let undo = call(
        &application,
        "undo",
        "history.undo",
        json!({"document_id":id,"expected_revision":1}),
    );
    assert_eq!(undo["outcome"], "ok", "{undo}");
    assert_eq!(
        snapshot(&application, &id)["document"]["revision"],
        2,
        "undo advances the revision, so the undone labelling is revision 2"
    );
}

#[test]
fn an_atomless_document_exports_ok_with_a_zero_view_box() {
    let (application, _directory, id) = setup();
    let reply = export(&application, "empty", &id, None);
    assert_eq!(reply["outcome"], "ok", "{reply}");
    assert_eq!(reply["loss_report"], json!([]), "{reply}");
    assert_eq!(
        reply["result"]["view_box"],
        json!({"x":0.0,"y":0.0,"width":0.0,"height":0.0}),
        "{reply}"
    );
    let golden_text = golden("empty.svg");
    assert_eq!(
        content(&reply),
        golden_text.as_str(),
        "the atom-less drawing is the empty golden"
    );
}
