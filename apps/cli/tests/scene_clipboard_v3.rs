//! CLI operation streaming keeps clipboard v3 width through replay, copy and history.

use std::io::Write;
use std::process::{Command, Stdio};

use chemifree_application::Application;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};

fn request(id: &str, operation: &str, parameters: Value) -> Value {
    json!({"contract":1,"request_id":id,"profile":"preserve-semantics","operation":operation,"parameters":parameters})
}

#[test]
fn cli_paste_replay_copy_and_undo_redo_preserve_explicit_clipboard_width() {
    let root = tempfile::tempdir().unwrap();
    let app = Application::new(LocalCapabilities, FileDocumentRepository::new(root.path()));
    let id = app
        .create_document(None, Some("v3 CLI"))
        .unwrap()
        .document
        .id;
    let seeded = app
        .apply_transaction(
            &id,
            None,
            0,
            serde_json::from_value(
                json!([{"op":"add_atom","element":"C","position":{"x":3,"y":4}}]),
            )
            .unwrap(),
        )
        .unwrap();
    let document = serde_json::to_value(&seeded).unwrap();
    app.save(&id, None, 1).unwrap();
    let payload = json!({"format":"chemifree.clipboard","version":3,"kind":"graphics","settings":document["document"]["settings"],"roots":["s6"],"objects":[{"id":"s6","kind":"graphic","shape":"rectangle","points":[{"x":0,"y":0},{"x":10,"y":10}],"line_width_pt":2.75}],"references":[]});
    let paste = request(
        "paste",
        "document.clipboard_scene_paste",
        json!({"document_id":id,"expected_revision":1,"payload":payload.to_string(),"offset":{"x":20,"y":30}}),
    );
    let requests = [
        paste.clone(),
        paste,
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
        request(
            "redo",
            "history.redo",
            json!({"document_id":id,"expected_revision":3}),
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
    assert_eq!(replies[0]["outcome"], "ok", "{}", replies[0]);
    assert_eq!(replies[0], replies[1]);
    let graphic = &replies[0]["result"]["snapshot"]["document"]["scene"]["objects"][1];
    assert_eq!(graphic["line_width_pt"], 2.75);
    assert_eq!(
        graphic["points"],
        json!([{"x":20.0,"y":30.0},{"x":30.0,"y":40.0}])
    );
    let copied: Value =
        serde_json::from_str(replies[2]["result"]["payload"].as_str().unwrap()).unwrap();
    assert_eq!(copied["version"], 3);
    assert_eq!(copied["objects"][0]["line_width_pt"], 2.75);
    assert_eq!(
        replies[3]["result"]["document"]["scene"]["objects"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        replies[4]["result"]["document"]["scene"]["objects"][1]["line_width_pt"],
        2.75
    );
    assert_eq!(
        replies[4]["result"]["document"]["atoms"],
        document["document"]["atoms"]
    );
}
