//! The public operation path retains chemistry while Et is compacted and reports format limits.

use std::sync::atomic::{AtomicU64, Ordering};

use chemifree_application::Application;
use chemifree_providers::interchange_codec::LocalInterchangeCodec;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities, OpenSmilesCodec, V2000Codec};
use serde_json::{Value, json};

type App = Application<LocalCapabilities, FileDocumentRepository>;
fn send(app: &App, request_id: &str, operation: &str, parameters: Value) -> Value {
    serde_json::to_value(app.execute(json!({"contract":1,"request_id":request_id,"profile":"preserve-semantics","operation":operation,"parameters":parameters}).to_string().as_bytes())).unwrap()
}
fn call(app: &App, operation: &str, parameters: Value) -> Value {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    send(
        app,
        &format!("ethyl-{}", NEXT.fetch_add(1, Ordering::Relaxed)),
        operation,
        parameters,
    )
}
fn setup() -> (App, tempfile::TempDir, String) {
    let root = tempfile::tempdir().unwrap();
    let app = Application::new(LocalCapabilities, FileDocumentRepository::new(root.path()))
        .with_ctfile_codec(V2000Codec)
        .with_smiles_codec(OpenSmilesCodec)
        .with_interchange_codec(LocalInterchangeCodec);
    let created = call(&app, "document.create", json!({"title":"Et drawing"}));
    let id = created["result"]["document"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let drawn = call(
        &app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":0,"commands":[
            {"op":"add_atom","element":"O","position":{"x":0,"y":0}},
            {"op":"add_atom","element":"C","position":{"x":30,"y":0},"bond_to":{"atom":"$0","order":"single"}},
            {"op":"add_atom","element":"C","position":{"x":45,"y":25},"bond_to":{"atom":"$1","order":"single"}}
        ]}),
    );
    assert_eq!(drawn["outcome"], "ok", "{drawn}");
    (app, root, id)
}
fn collapse(id: &str) -> Value {
    json!({"document_id":id,"expected_revision":1,"commands":[{"op":"collapse_ethyl","instance":"s1","anchor":"a2","terminal":"a3"}]})
}
fn snapshot(app: &App, id: &str) -> Value {
    call(app, "document.snapshot", json!({"document_id":id}))["result"].clone()
}

#[test]
fn replay_save_reopen_recovery_and_history_keep_the_exact_graph_and_display() {
    let (app, root, id) = setup();
    let original = snapshot(&app, &id);
    let parameters = collapse(&id);
    let collapsed = send(
        &app,
        "collapse-et",
        "document.transaction",
        parameters.clone(),
    );
    assert_eq!(collapsed["outcome"], "ok", "{collapsed}");
    assert_eq!(
        collapsed["result"]["document"]["abbreviations"],
        json!([{"instance":"s1","anchor":"a2","terminal":"a3"}])
    );
    assert_eq!(
        collapsed["result"]["document"]["atoms"],
        original["document"]["atoms"]
    );
    assert_eq!(
        collapsed["result"]["document"]["bonds"],
        original["document"]["bonds"]
    );
    assert_eq!(
        collapsed["result"]["document"]["scene"],
        original["document"]["scene"]
    );
    assert_eq!(
        send(&app, "collapse-et", "document.transaction", parameters),
        collapsed
    );
    app.save(&id, Some("save-et"), 2).unwrap();
    app.close(&id, Some("close-et"), false, Some(2)).unwrap();
    assert_eq!(
        serde_json::to_value(app.document(&id).unwrap()).unwrap()["document"],
        collapsed["result"]["document"]
    );
    let expanded = call(
        &app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":2,"commands":[{"op":"expand_ethyl","instance":"s1","anchor":"a2"}]}),
    );
    assert_eq!(expanded["result"]["document"]["abbreviations"], json!([]));
    app.autosave(&id, Some("draft-et")).unwrap();
    drop(app);
    let app = Application::new(LocalCapabilities, FileDocumentRepository::new(root.path()));
    app.document(&id).unwrap();
    let recovered = app.recover(&id, Some("recover-et")).unwrap();
    assert_eq!(recovered.document.abbreviations.len(), 0);
    let undone = call(
        &app,
        "history.undo",
        json!({"document_id":id,"expected_revision":recovered.document.revision}),
    );
    assert_eq!(
        undone["result"]["document"]["abbreviations"],
        collapsed["result"]["document"]["abbreviations"]
    );
    assert_eq!(
        undone["result"]["document"]["atoms"],
        original["document"]["atoms"]
    );
}

