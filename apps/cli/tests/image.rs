//! CLI image creation and geometry edits preserve original PNG source.

use std::io::Write;
use std::process::{Command, Stdio};

use chemifree_application::Application;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};

fn request(id: &str, operation: &str, parameters: Value) -> Value {
    json!({"contract":1,"request_id":id,"profile":"preserve-semantics","operation":operation,"parameters":parameters})
}

const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DQAAAEgQGALFXOsAAAAABJRU5ErkJggg==";
#[test]
fn cli_embedded_png_keeps_source_through_geometry_replay_undo_and_native_save() {
    let root = tempfile::tempdir().unwrap();
    let app = Application::new(LocalCapabilities, FileDocumentRepository::new(root.path()));
    let id = app
        .create_document(None, Some("Image CLI"))
        .unwrap()
        .document
        .id;
    app.save(&id, None, 0).unwrap();
    let edit = request(
        "edit",
        "document.transaction",
        json!({"document_id":id,"expected_revision":1,"commands":[{"op":"update_image_geometry","object":"s2","origin":{"x":5,"y":6},"size_pt":{"x":7,"y":8}}]}),
    );
    let requests = [
        request(
            "add",
            "document.transaction",
            json!({"document_id":id,"expected_revision":0,"commands":[{"op":"add_image","mime_type":"image/png","data_base64":PNG,"origin":{"x":1,"y":2},"size_pt":{"x":30,"y":40}}]}),
        ),
        edit.clone(),
        edit,
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
        .split(|b| *b == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(replies.len(), 4);
    assert_eq!(replies[1]["outcome"], "ok", "{}", replies[1]);
    assert_eq!(replies[1], replies[2]);
    assert_eq!(
        replies[1]["result"]["document"]["scene"]["objects"][1]["data_base64"],
        PNG
    );
    assert_eq!(
        replies[3]["result"]["document"]["scene"]["objects"][1]["origin"],
        json!({"x":1.,"y":2.})
    );
}
