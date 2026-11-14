//! The `layout.structure` apply through the application, as one undoable edit. A non-empty apply
//! is one history entry, and undo and redo restore every stored coordinate bit for bit. A replayed
//! request returns its first reply and applies nothing again, and a reused request ID with a
//! changed body is refused. The selection an apply stores is the one it is given, and a no-op apply
//! changes nothing and is still replayed. The placement itself is covered by `layout_operation.rs`.

use std::error::Error;

use chemifree_application::{Application, DocumentResponse, DocumentView, SelectionDto};
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};

type App = Application<LocalCapabilities, FileDocumentRepository>;
type TestResult = Result<(), Box<dyn Error>>;

/// The model that every `layout.structure` reply names.
const MODEL_ID: &str = "chemifree-chain-layout-1";
/// The message of the `layout_unchanged` diagnostic of an apply that changes nothing.
const NO_OP_MESSAGE: &str =
    "Every layout atom is already at its proposed position; nothing was changed.";

/// One stored coordinate: its instance (`atoms` for the first instance's mirror in
/// `atoms[].position`), its atom, and the bit patterns of x and y, which tell -0.0 and 0.0 apart.
type Coordinate = (String, String, u64, u64);

/// What an undo or a redo must restore exactly: every coordinate and the selection.
#[derive(Debug, PartialEq)]
struct Geometry {
    coordinates: Vec<Coordinate>,
    selection: SelectionDto,
}

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

/// The JSON reply to the request body `body`.
fn send(app: &App, body: &[u8]) -> Result<Value, Box<dyn Error>> {
    Ok(serde_json::to_value(app.execute(body))?)
}

/// The JSON reply to the request `request_id` for `operation` with `parameters`.
fn call(
    app: &App,
    request_id: &str,
    operation: &str,
    parameters: Value,
) -> Result<Value, Box<dyn Error>> {
    send(app, &request(request_id, operation, parameters))
}

/// Sends the `document.transaction` request `request_id` with `parameters`, which must succeed, and
/// returns its reply.
fn transact(app: &App, request_id: &str, parameters: Value) -> Result<Value, Box<dyn Error>> {
    let reply = call(app, request_id, "document.transaction", parameters)?;
    assert_eq!(reply["outcome"], "ok", "{request_id}: {reply}");
    Ok(reply)
}

/// Creates a document titled `title`, applies `commands` to it at revision 0, and returns its ID.
fn draw(app: &App, title: &str, commands: Value) -> Result<String, Box<dyn Error>> {
    let id = app.create_document(None, Some(title))?.document.id;
    transact(
        app,
        &format!("draw-{title}"),
        json!({"document_id": id, "expected_revision": 0, "commands": commands}),
    )?;
    Ok(id)
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

/// An unbonded carbon at (`x`, 0).
fn loose_carbon(x: f64) -> Value {
    json!({"op": "add_atom", "element": "C", "position": {"x": x, "y": 0}})
}

/// The parameters of an apply of `atoms` in document `id` at `expected_revision`, with the default
/// scope and instance.
fn apply_parameters(id: &str, expected_revision: u64, atoms: &[&str]) -> Value {
    json!({"document_id": id, "mode": "apply", "selection": {"atoms": atoms}, "expected_revision": expected_revision})
}

/// Every stored coordinate of `document`, bit for bit: the positions of each scene instance, and
/// the first instance's mirror in `atoms[].position`.
fn coordinates(document: &DocumentView) -> Vec<Coordinate> {
    let mut stored = Vec::new();
    for instance in &document.scene.instances {
        for position in &instance.positions {
            stored.push((
                instance.id.clone(),
                position.atom.clone(),
                position.x.to_bits(),
                position.y.to_bits(),
            ));
        }
    }
    for atom in &document.atoms {
        stored.push((
            "atoms".to_owned(),
            atom.id.clone(),
            atom.position.x.to_bits(),
            atom.position.y.to_bits(),
        ));
    }
    stored
}

/// The geometry of `response`: what an undo or a redo must restore exactly.
fn geometry(response: &DocumentResponse) -> Geometry {
    Geometry {
        coordinates: coordinates(&response.document),
        selection: response.document.selection.clone(),
    }
}

/// The diagnostic codes of `reply`, in order.
fn codes(reply: &Value) -> Vec<&str> {
    reply["diagnostics"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|diagnostic| diagnostic["code"].as_str())
        .collect()
}

/// The first diagnostic of `reply` with `code`, if it has one.
fn diagnostic<'a>(reply: &'a Value, code: &str) -> Option<&'a Value> {
    reply["diagnostics"]
        .as_array()?
        .iter()
        .find(|diagnostic| diagnostic["code"] == code)
}