#[test]
fn all_writers_emit_the_full_graph_and_a_rendering_loss_without_mutating_the_document() {
    let (app, _root, id) = setup();
    let mol = call(
        &app,
        "document.export",
        json!({"document_id":id,"format":"mol"}),
    );
    let smiles = call(
        &app,
        "document.export",
        json!({"document_id":id,"format":"smiles"}),
    );
    assert!(
        mol["result"]["content"]
            .as_str()
            .is_some_and(|text| text.contains("  3  2")),
        "{mol}"
    );
    assert!(
        smiles["result"]["content"]
            .as_str()
            .is_some_and(|text| text.contains('O') && text.matches('C').count() == 2),
        "{smiles}"
    );
    call(&app, "document.transaction", collapse(&id));
    let formula = call(
        &app,
        "chem.properties",
        json!({"document_id":id,"scope":"molecule","property":"formula"}),
    );
    assert_eq!(formula["result"]["components"][0]["formula"], "C2H6O");
    let before = snapshot(&app, &id);
    for format in ["mol", "sdf", "smiles", "cdxml", "cdx"] {
        let exported = call(
            &app,
            "document.export",
            json!({"document_id":id,"format":format}),
        );
        assert_eq!(exported["outcome"], "partial", "{format}: {exported}");
        let losses: Vec<&Value> = exported["loss_report"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|loss| loss["feature"] == "abbreviation_display")
            .collect();
        assert_eq!(losses.len(), 1, "{format}: {exported}");
        assert_eq!(losses[0]["semantic_impact"], "rendering");
        assert_eq!(losses[0]["disposition"], "dropped");
        if format == "mol" {
            assert_eq!(exported["result"]["content"], mol["result"]["content"]);
        }
        if format == "smiles" {
            assert_eq!(exported["result"]["content"], smiles["result"]["content"]);
        }
        assert_eq!(snapshot(&app, &id), before);
    }
}

#[test]
fn native_copy_and_bad_commands_have_typed_refusals_and_no_side_effects() {
    let (app, _root, id) = setup();
    let before = snapshot(&app, &id);
    let invalid = call(
        &app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":1,"commands":[{"op":"rename","title":"roll back"},{"op":"collapse_ethyl","instance":"s1","anchor":"a3","terminal":"a2"}]}),
    );
    assert_eq!(
        invalid["diagnostics"][0]["details"]["reason"], "abbreviation_invalid",
        "{invalid}"
    );
    assert_eq!(invalid["diagnostics"][0]["details"]["command_index"], 1);
    assert_eq!(snapshot(&app, &id), before);
    let malformed = call(
        &app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":1,"commands":[{"op":"collapse_ethyl","instance":null,"anchor":"a2","terminal":"a3"}]}),
    );
    assert_eq!(malformed["diagnostics"][0]["code"], "invalid_request");
    call(&app, "document.transaction", collapse(&id));
    let before = snapshot(&app, &id);
    let copied = call(
        &app,
        "document.clipboard_copy",
        json!({"document_id":id,"instance":"s1","selection":{"atoms":["a2","a3"],"bonds":["b2"]}}),
    );
    assert_eq!(
        copied["diagnostics"][0]["code"], "clipboard_abbreviation_display_unsupported",
        "{copied}"
    );
    assert_eq!(snapshot(&app, &id), before);
}
