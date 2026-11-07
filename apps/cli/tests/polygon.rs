//! CLI polygon creation and full vertex replacement share replay and history.

use std::io::Write;
use std::process::{Command, Stdio};

use chemifree_application::Application;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};

fn request(id: &str, operation: &str, parameters: Value) -> Value {
    json!({"contract":1,"request_id":id,"profile":"preserve-semantics","operation":operation,"parameters":parameters})
}

#[test]
fn polygon_cli_replays_full_vertex_edits_and_keeps_undo_and_clipboard_v5() {
    let root = tempfile::tempdir().unwrap();
    let app = Application::new(LocalCapabilities, FileDocumentRepository::new(root.path()));
    let id = app
        .create_document(None, Some("Polygon CLI"))
        .unwrap()
        .document
        .id;
    app.save(&id, None, 0).unwrap();
    let vertices = json!([{"x":0.,"y":0.},{"x":2.,"y":4.},{"x":7.,"y":-8.}]);
    let edited = json!([{"x":0.,"y":0.},{"x":2.,"y":4.},{"x":7.,"y":-8.},{"x":9.,"y":11.}]);
    let update = request(
        "update",
        "document.transaction",
        json!({"document_id":id,"expected_revision":1,"commands":[{"op":"update_polygon_vertices","object":"s2","vertices":edited}]}),
    );
    let requests = [
        request(
            "add",
            "document.transaction",
            json!({"document_id":id,"expected_revision":0,"commands":[{"op":"add_polygon","vertices":vertices}]}),
        ),
        update.clone(),
        update,
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
    let replies = output
        .stdout
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(replies.len(), 5);
    assert_eq!(replies[1]["outcome"], "ok", "{}", replies[1]);
    assert_eq!(replies[1], replies[2]);
    assert_eq!(
        replies[1]["result"]["document"]["scene"]["objects"][1]["points"],
        edited
    );
    let payload: Value =
        serde_json::from_str(replies[3]["result"]["payload"].as_str().unwrap()).unwrap();
    assert_eq!(payload["version"], 5);
    assert_eq!(payload["objects"][0]["points"], edited);
    assert_eq!(
        replies[4]["result"]["document"]["scene"]["objects"][1]["points"],
        vertices
    );
}
