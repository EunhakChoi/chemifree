//! Graphic point updates use the common transaction boundary and unchanged native history.

use chemifree_application::{Application, CommandDto};
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};

type App = Application<LocalCapabilities, FileDocumentRepository>;
fn commands(value: Value) -> Vec<CommandDto> {
    serde_json::from_value(value).unwrap()
}
fn setup() -> (App, tempfile::TempDir, String) {
    let root = tempfile::tempdir().unwrap();
    let app = Application::new(LocalCapabilities, FileDocumentRepository::new(root.path()));
    let id = app
        .create_document(None, Some("Graphic points"))
        .unwrap()
        .document
        .id;
    app.apply_transaction(
        &id,
        None,
        0,
        commands(json!([
            {"op":"add_atom","element":"C","position":{"x":2,"y":3}},
            {"op":"add_graphic","shape":"line","points":[{"x":0,"y":0},{"x":10,"y":0}]},
            {"op":"add_graphic","shape":"arrow","points":[{"x":20,"y":0},{"x":30,"y":0}]},
            {"op":"group","objects":["s2","s3"]},
            {"op":"add_reference","source":"s2","target":{"kind":"atom","id":"a1"}},
            {"op":"add_reference","source":"s3","target":{"kind":"object","id":"s2"}}
        ])),
    )
    .unwrap();
    (app, root, id)
}
fn graphic(doc: &Value, id: &str) -> Value {
    doc["scene"]["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|object| object["id"] == id)
        .unwrap()
        .clone()
}

#[test]
fn update_replay_noop_save_reopen_and_history_preserve_parent_references_and_graph() {
    let (app, root, id) = setup();
    let original = serde_json::to_value(app.document(&id).unwrap()).unwrap();
    let new_points = json!([{"x":-100000.0,"y":100000.0},{"x":100000.0,"y":-100000.0}]);
    let edit = commands(json!([{"op":"update_graphic_points","object":"s2","points":new_points}]));
    let edited = app
        .apply_transaction(&id, Some("update-points"), 1, edit.clone())
        .unwrap();
    assert_eq!(
        app.apply_transaction(&id, Some("update-points"), 1, edit.clone())
            .unwrap(),
        edited
    );
    let after = serde_json::to_value(&edited).unwrap();
    let object = graphic(&after["document"], "s2");
    assert_eq!(object["points"], new_points);
    assert_eq!(object["shape"], "line");
    assert_eq!(object["parent"], "s4");
    for field in [
        "atoms",
        "bonds",
        "molecule",
        "stereo",
        "settings",
        "selection",
    ] {
        assert_eq!(after["document"][field], original["document"][field]);
    }
    for field in ["roots", "references", "instances"] {
        assert_eq!(
            after["document"]["scene"][field],
            original["document"]["scene"][field]
        );
    }
    assert_eq!(
        graphic(&after["document"], "s3"),
        graphic(&original["document"], "s3")
    );
    let noop = app
        .apply_transaction(&id, Some("same-points"), 2, edit)
        .unwrap();
    assert_eq!(noop.document, edited.document);
    let undone = serde_json::to_value(app.undo(&id, None, 2).unwrap()).unwrap();
    assert_eq!(
        graphic(&undone["document"], "s2"),
        graphic(&original["document"], "s2")
    );
    let redone = app.redo(&id, None, 3).unwrap();
    app.save(&id, None, 4).unwrap();
    app.close(&id, None, false, None).unwrap();
    assert_eq!(app.document(&id).unwrap().document, redone.document);
    let raw: Value = serde_json::from_slice(
        &std::fs::read(root.path().join(format!("documents/{id}.json"))).unwrap(),
    )
    .unwrap();
    assert_eq!(
        raw["format_version"], 25,
        "point edits reuse the existing native graphic state"
    );
    let undone = serde_json::to_value(app.undo(&id, None, 4).unwrap()).unwrap();
    assert_eq!(
        graphic(&undone["document"], "s2"),
        graphic(&original["document"], "s2")
    );
}

#[test]
fn malformed_points_bounds_targets_and_later_failures_are_atomic_at_the_public_boundary() {
    let (app, _root, id) = setup();
    let original = app.document(&id).unwrap();
    for (index,(command,code)) in [
        (json!({"op":"update_graphic_points","object":"s2","points":[]}),"invalid_request"),
        (json!({"op":"update_graphic_points","object":"s2","points":[{"x":0,"y":0}]}),"invalid_request"),
        (json!({"op":"update_graphic_points","object":"s2","points":[{"x":0,"y":0},{"x":1,"y":1},{"x":2,"y":2}]}),"invalid_request"),
        (json!({"op":"update_graphic_points","object":"s2","points":[{"x":0,"y":0},{"x":1,"y":null}]}),"invalid_request"),
        (json!({"op":"update_graphic_points","object":"s2","shape":"arrow","points":[{"x":0,"y":0},{"x":1,"y":1}]}),"invalid_request"),
        (json!({"op":"update_graphic_points","object":"s2","points":[{"x":100001,"y":0},{"x":1,"y":1}]}),"invalid_command"),
        (json!({"op":"update_graphic_points","object":"s2","points":[{"x":0,"y":0},{"x":1,"y":-100001}]}),"invalid_command"),
        (json!({"op":"update_graphic_points","object":"s99","points":[{"x":0,"y":0},{"x":1,"y":1}]}),"invalid_command"),
        (json!({"op":"update_graphic_points","object":"s1","points":[{"x":0,"y":0},{"x":1,"y":1}]}),"invalid_command"),
        (json!({"op":"update_graphic_points","object":"s4","points":[{"x":0,"y":0},{"x":1,"y":1}]}),"invalid_command"),
    ].into_iter().enumerate() {
        let request=json!({"contract":1,"request_id":format!("refusal-{index}"),"profile":"preserve-semantics","operation":"document.transaction","parameters":{"document_id":id,"expected_revision":1,"commands":[{"op":"rename","title":"rollback"},command]}});
        let result=serde_json::to_value(app.execute(request.to_string().as_bytes())).unwrap();assert_eq!(result["diagnostics"][0]["code"],code,"{result}");assert_eq!(app.document(&id).unwrap(),original);
    }
}
