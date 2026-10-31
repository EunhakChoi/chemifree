//! Binary transport uses the same application operations and persistence as rich CDXML.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chemifree_application::Application;
use chemifree_providers::{
    FileDocumentRepository, LocalCapabilities, interchange_codec::LocalInterchangeCodec,
};
use serde_json::{Value, json};

type App = Application<LocalCapabilities, FileDocumentRepository>;
fn call(app: &App, id: &str, operation: &str, parameters: Value) -> Value {
    serde_json::to_value(app.execute(&serde_json::to_vec(&json!({"contract":1,"request_id":id,"profile":"preserve-semantics","operation":operation,"parameters":parameters})).unwrap())).unwrap()
}
fn position(bytes: &mut Vec<u8>, x: i32, y: i32) {
    bytes.extend(0x0200u16.to_le_bytes());
    bytes.extend(8u16.to_le_bytes());
    bytes.extend(y.to_le_bytes());
    bytes.extend(x.to_le_bytes());
}
fn example() -> Vec<u8> {
    let mut bytes = b"VjCD0100\x04\x03\x02\x01".to_vec();
    bytes.extend([0; 16]);
    for (tag, id) in [(0x8001u16, 1u32), (0x8003, 2), (0x8004, 3)] {
        bytes.extend(tag.to_le_bytes());
        bytes.extend(id.to_le_bytes());
    }
    position(&mut bytes, 0, 0);
    bytes.extend(0x0402u16.to_le_bytes());
    bytes.extend(2u16.to_le_bytes());
    bytes.extend(8u16.to_le_bytes());
    for _ in 0..4 {
        bytes.extend([0, 0]);
    }
    bytes
}

fn graphic_with_unknown_binary_content() -> Vec<u8> {
    let mut bytes = example();
    // Leave the selected node and fragment closed, and add a graphic to their page.
    bytes.truncate(bytes.len() - 4);
    bytes.extend(0x8007u16.to_le_bytes());
    bytes.extend(4u32.to_le_bytes());
    bytes.extend(0x0204u16.to_le_bytes());
    bytes.extend(16u16.to_le_bytes());
    bytes.extend([0; 16]);
    bytes.extend(0x7ff0u16.to_le_bytes());
    bytes.extend(2u16.to_le_bytes());
    bytes.extend([0xaa, 0x55]);
    bytes.extend(0x8ffeu16.to_le_bytes());
    bytes.extend(5u32.to_le_bytes());
    for _ in 0..4 {
        bytes.extend([0, 0]);
    }
    bytes
}

#[test]
fn discarded_graphic_reports_unknown_binary_content() {
    let dir = tempfile::tempdir().unwrap();
    let app = Application::new(LocalCapabilities, FileDocumentRepository::new(dir.path()))
        .with_interchange_codec(LocalInterchangeCodec);
    let bytes = graphic_with_unknown_binary_content();
    let source = STANDARD.encode(&bytes);
    let read = call(
        &app,
        "unknown-graphic-read",
        "document.import",
        json!({"mode":"read","format":"cdx","source":source}),
    );
    assert_eq!(read["outcome"], "ok", "{read}");
    assert!(read["loss_report"].as_array().unwrap().iter().any(|loss| {
        loss["feature"] == "opaque_extensions" && loss["disposition"] == "retained"
    }));
    let write = call(
        &app,
        "unknown-graphic-write",
        "document.export",
        json!({"format":"cdx","document":read["result"]["document"],"original":source}),
    );
    assert_eq!(
        STANDARD
            .decode(write["result"]["content"].as_str().unwrap())
            .unwrap(),
        bytes
    );
    let preview = call(
        &app,
        "unknown-graphic-preview",
        "document.import",
        json!({"mode":"preview","format":"cdx","source":source,"title":"Binary","page_id":1,"fragment_id":2}),
    );
    let losses = preview["loss_report"].as_array().unwrap();
    assert!(
        losses
            .iter()
            .any(|loss| loss["feature"] == "graphic" && loss["semantic_impact"] == "rendering")
    );
    assert!(
        losses
            .iter()
            .any(|loss| loss["feature"] == "unprojected_property"
                && loss["semantic_impact"] == "unknown")
    );
    assert!(
        losses
            .iter()
            .any(|loss| loss["feature"] == "opaque_object" && loss["semantic_impact"] == "unknown")
    );
    assert!(
        app.list_documents(&Default::default())
            .unwrap()
            .documents
            .is_empty()
    );
}

