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
fn example() -> Vec<u8> {
    let mut bytes = b"VjCD0100\x04\x03\x02\x01".to_vec();
    bytes.extend([0; 16]);
    for (tag, id) in [(0x8001u16, 1u32), (0x8003, 2), (0x8004, 3)] {
        bytes.extend(tag.to_le_bytes());
        bytes.extend(id.to_le_bytes());
    }
    bytes.extend(0x0402u16.to_le_bytes());
    bytes.extend(2u16.to_le_bytes());
    bytes.extend(8u16.to_le_bytes());
    for _ in 0..4 {
        bytes.extend([0, 0]);
    }
    bytes
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
}
