//! Prints, for each SMILES on a line of the input file, the shared aromaticity and chemical
//! identity outcome, so two builds of the model can be compared line by line.
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
    let request = json!({"contract":1,"request_id":format!("diff-{index}-{}", operation.replace('.', "-")),"profile":"preserve-semantics","operation":operation,"parameters":parameters});
    Ok(serde_json::to_value(
        app.execute(&serde_json::to_vec(&request)?),
    )?)
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
        let id = app.create_document(None, Some("Diff"))?.document.id;
        let base = json!({"document_id":id,"expected_revision":0,"format":"smiles","source":line});
        let mut preview = base.clone();
        preview["preview"] = json!(true);
        let reply = call(&app, index, "document.import", preview)?;
        if reply["result"].is_null() {
            println!(
                "{line}\tread-error {}",
                reply["diagnostics"][0]["details"]["reason"]
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
        let arom = call(&app, index, "chem.aromaticity", json!({"document_id":id}))?;
        let systems: Vec<String> = arom["result"]["systems"]
            .as_array()
            .map(|systems| {
                systems
                    .iter()
                    .map(|s| {
                        format!(
                            "{}/{}/{}",
                            s["classification"].as_str().unwrap_or("?"),
                            s["reason"].as_str().unwrap_or("-"),
                            s["atoms"].as_array().map_or(0, Vec::len)
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        let identity = call(
            &app,
            index,
            "chem.identity",
            json!({"document_id":id,"mode":"chemical"}),
        )?;
        let chemical = if identity["result"].is_object() {
            "ok".to_owned()
        } else {
            identity["diagnostics"][0]["details"]["reason"]
                .as_str()
                .unwrap_or("?")
                .to_owned()
        };
        println!("{line}\t{}\tidentity:{chemical}", systems.join(" "));
        call(
            &app,
            index,
            "document.close",
            json!({"document_id":id,"expected_revision":1,"discard_unsaved":true}),
        )?;
    }
    Ok(())
}
