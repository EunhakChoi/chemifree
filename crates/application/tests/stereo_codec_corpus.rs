//! P29g: the independently checked drawing corpus through both rich codecs and drawing export.
//!
//! Expected graph, configuration, membership and marks are compared separately. The source
//! fixture has hand-stated configurations, independently checked by verify-stereo-rdkit.py;
//! this test does not make either Chemifree codec the oracle for the other. CDXML/CDX import
//! deliberately stores no double-bond configuration. The group cases add hand-declared
//! partitions to the corpus's public two-center drawing; the original corpus has no groups.

#[path = "../../providers/tests/common/mod.rs"]
mod common;

use std::sync::atomic::{AtomicU64, Ordering};

use chemifree_application::{Application, transport_base64};
use chemifree_domain::element::Element;
use chemifree_providers::interchange_codec::LocalInterchangeCodec;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};

type App = Application<LocalCapabilities, FileDocumentRepository>;

struct Group {
    kind: &'static str,
    atoms: Vec<usize>,
}

fn fixtures() -> Vec<Value> {
    let corpus: Value = serde_json::from_str(include_str!("data/stereo-fixtures.json")).unwrap();
    assert_eq!(corpus["schema"], 1);
    assert_eq!(corpus["rdkit_version"], "2022.03.2");
    let fixtures = corpus["fixtures"].as_array().unwrap().clone();
    assert_eq!(
        fixtures.len(),
        23,
        "changes to the corpus require a coverage review"
    );
    fixtures
}

fn call(app: &App, operation: &str, parameters: Value) -> Value {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let request = json!({"contract":1,"request_id":format!("corpus-{}", NEXT.fetch_add(1, Ordering::Relaxed)),"profile":"preserve-semantics","operation":operation,"parameters":parameters});
    serde_json::to_value(app.execute(request.to_string().as_bytes())).unwrap()
}

fn import(app: &App, format: &str, source: &str) -> Value {
    let read = call(
        app,
        "document.import",
        json!({"mode":"read","format":format,"source":source}),
    );
    let choices = read["result"]["choices"]
        .as_array()
        .unwrap_or_else(|| panic!("{format}: {read}"));
    assert_eq!(choices.len(), 1, "the corpus encodes one fragment: {read}");
    let choice = &choices[0];
    assert!(choice["page_id"].is_number(), "{format}: {read}");
    assert!(choice["fragment_id"].is_number(), "{format}: {read}");
    let parameters = json!({"format":format,"source":source,"title":"Corpus",
        "page_id":choice["page_id"],"fragment_id":choice["fragment_id"]});
    let mut preview_parameters = parameters.clone();
    preview_parameters["mode"] = json!("preview");
    let preview = call(app, "document.import", preview_parameters);
    assert!(
        preview["result"]["preview_digest"].is_string(),
        "{format}: {preview}"
    );
    let mut create = parameters;
    create["mode"] = json!("create");
    create["preview_digest"] = preview["result"]["preview_digest"].clone();
    create["accept_loss"] = json!(true);
    let reply = call(app, "document.import", create);
    assert!(
        reply["result"]["document"]["id"].is_string(),
        "{format}: {reply}"
    );
    reply["result"]["document"].clone()
}

fn display(mark: &str) -> (&'static str, i16) {
    match mark {
        "" => ("Solid", 0),
        "W" => ("WedgeBegin", 6),
        "W<" => ("WedgeEnd", 7),
        "H" => ("WedgedHashBegin", 3),
        "H<" => ("WedgedHashEnd", 4),
        "w" | "x" => ("Wavy", 8),
        other => panic!("unknown corpus mark {other}"),
    }
}

fn group_at(groups: &[Group], atom: usize) -> Option<&Group> {
    groups.iter().find(|group| group.atoms.contains(&atom))
}

