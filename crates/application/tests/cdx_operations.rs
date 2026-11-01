//! CDX through the shared document.import / document.export operations.
//!
//! The binary payload travels as canonical base64 in the same string fields as CDXML text.

#[path = "../../providers/tests/common/mod.rs"]
mod common;

use chemifree_application::transport_base64;
use chemifree_application::{Application, ListQuery};
use chemifree_providers::interchange_codec::LocalInterchangeCodec;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};

type App = Application<LocalCapabilities, FileDocumentRepository>;

fn app(path: &std::path::Path) -> App {
    Application::new(LocalCapabilities, FileDocumentRepository::new(path))
        .with_interchange_codec(LocalInterchangeCodec)
}

fn call(app: &App, id: &str, op: &str, parameters: Value) -> Value {
    let request = json!({"contract":1,"request_id":id,"profile":"preserve-semantics","operation":op,"parameters":parameters});
    serde_json::to_value(app.execute(request.to_string().as_bytes())).unwrap()
}

fn count(app: &App) -> usize {
    app.list_documents(&ListQuery::default())
        .unwrap()
        .documents
        .len()
}

fn source() -> String {
    transport_base64::encode(&common::rich_file())
}

fn preview(app: &App, source: &str) -> Value {
    call(
        app,
        "preview",
        "document.import",
        json!({"mode":"preview","format":"cdx","source":source,"title":"Projected","page_id":1,"fragment_id":2}),
    )
}

#[test]
fn read_then_unchanged_export_returns_the_same_base64() {
    let dir = tempfile::tempdir().unwrap();
    let application = app(dir.path());
    let source = source();
    let read = call(
        &application,
        "read",
        "document.import",
        json!({"mode":"read","format":"cdx","source":source}),
    );
    assert_eq!(read["outcome"], "ok", "{read}");
    assert_eq!(read["result"]["choices"][0]["page_id"], 1);
    assert_eq!(read["result"]["choices"][0]["fragment_id"], 2);
    let export = call(
        &application,
        "export",
        "document.export",
        json!({"format":"cdx","document":read["result"]["document"],"original":source}),
    );
    assert_eq!(export["outcome"], "ok", "{export}");
    assert_eq!(export["result"]["content"], source.as_str());
    assert_eq!(
        count(&application),
        0,
        "reading and exporting never create a drawing"
    );
}

#[test]
fn preview_consent_and_create_bind_the_decoded_source() {
    let dir = tempfile::tempdir().unwrap();
    let application = app(dir.path());
    let source = source();
    let report = preview(&application, &source);
    assert_eq!(report["outcome"], "partial", "{report}");
    assert_eq!(count(&application), 0);
    let features: Vec<&str> = report["loss_report"]
        .as_array()
        .unwrap()
        .iter()
        .map(|loss| loss["feature"].as_str().unwrap())
        .collect();
    assert!(features.contains(&"styled_text"), "{features:?}");
    assert_eq!(report["result"]["document"]["atoms"][1]["element"], "O");
    assert_eq!(report["result"]["mapping"]["atoms"][0]["source_id"], 3);
    let digest = report["result"]["preview_digest"]
        .as_str()
        .unwrap()
        .to_owned();
    let create = |id: &str, source: &str, digest: &str, accept: bool| {
        call(
            &application,
            id,
            "document.import",
            json!({"mode":"create","format":"cdx","source":source,"title":"Projected","page_id":1,"fragment_id":2,"preview_digest":digest,"accept_loss":accept}),
        )
    };
    assert_eq!(
        create("no-consent", &source, &digest, false)["diagnostics"][0]["code"],
        "loss_consent_required"
    );
    // Different bytes (one coordinate changed) are a different snapshot.
    let mut other = common::rich_file();
    let at = other
        .windows(4)
        .position(|w| w == 655_360_i32.to_le_bytes())
        .unwrap();
    other[at] ^= 1;
    let other = transport_base64::encode(&other);
    assert_eq!(
        create("changed-source", &other, &digest, true)["diagnostics"][0]["code"],
        "preview_changed"
    );
    assert_eq!(count(&application), 0);
    let created = create("accepted", &source, &digest, true);
    assert_eq!(
        created["result"]["document"]["atoms"][1]["element"], "O",
        "{created}"
    );
    assert_eq!(count(&application), 1);
    let replay = create("accepted", &source, &digest, true);
    assert_eq!(
        replay["result"]["document"]["id"],
        created["result"]["document"]["id"]
    );
    assert_eq!(count(&application), 1);
}

