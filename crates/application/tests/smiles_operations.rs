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
    let losses = exported["loss_report"].as_array().unwrap();
    for (feature, impact) in [
        ("drawing_scene", "document"),
        ("drawing_positions", "geometry"),
        ("drawing_style", "rendering"),
    ] {
        assert!(
            losses
                .iter()
                .any(|loss| loss["feature"] == feature && loss["semantic_impact"] == impact)
        );
    }
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
    let redone = call(
        &app,
        "redo",
        "history.redo",
        json!({"document_id":id,"expected_revision":2}),
    );
    assert_eq!(
        redone["result"]["document"]["atoms"],
        imported["result"]["document"]["atoms"]
    );
    assert_eq!(redone["document_revision"], 3);
    let saved = call(
        &app,
        "save",
        "document.save",
        json!({"document_id":id,"expected_revision":3}),
    );
    assert_eq!(saved["result"]["session"]["saved_revision"], 3);
    drop(app);
    let restarted = App::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    );
    let reopened = serde_json::to_value(restarted.document(&id).unwrap()).unwrap();
    assert_eq!(
        reopened["document"]["atoms"],
        imported["result"]["document"]["atoms"]
    );
    assert_eq!(reopened["session"]["undo_depth"], 1);
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

#[test]
fn canonical_work_limit_returns_a_stable_diagnostic_without_text() {
    let directory = tempfile::tempdir().unwrap();
    let app = App::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    );
    let id = app.create_document(None, None).unwrap().document.id;
    let commands: Vec<_> = (0..129)
        .map(|i| json!({"op":"add_atom","element":"C","position":{"x":i,"y":0}}))
        .collect();
    let edit = call(
        &app,
        "large-graph",
        "document.transaction",
        json!({"document_id":id,"expected_revision":0,"commands":commands}),
    );
    assert_eq!(edit["outcome"], "ok");
    let before = serde_json::to_value(app.document(&id).unwrap()).unwrap();
    let result = call(
        &app,
        "bounded",
        "chem.smiles_write",
        json!({"document_id":id,"mode":"canonical"}),
    );
    assert_eq!(result["outcome"], "error");
    assert_eq!(result["diagnostics"][0]["code"], "smiles_write_limit");
    assert_eq!(result["diagnostics"][0]["details"]["reason"], "atom_limit");
    assert!(result["result"].is_null());
    assert_eq!(
        serde_json::to_value(app.document(&id).unwrap()).unwrap(),
        before
    );
}

#[test]
fn syntax_valid_valence_warnings_preserve_graph_and_distinguish_unsupported_elements() {
    let directory = tempfile::tempdir().unwrap();
    let app = App::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    );
    let id = app.create_document(None, None).unwrap().document.id;
    let before = serde_json::to_value(app.document(&id).unwrap()).unwrap();
    let hypervalent = preview(&app, &id, "C(C)(C)(C)(C)C");
    assert_eq!(
        hypervalent["result"]["atoms"], 6,
        "the syntax graph is retained"
    );
    let warnings = hypervalent["loss_report"].as_array().unwrap();
    let carbon = warnings
        .iter()
        .find(|loss| loss["feature"] == "valence_diagnostic")
        .unwrap();
    assert!(
        carbon["reason"]
            .as_str()
            .unwrap()
            .contains("atom 1 is invalid under chemifree-valence-2 (unusual_valence)")
    );
    assert_eq!(
        hypervalent["diagnostics"][0]["details"],
        json!({"model":"chemifree-valence-2","status":"invalid","reason":"unusual_valence","source_atom_index":1})
    );
    let metal = preview(&app, &id, "[Fe](Cl)(Cl)(Cl)(Cl)(Cl)Cl");
    let warning = metal["loss_report"]
        .as_array()
        .unwrap()
        .iter()
        .find(|loss| loss["feature"] == "valence_diagnostic")
        .unwrap();
    assert!(
        warning["reason"]
            .as_str()
            .unwrap()
            .contains("atom 1 is unsupported under chemifree-valence-2 (element_or_charge)")
    );
    assert_eq!(metal["diagnostics"][0]["details"]["status"], "unsupported");
    assert_eq!(
        metal["diagnostics"][0]["details"]["reason"],
        "element_or_charge"
    );
    let benzene = preview(&app, &id, "c1ccccc1");
    assert!(
        !benzene["loss_report"]
            .as_array()
            .unwrap()
            .iter()
            .any(|loss| loss["feature"] == "valence_diagnostic"),
        "P17 checks supported aromatic input as a whole"
    );
    assert_eq!(
        serde_json::to_value(app.document(&id).unwrap()).unwrap(),
        before
    );

    let parameters = json!({"document_id":id,"expected_revision":0,"source":"C(C)(C)(C)(C)C",
        "preview_digest":hypervalent["result"]["preview_digest"],"accept_loss":true});
    let imported = call(&app, "hypervalent", "chem.smiles_read", parameters);
    assert_eq!(
        imported["result"]["document"]["atoms"]
            .as_array()
            .unwrap()
            .len(),
        6
    );
    let written = call(
        &app,
        "recorded",
        "chem.smiles_write",
        json!({"document_id":id,"mode":"recorded"}),
    );
    assert_eq!(written["outcome"], "partial");
    assert!(
        written["loss_report"]
            .as_array()
            .unwrap()
            .iter()
            .any(|loss| loss["feature"] == "valence_diagnostic"
                && loss["reason"].as_str().unwrap().contains("invalid"))
    );
    let canonical = call(
        &app,
        "canonical",
        "chem.smiles_write",
        json!({"document_id":id,"mode":"canonical"}),
    );
    assert_eq!(canonical["outcome"], "unsupported");
    assert!(canonical["result"].is_null());
    assert_eq!(
        canonical["diagnostics"][0]["details"]["reason"],
        "unusual_valence"
    );
}

