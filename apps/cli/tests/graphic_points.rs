//! CLI operations share graphic point transactions, replay and undo with the application.

use std::io::Write;
use std::process::{Command, Stdio};

use chemifree_application::Application;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};

fn request(id: &str, operation: &str, parameters: Value) -> Value {
    json!({"contract":1,"request_id":id,"profile":"preserve-semantics","operation":operation,"parameters":parameters})
}

#[test]
fn cli_updates_arrow_points_replays_refuses_wrong_kind_and_undo_restores_original() {
    let root = tempfile::tempdir().unwrap();
    let app = Application::new(LocalCapabilities, FileDocumentRepository::new(root.path()));
    let id = app
        .create_document(None, Some("CLI points"))
        .unwrap()
        .document
        .id;
    let original = json!([{"x":0.0,"y":0.0},{"x":20.0,"y":10.0}]);
    app.apply_transaction(
        &id,
        None,
        0,
        serde_json::from_value(json!([{"op":"add_graphic","shape":"arrow","points":original}]))
            .unwrap(),
    )
    .unwrap();
    app.save(&id, None, 1).unwrap();
    let updated = json!([{"x":5.0,"y":6.0},{"x":5.0,"y":6.0}]);
    let update = request(
        "update",
        "document.transaction",
        json!({"document_id":id,"expected_revision":1,"commands":[{"op":"update_graphic_points","object":"s2","points":updated}]}),
    );
    let requests = [
        update.clone(),
        update,
        request(
            "bad",
            "document.transaction",
            json!({"document_id":id,"expected_revision":2,"commands":[{"op":"update_graphic_points","object":"s1","points":updated}]}),
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
    assert_eq!(replies.len(), 4);
    assert_eq!(replies[0]["outcome"], "ok", "{}", replies[0]);
    assert_eq!(replies[0], replies[1]);
    assert_eq!(
        replies[0]["result"]["document"]["scene"]["objects"][1]["shape"],
        "arrow"
    );
    assert_eq!(
        replies[0]["result"]["document"]["scene"]["objects"][1]["points"],
        updated
    );
    assert_eq!(
        replies[2]["diagnostics"][0]["details"]["reason"],
        "object_kind_mismatch"
    );
    assert_eq!(
        replies[3]["result"]["document"]["scene"]["objects"][1]["points"],
        original
    );
}