#[test]
fn base64_text_must_be_canonical_and_within_the_binary_limit() {
    let dir = tempfile::tempdir().unwrap();
    let application = app(dir.path());
    let good = source();
    let read = |text: &str| {
        call(
            &application,
            "bad",
            "document.import",
            json!({"mode":"read","format":"cdx","source":text}),
        )
    };
    for text in [
        "Zg", "Zg==\n", "Zm9v-_8=", " Zm9v", "Zh==", "Zm9=", "Zg==Zm9v",
    ] {
        let response = read(text);
        assert_eq!(
            response["diagnostics"][0]["code"], "invalid_base64",
            "{text:?}"
        );
    }
    // Canonical base64 of something that is not a CDX file reaches the codec.
    assert_eq!(read("Zm9v")["diagnostics"][0]["code"], "invalid_signature");
    // One character past the longest text for 1 MiB of bytes.
    let too_long = "A".repeat(transport_base64::encoded_len(1024 * 1024) + 4);
    let response = read(&too_long);
    assert_eq!(response["diagnostics"][0]["code"], "resource_limit");
    assert_eq!(response["diagnostics"][0]["details"]["offset"], 1024 * 1024);
    // CDXML text is not base64, and base64 is not CDXML.
    let wrong = call(
        &application,
        "wrong",
        "document.import",
        json!({"mode":"read","format":"cdxml","source":good}),
    );
    assert_eq!(wrong["diagnostics"][0]["code"], "invalid_xml");
}

#[test]
fn binary_errors_report_the_offset_in_details_once() {
    let dir = tempfile::tempdir().unwrap();
    let application = app(dir.path());
    let bytes = common::rich_file();
    let truncated = transport_base64::encode(&bytes[..bytes.len() - 7]);
    let response = call(
        &application,
        "truncated",
        "document.import",
        json!({"mode":"read","format":"cdx","source":truncated}),
    );
    let diagnostic = &response["diagnostics"][0];
    assert_eq!(diagnostic["code"], "truncated");
    assert!(diagnostic["details"]["offset"].is_u64(), "{diagnostic}");
    assert!(diagnostic["details"].get("line").is_none());
    let offset = diagnostic["details"]["offset"]
        .as_u64()
        .unwrap()
        .to_string();
    assert!(
        !diagnostic["message"]
            .as_str()
            .unwrap()
            .contains(&format!("byte {offset}.")),
        "the message must not repeat the detail: {}",
        diagnostic["message"]
    );
    assert!(
        !diagnostic["message"]
            .as_str()
            .unwrap()
            .contains("Truncated")
    );
}

