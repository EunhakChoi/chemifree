use chemifree_application::{Application, PreferencesChange};
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
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
fn formula_scopes_and_precision_use_one_unchanged_snapshot() {
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
    assert_eq!(all["result"]["display_decimals"], 5);
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
    assert_eq!(new["result"]["display_decimals"], 2);
    assert_eq!(
        all["result"]["display_decimals"], 5,
        "an earlier result is immutable"
    );
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