/// Hand-assembled encodings use only the public XML spellings and CDX wire layout.
fn source(fixture: &Value, format: &str, groups: &[Group]) -> String {
    let mut xml = String::from("<CDXML><page id=\"1\"><fragment id=\"2\">");
    let mut objects = Vec::new();
    for (index, atom) in fixture["atoms"].as_array().unwrap().iter().enumerate() {
        let element = Element::from_symbol(atom[0].as_str().unwrap())
            .unwrap()
            .atomic_number();
        let x = atom[1].as_f64().unwrap();
        let y = atom[2].as_f64().unwrap();
        let mut attrs = String::new();
        let mut properties = vec![
            common::prop(0x0200, &common::point(x, y)),
            common::prop(0x0402, &i16::from(element).to_le_bytes()),
        ];
        if let Some(group) = group_at(groups, index) {
            let (name, byte) = match group.kind {
                "absolute" => ("Absolute", 2),
                "and" => ("And", 4),
                "or" => ("Or", 3),
                other => panic!("{other}"),
            };
            attrs = format!(" EnhancedStereoType=\"{name}\"");
            properties.push(common::prop(0x0446, &[byte]));
            if group.kind != "absolute" {
                attrs.push_str(" EnhancedStereoGroupNum=\"1\"");
                properties.push(common::prop(0x0447, &[1, 0]));
            }
        }
        xml.push_str(&format!(
            "<n id=\"{}\" p=\"{x} {y}\" Element=\"{element}\"{attrs}/>",
            index + 10
        ));
        objects.push(common::obj(
            0x8004,
            u32::try_from(index + 10).unwrap(),
            &properties,
        ));
    }
    for (index, bond) in fixture["bonds"].as_array().unwrap().iter().enumerate() {
        let begin = u32::try_from(bond[0].as_u64().unwrap() + 10).unwrap();
        let end = u32::try_from(bond[1].as_u64().unwrap() + 10).unwrap();
        let order = match bond[2].as_str().unwrap() {
            "-" => 1_u16,
            "=" => 2,
            other => panic!("{other}"),
        };
        let (name, code) = display(bond[3].as_str().unwrap());
        xml.push_str(&format!(
            "<b id=\"{}\" B=\"{begin}\" E=\"{end}\" Order=\"{order}\" Display=\"{name}\"/>",
            index + 100
        ));
        objects.push(common::obj(
            0x8005,
            u32::try_from(index + 100).unwrap(),
            &[
                common::prop(0x0604, &begin.to_le_bytes()),
                common::prop(0x0605, &end.to_le_bytes()),
                common::prop(0x0600, &order.to_le_bytes()),
                common::prop(0x0601, &code.to_le_bytes()),
            ],
        ));
    }
    xml.push_str("</fragment></page></CDXML>");
    match format {
        "cdxml" => xml,
        "cdx" => transport_base64::encode(&common::file(
            0,
            &[common::obj(0x8001, 1, &[common::obj(0x8003, 2, &objects)])],
        )),
        other => panic!("{other}"),
    }
}

fn atom_id(index: u64) -> String {
    format!("a{}", index + 1)
}

/// Convert a hand-stated slot order to the snapshot order by counting inversions here,
/// independently of the domain's configuration constructors and drawing derivation.
fn expected_centers(fixture: &Value) -> Vec<Value> {
    let mut result = Vec::new();
    for center in fixture["expect"]["centers"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let mut slots: Vec<u64> = center["order"]
            .as_array()
            .unwrap()
            .iter()
            .map(|slot| match slot.as_str() {
                Some("H") => u64::MAX - 1,
                Some("LP") => u64::MAX,
                None => slot.as_u64().unwrap(),
                other => panic!("{other:?}"),
            })
            .collect();
        let mut inversions = 0;
        for first in 0..slots.len() {
            for second in first + 1..slots.len() {
                if slots[first] > slots[second] {
                    inversions += 1;
                }
            }
        }
        slots.sort_unstable();
        let neighbors: Vec<String> = slots
            .into_iter()
            .map(|slot| match slot {
                value if value == u64::MAX - 1 => "implicit_hydrogen".into(),
                u64::MAX => "lone_pair".into(),
                value => atom_id(value),
            })
            .collect();
        let mut configuration = json!({"kind":"unknown","neighbors":neighbors});
        if center["turn"] != "unknown" {
            let clockwise = (center["turn"] == "clockwise") ^ (inversions % 2 == 1);
            configuration["kind"] = json!("tetrahedral");
            configuration["turn"] = json!(if clockwise {
                "clockwise"
            } else {
                "anticlockwise"
            });
        }
        result.push(
            json!({"atom":atom_id(center["atom"].as_u64().unwrap()),"configuration":configuration}),
        );
    }
    result
}

fn expected_mark(mark: &str) -> Value {
    match mark {
        "" => Value::Null,
        "W" => json!({"kind":"wedge","narrow":"begin"}),
        "W<" => json!({"kind":"wedge","narrow":"end"}),
        "H" => json!({"kind":"hash","narrow":"begin"}),
        "H<" => json!({"kind":"hash","narrow":"end"}),
        "w" => json!({"kind":"wavy"}),
        "x" => json!({"kind":"crossed"}),
        other => panic!("{other}"),
    }
}

