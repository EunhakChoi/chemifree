//! Rich CDXML operations and editor projection through the real application and file store.
use chemifree_application::{Application, ListQuery};
use chemifree_providers::interchange_codec::LocalInterchangeCodec;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};

type App = Application<LocalCapabilities, FileDocumentRepository>;
const SOURCE: &str = "\u{feff}<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!--outside-->\n<CDXML><page id=\"1\"><fragment id=\"2\"><n id=\"3\" p=\"1 2\"/><n id=\"4\" p=\"3 4\" Element=\"8\" Charge=\"-1\"/><b id=\"5\" B=\"3\" E=\"4\" Order=\"2\"/></fragment><t id=\"6\"><s>α &amp; β</s></t></page><page id=\"7\"><fragment id=\"8\"><n id=\"9\"/></fragment></page></CDXML>";

fn app(path: &std::path::Path) -> App {
    Application::new(LocalCapabilities, FileDocumentRepository::new(path))
        .with_interchange_codec(LocalInterchangeCodec)
}
fn call(app: &App, id: &str, op: &str, parameters: Value) -> Value {
    let request = json!({"contract":1,"request_id":id,"profile":"preserve-semantics","operation":op,"parameters":parameters});
    serde_json::to_value(app.execute(request.to_string().as_bytes())).unwrap()
}
fn preview(app: &App) -> Value {
    call(
        app,
        "preview",
        "document.import",
        json!({"mode":"preview","format":"cdxml","source":SOURCE,"title":"Projected","page_id":1,"fragment_id":2}),
    )
}
fn count(app: &App) -> usize {
    app.list_documents(&ListQuery::default())
        .unwrap()
        .documents
        .len()
}

#[test]
fn rich_tree_and_unchanged_bytes_preserve_bom_text_style_and_references() {
    let dir = tempfile::tempdir().unwrap();
    let application = app(dir.path());
    let decoded = call(
        &application,
        "read",
        "document.import",
        json!({"mode":"read","format":"cdxml","source":SOURCE}),
    );
    assert_eq!(decoded["outcome"], "ok");
    assert_eq!(decoded["loss_report"][0]["disposition"], "retained");
    assert_eq!(decoded["result"]["choices"][0]["page_id"], 1);
    assert_eq!(decoded["result"]["choices"][0]["fragment_id"], 2);
    assert_eq!(decoded["result"]["choices"][1]["page_id"], 7);
    assert_eq!(decoded["result"]["choices"][1]["fragment_id"], 8);
    let tree = decoded["result"]["document"].clone();
    let encoded = call(
        &application,
        "write",
        "document.export",
        json!({"format":"cdxml","document":tree,"original":SOURCE}),
    );
    assert_eq!(encoded["outcome"], "ok");
    assert_eq!(
        encoded["result"]["content"].as_str().unwrap().as_bytes(),
        SOURCE.as_bytes()
    );
    assert_eq!(count(&application), 0);
}

