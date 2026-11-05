//! Drawing export through the operation boundary, including stereo carriers and loss consent.

use std::sync::atomic::{AtomicU64, Ordering};

use chemifree_application::{Application, transport_base64};
use chemifree_providers::interchange_codec::LocalInterchangeCodec;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};

type App = Application<LocalCapabilities, FileDocumentRepository>;

fn app(path: &std::path::Path) -> App {
    Application::new(LocalCapabilities, FileDocumentRepository::new(path))
        .with_interchange_codec(LocalInterchangeCodec)
}

fn call(app: &App, operation: &str, parameters: Value) -> Value {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let request = json!({"contract":1,"request_id":format!("export-{}", NEXT.fetch_add(1, Ordering::Relaxed)),
        "profile":"preserve-semantics","operation":operation,"parameters":parameters});
    serde_json::to_value(app.execute(request.to_string().as_bytes())).unwrap()
}

fn snapshot(app: &App, id: &str) -> Value {
    call(app, "document.snapshot", json!({"document_id":id}))["result"]["document"].clone()
}

fn create_from(app: &App, format: &str, source: &str) -> Value {
    let read = call(
        app,
        "document.import",
        json!({"mode":"read","format":format,"source":source}),
    );
    let choice = &read["result"]["choices"][0];
    assert!(choice["fragment_id"].is_number(), "{read}");
    let mut parameters = json!({"mode":"preview","format":format,"source":source,"title":"Stereo",
        "page_id":choice["page_id"],"fragment_id":choice["fragment_id"]});
    let preview = call(app, "document.import", parameters.clone());
    assert!(preview["result"]["preview_digest"].is_string(), "{preview}");
    parameters["mode"] = json!("create");
    parameters["preview_digest"] = preview["result"]["preview_digest"].clone();
    parameters["accept_loss"] = json!(true);
    let created = call(app, "document.import", parameters);
    assert!(created["result"]["document"]["id"].is_string(), "{created}");
    created["result"]["document"].clone()
}

fn transact(app: &App, id: &str, commands: Value) {
    let before = snapshot(app, id);
    let reply = call(
        app,
        "document.transaction",
        json!({"document_id":id,
        "expected_revision":before["revision"],"commands":commands}),
    );
    assert!(
        matches!(reply["outcome"].as_str(), Some("ok" | "partial")),
        "{reply}"
    );
}

fn export(app: &App, id: &str, format: &str, digest: Option<&str>) -> Value {
    let mut parameters = json!({"document_id":id,"format":format});
    if let Some(digest) = digest {
        parameters["accepted_losses"] = json!(digest);
    }
    call(app, "document.export", parameters)
}

fn content(reply: &Value) -> &str {
    reply["result"]["content"]
        .as_str()
        .unwrap_or_else(|| panic!("no export content: {reply}"))
}

fn loss<'a>(reply: &'a Value, feature: &str) -> &'a Value {
    reply["loss_report"]
        .as_array()
        .unwrap()
        .iter()
        .find(|loss| loss["feature"] == feature)
        .unwrap_or_else(|| panic!("no {feature} loss in {reply}"))
}

/// A carbon/F/Cl/Br fan per entry, all coordinates exactly representable in CDX.
fn fans(groups: &[(&str, Option<i32>)]) -> String {
    let mut source = String::from("<CDXML><page id=\"1\"><fragment id=\"2\">");
    for (index, (kind, number)) in groups.iter().enumerate() {
        let n = 10 + index * 4;
        let x = 100 + index * 100;
        let group = if kind.is_empty() {
            String::new()
        } else {
            format!(
                "EnhancedStereoType=\"{kind}\" {}",
                number
                    .map(|number| format!("EnhancedStereoGroupNum=\"{number}\""))
                    .unwrap_or_default()
            )
        };
        source.push_str(&format!("<n id=\"{n}\" p=\"{x} 100\" {group}/><n id=\"{}\" p=\"{x} 70\" Element=\"9\"/><n id=\"{}\" p=\"{} 115\" Element=\"17\"/><n id=\"{}\" p=\"{} 115\" Element=\"35\"/>", n+1,n+2,x+26,n+3,x-26));
        for leaf in 1..=3 {
            let display = if leaf == 2 {
                "Display=\"WedgeBegin\""
            } else {
                ""
            };
            source.push_str(&format!(
                "<b id=\"{}\" B=\"{n}\" E=\"{}\" Order=\"1\" {display}/>",
                100 + index * 3 + leaf,
                n + leaf
            ));
        }
    }
    source.push_str("</fragment></page></CDXML>");
    source
}