fn assert_profile(document: &Value, fixture: &Value, groups: &[Group], context: &str) {
    let atoms: Vec<Value> = document["atoms"]
        .as_array()
        .unwrap()
        .iter()
        .map(|atom| json!([atom["id"], atom["element"], atom["charge"], atom["isotope"]]))
        .collect();
    let expected_atoms: Vec<Value> = fixture["atoms"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .map(|(index, atom)| json!([atom_id(index as u64), atom[0], 0, null]))
        .collect();
    assert_eq!(atoms, expected_atoms, "{context}: atom graph");
    let positions: Vec<(f64, f64)> = document["atoms"]
        .as_array()
        .unwrap()
        .iter()
        .map(|atom| {
            (
                atom["position"]["x"].as_f64().unwrap(),
                atom["position"]["y"].as_f64().unwrap(),
            )
        })
        .collect();
    let expected_positions: Vec<(f64, f64)> = fixture["atoms"]
        .as_array()
        .unwrap()
        .iter()
        .map(|atom| (atom[1].as_f64().unwrap(), atom[2].as_f64().unwrap()))
        .collect();
    assert_eq!(
        positions, expected_positions,
        "{context}: drawing coordinates"
    );
    let bonds: Vec<Value> = document["bonds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|bond| json!([bond["begin"], bond["end"], bond["order"]]))
        .collect();
    let expected_bonds: Vec<Value> = fixture["bonds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|bond| {
            json!([
                atom_id(bond[0].as_u64().unwrap()),
                atom_id(bond[1].as_u64().unwrap()),
                if bond[2] == "=" { "double" } else { "single" }
            ])
        })
        .collect();
    assert_eq!(bonds, expected_bonds, "{context}: bond graph");
    let marks: Vec<Value> = document["bonds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|bond| bond["mark"].clone())
        .collect();
    let expected_marks: Vec<Value> = fixture["bonds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|bond| expected_mark(bond[3].as_str().unwrap()))
        .collect();
    assert_eq!(marks, expected_marks, "{context}: drawing marks");
    assert_eq!(
        document["stereo"]["atoms"],
        json!(expected_centers(fixture)),
        "{context}: stored configurations"
    );
    assert_eq!(
        document["stereo"]["bonds"],
        json!([]),
        "{context}: E/Z derivation is outside this codec profile"
    );
    let mut memberships: Vec<Value> = document["stereo"]["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|group| json!([group["kind"], group["number"], group["members"]]))
        .collect();
    let mut expected_groups: Vec<Value> = groups
        .iter()
        .map(|group| {
            let members: Vec<String> = group
                .atoms
                .iter()
                .map(|index| atom_id(*index as u64))
                .collect();
            json!([
                group.kind,
                if group.kind == "absolute" {
                    Value::Null
                } else {
                    json!(1)
                },
                members
            ])
        })
        .collect();
    memberships.sort_by_key(Value::to_string);
    expected_groups.sort_by_key(Value::to_string);
    assert_eq!(
        memberships, expected_groups,
        "{context}: membership partition and interpreted numbers"
    );
}

fn check_fixture(fixture: &Value, groups: &[Group]) {
    for input in ["cdxml", "cdx"] {
        let directory = tempfile::tempdir().unwrap();
        let app = Application::new(
            LocalCapabilities,
            FileDocumentRepository::new(directory.path()),
        )
        .with_interchange_codec(LocalInterchangeCodec);
        let imported = import(&app, input, &source(fixture, input, groups));
        let context = format!("{} from {input}", fixture["name"]);
        assert_profile(&imported, fixture, groups, &context);
        for output in ["cdxml", "cdx"] {
            let mut written = call(
                &app,
                "document.export",
                json!({"format":output,"document_id":imported["id"]}),
            );
            // Ungrouped centers have a documented enhanced-stereo interpretation loss.
            // Consent must bind the actual export; the retained graph/configuration/marks
            // still have to equal the independent expectations after reimport.
            if written["outcome"] == "error" {
                assert_eq!(
                    written["diagnostics"][0]["code"], "loss_consent_required",
                    "{context}: {written}"
                );
                let digest = written["diagnostics"][0]["details"]["loss_digest"]
                    .as_str()
                    .unwrap()
                    .to_owned();
                written = call(
                    &app,
                    "document.export",
                    json!({"format":output,"document_id":imported["id"],"accepted_losses":digest}),
                );
            }
            assert!(
                written["result"]["content"].is_string(),
                "{context} -> {output}: {written}"
            );
            let reread = import(&app, output, written["result"]["content"].as_str().unwrap());
            assert_profile(&reread, fixture, groups, &format!("{context} -> {output}"));
        }
    }
}

#[test]
fn all_independently_checked_drawings_cross_both_codec_boundaries() {
    for fixture in fixtures() {
        check_fixture(&fixture, &[]);
    }
}

#[test]
fn hand_declared_memberships_over_the_public_two_center_corpus_drawing_cross_both_codecs() {
    let fixture = fixtures()
        .into_iter()
        .find(|fixture| fixture["name"] == "public-two-center-drawing")
        .unwrap();
    for kind in ["absolute", "and", "or"] {
        check_fixture(
            &fixture,
            &[Group {
                kind,
                atoms: vec![1, 3],
            }],
        );
    }
    check_fixture(
        &fixture,
        &[
            Group {
                kind: "and",
                atoms: vec![1],
            },
            Group {
                kind: "or",
                atoms: vec![3],
            },
        ],
    );
}
