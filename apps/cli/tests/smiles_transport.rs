//! SMILES through the CLI's `operations` stream: the entry adapter connects the codec.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use serde_json::{Value, json};

#[test]
fn smiles_is_previewed_and_written_through_the_operations_stream() {
    let dir = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_chemifree"))
        .env_remove("CHEMIFREE_MODEL_DATA_ROOT")
        .env_remove("CHEMIFREE_MODEL_TRUSTED_REGISTRY")
        .arg("--data-dir")
        .arg(dir.path())
        .arg("operations")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut call = |request_id: &str, operation: &str, parameters: Value| -> Value {
        let request = json!({"contract":1,"request_id":request_id,"profile":"preserve-semantics","operation":operation,"parameters":parameters});
        writeln!(stdin, "{request}").unwrap();
        stdin.flush().unwrap();
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap()
    };
    let created = call("create", "document.create", json!({}));
    let id = created["result"]["document"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let preview = call(
        "preview",
        "document.import",
        json!({"format":"smiles","document_id":id,"expected_revision":0,"source":"c1ccccc1O","preview":true}),
    );
    assert_eq!(preview["outcome"], "partial", "{preview}");
    assert_eq!(preview["result"]["atoms"], 7);
    let digest = preview["result"]["preview_digest"].clone();
    let committed = call(
        "commit",
        "document.import",
        json!({"format":"smiles","document_id":id,"expected_revision":0,"source":"c1ccccc1O","preview_digest":digest}),
    );
    assert_eq!(committed["document_revision"], 1, "{committed}");
    let written = call(
        "write",
        "document.export",
        json!({"format":"smiles","document_id":id,"style":"canonical"}),
    );
    assert_eq!(written["result"]["content"], "c1(ccccc1)O", "{written}");
    assert_eq!(written["model_id"], "chemifree-smiles-canonical-1");
    let refused = call(
        "stereo",
        "document.import",
        json!({"format":"smiles","document_id":id,"expected_revision":1,"source":"C[C@H](O)N","preview":true}),
    );
    assert_eq!(refused["diagnostics"][0]["code"], "stereo_unsupported");
    drop(stdin);
    assert!(child.wait().unwrap().success());
}