#[test]
fn an_apply_is_one_history_entry_that_clears_redo() -> TestResult {
    let directory = tempfile::tempdir()?;
    let app = new_app(&directory);
    let id = draw(&app, "butane", butane())?;
    transact(
        &app,
        "loose-carbon",
        json!({"document_id": id, "expected_revision": 1, "commands": [loose_carbon(300.0)]}),
    )?;
    let undone = call(
        &app,
        "undo-loose-carbon",
        "history.undo",
        json!({"document_id": id, "expected_revision": 2}),
    )?;
    assert_eq!(undone["document_revision"], 3, "{undone}");
    assert_eq!(undone["result"]["session"]["redo_depth"], 1, "{undone}");
    // The document is saved first, so that the apply is what leaves it with unsaved changes.
    app.save(&id, None, 3)?;
    let applied = call(
        &app,
        "apply",
        "layout.structure",
        apply_parameters(&id, 3, &["a2"]),
    )?;
    assert_eq!(applied["outcome"], "ok", "{applied}");
    assert_eq!(applied["document_revision"], 4, "{applied}");
    let session = &applied["result"]["session"];
    assert_eq!(
        session["undo_depth"], 2,
        "the apply adds one entry to the butane's: {applied}"
    );
    assert_eq!(
        session["redo_depth"], 0,
        "the apply clears the redo stack: {applied}"
    );
    assert_eq!(session["unsaved_changes"], json!(true), "{applied}");
    Ok(())
}

#[test]
fn undo_restores_every_position_bit_for_bit_and_redo_the_committed_ones() -> TestResult {
    let directory = tempfile::tempdir()?;
    let app = new_app(&directory);
    let id = draw(&app, "butane", butane())?;
    // A second instance of the butane, 200 pt to the right, with the first carbon selected.
    transact(
        &app,
        "second-instance",
        json!({
            "document_id": id,
            "expected_revision": 1,
            "commands": [{"op": "add_instance", "offset": {"x": 200, "y": 0}}],
            "selection_after": {"atoms": ["a1"]},
        }),
    )?;
    let before = geometry(&app.document(&id)?);
    let applied = call(
        &app,
        "apply",
        "layout.structure",
        json!({
            "document_id": id,
            "mode": "apply",
            "selection": {"atoms": ["a2"]},
            "instance": "s1",
            "expected_revision": 2,
        }),
    )?;
    assert_eq!(applied["outcome"], "ok", "{applied}");
    assert_eq!(applied["document_revision"], 3, "{applied}");
    let committed = geometry(&app.document(&id)?);
    assert_ne!(
        committed.coordinates, before.coordinates,
        "the apply must move the first instance's atoms"
    );
    let undone = app.undo(&id, None, 3)?;
    assert_eq!(undone.document.revision, 4, "undo is one more revision");
    assert_eq!(
        geometry(&undone),
        before,
        "undo must restore every position and the selection bit for bit"
    );
    let redone = app.redo(&id, None, 4)?;
    assert_eq!(redone.document.revision, 5, "redo is one more revision");
    assert_eq!(
        geometry(&redone),
        committed,
        "redo must restore the committed positions bit for bit"
    );
    Ok(())
}

#[test]
fn a_replay_after_another_edit_returns_the_identical_reply_and_applies_once() -> TestResult {
    let directory = tempfile::tempdir()?;
    let app = new_app(&directory);
    let id = draw(&app, "butane", butane())?;
    let body = request(
        "apply-once",
        "layout.structure",
        apply_parameters(&id, 1, &["a2"]),
    );
    let first = send(&app, &body)?;
    assert_eq!(first["outcome"], "ok", "{first}");
    assert_eq!(codes(&first), ["layout_not_implemented"], "{first}");
    assert_eq!(first["document_revision"], 2, "{first}");
    transact(
        &app,
        "loose-carbon",
        json!({"document_id": id, "expected_revision": 2, "commands": [loose_carbon(300.0)]}),
    )?;
    let replayed = send(&app, &body)?;
    assert_eq!(
        replayed, first,
        "the same request body must return the first reply, diagnostics and model included"
    );
    let current = app.document(&id)?;
    assert_eq!(
        current.document.revision, 3,
        "the replay must not apply the edit a second time"
    );
    assert_eq!(
        current.session.undo_depth, 3,
        "the butane, the apply and the loose carbon are the three entries: {replayed}"
    );
    Ok(())
}

