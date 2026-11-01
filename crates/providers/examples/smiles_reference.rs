//! Executable boundary for the independent RDKit comparison of the SMILES operations.
//!
//! Reads one SMILES per line from a file and prints one JSON object per line with the recorded
//! and canonical results, or the reason each was refused. No reference code is linked.
use std::error::Error;
use std::fs;

use chemifree_application::Application;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities, SmilesTextCodec};
use serde_json::{Value, json};

fn call(
    app: &Application<LocalCapabilities, FileDocumentRepository>,
    index: usize,
    operation: &str,
    parameters: Value,
) -> Result<Value, Box<dyn Error>> {
    let request = json!({"contract":1,"request_id":format!("ref-{index}-{}", operation.replace('.', "-")),"profile":"preserve-semantics","operation":operation,"parameters":parameters});
    Ok(serde_json::to_value(
        app.execute(&serde_json::to_vec(&request)?),
    )?)
}

fn outcome(reply: &Value) -> Value {
    match reply["result"]["smiles"].as_str() {
        Some(text) => json!({"smiles": text}),
        None => json!({"refused": reply["diagnostics"][0]["details"]["reason"]}),
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let path = std::env::args_os().nth(1).ok_or("expected a path")?;
    let text = fs::read_to_string(path)?;
    let directory = tempfile::tempdir()?;
    let app = Application::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    )
    .with_smiles_codec(SmilesTextCodec);
    for (index, line) in text.lines().enumerate() {
        let id = app.create_document(None, Some("Reference"))?.document.id;
        let base = json!({"document_id":id,"expected_revision":0,"format":"smiles","source":line});
        let mut preview = base.clone();
        preview["preview"] = json!(true);
        let reply = call(&app, index, "document.import", preview)?;
        if reply["result"].is_null() {
            let details = &reply["diagnostics"][0]["details"];
            println!(
                "{}",
                json!({"input": line, "read_error": {"reason": details["reason"], "start": details["start"], "end": details["end"]}})
            );
            call(
                &app,
                index,
                "document.close",
                json!({"document_id":id,"expected_revision":0,"discard_unsaved":true}),
            )?;
            continue;
        }
        let mut commit = base;
        commit["preview_digest"] = reply["result"]["preview_digest"].clone();
        commit["accept_loss"] = json!(true);
        call(&app, index, "document.import", commit)?;
        let export = |mode: &str| {
            call(
                &app,
                index,
                "document.export",
                json!({"document_id":id,"format":"smiles","mode":mode}),
            )
        };
        println!(
            "{}",
            json!({"input": line, "recorded": outcome(&export("recorded")?), "canonical": outcome(&export("canonical")?)})
        );
        call(
            &app,
            index,
            "document.close",
            json!({"document_id":id,"expected_revision":1,"discard_unsaved":true}),
        )?;
    }
    Ok(())
}
