//! CLI operations use the same graph-retaining fragment contract and explicit display text.

use std::io::Write;
use std::process::{Command, Stdio};

use chemifree_application::Application;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};

fn request(id: &str, operation: &str, parameters: Value) -> Value {
    json!({"contract":1,"request_id":id,"profile":"preserve-semantics","operation":operation,"parameters":parameters})
}

#[test]
fn cli_contracts_edits_text_refuses_nested_members_and_expands_without_graph_changes() {
    let root = tempfile::tempdir().unwrap();
    let app = Application::new(LocalCapabilities, FileDocumentRepository::new(root.path()));
    let created: Value = serde_json::to_value(
        app.execute(
            request("create", "document.create", json!({"title":"CLI graph"}))
                .to_string()
                .as_bytes(),
        ),
    )
    .unwrap();
    let id = created["result"]["document"]["id"].as_str().unwrap();
    let drawn:Value=serde_json::to_value(app.execute(request("draw","document.transaction",json!({"document_id":id,"expected_revision":0,"commands":[
        {"op":"add_atom","element":"C","position":{"x":0,"y":0}},
        {"op":"add_atom","element":"O","position":{"x":30,"y":0},"bond_to":{"atom":"$0","order":"single"}},
        {"op":"add_atom","element":"C","position":{"x":60,"y":0},"bond_to":{"atom":"$1","order":"single"}}
    ]})).to_string().as_bytes())).unwrap();
    app.save(id, Some("seed-cli"), 1).unwrap();
    let requests = [
        request(
            "contract",
            "document.transaction",
            json!({"document_id":id,"expected_revision":1,"commands":[{"op":"contract_fragment","instance":"s1","anchor":"a2","members":["a2","a3"],"text":"OMe"}]}),
        ),
        request(
            "text",
            "document.transaction",
            json!({"document_id":id,"expected_revision":2,"commands":[{"op":"set_fragment_display_text","instance":"s1","anchor":"a2","text":"Ph 한 e\u{301} 🧪"}]}),
        ),
        request(
            "bad",
            "document.transaction",
            json!({"document_id":id,"expected_revision":3,"commands":[{"op":"contract_fragment","instance":"s1","anchor":"a3","members":["a3"],"text":"nested"}]}),
        ),
        request(
            "expand",
            "document.transaction",
            json!({"document_id":id,"expected_revision":3,"commands":[{"op":"expand_fragment","instance":"s1","anchor":"a2"}]}),
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
    assert_eq!(
        replies[0]["result"]["document"]["fragment_displays"][0]["text"],
        "OMe"
    );
    assert_eq!(
        replies[1]["result"]["document"]["fragment_displays"][0]["text"],
        "Ph 한 e\u{301} 🧪"
    );
    assert_eq!(
        replies[0]["result"]["document"]["fragment_displays"][0]["runs"],
        json!([{"text":"OMe","script":"baseline"}])
    );
    assert_eq!(
        replies[1]["result"]["document"]["fragment_displays"][0]["runs"],
        json!([{"text":"Ph 한 e\u{301} 🧪","script":"baseline"}])
    );
    assert_eq!(
        replies[2]["diagnostics"][0]["details"]["reason"],
        "fragment_display_invalid"
    );
    assert_eq!(
        replies[3]["result"]["document"]["fragment_displays"],
        json!([])
    );
    for reply in [&replies[0], &replies[1], &replies[3]] {
        for field in ["atoms", "bonds", "scene", "stereo", "settings"] {
            assert_eq!(
                reply["result"]["document"][field],
                drawn["result"]["document"][field]
            );
        }
    }
}