#[test]
fn scoped_tables_refuse_preview_and_creation_without_changing_documents() {
    let dir = tempfile::tempdir().unwrap();
    let app = Application::new(LocalCapabilities, FileDocumentRepository::new(dir.path()))
        .with_interchange_codec(LocalInterchangeCodec);
    let existing = call(
        &app,
        "existing",
        "document.create",
        json!({"title":"Existing"}),
    );
    let id = existing["result"]["document"]["id"].as_str().unwrap();
    let before = call(
        &app,
        "before",
        "document.snapshot",
        json!({"document_id":id}),
    );
    let mut bytes = example();
    bytes.truncate(bytes.len() - 4);
    bytes.extend(0x0100u16.to_le_bytes());
    bytes.extend(4u16.to_le_bytes());
    bytes.extend([1, 0, 0, 0]); // font table with platform 1 and no fonts
    bytes.extend([0, 0, 0, 0]); // page and file terminators
    let source = STANDARD.encode(bytes);
    for mode in ["preview", "create"] {
        let mut parameters = json!({"mode":mode,"format":"cdx","source":source,"title":"Binary","page_id":1,"fragment_id":2});
        if mode == "create" {
            parameters["preview_digest"] = json!("unavailable");
            parameters["accept_loss"] = json!(true);
        }
        let result = call(&app, mode, "document.import", parameters);
        assert_eq!(result["diagnostics"][0]["code"], "unsupported_table_scope");
        assert!(
            result["diagnostics"][0]["details"]["byte_offset"]
                .as_u64()
                .unwrap()
                > 28
        );
        assert!(result["result"].is_null());
        assert_eq!(
            app.list_documents(&Default::default())
                .unwrap()
                .documents
                .len(),
            1
        );
        let after = call(
            &app,
            &format!("after-{mode}"),
            "document.snapshot",
            json!({"document_id":id}),
        );
        assert_eq!(after["result"], before["result"]);
    }
}

#[test]
fn cdx_base64_read_write_projection_and_xml_transcode_use_real_operations() {
    let dir = tempfile::tempdir().unwrap();
    let app = Application::new(LocalCapabilities, FileDocumentRepository::new(dir.path()))
        .with_interchange_codec(LocalInterchangeCodec);
    let source = STANDARD.encode(example());
    let read = call(
        &app,
        "cdx-read",
        "document.import",
        json!({"mode":"read","format":"cdx","source":source}),
    );
    assert_eq!(read["outcome"], "ok");
    assert_eq!(read["result"]["choices"][0]["fragment_id"], 2);
    let tree = read["result"]["document"].clone();
    let encoded = call(
        &app,
        "cdx-write",
        "document.export",
        json!({"format":"cdx","document":tree,"original":source}),
    );
    assert_eq!(encoded["result"]["encoding"], "base64");
    assert_eq!(
        STANDARD
            .decode(encoded["result"]["content"].as_str().unwrap())
            .unwrap(),
        example()
    );
    let xml = call(
        &app,
        "xml-write",
        "document.export",
        json!({"format":"cdxml","document":read["result"]["document"]}),
    );
    assert_eq!(xml["result"]["encoding"], "utf8");
    let reread = call(
        &app,
        "xml-read",
        "document.import",
        json!({"mode":"read","format":"cdxml","source":xml["result"]["content"]}),
    );
    assert_eq!(reread["result"]["document"], read["result"]["document"]);
    let params = json!({"mode":"preview","format":"cdx","source":source,"title":"Binary","page_id":1,"fragment_id":2});
    let preview = call(&app, "cdx-preview", "document.import", params);
    assert_eq!(preview["result"]["document"]["atoms"][0]["element"], "O");
    let create = call(
        &app,
        "cdx-create",
        "document.import",
        json!({"mode":"create","format":"cdx","source":source,"title":"Binary","page_id":1,"fragment_id":2,"preview_digest":preview["result"]["preview_digest"],"accept_loss":true}),
    );
    assert_eq!(create["result"]["document"]["atoms"][0]["element"], "O");
    let invalid = call(
        &app,
        "bad-base64",
        "document.import",
        json!({"mode":"read","format":"cdx","source":"??"}),
    );
    assert_eq!(invalid["diagnostics"][0]["code"], "invalid_base64");
    let bytes = example();
    let truncated = call(
        &app,
        "truncated",
        "document.import",
        json!({"mode":"read","format":"cdx","source":STANDARD.encode(&bytes[..bytes.len()-1])}),
    );
    assert_eq!(truncated["diagnostics"][0]["code"], "truncated_cdx");
    assert!(
        truncated["diagnostics"][0]["details"]["byte_offset"]
            .as_u64()
            .unwrap()
            > 28
    );
    assert!(truncated["result"].is_null());
}

