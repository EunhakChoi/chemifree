//! CLI operations preserve ellipse corners and width through transforms, copy and undo.

use std::io::Write;
use std::process::{Command, Stdio};

use chemifree_application::Application;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};

fn request(id: &str, operation: &str, parameters: Value) -> Value {
    json!({"contract":1,"request_id":id,"profile":"preserve-semantics","operation":operation,"parameters":parameters})
}

#[test]
fn cli_rejects_arbitrary_ellipse_angle_and_replays_quarter_turn_v4_copy_and_undo() {
    let root = tempfile::tempdir().unwrap();
    let app = Application::new(LocalCapabilities, FileDocumentRepository::new(root.path()));
    let id = app
        .create_document(None, Some("Ellipse CLI"))
        .unwrap()
        .document
        .id;
    let original = json!([{"x":-4.0,"y":-2.0},{"x":6.0,"y":8.0}]);
    let seeded=app.apply_transaction(&id,None,0,serde_json::from_value(json!([{"op":"add_atom","element":"C","position":{"x":2,"y":3}},{"op":"add_graphic","shape":"ellipse","points":original},{"op":"set_graphic_line_width","object":"s2","line_width_pt":0.1}])).unwrap()).unwrap();
    let seeded = serde_json::to_value(&seeded).unwrap();
    app.save(&id, None, 1).unwrap();
    let rotate = |degrees: f64| json!({"op":"transform_selection","selection":{"objects":["s2"]},"pivot":{"x":0,"y":0},"transform":{"type":"rotate","degrees":degrees}});
    let valid = request(
        "quarter",
        "document.transaction",
        json!({"document_id":id,"expected_revision":1,"commands":[rotate(90.0)]}),
    );
    let requests = [
        request(
            "bad",
            "document.transaction",
            json!({"document_id":id,"expected_revision":1,"commands":[rotate(37.0)]}),
        ),
        valid.clone(),
        valid,
        request(
            "copy",
            "document.clipboard_scene_copy",
            json!({"document_id":id,"selection":{"objects":["s2"]}}),
        ),
        request(
            "undo",
            "history.undo",
            json!({"document_id":id,"expected_revision":2}),
        ),
    ];
    let mut child = Command::new(env!("CARGO_BIN_EXE_chemifree"))
        .env_remove("CHEMIFREE_MODEL_DATA_ROOT")
        .env_remove("CHEMIFREE_MODEL_TRUSTED_REGISTRY")
        .arg("--data-dir")
        .arg(root.path())
        .arg("operations")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    for value in requests {
        writeln!(input, "{value}").unwrap();
    }
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let replies: Vec<Value> = output
        .stdout
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).unwrap())
        .collect();
    assert_eq!(replies.len(), 5);
    assert_eq!(
        replies[0]["diagnostics"][0]["details"]["reason"],
        "transform_ellipse_unsupported"
    );
    assert_eq!(replies[1]["outcome"], "ok", "{}", replies[1]);
    assert_eq!(replies[1], replies[2]);
    assert_eq!(
        replies[1]["result"]["document"]["scene"]["objects"][1]["shape"],
        "ellipse"
    );
    let payload: Value =
        serde_json::from_str(replies[3]["result"]["payload"].as_str().unwrap()).unwrap();
    assert_eq!(payload["version"], 4);
    assert_eq!(payload["objects"][0]["line_width_pt"], 0.1);
    assert_eq!(
        replies[4]["result"]["document"]["scene"]["objects"][1]["points"],
        original
    );
    assert_eq!(
        replies[4]["result"]["document"]["atoms"],
        seeded["document"]["atoms"]
    );
}
