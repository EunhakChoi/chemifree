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
    assert!(
        response["diagnostics"][0]["details"]
            .get("offset")
            .is_none(),
        "a base64 text length is not a byte position: {response}"
    );
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
    assert_eq!(
        features,
        ["precision"],
        "a dropped namespace changes nothing and needs no consent"
    );
    // The refusal shows the full entries the consent would cover, with the worst rounding.
    let shown = diagnostic["details"]["losses"].as_array().unwrap();
    assert_eq!(shown.len(), 1, "{diagnostic}");
    assert_eq!(shown[0]["feature"], "precision");
    assert!(shown[0]["source_path"].as_str().unwrap().contains("node"));
    assert!(
        shown[0]["reason"]
            .as_str()
            .unwrap()
            .contains("largest change")
    );
    let digest = diagnostic["details"]["loss_digest"].as_str().unwrap();
    assert!(refused["result"].is_null());
    let accepted = call(
        &application,
        "export-accepted",
        "document.export",
        json!({"format":"cdx","document":tree,"accepted_losses":digest}),
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
    // Consent covers what was shown: a different document produces different losses, and the
    // digest of the first does not accept them.
    let other_source = cdxml.replace("0.1 2", "0.2 2");
    let other = call(
        &application,
        "read-xml-2",
        "document.import",
        json!({"mode":"read","format":"cdxml","source":other_source}),
    );
    let changed = call(
        &application,
        "export-changed",
        "document.export",
        json!({"format":"cdx","document":other["result"]["document"],"accepted_losses":digest}),
    );
    assert_eq!(
        changed["diagnostics"][0]["code"], "loss_consent_changed",
        "{changed}"
    );
    assert_ne!(changed["diagnostics"][0]["details"]["loss_digest"], digest);
    assert!(changed["result"].is_null());
    // The blanket flag of the first design no longer exists.
    let flag = call(
        &application,
        "export-flag",
        "document.export",
        json!({"format":"cdx","document":tree,"accept_loss":true}),
    );
    assert_eq!(flag["diagnostics"][0]["code"], "invalid_request");
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
        json!({"mode":"read","format":"cdx","source":original,"compact_bytes":true}),
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
        "compact opaque bytes are a base64 string, got {text:.80}"
    );
    assert_eq!(
        tree["root"]["properties"][0]["value"]["type"],
        "bytes_base64"
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
fn the_base64_size_limit_comes_from_the_codec_and_reports_no_offset() {
    // The application asks the codec for its limit instead of restating it.
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
    assert!(
        response["diagnostics"][0]["details"]
            .get("offset")
            .is_none()
    );
}

#[test]
fn a_bad_original_is_reported_as_the_originals_fault() {
    let dir = tempfile::tempdir().unwrap();
    let application = app(dir.path());
    let good = source();
    let read = call(
        &application,
        "read",
        "document.import",
        json!({"mode":"read","format":"cdx","source":good}),
    );
    let bytes = common::rich_file();
    let broken = transport_base64::encode(&bytes[..bytes.len() - 9]);
    let export = call(
        &application,
        "export-broken-original",
        "document.export",
        json!({"format":"cdx","document":read["result"]["document"],"original":broken}),
    );
    let diagnostic = &export["diagnostics"][0];
    assert_eq!(diagnostic["code"], "invalid_original", "{export}");
    assert!(
        diagnostic["message"].as_str().unwrap().contains("original"),
        "{diagnostic}"
    );
    // The document itself is fine: the same export without the broken original succeeds.
    let fine = call(
        &application,
        "export-fine",
        "document.export",
        json!({"format":"cdx","document":read["result"]["document"],"original":good}),
    );
    assert_eq!(fine["outcome"], "ok");
}

#[test]
fn contract_one_keeps_the_number_array_form_of_opaque_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let application = app(dir.path());
    use common::{file, obj, prop};
    let bytes = file(0, &[prop(0x0990, &[1, 2, 3]), obj(0x8001, 1, &[])]);
    let original = transport_base64::encode(&bytes);
    let read = call(
        &application,
        "read-array",
        "document.import",
        json!({"mode":"read","format":"cdx","source":original}),
    );
    let value = &read["result"]["document"]["root"]["properties"][0]["value"];
    assert_eq!(value, &json!({"type":"bytes","value":[1,2,3]}), "{read}");
    let export = call(
        &application,
        "export-array",
        "document.export",
        json!({"format":"cdx","document":read["result"]["document"],"original":original}),
    );
    assert_eq!(export["outcome"], "ok", "{export}");
    assert_eq!(export["result"]["content"], original.as_str());
    // A client may also send the compact form, and it means the same bytes.
    let mut tree = read["result"]["document"].clone();
    tree["root"]["properties"][0]["value"] = json!({"type":"bytes_base64","value":"AQID"});
    let compact = call(
        &application,
        "export-compact",
        "document.export",
        json!({"format":"cdx","document":tree,"original":original}),
    );
    assert_eq!(compact["result"]["content"], original.as_str(), "{compact}");
}