#[test]
fn opaque_target_references_remain_readable_and_are_reported_before_projection() {
    let dir = tempfile::tempdir().unwrap();
    let application = app(dir.path());
    let source = SOURCE
        .replacen(
            "<page id=\"1\">",
            "<page id=\"1\"><bracketedgroup id=\"20\" RepeatCount=\"3\"/>",
            1,
        )
        .replacen(
            "</fragment>",
            "</fragment><graphic id=\"21\" BoundingBox=\"0 0 1 1\"><represent object=\"20\" attribute=\"RepeatCount\"/></graphic>",
            1,
        );
    let decoded = call(
        &application,
        "opaque-reference-read",
        "document.import",
        json!({"mode":"read","format":"cdxml","source":source}),
    );
    assert!(decoded["result"]["document"].is_object(), "{decoded}");
    let written = call(
        &application,
        "opaque-reference-write",
        "document.export",
        json!({"format":"cdxml","document":decoded["result"]["document"],"original":source}),
    );
    assert_eq!(written["result"]["content"], source);
    let preview = call(
        &application,
        "opaque-reference-preview",
        "document.import",
        json!({"mode":"preview","format":"cdxml","source":source,"title":"Projected","page_id":1,"fragment_id":2}),
    );
    let losses = preview["loss_report"].as_array().unwrap();
    assert!(losses.iter().any(|loss| loss["feature"] == "opaque_object"));
    assert!(losses.iter().any(|loss| {
        loss["feature"] == "reference"
            && loss["reason"].as_str().unwrap().contains("object 20")
            && loss["reason"].as_str().unwrap().contains("RepeatCount")
    }));
    assert_eq!(count(&application), 0);
    let digest = preview["result"]["preview_digest"].as_str().unwrap();
    let request = json!({"mode":"create","format":"cdxml","source":source,"title":"Projected","page_id":1,"fragment_id":2,"preview_digest":digest,"accept_loss":true});
    let created = call(
        &application,
        "opaque-reference-create",
        "document.import",
        request,
    );
    assert_eq!(created["result"]["document"]["atoms"][1]["element"], "O");
    assert_eq!(count(&application), 1);
}

#[test]
fn exporting_an_empty_explicit_table_is_refused_without_creating_a_document() {
    let dir = tempfile::tempdir().unwrap();
    let application = app(dir.path());
    let source = r#"<CDXML><fonttable><font id="3" name="Serif"/></fonttable><colortable><color r="1" g="1" b="1"/></colortable><page id="1"/></CDXML>"#;
    let read = call(
        &application,
        "tables-read",
        "document.import",
        json!({"mode":"read","format":"cdxml","source":source}),
    );
    for (index, expected) in [(0, "EmptyFontTable"), (1, "EmptyColorTable")] {
        let mut tree = read["result"]["document"].clone();
        tree["root"]["children"][index]["value"]["children"] = json!([]);
        let write = call(
            &application,
            &format!("empty-table-{index}"),
            "document.export",
            json!({"format":"cdxml","document":tree}),
        );
        assert_eq!(write["diagnostics"][0]["code"], "invalid_document");
        assert!(
            write["diagnostics"][0]["message"]
                .as_str()
                .unwrap()
                .contains(expected)
        );
        assert!(write["result"].is_null());
        assert_eq!(count(&application), 0);
    }
}

#[test]
fn projection_reports_nested_reference_opaque_property_and_unselected_chemistry() {
    let dir = tempfile::tempdir().unwrap();
    let application = app(dir.path());
    let source = include_str!("../../providers/tests/data/cdxml/multi-page.cdxml").replace(
        "GraphicType=\"Line\"",
        "GraphicType=\"Line\" future=\"opaque\"",
    );
    let report = call(
        &application,
        "nested-losses",
        "document.import",
        json!({"mode":"preview","format":"cdxml","source":source,"title":"Projected","page_id":1,"fragment_id":2}),
    );
    assert_eq!(report["outcome"], "partial", "{report}");
    let losses = report["loss_report"].as_array().unwrap();
    assert!(
        losses
            .iter()
            .any(|loss| { loss["feature"] == "graphic" && loss["semantic_impact"] == "rendering" })
    );
    assert!(losses.iter().any(|loss| {
        loss["feature"] == "reference"
            && loss["semantic_impact"] == "document"
            && loss["reason"].as_str().unwrap().contains("object 4")
            && loss["reason"].as_str().unwrap().contains("Charge")
    }));
    assert!(losses.iter().any(|loss| {
        loss["feature"] == "unprojected_property" && loss["semantic_impact"] == "unknown"
    }));
    assert!(losses.iter().any(|loss| {
        loss["feature"] == "unselected_fragment" && loss["semantic_impact"] == "chemistry"
    }));
    assert_eq!(count(&application), 0);
}

