//! Public graphic styles retain graph state, replay and versioned clipboard fidelity.

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
        .create_document(None, Some("Widths"))
        .unwrap()
        .document
        .id;
    app.apply_transaction(
        &id,
        None,
        0,
        commands(json!([
         {"op":"add_atom","element":"C","position":{"x":0,"y":0}},
         {"op":"add_graphic","shape":"line","points":[{"x":0,"y":0},{"x":10,"y":10}]},
         {"op":"add_graphic","shape":"arrow","points":[{"x":20,"y":0},{"x":30,"y":10}]},
         {"op":"group","objects":["s2","s3"]},
         {"op":"add_reference","source":"s2","target":{"kind":"object","id":"s3"}}
        ])),
    )
    .unwrap();
    (app, root, id)
}
fn send(app: &App, id: &str, operation: &str, parameters: Value) -> Value {
    serde_json::to_value(app.execute(json!({"contract":1,"request_id":id,"profile":"preserve-semantics","operation":operation,"parameters":parameters}).to_string().as_bytes())).unwrap()
}
fn view(app: &App, id: &str) -> Value {
    serde_json::to_value(app.document(id).unwrap()).unwrap()
}
fn graphic(view: &Value, id: &str) -> Value {
    view["document"]["scene"]["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|object| object["id"] == id)
        .unwrap()
        .clone()
}

#[test]
fn width_override_reset_replay_and_native_history_preserve_graph_and_scene_structure() {
    let (app, _root, id) = setup();
    let original = view(&app, &id);
    assert_eq!(graphic(&original, "s2")["line_width_pt"], Value::Null);
    let edits =
        commands(json!([{"op":"set_graphic_line_width","object":"s2","line_width_pt":2.75}]));
    let edited = app
        .apply_transaction(&id, Some("width-once"), 1, edits.clone())
        .unwrap();
    assert_eq!(
        app.apply_transaction(&id, Some("width-once"), 1, edits.clone())
            .unwrap(),
        edited
    );
    let changed = serde_json::to_value(&edited).unwrap();
    let object = graphic(&changed, "s2");
    assert_eq!(object["line_width_pt"], 2.75);
    assert_eq!(object["shape"], "line");
    assert_eq!(object["parent"], "s4");
    assert_eq!(object["points"], graphic(&original, "s2")["points"]);
    for field in ["atoms", "bonds", "stereo", "settings"] {
        assert_eq!(changed["document"][field], original["document"][field]);
    }
    for field in ["roots", "instances", "references"] {
        assert_eq!(
            changed["document"]["scene"][field],
            original["document"]["scene"][field]
        );
    }
    assert_eq!(
        app.apply_transaction(&id, Some("same-width"), 2, edits)
            .unwrap()
            .document,
        edited.document
    );
    let reset = app
        .apply_transaction(
            &id,
            Some("inherit"),
            2,
            commands(json!([{"op":"set_graphic_line_width","object":"s2","line_width_pt":null}])),
        )
        .unwrap();
    assert_eq!(
        graphic(&serde_json::to_value(&reset).unwrap(), "s2")["line_width_pt"],
        Value::Null
    );
    let undone = app.undo(&id, None, 3).unwrap();
    assert_eq!(
        graphic(&serde_json::to_value(&undone).unwrap(), "s2")["line_width_pt"],
        2.75
    );
    app.save(&id, None, 4).unwrap();
    app.close(&id, None, false, None).unwrap();
    assert_eq!(app.document(&id).unwrap().document, undone.document);
    let redone = app.redo(&id, None, 4).unwrap();
    assert_eq!(
        graphic(&serde_json::to_value(&redone).unwrap(), "s2")["line_width_pt"],
        Value::Null
    );
}

#[test]
fn malformed_widths_and_wrong_targets_are_atomic_with_existing_error_codes() {
    let (app, _root, id) = setup();
    let before = app.document(&id).unwrap();
    for (index, (command, code, reason)) in [
        (
            json!({"op":"set_graphic_line_width","object":"s2"}),
            "invalid_request",
            None,
        ),
        (
            json!({"op":"set_graphic_line_width","object":"s2","line_width_pt":"inherit"}),
            "invalid_request",
            None,
        ),
        (
            json!({"op":"set_graphic_line_width","object":"s2","line_width_pt":false}),
            "invalid_request",
            None,
        ),
        (
            json!({"op":"set_graphic_line_width","object":"s2","line_width_pt":0.099}),
            "invalid_command",
            Some("line_width_out_of_range"),
        ),
        (
            json!({"op":"set_graphic_line_width","object":"s2","line_width_pt":10.001}),
            "invalid_command",
            Some("line_width_out_of_range"),
        ),
        (
            json!({"op":"set_graphic_line_width","object":"s1","line_width_pt":1.0}),
            "invalid_command",
            Some("object_kind_mismatch"),
        ),
        (
            json!({"op":"set_graphic_line_width","object":"s4","line_width_pt":1.0}),
            "invalid_command",
            Some("object_kind_mismatch"),
        ),
        (
            json!({"op":"set_graphic_line_width","object":"s99","line_width_pt":1.0}),
            "invalid_command",
            Some("object_unknown"),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let result = send(
            &app,
            &format!("bad-{index}"),
            "document.transaction",
            json!({"document_id":id,"expected_revision":1,"commands":[{"op":"rename","title":"rollback"},command]}),
        );
        assert_eq!(result["diagnostics"][0]["code"], code, "{result}");
        if let Some(reason) = reason {
            assert_eq!(result["diagnostics"][0]["details"]["reason"], reason);
        }
        assert_eq!(app.document(&id).unwrap(), before);
    }
}

#[test]
fn graphics_clipboard_preserves_override_in_v3_and_null_restoration_keeps_exact_v2_payload() {
    let (app, _root, id) = setup();
    let copy = |request: &str| {
        send(
            &app,
            request,
            "document.clipboard_scene_copy",
            json!({"document_id":id,"selection":{"objects":["s4"]}}),
        )
    };
    let original = copy("plain-copy");
    assert_eq!(original["outcome"], "ok", "{original}");
    let payload: Value =
        serde_json::from_str(original["result"]["payload"].as_str().unwrap()).unwrap();
    assert_eq!(payload["version"], 2);
    for object in payload["objects"].as_array().unwrap() {
        assert!(!object.as_object().unwrap().contains_key("line_width_pt"));
    }
    app.apply_transaction(
        &id,
        None,
        1,
        commands(json!([{"op":"set_graphic_line_width","object":"s2","line_width_pt":1.2}])),
    )
    .unwrap();
    let before = view(&app, &id);
    let styled = copy("styled-copy");
    assert_eq!(styled["outcome"], "ok", "{styled}");
    assert_eq!(styled["loss_report"], json!([]));
    let styled_payload: Value =
        serde_json::from_str(styled["result"]["payload"].as_str().unwrap()).unwrap();
    assert_eq!(styled_payload["version"], 3);
    let copied_graphic = |id: &str| {
        styled_payload["objects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|object| object["id"] == id)
            .unwrap()
    };
    assert_eq!(copied_graphic("s2")["line_width_pt"], 1.2);
    assert_eq!(copied_graphic("s3")["line_width_pt"], Value::Null);
    assert!(
        copied_graphic("s3")
            .as_object()
            .unwrap()
            .contains_key("line_width_pt")
    );
    assert_eq!(view(&app, &id), before);
    app.apply_transaction(
        &id,
        None,
        2,
        commands(json!([{"op":"set_graphic_line_width","object":"s2","line_width_pt":null}])),
    )
    .unwrap();
    let restored = copy("restored-copy");
    assert_eq!(restored["result"]["payload"], original["result"]["payload"]);
    let mut malformed = payload.clone();
    malformed["objects"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|object| object["kind"] == "graphic")
        .unwrap()["line_width_pt"] = Value::Null;
    let before = view(&app, &id);
    let refused = send(
        &app,
        "injected-style",
        "document.clipboard_scene_paste",
        json!({"document_id":id,"expected_revision":3,"payload":malformed.to_string(),"offset":{"x":0,"y":0}}),
    );
    assert_eq!(refused["outcome"], "error", "{refused}");
    assert_eq!(view(&app, &id), before);
}
