use chemifree_application::Application;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};

type App = Application<LocalCapabilities, FileDocumentRepository>;

fn call(app: &App, id: &str, operation: &str, parameters: Value) -> Value {
    let request = json!({"contract":1,"request_id":id,"profile":"preserve-semantics","operation":operation,"parameters":parameters});
    serde_json::to_value(app.execute(request.to_string().as_bytes())).unwrap()
}

fn preview(app: &App, document_id: &str, source: &str) -> Value {
    call(
        app,
        "preview",
        "chem.smiles_read",
        json!({
            "document_id":document_id,"expected_revision":0,"source":source,"preview":true
        }),
    )
}

#[test]
fn preview_commit_replay_undo_and_canonical_export_share_the_exact_graph() {
    let directory = tempfile::tempdir().unwrap();
    let app = App::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    );
    let id = app
        .create_document(None, Some("SMILES"))
        .unwrap()
        .document
        .id;
    let before = serde_json::to_value(app.document(&id).unwrap()).unwrap();
    let report = preview(&app, &id, "[13CH3-].[NH4+]");
    assert_eq!(report["outcome"], "partial", "{report}");
    assert_eq!(report["model_id"], "chemifree-smiles-basic-1");
    assert_eq!(report["result"]["snapshot"]["document"], before["document"]);
    assert_eq!(report["result"]["atoms"], 2);
    assert_eq!(report["result"]["bonds"], 0);
    assert_eq!(report["loss_report"][0]["feature"], "generated_positions");
    assert_eq!(
        serde_json::to_value(app.document(&id).unwrap()).unwrap(),
        before
    );
    let parameters = json!({"document_id":id,"expected_revision":0,"source":"[13CH3-].[NH4+]",
        "preview_digest":report["result"]["preview_digest"],"accept_loss":true});
    let imported = call(&app, "import", "chem.smiles_read", parameters.clone());
    assert_eq!(imported["outcome"], "partial", "{imported}");
    assert_eq!(imported["document_revision"], 1);
    assert_eq!(imported["result"]["document"]["atoms"][0]["isotope"], 13);
    assert_eq!(imported["result"]["document"]["atoms"][0]["charge"], -1);
    assert_eq!(imported["result"]["document"]["atoms"][1]["charge"], 1);
    assert_eq!(
        call(&app, "import", "chem.smiles_read", parameters),
        imported
    );
    let exported = call(
        &app,
        "write",
        "chem.smiles_write",
        json!({"document_id":id,"mode":"canonical"}),
    );
    assert_eq!(exported["outcome"], "partial", "{exported}");
    assert_eq!(exported["model_id"], "chemifree-smiles-canonical-1");
    assert_eq!(
        exported["result"]["snapshot"]["document"],
        imported["result"]["document"]
    );
    let text = exported["result"]["text"].as_str().unwrap();
    assert!(text.contains("13C") && text.contains("NH4+"), "{text}");
    let undone = call(
        &app,
        "undo",
        "history.undo",
        json!({"document_id":id,"expected_revision":1}),
    );
    assert_eq!(undone["result"]["document"]["atoms"], json!([]));
}

#[test]
fn syntax_spans_preview_consent_and_restart_do_not_mutate_a_drawing() {
    let directory = tempfile::tempdir().unwrap();
    let app = App::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    );
    let id = app
        .create_document(None, Some("SMILES"))
        .unwrap()
        .document
        .id;
    let before = serde_json::to_value(app.document(&id).unwrap()).unwrap();
    let invalid = preview(&app, &id, "[Cé]");
    assert_eq!(invalid["diagnostics"][0]["code"], "invalid_bracket_atom");
    assert_eq!(invalid["diagnostics"][0]["details"]["start_byte"], 2);
    assert_eq!(invalid["diagnostics"][0]["details"]["end_byte"], 4);
    assert!(invalid["result"].is_null());
    let report = preview(&app, &id, "C1=CC=CC=C1");
    let digest = report["result"]["preview_digest"].clone();
    let without_consent = call(
        &app,
        "without-consent",
        "chem.smiles_read",
        json!({
            "document_id":id,"expected_revision":0,"source":"C1=CC=CC=C1","preview_digest":digest
        }),
    );
    assert_eq!(
        without_consent["diagnostics"][0]["code"],
        "loss_consent_required"
    );
    assert_eq!(
        serde_json::to_value(app.document(&id).unwrap()).unwrap(),
        before
    );
    drop(app);
    let restarted = App::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    );
    let stale = call(
        &restarted,
        "stale",
        "chem.smiles_read",
        json!({
            "document_id":id,"expected_revision":0,"source":"C1=CC=CC=C1",
            "preview_digest":digest,"accept_loss":true
        }),
    );
    assert_eq!(stale["diagnostics"][0]["code"], "preview_changed");
    assert_eq!(
        serde_json::to_value(restarted.document(&id).unwrap()).unwrap(),
        before
    );
}

#[test]
fn unknown_radical_is_reported_and_stereo_is_refused() {
    let directory = tempfile::tempdir().unwrap();
    let app = App::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    );
    let id = app.create_document(None, None).unwrap().document.id;
    let radical = preview(&app, &id, "[CH3]");
    assert_eq!(
        radical["result"]["document"]["atoms"][0]["radical"],
        "unknown"
    );
    assert!(
        radical["loss_report"]
            .as_array()
            .unwrap()
            .iter()
            .any(|loss| loss["feature"] == "radical_state")
    );
    let stereo = preview(&app, &id, "C/C=C/C");
    assert_eq!(stereo["diagnostics"][0]["code"], "stereo_unsupported");
    assert!(stereo["result"].is_null());
    assert_eq!(app.document(&id).unwrap().document.revision, 0);
}
