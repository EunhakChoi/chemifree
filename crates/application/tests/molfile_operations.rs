use chemifree_application::{Application, ListQuery};
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};
use tempfile::TempDir;

const INPUT: &str = include_str!("../../providers/tests/data/molfile/sodium-acetate.mol");
type App = Application<LocalCapabilities, FileDocumentRepository>;

fn app() -> (App, TempDir) {
    let data = tempfile::tempdir().unwrap();
    (
        Application::new(LocalCapabilities, FileDocumentRepository::new(data.path())),
        data,
    )
}
fn call(app: &App, id: &str, op: &str, params: Value) -> Value {
    let request = json!({"contract":1,"request_id":id,"profile":"preserve-semantics","operation":op,"parameters":params});
    serde_json::to_value(app.execute(request.to_string().as_bytes())).unwrap()
}
fn count(app: &App) -> usize {
    app.list_documents(&ListQuery::default())
        .unwrap()
        .documents
        .len()
}

#[test]
fn import_is_persisted_and_replayed_without_creating_a_second_document() {
    let (app, _data) = app();
    let parameters = json!({"format":"mol", "text":INPUT});
    let first = call(&app, "import-1", "document.import", parameters.clone());
    assert_eq!(first["outcome"], "ok", "{first}");
    assert_eq!(first["result"]["record_count"], 1);
    assert_eq!(
        first["result"]["document"]["atoms"]
            .as_array()
            .unwrap()
            .len(),
        5
    );
    assert_eq!(first["result"]["document"]["atoms"][3]["charge"], -1);
    assert_eq!(
        first["result"]["document"]["bonds"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(first["result"]["session"]["unsaved_changes"], false);
    let replay = call(&app, "import-1", "document.import", parameters);
    assert_eq!(replay, first);
    assert_eq!(count(&app), 1);
    let reused = call(
        &app,
        "import-1",
        "document.import",
        json!({"format":"mol", "text":INPUT, "record":1}),
    );
    assert_eq!(reused["diagnostics"][0]["code"], "request_id_reused");
    assert_eq!(count(&app), 1);
}

#[test]
fn invalid_or_unselected_records_create_no_document() {
    let (app, _data) = app();
    let multiple = format!("{INPUT}$$$$\n{INPUT}$$$$\n");
    let response = call(
        &app,
        "ambiguous",
        "document.import",
        json!({"format":"sdf", "text":multiple}),
    );
    assert_eq!(response["diagnostics"][0]["code"], "invalid_record");
    assert_eq!(count(&app), 0);
    let bad = format!("{INPUT}$$$$\ntruncated\n$$$$\n");
    let response = call(
        &app,
        "broken",
        "document.import",
        json!({"format":"sdf", "text":bad, "record":1}),
    );
    assert_eq!(response["diagnostics"][0]["code"], "truncated_record");
    assert_eq!(response["diagnostics"][0]["details"]["record"], 2);
    assert_eq!(count(&app), 0);
}

#[test]
fn sdf_fields_are_returned_with_a_persistence_loss_and_can_be_reexported() {
    let (app, _data) = app();
    let source = format!("{INPUT}> <NAME>\nacetate\n\n$$$$\n{INPUT}$$$$\n");
    let imported = call(
        &app,
        "fields-in",
        "document.import",
        json!({"format":"sdf", "text":source, "record":1}),
    );
    assert_eq!(imported["outcome"], "partial");
    assert_eq!(imported["result"]["record_count"], 2);
    assert_eq!(
        imported["result"]["fields"],
        json!([{"name":"NAME","value":"acetate"}])
    );
    assert_eq!(imported["loss_report"][0]["feature"], "sdf_data_fields");
    let id = imported["result"]["document"]["id"].as_str().unwrap();
    let exported = call(
        &app,
        "fields-out",
        "document.export",
        json!({"format":"sdf", "documents":[{"document_id":id,"fields":imported["result"]["fields"]}]}),
    );
    assert_eq!(exported["result"]["record_count"], 1, "{exported}");
    assert!(
        exported["result"]["text"]
            .as_str()
            .unwrap()
            .contains("> <NAME>\nacetate\n\n$$$$\n")
    );
    assert_eq!(count(&app), 1);
}

#[test]
fn export_reports_losses_and_failure_does_not_change_the_document() {
    let (app, _data) = app();
    let imported = call(
        &app,
        "in",
        "document.import",
        json!({"format":"mol", "text":INPUT}),
    );
    let id = imported["result"]["document"]["id"].as_str().unwrap();
    let before = call(
        &app,
        "before",
        "document.snapshot",
        json!({"document_id":id}),
    );
    let response = call(
        &app,
        "invalid-export",
        "document.export",
        json!({"format":"sdf", "documents":[{"document_id":id,"fields":[{"name":"X","value":"bad\n\nvalue"}]}]}),
    );
    assert_eq!(response["outcome"], "error");
    assert_eq!(response["diagnostics"][0]["code"], "unsupported_feature");
    let after = call(
        &app,
        "after",
        "document.snapshot",
        json!({"document_id":id}),
    );
    assert_eq!(before["result"], after["result"]);
}

#[test]
fn exports_multiple_sdf_records_in_the_requested_order() {
    let (app, _data) = app();
    let first = call(
        &app,
        "first",
        "document.import",
        json!({"format":"mol", "text":INPUT}),
    );
    let second_source = INPUT.replacen("Sodium acetate", "Second molecule", 1);
    let second = call(
        &app,
        "second",
        "document.import",
        json!({"format":"mol", "text":second_source}),
    );
    let first_id = first["result"]["document"]["id"].as_str().unwrap();
    let second_id = second["result"]["document"]["id"].as_str().unwrap();
    let output = call(
        &app,
        "multi-out",
        "document.export",
        json!({"format":"sdf", "documents":[{"document_id":second_id,"fields":[{"name":"LABEL","value":"second"}]},{"document_id":first_id}]}),
    );
    assert_eq!(output["outcome"], "partial", "{output}");
    assert_eq!(output["document_revision"], Value::Null);
    assert_eq!(output["result"]["record_count"], 2);
    let text = output["result"]["text"].as_str().unwrap();
    assert!(text.starts_with("Second molecule\n"));
    assert!(text.contains("> <LABEL>\nsecond\n\n$$$$\nSodium acetate\n"));
    assert_eq!(text.matches("$$$$\n").count(), 2);
}

#[test]
fn export_reports_multiple_instances_graphics_and_references() {
    let (app, _data) = app();
    let imported = call(
        &app,
        "scene-in",
        "document.import",
        json!({"format":"mol", "text":INPUT}),
    );
    let id = imported["result"]["document"]["id"].as_str().unwrap();
    let edited = call(
        &app,
        "scene-edit",
        "document.transaction",
        json!({"document_id":id, "expected_revision":0, "commands":[
            {"op":"add_instance", "offset":{"x":0,"y":40}},
            {"op":"add_graphic", "shape":"line", "points":[{"x":0,"y":0},{"x":10,"y":10}]},
            {"op":"add_reference", "source":"s1", "target":{"kind":"atom","id":"a1"}}
        ]}),
    );
    assert_eq!(edited["outcome"], "ok", "{edited}");
    let exported = call(
        &app,
        "scene-out",
        "document.export",
        json!({"format":"mol", "document_id":id}),
    );
    assert_eq!(exported["outcome"], "partial", "{exported}");
    let losses: Vec<&str> = exported["loss_report"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["feature"].as_str().unwrap())
        .collect();
    assert!(losses.contains(&"scene_instances"), "{losses:?}");
    assert!(losses.contains(&"scene_objects"), "{losses:?}");
    assert!(losses.contains(&"scene_references"), "{losses:?}");
    assert!(losses.contains(&"history"), "{losses:?}");
}