#[test]
fn all_drawn_marks_survive_cdxml_and_cdx_operation_roundtrips() {
    for format in ["cdxml", "cdx"] {
        let directory = tempfile::tempdir().unwrap();
        let app = app(directory.path());
        let mut source = String::from("<CDXML><page id=\"1\"><fragment id=\"2\">");
        for (i, (display, order)) in [
            ("WedgeBegin", 1),
            ("WedgeEnd", 1),
            ("WedgedHashBegin", 1),
            ("WedgedHashEnd", 1),
            ("Wavy", 1),
            ("Wavy", 2),
        ]
        .iter()
        .enumerate()
        {
            source.push_str(&format!("<n id=\"{}\" p=\"0 {}\"/><n id=\"{}\" p=\"30 {}\"/><b id=\"{}\" B=\"{}\" E=\"{}\" Order=\"{order}\" Display=\"{display}\"/>",10+i*2,i*30,11+i*2,i*30,100+i,10+i*2,11+i*2));
        }
        source.push_str("</fragment></page></CDXML>");
        let original = create_from(&app, "cdxml", &source);
        let id = original["id"].as_str().unwrap();
        let before = snapshot(&app, id);
        let written = export(&app, id, format, None);
        assert_eq!(snapshot(&app, id), before, "{format}: export is read-only");
        if format == "cdxml" {
            for name in [
                "WedgeBegin",
                "WedgeEnd",
                "WedgedHashBegin",
                "WedgedHashEnd",
                "Wavy",
            ] {
                assert!(
                    content(&written).contains(&format!("Display=\"{name}\"")),
                    "{name}: {written}"
                );
            }
        } else {
            let bytes = transport_base64::decode(content(&written)).unwrap();
            assert_eq!(&bytes[..8], b"VjCD0100");
        }
        let roundtrip = create_from(&app, format, content(&written));
        assert_eq!(roundtrip["bonds"], original["bonds"], "{format}");
        assert_eq!(roundtrip["atoms"], original["atoms"], "{format}");
        let marks: Vec<_> = roundtrip["bonds"]
            .as_array()
            .unwrap()
            .iter()
            .map(|bond| bond["mark"].clone())
            .collect();
        assert_eq!(
            marks,
            vec![
                json!({"kind":"wedge","narrow":"begin"}),
                json!({"kind":"wedge","narrow":"end"}),
                json!({"kind":"hash","narrow":"begin"}),
                json!({"kind":"hash","narrow":"end"}),
                json!({"kind":"wavy"}),
                json!({"kind":"crossed"})
            ]
        );
    }
}

#[test]
fn grouped_configurations_roundtrip_without_invented_cip_or_geometry_carriers() {
    for format in ["cdxml", "cdx"] {
        let directory = tempfile::tempdir().unwrap();
        let app = app(directory.path());
        let original = create_from(
            &app,
            "cdxml",
            &fans(&[("Absolute", None), ("And", Some(7)), ("Or", Some(9))]),
        );
        let written = export(&app, original["id"].as_str().unwrap(), format, None);
        // Returning content without accepted_losses proves these notes need no consent.
        assert_eq!(written["outcome"], "partial", "{written}");
        assert_eq!(
            loss(&written, "stereo_group_identity")["disposition"],
            "dropped"
        );
        if format == "cdxml" {
            for kind in ["Absolute", "And", "Or"] {
                assert!(
                    content(&written).contains(&format!("EnhancedStereoType=\"{kind}\"")),
                    "{written}"
                );
            }
            for absent in [" AS=", " BS=", " Geometry=", " BondOrdering="] {
                assert!(!content(&written).contains(absent), "{absent}: {written}");
            }
        }
        let roundtrip = create_from(&app, format, content(&written));
        assert_eq!(roundtrip["stereo"], original["stereo"], "{format}");
        assert_eq!(roundtrip["bonds"], original["bonds"], "{format}");
    }
}