#[test]
fn projected_creation_reports_losses_maps_ids_replays_and_persists() {
    let dir = tempfile::tempdir().unwrap();
    let application = app(dir.path());
    let report = preview(&application);
    assert_eq!(report["outcome"], "partial");
    assert_eq!(count(&application), 0);
    let losses = report["loss_report"].as_array().unwrap();
    assert!(losses.iter().any(|loss| loss["feature"] == "styled_text"));
    assert!(
        losses
            .iter()
            .any(|loss| loss["feature"] == "unselected_page")
    );
    assert!(losses.iter().any(|loss| loss["feature"] == "outside_root"));
    assert!(!losses.iter().any(|loss| {
        loss["feature"] == "unprojected_object"
            && loss["source_path"]
                .as_str()
                .unwrap()
                .ends_with("children[0]")
    }));
    let digest = report["result"]["preview_digest"].as_str().unwrap();
    let mut request = json!({"mode":"create","format":"cdxml","source":SOURCE,"title":"Projected","page_id":1,"fragment_id":2,"preview_digest":digest,"accept_loss":false});
    assert_eq!(
        call(
            &application,
            "no-consent",
            "document.import",
            request.clone()
        )["diagnostics"][0]["code"],
        "loss_consent_required"
    );
    assert_eq!(count(&application), 0);
    request["accept_loss"] = json!(true);
    let result = call(&application, "create", "document.import", request.clone());
    assert_eq!(result["outcome"], "partial");
    assert_eq!(result["result"]["document"]["atoms"][1]["element"], "O");
    assert_eq!(result["result"]["document"]["atoms"][1]["charge"], -1);
    assert_eq!(
        result["result"]["interchange_mapping"]["atoms"][0]["source_id"],
        3
    );
    assert_eq!(
        result["result"]["interchange_mapping"]["atoms"][0]["target_id"],
        "a1"
    );
    assert_eq!(
        result["result"]["interchange_mapping"]["bonds"][0]["target_id"],
        "b1"
    );
    assert_eq!(count(&application), 1);
    assert_eq!(
        call(&application, "create", "document.import", request),
        result
    );
    let id = result["result"]["document"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    drop(application);
    let restarted = app(dir.path());
    let reopened = call(
        &restarted,
        "get",
        "document.snapshot",
        json!({"document_id":id}),
    );
    assert_eq!(
        reopened["result"]["document"]["atoms"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn restart_and_source_change_invalidate_consent_before_creation() {
    let dir = tempfile::tempdir().unwrap();
    let application = app(dir.path());
    let result = preview(&application);
    let digest = result["result"]["preview_digest"]
        .as_str()
        .unwrap()
        .to_owned();
    let changed_source = SOURCE.replace("α &amp; β", "γ &amp; β");
    let changed = json!({"mode":"create","format":"cdxml","source":changed_source,"title":"Projected","page_id":1,"fragment_id":2,"preview_digest":digest,"accept_loss":true});
    assert_eq!(
        call(&application, "changed-source", "document.import", changed)["diagnostics"][0]["code"],
        "preview_changed"
    );
    let preferences = serde_json::from_value(
        json!({"ui":{"theme":"system"},"new_documents":{"bond_length_pt":40}}),
    )
    .unwrap();
    application.set_preferences(None, 0, preferences).unwrap();
    let changed_settings = json!({"mode":"create","format":"cdxml","source":SOURCE,"title":"Projected","page_id":1,"fragment_id":2,"preview_digest":digest,"accept_loss":true});
    assert_eq!(
        call(
            &application,
            "changed-settings",
            "document.import",
            changed_settings
        )["diagnostics"][0]["code"],
        "preview_changed"
    );
    assert_eq!(count(&application), 0);
    drop(application);
    let restarted = app(dir.path());
    let request = json!({"mode":"create","format":"cdxml","source":SOURCE,"title":"Projected","page_id":1,"fragment_id":2,"preview_digest":digest,"accept_loss":true});
    let refused = call(&restarted, "old", "document.import", request);
    assert_eq!(refused["diagnostics"][0]["code"], "preview_changed");
    assert_eq!(count(&restarted), 0);
}

#[test]
fn invalid_references_and_unsafe_opaque_rewrites_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let application = app(dir.path());
    let bad = SOURCE.replace("E=\"4\"", "E=\"99\"");
    let error = call(
        &application,
        "dangling",
        "document.import",
        json!({"mode":"read","format":"cdxml","source":bad}),
    );
    assert_eq!(error["diagnostics"][0]["code"], "invalid_document");
    let opaque = SOURCE.replace("<page id=\"1\">", "<page id=\"1\" future=\"3\">");
    let read = call(
        &application,
        "read-opaque",
        "document.import",
        json!({"mode":"read","format":"cdxml","source":opaque}),
    );
    let mut tree = read["result"]["document"].clone();
    tree["root"]["children"][0]["value"]["id"] = json!(10);
    let write = call(
        &application,
        "unsafe",
        "document.export",
        json!({"format":"cdxml","document":tree,"original":opaque,"discard_opaque":false}),
    );
    assert_eq!(write["diagnostics"][0]["code"], "unsafe_unknown_rewrite");
    assert!(write["result"].is_null());
    assert_eq!(count(&application), 0);
}

#[test]
fn explicit_unknown_hydrogen_semantics_are_not_silently_inferred() {
    let dir = tempfile::tempdir().unwrap();
    let application = app(dir.path());
    let source = SOURCE.replace("p=\"1 2\"", "p=\"1 2\" NumHydrogens=\"4\"");
    let result = call(
        &application,
        "explicit-h",
        "document.import",
        json!({"mode":"preview","format":"cdxml","source":source,"title":"Projected","page_id":1,"fragment_id":2}),
    );
    assert_eq!(result["diagnostics"][0]["code"], "unsupported_node");
    assert!(result["result"].is_null());
    assert_eq!(count(&application), 0);
}

#[test]
fn dative_direction_is_retained_in_rich_xml_but_cannot_create_an_editor_graph() {
    let dir = tempfile::tempdir().unwrap();
    let application = app(dir.path());
    let source = "<CDXML><page id=\"1\"><fragment id=\"2\"><n id=\"3\" p=\"0 0\" Element=\"7\"/><n id=\"4\" p=\"1 0\" Element=\"8\"/><b id=\"5\" B=\"3\" E=\"4\" Order=\"dative\"/></fragment></page></CDXML>";
    let read = call(
        &application,
        "dative-read",
        "document.import",
        json!({"mode":"read","format":"cdxml","source":source}),
    );
    assert_eq!(read["outcome"], "ok");
    let write = call(
        &application,
        "dative-write",
        "document.export",
        json!({"format":"cdxml","document":read["result"]["document"],"original":source}),
    );
    assert_eq!(write["result"]["content"], source);
    let parameters = json!({"mode":"preview","format":"cdxml","source":source,"title":"Dative","page_id":1,"fragment_id":2});
    let preview = call(
        &application,
        "dative-preview",
        "document.import",
        parameters,
    );
    assert_eq!(preview["diagnostics"][0]["code"], "unsupported_bond");
    assert!(preview["result"].is_null());
    let create = call(
        &application,
        "dative-create",
        "document.import",
        json!({"mode":"create","format":"cdxml","source":source,"title":"Dative","page_id":1,"fragment_id":2,"preview_digest":"unavailable","accept_loss":true}),
    );
    assert_eq!(create["diagnostics"][0]["code"], "unsupported_bond");
    assert!(create["result"].is_null());
    assert_eq!(count(&application), 0);
}

#[test]
fn unsafe_editor_projection_never_creates_or_changes_documents() {
    let dir = tempfile::tempdir().unwrap();
    let application = app(dir.path());
    let existing = call(
        &application,
        "existing",
        "document.create",
        json!({"title":"Existing"}),
    );
    let id = existing["result"]["document"]["id"].as_str().unwrap();
    let before = call(
        &application,
        "before",
        "document.snapshot",
        json!({"document_id":id}),
    );
    let examples = [
        (
            "<fragment id=\"2\"><n id=\"3\"/></fragment>",
            "missing_position",
        ),
        (
            "<fragment id=\"2\"><n id=\"3\" p=\"100001 0\"/></fragment>",
            "coordinate_out_of_range",
        ),
        (
            "<fragment id=\"2\"><n id=\"3\" p=\"0 0\" future=\"opaque\"/></fragment>",
            "unsupported_node",
        ),
        (
            "<fragment id=\"2\"><n id=\"3\" p=\"0 0\"><t id=\"4\"><s>O</s></t></n></fragment>",
            "unsupported_node",
        ),
        (
            "<fragment id=\"2\"><n id=\"3\" p=\"0 0\"><fragment id=\"4\"><n id=\"5\" p=\"1 1\"/></fragment></n></fragment>",
            "unsupported_node",
        ),
        (
            "<fragment id=\"2\"><n id=\"3\" p=\"0 0\"><future/></n></fragment>",
            "unsupported_node",
        ),
        (
            "<fragment id=\"2\" future=\"opaque\"><n id=\"3\" p=\"0 0\"/></fragment>",
            "unsupported_fragment",
        ),
        (
            "<fragment id=\"2\"><n id=\"3\" p=\"0 0\"/><future/></fragment>",
            "unsupported_fragment",
        ),
        (
            "<fragment id=\"2\"><n id=\"3\" p=\"0 0\"/><n id=\"4\" p=\"1 0\"/><b id=\"5\" B=\"3\" E=\"4\" future=\"opaque\"/></fragment>",
            "unsupported_bond",
        ),
        (
            "<fragment id=\"2\"><n id=\"3\" p=\"0 0\"/><n id=\"4\" p=\"1 0\"/><b id=\"5\" B=\"3\" E=\"4\"><future/></b></fragment>",
            "unsupported_bond",
        ),
    ];
    for (index, (fragment, code)) in examples.into_iter().enumerate() {
        let source = format!("<CDXML><page id=\"1\">{fragment}</page></CDXML>");
        let read = call(
            &application,
            &format!("read-{index}"),
            "document.import",
            json!({"mode":"read","format":"cdxml","source":source}),
        );
        assert_eq!(read["outcome"], "ok", "rich read {index}: {read}");
        let written = call(
            &application,
            &format!("write-{index}"),
            "document.export",
            json!({"format":"cdxml","document":read["result"]["document"],"original":source}),
        );
        assert_eq!(written["result"]["content"], source, "rich export {index}");
        let preview = call(
            &application,
            &format!("preview-{index}"),
            "document.import",
            json!({"mode":"preview","format":"cdxml","source":source,"title":"Rejected","page_id":1,"fragment_id":2}),
        );
        assert_eq!(
            preview["diagnostics"][0]["code"], code,
            "preview {index}: {preview}"
        );
        assert!(preview["result"].is_null());
        let create = call(
            &application,
            &format!("create-{index}"),
            "document.import",
            json!({"mode":"create","format":"cdxml","source":source,"title":"Rejected","page_id":1,"fragment_id":2,"preview_digest":"unavailable","accept_loss":true}),
        );
        assert_eq!(
            create["diagnostics"][0]["code"], code,
            "create {index}: {create}"
        );
        assert!(create["result"].is_null());
        assert_eq!(count(&application), 1);
        let after = call(
            &application,
            &format!("after-{index}"),
            "document.snapshot",
            json!({"document_id":id}),
        );
        assert_eq!(
            after["result"], before["result"],
            "existing graph and history {index}"
        );
    }
}
