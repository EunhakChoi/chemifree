use chemifree_application::{Application, PreferencesChange};
use chemifree_providers::{FileDocumentRepository, FilePreferenceStore, LocalCapabilities};
use serde_json::{Value, json};

fn call(
    app: &Application<LocalCapabilities, FileDocumentRepository>,
    id: &str,
    operation: &str,
    parameters: Value,
) -> Value {
    let request = json!({"contract":1,"request_id":id,"profile":"preserve-semantics","operation":operation,"parameters":parameters});
    serde_json::to_value(app.execute(request.to_string().as_bytes())).unwrap()
}

#[test]
fn formula_scopes_use_one_unchanged_snapshot_independent_of_mass_precision() {
    let data = tempfile::tempdir().unwrap();
    let app = Application::new(LocalCapabilities, FileDocumentRepository::new(data.path()));
    let id = app
        .create_document(None, Some("Methanol"))
        .unwrap()
        .document
        .id;
    let edit = call(
        &app,
        "edit",
        "document.transaction",
        json!({"document_id":id,"expected_revision":0,"commands":[
            {"op":"add_atom","element":"C","position":{"x":0,"y":0}},
            {"op":"add_atom","element":"O","position":{"x":20,"y":0},"bond_to":{"atom":"$0","order":"single"}}
        ]}),
    );
    assert_eq!(edit["outcome"], "ok");
    let before = serde_json::to_value(app.document(&id).unwrap()).unwrap();
    let change: PreferencesChange = serde_json::from_value(json!({"ui":{"theme":"system"},"new_documents":{},"computation":{"mass_precision":{"decimals":5}}})).unwrap();
    app.set_preferences(None, 0, change).unwrap();
    let all = call(
        &app,
        "formula",
        "chem.properties",
        json!({"document_id":id,"scope":"molecule","property":"formula"}),
    );
    assert_eq!(all["outcome"], "ok", "{all}");
    assert_eq!(all["model_id"], "chemifree-properties-1");
    assert_eq!(all["result"]["components"][0]["formula"], "CH4O");
    assert_eq!(all["result"]["display_decimals"], 0);
    assert_eq!(all["result"]["snapshot"]["document"], before["document"]);
    assert_eq!(all["result"]["snapshot"]["session"], before["session"]);
    let selected = call(
        &app,
        "selected",
        "chem.properties",
        json!({"document_id":id,"scope":"selection","selection":{"atoms":["a1"]},"property":"formula"}),
    );
    assert_eq!(selected["result"]["components"][0]["formula"], "CH3");
    app.reset_preferences(None, 1).unwrap();
    let new = call(
        &app,
        "new",
        "chem.properties",
        json!({"document_id":id,"scope":"molecule","property":"formula"}),
    );
    assert_eq!(new["result"]["display_decimals"], 0);
    assert_eq!(all["result"]["components"], new["result"]["components"]);
    assert_eq!(
        serde_json::to_value(app.document(&id).unwrap()).unwrap(),
        before
    );
}

#[test]
fn missing_mass_data_and_invalid_selection_return_no_partial_result() {
    let data = tempfile::tempdir().unwrap();
    let app = Application::new(LocalCapabilities, FileDocumentRepository::new(data.path()));
    let id = app
        .create_document(None, Some("No data"))
        .unwrap()
        .document
        .id;
    let before = serde_json::to_value(app.document(&id).unwrap()).unwrap();
    let missing = call(
        &app,
        "missing",
        "chem.properties",
        json!({"document_id":id,"scope":"molecule","property":"masses"}),
    );
    assert_eq!(
        (
            missing["outcome"].as_str(),
            missing["diagnostics"][0]["code"].as_str()
        ),
        (Some("unsupported"), Some("mass_data_unavailable"))
    );
    assert!(missing["result"].is_null());
    let invalid = call(
        &app,
        "invalid",
        "chem.properties",
        json!({"document_id":id,"scope":"selection","selection":{"atoms":["a99"]},"property":"formula"}),
    );
    assert_eq!(invalid["diagnostics"][0]["code"], "invalid_selection");
    assert!(invalid["result"].is_null());
    assert_eq!(
        serde_json::to_value(app.document(&id).unwrap()).unwrap(),
        before
    );
}

#[test]
fn formula_is_available_when_mass_preferences_are_unreadable() {
    let data = tempfile::tempdir().unwrap();
    let app = Application::new(LocalCapabilities, FileDocumentRepository::new(data.path()))
        .with_preferences(FilePreferenceStore::new(data.path()));
    let id = app
        .create_document(None, Some("Methane"))
        .unwrap()
        .document
        .id;
    let edited = call(
        &app,
        "draw",
        "document.transaction",
        json!({
            "document_id": id, "expected_revision": 0,
            "commands": [{"op":"add_atom", "element":"C", "position":{"x":0,"y":0}}]
        }),
    );
    assert_eq!(edited["outcome"], "ok");
    let before = serde_json::to_value(app.document(&id).unwrap()).unwrap();
    std::fs::write(data.path().join("preferences.json"), b"not valid JSON").unwrap();

    let formula = call(
        &app,
        "formula-unreadable",
        "chem.properties",
        json!({
            "document_id": id, "scope": "molecule", "property": "formula"
        }),
    );
    assert_eq!(formula["outcome"], "ok", "{formula}");
    assert_eq!(formula["result"]["components"][0]["formula"], "CH4");
    assert_eq!(formula["result"]["display_decimals"], 0);
    let mass = call(
        &app,
        "mass-unreadable",
        "chem.properties",
        json!({
            "document_id": id, "scope": "molecule", "property": "masses"
        }),
    );
    assert_eq!(mass["diagnostics"][0]["code"], "preferences_unreadable");
    assert!(mass["result"].is_null());
    assert_eq!(
        serde_json::to_value(app.document(&id).unwrap()).unwrap(),
        before
    );
    assert_eq!(
        std::fs::read(data.path().join("preferences.json")).unwrap(),
        b"not valid JSON"
    );
}