#[test]
fn group_renumbering_preserves_distinct_memberships_and_reserves_valid_numbers() {
    for format in ["cdxml", "cdx"] {
        let directory = tempfile::tempdir().unwrap();
        let app = app(directory.path());
        let original = create_from(&app, "cdxml", &fans(&[("And", Some(-3)), ("And", Some(1))]));
        let written = export(&app, original["id"].as_str().unwrap(), format, None);
        let normalized = loss(&written, "stereo_group_number");
        assert_eq!(normalized["disposition"], "normalized");
        assert_eq!(
            written["outcome"], "partial",
            "normalization returns content without accepted_losses: {written}"
        );
        let roundtrip = create_from(&app, format, content(&written));
        let groups = roundtrip["stereo"]["groups"].as_array().unwrap();
        let mut memberships: Vec<(Vec<&str>, i64)> = groups
            .iter()
            .map(|group| {
                (
                    group["members"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|member| member.as_str().unwrap())
                        .collect(),
                    group["number"].as_i64().unwrap(),
                )
            })
            .collect();
        memberships.sort_unstable();
        assert_eq!(
            memberships,
            vec![(vec!["a1"], 2), (vec!["a5"], 1)],
            "{format}: renumbering must preserve distinct member sets independently of group enumeration order"
        );
    }
}

#[test]
fn a_stored_drawing_mismatch_drops_the_entire_group_and_requires_exact_consent() {
    for format in ["cdxml", "cdx"] {
        let directory = tempfile::tempdir().unwrap();
        let app = app(directory.path());
        let original = create_from(&app, "cdxml", &fans(&[("And", Some(4)), ("And", Some(4))]));
        let id = original["id"].as_str().unwrap();
        let mut configuration = original["stereo"]["atoms"][0]["configuration"].clone();
        configuration["turn"] = if configuration["turn"] == "clockwise" {
            json!("anticlockwise")
        } else {
            json!("clockwise")
        };
        transact(
            &app,
            id,
            json!([{"op":"set_atom_configuration","atom":"a1","configuration":configuration}]),
        );
        let before = snapshot(&app, id);
        let refused = export(&app, id, format, None);
        assert_eq!(
            refused["diagnostics"][0]["code"], "loss_consent_required",
            "{refused}"
        );
        assert!(refused["result"].is_null());
        let details = &refused["diagnostics"][0]["details"];
        let required = details["losses"].as_array().unwrap();
        assert!(
            required
                .iter()
                .any(|loss| loss["feature"] == "stereo_configuration"
                    && loss["disposition"] == "dropped"),
            "{refused}"
        );
        assert!(
            required
                .iter()
                .any(|loss| loss["feature"] == "stereo_group" && loss["disposition"] == "dropped"),
            "{refused}"
        );
        assert_eq!(snapshot(&app, id), before);
        let wrong = export(&app, id, format, Some(&"0".repeat(64)));
        assert_eq!(
            wrong["diagnostics"][0]["code"], "loss_consent_changed",
            "{wrong}"
        );
        let accepted = export(
            &app,
            id,
            format,
            Some(details["loss_digest"].as_str().unwrap()),
        );
        assert_eq!(snapshot(&app, id), before);
        let roundtrip = create_from(&app, format, content(&accepted));
        assert_eq!(
            roundtrip["stereo"]["groups"],
            json!([]),
            "{format}: no surviving singleton group"
        );
        assert_eq!(
            roundtrip["stereo"]["atoms"], original["stereo"]["atoms"],
            "{format}: only the unchanged drawing is written"
        );
        assert!(
            !accepted["loss_report"]
                .as_array()
                .unwrap()
                .iter()
                .any(|loss| loss["feature"] == "stereo_ungrouped_centers"),
            "{accepted}"
        );
    }
}

#[test]
fn stored_only_configuration_and_group_are_reported_without_synthesizing_marks() {
    for format in ["cdxml", "cdx"] {
        let directory = tempfile::tempdir().unwrap();
        let app = app(directory.path());
        let original = create_from(&app, "cdxml", &fans(&[("Absolute", None)]));
        let id = original["id"].as_str().unwrap();
        transact(
            &app,
            id,
            json!([{"op":"set_bond_mark","bond":"b2","mark":null}]),
        );
        let before = snapshot(&app, id);
        assert_eq!(
            before["stereo"], original["stereo"],
            "clearing a drawn mark must retain stored statements"
        );
        let refused = export(&app, id, format, None);
        assert_eq!(
            refused["diagnostics"][0]["code"], "loss_consent_required",
            "{refused}"
        );
        let digest = refused["diagnostics"][0]["details"]["loss_digest"]
            .as_str()
            .unwrap();
        let required = refused["diagnostics"][0]["details"]["losses"]
            .as_array()
            .unwrap();
        let required_features: Vec<_> = required
            .iter()
            .map(|loss| loss["feature"].as_str().unwrap())
            .collect();
        assert_eq!(required_features, ["stereo_configuration", "stereo_group"]);
        let accepted = export(&app, id, format, Some(digest));
        assert_eq!(
            loss(&accepted, "stereo_configuration")["disposition"],
            "dropped"
        );
        assert_eq!(loss(&accepted, "stereo_group")["disposition"], "dropped");
        let roundtrip = create_from(&app, format, content(&accepted));
        assert_eq!(roundtrip["stereo"]["atoms"], json!([]));
        assert_eq!(roundtrip["stereo"]["groups"], json!([]));
        assert!(
            roundtrip["bonds"]
                .as_array()
                .unwrap()
                .iter()
                .all(|bond| bond["mark"].is_null()),
            "{roundtrip}"
        );
        assert_eq!(snapshot(&app, id), before);
        transact(
            &app,
            id,
            json!([{"op":"move_atoms","atoms":["a1","a2","a3","a4"],"delta":{"x":1,"y":0}}]),
        );
        let changed = export(&app, id, format, Some(digest));
        assert_eq!(
            changed["diagnostics"][0]["code"], "loss_consent_changed",
            "the same losses with different output bytes must invalidate consent: {changed}"
        );
        assert!(changed["result"].is_null());
    }
}