#[test]
fn a_changed_body_with_the_same_request_id_is_request_id_reused() -> TestResult {
    let directory = tempfile::tempdir()?;
    let app = new_app(&directory);
    let id = draw(&app, "butane", butane())?;
    let original = apply_parameters(&id, 1, &["a2"]);
    let applied = call(&app, "reused", "layout.structure", original.clone())?;
    assert_eq!(applied["outcome"], "ok", "{applied}");
    let changes = [
        ("scope", json!("components")),
        ("instance", json!("s1")),
        ("time_limit_ms", json!(4000)),
        ("selection", json!({"atoms": ["a3"]})),
        ("selection_after", json!({"atoms": ["a1"]})),
    ];
    for (field, value) in changes {
        let mut changed = original.clone();
        changed[field] = value;
        let reply = call(&app, "reused", "layout.structure", changed)?;
        assert_eq!(reply["outcome"], "error", "{field}: {reply}");
        assert_eq!(
            reply["diagnostics"][0]["code"], "request_id_reused",
            "{field}: {reply}"
        );
        assert_eq!(reply["model_id"], MODEL_ID, "{field}: {reply}");
    }
    let current = app.document(&id)?;
    assert_eq!(
        current.document.revision, 2,
        "a refused request changes nothing"
    );
    Ok(())
}

#[test]
fn selection_after_is_stored_and_undo_restores_selection_before() -> TestResult {
    let directory = tempfile::tempdir()?;
    let app = new_app(&directory);
    let id = draw(&app, "butane", butane())?;
    let mut parameters = apply_parameters(&id, 1, &["a2"]);
    parameters["selection_before"] = json!({"atoms": ["a1"]});
    parameters["selection_after"] = json!({"atoms": ["a3"]});
    let applied = call(&app, "apply", "layout.structure", parameters)?;
    assert_eq!(applied["outcome"], "ok", "{applied}");
    assert_eq!(
        applied["result"]["document"]["selection"]["atoms"],
        json!(["a3"]),
        "{applied}"
    );
    let undone = app.undo(&id, None, 2)?;
    assert_eq!(
        undone.document.selection.atoms,
        ["a1".to_owned()],
        "the undo must restore the selection the apply was given before"
    );
    Ok(())
}

#[test]
fn without_selection_fields_the_selection_is_kept_and_captured() -> TestResult {
    let directory = tempfile::tempdir()?;
    let app = new_app(&directory);
    let id = draw(&app, "butane", butane())?;
    transact(
        &app,
        "select-a4",
        json!({
            "document_id": id,
            "expected_revision": 1,
            "commands": [{"op": "rename", "title": "selected"}],
            "selection_after": {"atoms": ["a4"]},
        }),
    )?;
    let applied = call(
        &app,
        "apply",
        "layout.structure",
        apply_parameters(&id, 2, &["a2"]),
    )?;
    assert_eq!(applied["outcome"], "ok", "{applied}");
    assert_eq!(
        applied["result"]["document"]["selection"]["atoms"],
        json!(["a4"]),
        "an apply without selection fields keeps the selection: {applied}"
    );
    let undone = app.undo(&id, None, 3)?;
    assert_eq!(
        undone.document.selection.atoms,
        ["a4".to_owned()],
        "the undo must restore the selection the apply captured"
    );
    Ok(())
}

#[test]
fn a_no_op_apply_changes_nothing_and_is_replayed() -> TestResult {
    let directory = tempfile::tempdir()?;
    let app = new_app(&directory);
    let id = draw(&app, "carbon", json!([loose_carbon(0.0)]))?;
    let before = app.document(&id)?;
    let body = request(
        "no-op",
        "layout.structure",
        json!({
            "document_id": id,
            "mode": "apply",
            "selection": {"atoms": ["a1"]},
            "selection_after": {"atoms": ["a1"]},
            "expected_revision": 1,
        }),
    );
    let reply = send(&app, &body)?;
    assert_eq!(reply["outcome"], "ok", "{reply}");
    assert_eq!(reply["document_revision"], 1, "{reply}");
    assert_eq!(
        codes(&reply),
        [
            "layout_no_bonds",
            "layout_unchanged",
            "layout_not_implemented"
        ],
        "{reply}"
    );
    let unchanged = diagnostic(&reply, "layout_unchanged")
        .ok_or_else(|| format!("no layout_unchanged diagnostic: {reply}"))?;
    assert_eq!(unchanged["message"], NO_OP_MESSAGE, "{reply}");
    let after = app.document(&id)?;
    assert_eq!(
        serde_json::to_value(&after)?,
        serde_json::to_value(&before)?,
        "a no-op apply changes neither the document, the session nor the selection"
    );
    assert_eq!(
        send(&app, &body)?,
        reply,
        "the no-op reply is recorded for replay"
    );
    Ok(())
}