fn new_app() -> (tempfile::TempDir, App, String) {
    let directory = tempfile::tempdir().unwrap();
    let app = App::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    );
    let id = app.create_document(None, None).unwrap().document.id;
    (directory, app, id)
}

#[test]
fn text_after_whitespace_is_surfaced_as_an_ignored_loss_and_warning() {
    let (_directory, app, id) = new_app();
    let named = preview(&app, &id, "CCO ethanol");
    assert_eq!(named["outcome"], "partial", "{named}");
    assert_eq!(named["result"]["atoms"], 3);
    let loss = named["loss_report"]
        .as_array()
        .unwrap()
        .iter()
        .find(|loss| loss["feature"] == "ignored_text")
        .unwrap_or_else(|| panic!("no ignored_text loss: {named}"));
    assert_eq!(loss["disposition"], "dropped");
    let warning = named["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["code"] == "ignored_text")
        .unwrap_or_else(|| panic!("no ignored_text diagnostic: {named}"));
    assert_eq!(
        (
            &warning["details"]["start_byte"],
            &warning["details"]["end_byte"]
        ),
        (&json!(3), &json!(11))
    );
    for plain in ["CC(=O)O ", "CCO\n"] {
        let report = preview(&app, &id, plain);
        assert!(
            report["loss_report"]
                .as_array()
                .unwrap()
                .iter()
                .all(|loss| loss["feature"] != "ignored_text"),
            "{plain:?}: {report}"
        );
    }
}

#[test]
fn aromatic_refusal_reports_its_reason_and_the_failing_system_span() {
    let (_directory, app, id) = new_app();
    let report = preview(&app, &id, "c1ccccc1C.c1ccc2[nH]ccc2c1");
    let diagnostic = &report["diagnostics"][0];
    assert_eq!(diagnostic["code"], "aromaticity_outside_model", "{report}");
    assert_eq!(
        diagnostic["details"]["reason"],
        "fused_system_outside_model"
    );
    assert_eq!(diagnostic["details"]["start_byte"], 10);
}

#[test]
fn oversized_import_is_refused_before_the_whole_source_is_parsed() {
    let (_directory, app, id) = new_app();
    let report = preview(&app, &id, &"C".repeat(400));
    assert_eq!(
        report["diagnostics"][0]["code"], "resource_limit",
        "{report}"
    );
    assert!(report["result"].is_null());
}

#[test]
fn undetermined_radical_wording_is_honest_about_the_valence_model() {
    let (_directory, app, id) = new_app();
    let report = preview(&app, &id, "[Na+]");
    let loss = report["loss_report"]
        .as_array()
        .unwrap()
        .iter()
        .find(|loss| loss["feature"] == "radical_state")
        .unwrap_or_else(|| panic!("no radical_state loss: {report}"));
    let reason = loss["reason"].as_str().unwrap();
    assert!(!reason.contains("incomplete shell"), "{reason}");
    assert!(reason.contains("chemifree-valence-2"), "{reason}");
}

#[test]
fn whitespace_only_or_leading_whitespace_is_a_syntax_diagnostic_not_a_crash() {
    let (_directory, app, id) = new_app();
    for source in [" C", " ", "\nC", "\t\t"] {
        let report = preview(&app, &id, source);
        assert_eq!(
            report["diagnostics"][0]["code"], "leading_whitespace",
            "{source:?}: {report}"
        );
        assert_eq!(report["diagnostics"][0]["details"]["start_byte"], 0);
        assert!(report["result"].is_null());
    }
    // A zero charge is part of the OpenSMILES charge grammar.
    let report = preview(&app, &id, "[N+0]");
    assert_eq!(report["result"]["atoms"], 1, "{report}");
}