#[test]
fn double_bond_statement_needs_consent_while_the_crossed_mark_roundtrips() {
    for format in ["cdxml", "cdx"] {
        let directory = tempfile::tempdir().unwrap();
        let app = app(directory.path());
        let source = "<CDXML><page id=\"1\"><fragment id=\"2\"><n id=\"10\" p=\"0 0\"/><n id=\"11\" p=\"30 0\"/><n id=\"12\" p=\"-20 20\"/><n id=\"13\" p=\"50 20\"/><b id=\"100\" B=\"10\" E=\"11\" Order=\"2\"/><b id=\"101\" B=\"10\" E=\"12\"/><b id=\"102\" B=\"11\" E=\"13\"/></fragment></page></CDXML>";
        let original = create_from(&app, "cdxml", source);
        let id = original["id"].as_str().unwrap();
        transact(
            &app,
            id,
            json!([
                {"op":"set_bond_configuration","bond":"b1","configuration":{"kind":"relative","begin_neighbor":"a3","end_neighbor":"a4","relation":"opposite_sides"}},
                {"op":"set_bond_mark","bond":"b1","mark":{"kind":"crossed"}}
            ]),
        );
        let before = snapshot(&app, id);
        let refused = export(&app, id, format, None);
        assert_eq!(
            refused["diagnostics"][0]["code"], "loss_consent_required",
            "{refused}"
        );
        let accepted = export(
            &app,
            id,
            format,
            Some(
                refused["diagnostics"][0]["details"]["loss_digest"]
                    .as_str()
                    .unwrap(),
            ),
        );
        let dropped = loss(&accepted, "double_bond_configuration");
        let required = refused["diagnostics"][0]["details"]["losses"]
            .as_array()
            .unwrap();
        assert_eq!(required.len(), 1, "{refused}");
        assert_eq!(required[0]["feature"], "double_bond_configuration");
        assert_eq!(dropped["disposition"], "dropped");
        let roundtrip = create_from(&app, format, content(&accepted));
        assert_eq!(roundtrip["bonds"][0]["mark"], json!({"kind":"crossed"}));
        assert_eq!(roundtrip["stereo"]["bonds"], json!([]));
        assert_eq!(snapshot(&app, id), before);
    }
}

#[test]
fn rich_tree_export_still_uses_its_original_bytes_and_rejects_ambiguous_drawing_input() {
    let directory = tempfile::tempdir().unwrap();
    let app = app(directory.path());
    let source = "<!--keep--><CDXML><page id=\"1\"><fragment id=\"2\"><n id=\"3\" p=\"0 0\"/></fragment></page></CDXML>";
    let read = call(
        &app,
        "document.import",
        json!({"mode":"read","format":"cdxml","source":source}),
    );
    let written = call(
        &app,
        "document.export",
        json!({"format":"cdxml","document":read["result"]["document"],"original":source}),
    );
    assert_eq!(content(&written), source);
    let original = create_from(&app, "cdxml", source);
    let ambiguous = call(
        &app,
        "document.export",
        json!({"format":"cdxml","document_id":original["id"],"document":read["result"]["document"]}),
    );
    assert_eq!(
        ambiguous["diagnostics"][0]["code"], "invalid_request",
        "{ambiguous}"
    );
    assert!(ambiguous["result"].is_null());
}
