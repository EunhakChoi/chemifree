//! CLI operations share explicit graphic width, inheritance and history with HTTP.

use std::io::Write;
use std::process::{Command, Stdio};

use chemifree_application::Application;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};

fn request(id: &str, operation: &str, parameters: Value) -> Value {
    json!({"contract":1,"request_id":id,"profile":"preserve-semantics","operation":operation,"parameters":parameters})
}

#[test]
fn cli_sets_replays_resets_and_undoes_graphic_width_without_changing_geometry() {
    let root = tempfile::tempdir().unwrap();
    let app = Application::new(LocalCapabilities, FileDocumentRepository::new(root.path()));
    let id = app
        .create_document(None, Some("CLI width"))
        .unwrap()
        .document
        .id;
    let points = json!([{"x":0.0,"y":0.0},{"x":20.0,"y":10.0}]);
    app.apply_transaction(
        &id,
        None,
        0,
        serde_json::from_value(json!([{"op":"add_graphic","shape":"arrow","points":points}]))
            .unwrap(),
    )
    .unwrap();
    app.save(&id, None, 1).unwrap();
    let update = request(
        "override",
        "document.transaction",
        json!({"document_id":id,"expected_revision":1,"commands":[{"op":"set_graphic_line_width","object":"s2","line_width_pt":4.0}]}),
    );
    let requests = [
        update.clone(),
        update,
        request(
            "inherit",
            "document.transaction",
            json!({"document_id":id,"expected_revision":2,"commands":[{"op":"set_graphic_line_width","object":"s2","line_width_pt":null}]}),
        ),
        request(
            "undo",
            "history.undo",
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
    assert_eq!(replies.len(), 4);
    assert_eq!(replies[0]["outcome"], "ok", "{}", replies[0]);
    assert_eq!(replies[0], replies[1]);
    for (index, width) in [(0, json!(4.0)), (2, Value::Null), (3, json!(4.0))] {
        let object = &replies[index]["result"]["document"]["scene"]["objects"][1];
        assert_eq!(object["line_width_pt"], width);
        assert_eq!(object["shape"], "arrow");
        assert_eq!(object["points"], points);
    }
}
