//! CLI spline creation and full vertex replacement share replay and history.

use std::io::Write;
use std::process::{Command, Stdio};

use chemifree_application::Application;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};

fn request(id: &str, operation: &str, parameters: Value) -> Value {
    json!({"contract":1,"request_id":id,"profile":"preserve-semantics","operation":operation,"parameters":parameters})
}

#[test]
fn spline_cli_replays_full_vertex_edits_and_keeps_undo_and_clipboard_v6() {
    let root = tempfile::tempdir().unwrap();
    let app = Application::new(LocalCapabilities, FileDocumentRepository::new(root.path()));
    let id = app
        .create_document(None, Some("Spline CLI"))
        .unwrap()
        .document
        .id;
    app.save(&id, None, 0).unwrap();
    let points = json!([{"x":0.,"y":0.},{"x":2.,"y":4.},{"x":7.,"y":-8.},{"x":8.,"y":10.}]);
    let edited = json!([{"x":0.,"y":0.},{"x":2.,"y":4.},{"x":7.,"y":-8.},{"x":9.,"y":11.}]);
    let update = request(
        "update",
        "document.transaction",
        json!({"document_id":id,"expected_revision":1,"commands":[{"op":"update_spline_points","object":"s2","points":edited}]}),
    );
    let requests = [
        request(
            "add",
            "document.transaction",
            json!({"document_id":id,"expected_revision":0,"commands":[{"op":"add_spline","points":points}]}),
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
    assert_eq!(payload["version"], 6);
    assert_eq!(payload["objects"][0]["points"], edited);
    assert_eq!(
        replies[4]["result"]["document"]["scene"]["objects"][1]["points"],
        points
    );
}

#[test]
fn human_document_view_names_defining_point_order_and_prints_the_last_control_point() {
    let root = tempfile::tempdir().unwrap();
    let app = Application::new(LocalCapabilities, FileDocumentRepository::new(root.path()));
    let id = app
        .create_document(None, Some("Cubic human view"))
        .unwrap()
        .document
        .id;
    let commands=serde_json::from_value(json!([{"op":"add_spline","points":[{"x":0,"y":0},{"x":1,"y":2},{"x":3,"y":4},{"x":9,"y":11}]}])).unwrap();
    app.apply_transaction(&id, None, 0, commands).unwrap();
    app.save(&id, None, 1).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_chemifree"))
        .env_remove("CHEMIFREE_MODEL_DATA_ROOT")
        .env_remove("CHEMIFREE_MODEL_TRUSTED_REGISTRY")
        .arg("--data-dir")
        .arg(root.path())
        .args(["documents", "show", &id])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("P0, C1, C2, P3; open cubic"), "{text}");
    assert!(text.contains("(9, 11)"), "{text}");
}
