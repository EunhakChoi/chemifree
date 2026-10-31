use chemifree_application::{
    Application, CapabilityCatalog, ChemicalBond, ChemicalCodec, ChemicalFormat, ChemicalRecord,
    CodecError, DataField, ListQuery, WrittenChemical,
};
use chemifree_domain::{
    Capability,
    document::{BondOrder, Document},
};
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
fn selecting_one_record_reports_that_the_others_are_not_stored() {
    let (app, _data) = app();
    let second = INPUT.replacen("Sodium acetate", "Other", 1);
    let input = format!("{INPUT}$$$$\n{second}$$$$\n");
    let response = call(
        &app,
        "select-first",
        "document.import",
        json!({"format":"sdf", "text":input, "record":1}),
    );
    assert_eq!(response["outcome"], "partial", "{response}");
    assert_eq!(response["result"]["record_count"], 2);
    assert_eq!(response["result"]["document"]["title"], "Sodium acetate");
    assert!(
        response["loss_report"]
            .as_array()
            .unwrap()
            .iter()
            .any(|loss| loss["feature"] == "unselected_records"
                && loss["disposition"] == "dropped")
    );
    assert_eq!(count(&app), 1);
}

#[test]
fn invalid_later_record_titles_are_rejected_before_any_document_is_created() {
    let (app, _data) = app();
    for (index, title) in ["x".repeat(81), "bad\u{1}title".into()].iter().enumerate() {
        let second = INPUT.replacen("Sodium acetate", title, 1);
        let input = format!("{INPUT}$$$$\n{second}$$$$\n");
        let response = call(
            &app,
            &format!("bad-title-{index}"),
            "document.import",
            json!({
                "format":"sdf", "text":input, "record":1
            }),
        );
        assert_eq!(response["outcome"], "error", "{response}");
        assert_eq!(response["diagnostics"][0]["code"], "invalid_title");
        assert_eq!(response["diagnostics"][0]["details"]["record"], 2);
        assert_eq!(response["diagnostics"][0]["details"]["field"], "title");
        assert_eq!(count(&app), 0);
    }
}

struct InvalidProjectionCodec;

impl ChemicalCodec for InvalidProjectionCodec {
    fn read(&self, _format: ChemicalFormat, text: &str) -> Result<Vec<ChemicalRecord>, CodecError> {
        let make = |title: &str, bonds| ChemicalRecord {
            title: title.into(),
            atoms: Vec::new(),
            bonds,
            fields: Vec::new(),
            losses: Vec::new(),
        };
        let second = if text == "invalid-title" {
            make("bad\u{1}title", Vec::new())
        } else {
            make(
                "Bad graph",
                vec![ChemicalBond {
                    begin: 0,
                    end: 1,
                    order: BondOrder::Single,
                }],
            )
        };
        Ok(vec![make("First", Vec::new()), second])
    }
    fn write(
        &self,
        _format: ChemicalFormat,
        _document: &Document,
        _fields: &[DataField],
    ) -> Result<WrittenChemical, CodecError> {
        unreachable!("this provider test exercises import only")
    }
}

struct InvalidProjectionCapabilities;
impl CapabilityCatalog for InvalidProjectionCapabilities {
    fn capabilities(&self) -> Vec<Capability> {
        Vec::new()
    }
    fn chemical_codec(&self) -> Option<&dyn ChemicalCodec> {
        Some(&InvalidProjectionCodec)
    }
}

#[test]
fn projection_failures_from_a_provider_identify_the_selected_record() {
    let data = tempfile::tempdir().unwrap();
    let app = Application::new(
        InvalidProjectionCapabilities,
        FileDocumentRepository::new(data.path()),
    );
    for (text, field) in [("invalid-title", Some("title")), ("invalid-graph", None)] {
        let request = json!({"contract":1,"request_id":format!("projection-{text}"),"profile":"preserve-semantics",
            "operation":"document.import","parameters":{"format":"sdf","text":text,"record":2}});
        let response = serde_json::to_value(app.execute(request.to_string().as_bytes())).unwrap();
        assert_eq!(response["outcome"], "error", "{response}");
        assert_eq!(
            response["diagnostics"][0]["details"]["record"], 2,
            "{response}"
        );
        if let Some(field) = field {
            assert_eq!(response["diagnostics"][0]["details"]["field"], field);
        }
        assert_eq!(
            app.list_documents(&ListQuery::default())
                .unwrap()
                .documents
                .len(),
            0
        );
    }
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
fn multi_record_export_identifies_the_later_failing_record_without_partial_output() {
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

    let unsupported = call(
        &app,
        "bad-field",
        "document.export",
        json!({
            "format":"sdf", "documents":[
                {"document_id":id},
                {"document_id":id, "fields":[{"name":"X","value":"bad\n\nvalue"}]}
            ]
        }),
    );
    assert_eq!(unsupported["outcome"], "error");
    assert_eq!(unsupported["result"], Value::Null);
    assert_eq!(unsupported["diagnostics"][0]["code"], "unsupported_feature");
    assert_eq!(
        unsupported["diagnostics"][0]["details"]["record"], 2,
        "{unsupported}"
    );
    assert_eq!(
        unsupported["diagnostics"][0]["details"]["field"],
        "fields.value"
    );

    let reserved = call(
        &app,
        "reserved-name",
        "document.create",
        json!({"title":"$$$$ prefix"}),
    );
    let reserved_id = reserved["result"]["document"]["id"].as_str().unwrap();
    let reserved_export = call(
        &app,
        "reserved-export",
        "document.export",
        json!({
            "format":"sdf", "documents":[{"document_id":id},{"document_id":reserved_id}]
        }),
    );
    assert_eq!(reserved_export["outcome"], "error");
    assert_eq!(reserved_export["result"], Value::Null);
    assert_eq!(reserved_export["diagnostics"][0]["details"]["record"], 2);
    assert_eq!(
        reserved_export["diagnostics"][0]["details"]["field"],
        "title"
    );

    let missing = call(
        &app,
        "missing",
        "document.export",
        json!({
            "format":"sdf", "documents":[
                {"document_id":id}, {"document_id":"doc-0000000000000000"}
            ]
        }),
    );
    assert_eq!(missing["outcome"], "error");
    assert_eq!(missing["result"], Value::Null);
    assert_eq!(missing["diagnostics"][0]["code"], "document_not_found");
    assert_eq!(
        missing["diagnostics"][0]["details"]["record"], 2,
        "{missing}"
    );

    let large = vec!["x".repeat(199); 3000].join("\n");
    let oversized = call(
        &app,
        "too-large",
        "document.export",
        json!({
            "format":"sdf", "documents":[
                {"document_id":id, "fields":[{"name":"A","value":large}]},
                {"document_id":id, "fields":[{"name":"B","value":large}]}
            ]
        }),
    );
    assert_eq!(oversized["outcome"], "error");
    assert_eq!(oversized["result"], Value::Null);
    assert_eq!(oversized["diagnostics"][0]["code"], "resource_limit");
    assert_eq!(
        oversized["diagnostics"][0]["details"]["record"], 2,
        "{oversized}"
    );
    let after = call(
        &app,
        "after",
        "document.snapshot",
        json!({"document_id":id}),
    );
    assert_eq!(before["result"], after["result"]);
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