#[test]
fn export_to_cdx_needs_consent_for_rounded_and_dropped_content() {
    let dir = tempfile::tempdir().unwrap();
    let application = app(dir.path());
    let cdxml = r#"<CDXML xmlns="urn:example:cdxml"><page id="1"><fragment id="2"><n id="3" p="0.1 2"/></fragment></page></CDXML>"#;
    let read = call(
        &application,
        "read-xml",
        "document.import",
        json!({"mode":"read","format":"cdxml","source":cdxml}),
    );
    let tree = read["result"]["document"].clone();
    let refused = call(
        &application,
        "export-refused",
        "document.export",
        json!({"format":"cdx","document":tree}),
    );
    let diagnostic = &refused["diagnostics"][0];
    assert_eq!(diagnostic["code"], "loss_consent_required", "{refused}");
    let features: Vec<&str> = diagnostic["details"]["features"]
        .as_array()
        .unwrap()
        .iter()
        .map(|feature| feature.as_str().unwrap())
        .collect();
    assert_eq!(features, ["precision", "xml_namespace"]);
    assert!(refused["result"].is_null());
    let accepted = call(
        &application,
        "export-accepted",
        "document.export",
        json!({"format":"cdx","document":tree,"accept_loss":true}),
    );
    assert_eq!(accepted["outcome"], "partial", "{accepted}");
    let bytes = transport_base64::decode(accepted["result"]["content"].as_str().unwrap()).unwrap();
    assert_eq!(&bytes[..8], b"VjCD0100");
    let kinds: Vec<&str> = accepted["loss_report"]
        .as_array()
        .unwrap()
        .iter()
        .map(|loss| loss["feature"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["precision", "xml_namespace"]);
    // The same tree as CDXML needs no consent: nothing is rounded or dropped there.
    let xml = call(
        &application,
        "export-xml",
        "document.export",
        json!({"format":"cdxml","document":tree}),
    );
    assert_eq!(xml["outcome"], "ok", "{xml}");
}

#[test]
fn opaque_binary_content_needs_the_discard_flag_not_the_loss_flag() {
    let dir = tempfile::tempdir().unwrap();
    let application = app(dir.path());
    let original = transport_base64::encode(&common::rich_file_with_opaque());
    let read = call(
        &application,
        "read-opaque",
        "document.import",
        json!({"mode":"read","format":"cdx","source":original}),
    );
    assert_eq!(read["loss_report"][0]["feature"], "opaque_extensions");
    let mut tree = read["result"]["document"].clone();
    tree["root"]["id"] = json!(12);
    let refused = call(
        &application,
        "edit-refused",
        "document.export",
        json!({"format":"cdx","document":tree,"original":original}),
    );
    assert_eq!(
        refused["diagnostics"][0]["code"], "unsafe_unknown_rewrite",
        "{refused}"
    );
    let discarded = call(
        &application,
        "edit-discarded",
        "document.export",
        json!({"format":"cdx","document":tree,"original":original,"discard_opaque":true}),
    );
    assert_eq!(discarded["outcome"], "partial", "{discarded}");
    assert!(
        discarded["loss_report"]
            .as_array()
            .unwrap()
            .iter()
            .any(|loss| loss["feature"] == "opaque_extensions" && loss["disposition"] == "dropped")
    );
}

#[test]
fn a_large_unknown_property_round_trips_inside_the_http_body_limit() {
    use common::{file, obj, prop};
    let dir = tempfile::tempdir().unwrap();
    let application = app(dir.path());
    // 50 KiB of opaque bytes: 66.7 KB as base64, about 190 KB as a JSON array of numbers.
    let unknown: Vec<u8> = (0..50 * 1024).map(|i| (i * 7 % 251) as u8).collect();
    let bytes = file(0, &[prop(0x0990, &unknown), obj(0x8001, 1, &[])]);
    let original = transport_base64::encode(&bytes);
    let read = call(
        &application,
        "read-big",
        "document.import",
        json!({"mode":"read","format":"cdx","source":original}),
    );
    assert_eq!(read["outcome"], "ok", "{read}");
    let parameters =
        json!({"format":"cdx","document":read["result"]["document"],"original":original});
    let body = json!({"contract":1,"request_id":"export-big","profile":"preserve-semantics","operation":"document.export","parameters":parameters});
    assert!(
        body.to_string().len() < 256 * 1024,
        "the export request is {} bytes; the HTTP body limit is 262144",
        body.to_string().len()
    );
    let export = call(&application, "export-big", "document.export", parameters);
    assert_eq!(export["outcome"], "ok", "{export}");
    assert_eq!(export["result"]["content"], original.as_str());
    // The DTO carries the bytes as canonical base64, strictly: a padded-wrong text is refused.
    let mut tree = read["result"]["document"].clone();
    let text = tree["root"]["properties"][0]["value"]["value"].clone();
    assert!(
        text.is_string(),
        "opaque bytes are a base64 string, got {text:.80}"
    );
    tree["root"]["properties"][0]["value"]["value"] = json!(format!("{}=", text.as_str().unwrap()));
    let refused = call(
        &application,
        "export-bad",
        "document.export",
        json!({"format":"cdx","document":tree,"original":original}),
    );
    assert_eq!(refused["outcome"], "error", "{refused}");
}

#[test]
fn the_application_binary_limit_is_the_codecs_limit() {
    // The application cannot import the provider, so it repeats the number; this pins them.
    assert_eq!(chemifree_providers::cdx::MAX_BYTES, 1024 * 1024);
    let too_long =
        "A".repeat(transport_base64::encoded_len(chemifree_providers::cdx::MAX_BYTES) + 4);
    let dir = tempfile::tempdir().unwrap();
    let response = call(
        &app(dir.path()),
        "limit",
        "document.import",
        json!({"mode":"read","format":"cdx","source":too_long}),
    );
    assert_eq!(
        response["diagnostics"][0]["details"]["offset"],
        chemifree_providers::cdx::MAX_BYTES
    );
}