#[test]
fn a_bad_base64_original_is_the_originals_fault_too() {
    let dir = tempfile::tempdir().unwrap();
    let application = app(dir.path());
    let read = call(
        &application,
        "read",
        "document.import",
        json!({"mode":"read","format":"cdx","source":source()}),
    );
    for bad in ["Zg", "Zm9v\n"] {
        let export = call(
            &application,
            "export-bad-original",
            "document.export",
            json!({"format":"cdx","document":read["result"]["document"],"original":bad}),
        );
        let diagnostic = &export["diagnostics"][0];
        assert_eq!(diagnostic["code"], "invalid_original", "{bad:?}: {export}");
        assert!(diagnostic["message"].as_str().unwrap().contains("original"));
    }
}

#[test]
fn consent_features_are_listed_once_in_a_stable_order() {
    let dir = tempfile::tempdir().unwrap();
    let application = app(dir.path());
    // A rounded coordinate, a rounded color and a rounded font size are three losses of one feature.
    let cdxml = r#"<CDXML><colortable><color r="0.1" g="0" b="0"/></colortable><fonttable><font id="3" charset="iso-8859-1" name="A"/></fonttable><page id="1"><fragment id="2"><n id="3" p="0.1 2"/></fragment><t id="4"><s font="3" size="10.01" face="0" color="2">x</s></t></page></CDXML>"#;
    let read = call(
        &application,
        "read-three",
        "document.import",
        json!({"mode":"read","format":"cdxml","source":cdxml}),
    );
    let refused = call(
        &application,
        "export-three",
        "document.export",
        json!({"format":"cdx","document":read["result"]["document"]}),
    );
    let details = &refused["diagnostics"][0]["details"];
    assert_eq!(details["losses"].as_array().unwrap().len(), 3, "{refused}");
    assert_eq!(details["features"], json!(["precision"]));
}

#[test]
fn a_model_failure_has_a_specific_code_in_cdx_and_the_p22_code_in_cdxml() {
    // Documented in CONTRACT.md, once: CDXML keeps its contract 1 wording for compatibility.
    let dir = tempfile::tempdir().unwrap();
    let application = app(dir.path());
    let cdxml = r#"<CDXML><fonttable><font id="3" charset="iso-8859-1" name="A"/></fonttable><page id="1"/></CDXML>"#;
    let read = call(
        &application,
        "read-empty-table",
        "document.import",
        json!({"mode":"read","format":"cdxml","source":cdxml}),
    );
    let mut tree = read["result"]["document"].clone();
    tree["root"]["children"][0]["value"]["children"] = json!([]);
    let xml = call(
        &application,
        "empty-table-xml",
        "document.export",
        json!({"format":"cdxml","document":tree}),
    );
    assert_eq!(xml["diagnostics"][0]["code"], "invalid_document");
    let binary = call(
        &application,
        "empty-table-cdx",
        "document.export",
        json!({"format":"cdx","document":tree}),
    );
    assert_eq!(binary["diagnostics"][0]["code"], "invalid_font_table");
}