#[test]
fn unknown_binary_node_semantics_are_preserved_but_editor_projection_refuses_them() {
    let dir = tempfile::tempdir().unwrap();
    let app = Application::new(LocalCapabilities, FileDocumentRepository::new(dir.path()))
        .with_interchange_codec(LocalInterchangeCodec);
    let mut bytes = example();
    let node_end = bytes.len() - 8;
    bytes.splice(node_end..node_end, [0x03, 0x04, 1, 0, 7]);
    let source = STANDARD.encode(bytes);
    let read = call(
        &app,
        "opaque-read",
        "document.import",
        json!({"mode":"read","format":"cdx","source":source}),
    );
    assert_eq!(read["outcome"], "ok");
    assert_eq!(read["loss_report"][0]["disposition"], "retained");
    let projection = call(
        &app,
        "opaque-preview",
        "document.import",
        json!({"mode":"preview","format":"cdx","source":source,"title":"Opaque","page_id":1,"fragment_id":2}),
    );
    assert_eq!(projection["diagnostics"][0]["code"], "unsupported_node");
    assert!(projection["result"].is_null());
    let write = call(
        &app,
        "opaque-write",
        "document.export",
        json!({"format":"cdx","document":read["result"]["document"],"original":source}),
    );
    assert_eq!(write["result"]["content"], source);
    let create = call(
        &app,
        "opaque-create",
        "document.import",
        json!({"mode":"create","format":"cdx","source":source,"title":"Opaque","page_id":1,"fragment_id":2,"preview_digest":"unavailable","accept_loss":true}),
    );
    assert_eq!(create["diagnostics"][0]["code"], "unsupported_node");
    assert!(create["result"].is_null());
    assert_eq!(
        app.list_documents(&Default::default())
            .unwrap()
            .documents
            .len(),
        0
    );
}

#[test]
fn dative_binary_is_preserved_but_refused_before_editor_creation() {
    let dir = tempfile::tempdir().unwrap();
    let app = Application::new(LocalCapabilities, FileDocumentRepository::new(dir.path()))
        .with_interchange_codec(LocalInterchangeCodec);
    let mut bytes = b"VjCD0100\x04\x03\x02\x01".to_vec();
    bytes.extend([0; 16]);
    for (tag, id) in [(0x8001u16, 1u32), (0x8003, 2), (0x8004, 3)] {
        bytes.extend(tag.to_le_bytes());
        bytes.extend(id.to_le_bytes());
    }
    position(&mut bytes, 0, 0);
    bytes.extend([0, 0]);
    bytes.extend(0x8004u16.to_le_bytes());
    bytes.extend(4u32.to_le_bytes());
    position(&mut bytes, 65536, 0);
    bytes.extend([0, 0]);
    bytes.extend(0x8005u16.to_le_bytes());
    bytes.extend(5u32.to_le_bytes());
    for (tag, value) in [(0x0604u16, 3u32), (0x0605, 4)] {
        bytes.extend(tag.to_le_bytes());
        bytes.extend(4u16.to_le_bytes());
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(0x0600u16.to_le_bytes());
    bytes.extend(2u16.to_le_bytes());
    bytes.extend(0x1000u16.to_le_bytes());
    bytes.extend([0; 8]);
    let source = STANDARD.encode(&bytes);
    let read = call(
        &app,
        "dative-binary-read",
        "document.import",
        json!({"mode":"read","format":"cdx","source":source}),
    );
    assert_eq!(read["outcome"], "ok");
    let write = call(
        &app,
        "dative-binary-write",
        "document.export",
        json!({"format":"cdx","document":read["result"]["document"],"original":source}),
    );
    assert_eq!(write["result"]["content"], source);
    let create = call(
        &app,
        "dative-binary-create",
        "document.import",
        json!({"mode":"create","format":"cdx","source":source,"title":"Dative","page_id":1,"fragment_id":2,"preview_digest":"unavailable","accept_loss":true}),
    );
    assert_eq!(create["diagnostics"][0]["code"], "unsupported_bond");
    assert!(create["result"].is_null());
    assert_eq!(
        app.list_documents(&Default::default())
            .unwrap()
            .documents
            .len(),
        0
    );
}
