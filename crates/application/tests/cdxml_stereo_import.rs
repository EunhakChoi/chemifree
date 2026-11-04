//! CDXML and CDX stereo read (P29b) through the real operations: bond marks, configurations
//! derived from them, the enhanced stereo groups, the consent entries, and the refusals that
//! leave nothing created.
//!
//! The CDXML sources are hand-authored here. The configurations the shared corpus
//! `data/stereo-fixtures.json` states by hand are the expected values for the drawings it
//! describes, written as CDXML. The bond display tags of the binary CDX are not public, so the
//! binary tests cover the atom tags only and pin that a bond tag stays refused.

#[path = "../../providers/tests/common/mod.rs"]
mod common;

use std::sync::atomic::{AtomicU64, Ordering};

use chemifree_application::{Application, ListQuery, transport_base64};
use chemifree_domain::element::Element;
use chemifree_domain::stereo::{CenterState, Slot, Turn};
use chemifree_providers::interchange_codec::LocalInterchangeCodec;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};

type App = Application<LocalCapabilities, FileDocumentRepository>;

fn app(path: &std::path::Path) -> App {
    Application::new(LocalCapabilities, FileDocumentRepository::new(path))
        .with_interchange_codec(LocalInterchangeCodec)
}

fn call(app: &App, op: &str, parameters: Value) -> Value {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let id = format!("r{}", NEXT.fetch_add(1, Ordering::Relaxed));
    let request = json!({"contract":1,"request_id":id,"profile":"preserve-semantics","operation":op,"parameters":parameters});
    serde_json::to_value(app.execute(request.to_string().as_bytes())).unwrap()
}

fn count(app: &App) -> usize {
    app.list_documents(&ListQuery::default())
        .unwrap()
        .documents
        .len()
}

/// Previews `source` and, when the preview worked, creates the drawing accepting every loss.
/// Returns the preview and the creation reply (null when nothing was created).
fn import(app: &App, format: &str, source: &str) -> (Value, Value) {
    let preview = call(
        app,
        "document.import",
        json!({"mode":"preview","format":format,"source":source,"title":"S","page_id":1,"fragment_id":2}),
    );
    if preview["result"].is_null() {
        return (preview, Value::Null);
    }
    let digest = preview["result"]["preview_digest"].as_str().unwrap();
    let created = call(
        app,
        "document.import",
        json!({"mode":"create","format":format,"source":source,"title":"S","page_id":1,"fragment_id":2,"preview_digest":digest,"accept_loss":true}),
    );
    (preview, created)
}

fn features(reply: &Value) -> Vec<String> {
    reply["loss_report"]
        .as_array()
        .unwrap()
        .iter()
        .map(|loss| loss["feature"].as_str().unwrap().to_owned())
        .collect()
}

fn loss<'a>(reply: &'a Value, feature: &str) -> &'a Value {
    reply["loss_report"]
        .as_array()
        .unwrap()
        .iter()
        .find(|loss| loss["feature"] == feature)
        .unwrap_or_else(|| panic!("no {feature} loss in {reply}"))
}

/// One CDXML fragment: atoms `(symbol, x, y, extra attributes)` with ids 10.., bonds
/// `(first, second, order, extra attributes)` by atom index with ids 100.. and `B` the first.
fn cdxml(atoms: &[(&str, f64, f64, &str)], bonds: &[(usize, usize, u8, &str)]) -> String {
    let mut text = String::from("<CDXML><page id=\"1\"><fragment id=\"2\">");
    for (i, (symbol, x, y, extra)) in atoms.iter().enumerate() {
        let element = Element::from_symbol(symbol).unwrap().atomic_number();
        text += &format!(
            "<n id=\"{}\" p=\"{x} {y}\" Element=\"{element}\" {extra}/>",
            10 + i
        );
    }
    for (i, (first, second, order, extra)) in bonds.iter().enumerate() {
        text += &format!(
            "<b id=\"{}\" B=\"{}\" E=\"{}\" Order=\"{order}\" {extra}/>",
            100 + i,
            10 + first,
            10 + second
        );
    }
    text + "</fragment></page></CDXML>"
}

/// A carbon (atom 0) with F above, Cl lower right and Br lower left: the wedge-fan corpus
/// drawing. `center`, `wedge` are extra attributes of the carbon and of the carbon-Cl bond.
fn fan(center: &str, wedge: &str) -> String {
    cdxml(
        &[
            ("C", 100.0, 100.0, center),
            ("F", 100.0, 70.0, ""),
            ("Cl", 126.0, 115.0, ""),
            ("Br", 74.0, 115.0, ""),
        ],
        &[(0, 1, 1, ""), (0, 2, 1, wedge), (0, 3, 1, "")],
    )
}

fn atom_name(slot: Slot) -> String {
    match slot {
        Slot::Atom(id) => id.to_string(),
        Slot::ImplicitHydrogen => "implicit_hydrogen".into(),
        Slot::LonePair => "lone_pair".into(),
    }
}

fn expected_configuration(state: &CenterState) -> Value {
    let neighbors: Vec<String> = state.slots().iter().map(|slot| atom_name(*slot)).collect();
    match state {
        CenterState::Tetrahedral { turn, .. } => json!({
            "kind": "tetrahedral", "neighbors": neighbors,
            "turn": if *turn == Turn::Clockwise { "clockwise" } else { "anticlockwise" },
        }),
        _ => json!({"kind": "unknown", "neighbors": neighbors}),
    }
}

fn stereo_atoms(created: &Value) -> Vec<Value> {
    created["result"]["document"]["stereo"]["atoms"]
        .as_array()
        .unwrap()
        .clone()
}

// ---- the shared corpus ------------------------------------------------------------------

fn slot_of(value: &Value) -> Slot {
    use chemifree_domain::document::AtomId;
    match value.as_str() {
        Some("H") => Slot::ImplicitHydrogen,
        Some("LP") => Slot::LonePair,
        Some(other) => panic!("unknown slot {other}"),
        None => Slot::Atom(AtomId::from_serial(value.as_u64().unwrap() as u32 + 1).unwrap()),
    }
}

/// The corpus drawing as CDXML: a fixture mark `W`/`H` (narrow end at the first atom, which is
/// `B`) is `WedgeBegin`/`WedgedHashBegin`, and `W<`/`H<` (narrow end at the second atom) is
/// `...End`; `w` is `Wavy`. `None` for a drawing with a crossed mark, which CDXML cannot hold.
fn corpus_cdxml(fixture: &Value) -> Option<String> {
    let atoms: Vec<(String, f64, f64)> = fixture["atoms"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| {
            (
                a[0].as_str().unwrap().to_owned(),
                a[1].as_f64().unwrap(),
                a[2].as_f64().unwrap(),
            )
        })
        .collect();
    let mut bonds = Vec::new();
    for bond in fixture["bonds"].as_array().unwrap() {
        let display = match bond[3].as_str().unwrap() {
            "" => String::new(),
            "W" => "Display=\"WedgeBegin\"".into(),
            "W<" => "Display=\"WedgeEnd\"".into(),
            "H" => "Display=\"WedgedHashBegin\"".into(),
            "H<" => "Display=\"WedgedHashEnd\"".into(),
            "w" => "Display=\"Wavy\"".into(),
            "x" => return None,
            other => panic!("unknown mark {other}"),
        };
        let order = if bond[2] == "=" { 2 } else { 1 };
        bonds.push((
            bond[0].as_u64().unwrap() as usize,
            bond[1].as_u64().unwrap() as usize,
            order,
            display,
        ));
    }
    let atom_refs: Vec<(&str, f64, f64, &str)> = atoms
        .iter()
        .map(|(s, x, y)| (s.as_str(), *x, *y, ""))
        .collect();
    let bond_refs: Vec<(usize, usize, u8, &str)> = bonds
        .iter()
        .map(|(a, b, o, d)| (*a, *b, *o, d.as_str()))
        .collect();
    Some(cdxml(&atom_refs, &bond_refs))
}

#[test]
fn the_shared_corpus_drawn_as_cdxml_imports_the_hand_stated_configurations() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/stereo-fixtures.json"
    );
    let corpus: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let mut checked = 0;
    for fixture in corpus["fixtures"].as_array().unwrap() {
        let name = fixture["name"].as_str().unwrap();
        let Some(source) = corpus_cdxml(fixture) else {
            continue;
        };
        let dir = tempfile::tempdir().unwrap();
        let application = app(dir.path());
        let (preview, created) = import(&application, "cdxml", &source);
        assert!(!created.is_null(), "{name}: {preview}");
        let marks: Vec<Value> = created["result"]["document"]["bonds"]
            .as_array()
            .unwrap()
            .iter()
            .map(|bond| bond["mark"].clone())
            .collect();
        let expected_marks: Vec<Value> = fixture["bonds"]
            .as_array()
            .unwrap()
            .iter()
            .map(|bond| match bond[3].as_str().unwrap() {
                "" => Value::Null,
                "W" => json!({"kind":"wedge","narrow":"begin"}),
                "W<" => json!({"kind":"wedge","narrow":"end"}),
                "H" => json!({"kind":"hash","narrow":"begin"}),
                "H<" => json!({"kind":"hash","narrow":"end"}),
                "w" => json!({"kind":"wavy"}),
                other => panic!("{name}: {other}"),
            })
            .collect();
        assert_eq!(marks, expected_marks, "{name}: marks");
        let stored = stereo_atoms(&created);
        let expect = &fixture["expect"];
        let mut expected_atoms = Vec::new();
        for center in expect["centers"].as_array().into_iter().flatten() {
            let slots: Vec<Slot> = center["order"]
                .as_array()
                .unwrap()
                .iter()
                .map(slot_of)
                .collect();
            let slots: [Slot; 4] = slots.try_into().unwrap();
            let state = match center["turn"].as_str().unwrap() {
                "clockwise" => CenterState::tetrahedral(slots, Turn::Clockwise).unwrap(),
                "anticlockwise" => CenterState::tetrahedral(slots, Turn::Anticlockwise).unwrap(),
                _ => CenterState::unknown(slots).unwrap(),
            };
            expected_atoms.push(json!({
                "atom": format!("a{}", center["atom"].as_u64().unwrap() + 1),
                "configuration": expected_configuration(&state),
            }));
        }
        assert_eq!(stored, expected_atoms, "{name}: stored configurations");
        if let Some(refused) = fixture.get("refused") {
            let marked = marks.iter().any(|mark| !mark.is_null());
            let reported = features(&created).contains(&"stereo_unreadable".to_owned());
            assert_eq!(
                reported, marked,
                "{name}: a refused center is reported exactly when it is marked ({refused})"
            );
            if marked {
                // The loss names the source node, not a position among created atoms.
                let entry = loss(&created, "stereo_unreadable");
                assert_eq!(
                    entry["source_path"],
                    format!(
                        "root.children[0].children[0].children[{}]",
                        refused["atom"].as_u64().unwrap()
                    ),
                    "{name}"
                );
            }
        }
        assert!(
            created["result"]["document"]["stereo"]["bonds"]
                .as_array()
                .unwrap()
                .is_empty(),
            "{name}: a double bond is never derived at import"
        );
        checked += 1;
    }
    assert!(checked >= 20, "the corpus drawings imported: {checked}");
}

#[test]
fn a_double_bond_import_reports_the_stereo_it_did_not_examine() {
    let dir = tempfile::tempdir().unwrap();
    let application = app(dir.path());
    let source = cdxml(
        &[
            ("C", 0.0, 0.0, ""),
            ("C", 30.0, 0.0, ""),
            ("C", 45.0, 20.0, ""),
            ("C", -15.0, 20.0, ""),
        ],
        &[(0, 1, 2, ""), (1, 2, 1, ""), (0, 3, 1, "")],
    );
    let (_, created) = import(&application, "cdxml", &source);
    let report = loss(&created, "double_bond_stereo");
    assert_eq!(report["disposition"], "unknown");
    assert!(
        created["result"]["document"]["stereo"]["bonds"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

// ---- display values ---------------------------------------------------------------------

#[test]
fn begin_is_the_narrow_end_at_b_and_end_the_narrow_end_at_e() {
    let dir = tempfile::tempdir().unwrap();
    let application = app(dir.path());
    // The same picture with the bond written from the center (B) and from the chlorine (B).
    let from_center = fan("", "Display=\"WedgeBegin\"");
    let from_chlorine = cdxml(
        &[
            ("C", 100.0, 100.0, ""),
            ("F", 100.0, 70.0, ""),
            ("Cl", 126.0, 115.0, ""),
            ("Br", 74.0, 115.0, ""),
        ],
        &[
            (0, 1, 1, ""),
            (2, 0, 1, "Display=\"WedgeEnd\""),
            (0, 3, 1, ""),
        ],
    );
    let (_, first) = import(&application, "cdxml", &from_center);
    let (_, second) = import(&application, "cdxml", &from_chlorine);
    assert_eq!(
        first["result"]["document"]["bonds"][1]["mark"],
        json!({"kind":"wedge","narrow":"begin"})
    );
    assert_eq!(
        second["result"]["document"]["bonds"][1]["mark"],
        json!({"kind":"wedge","narrow":"end"})
    );
    // Both pictures show one configuration: the center is the narrow end in each.
    assert_eq!(stereo_atoms(&first), stereo_atoms(&second));
    assert_eq!(stereo_atoms(&first).len(), 1);
}

#[test]
fn a_wedge_whose_narrow_end_is_the_far_atom_derives_nothing_for_the_center() {
    let dir = tempfile::tempdir().unwrap();
    let application = app(dir.path());
    // Narrow end at the chlorine: the carbon is not the stereocenter this mark describes.
    let source = fan("", "Display=\"WedgeEnd\"");
    let (_, created) = import(&application, "cdxml", &source);
    assert_eq!(
        created["result"]["document"]["bonds"][1]["mark"],
        json!({"kind":"wedge","narrow":"end"})
    );
    assert!(
        !stereo_atoms(&created)
            .iter()
            .any(|entry| entry["atom"] == "a1"),
        "the carbon must not receive the configuration of a wedge that points away from it"
    );
}

#[test]
fn harmless_styles_import_plain_and_only_the_decorated_ones_are_noted() {
    let dir = tempfile::tempdir().unwrap();
    let application = app(dir.path());
    let (_, solid) = import(&application, "cdxml", &fan("", "Display=\"Solid\""));
    assert!(!features(&solid).contains(&"bond_display".to_owned()));
    assert!(!features(&solid).contains(&"stereo_mark_unplaceable".to_owned()));
    assert!(stereo_atoms(&solid).is_empty());
    for style in ["Bold", "Dash"] {
        let (_, created) = import(
            &application,
            "cdxml",
            &fan("", &format!("Display=\"{style}\"")),
        );
        let note = loss(&created, "bond_display");
        assert_eq!(note["semantic_impact"], "rendering", "{style}");
        assert_eq!(
            created["result"]["document"]["bonds"][1]["mark"],
            Value::Null,
            "{style}: a style is not a mark"
        );
        assert!(!features(&created).contains(&"stereo_mark_unplaceable".to_owned()));
    }
}

#[test]
fn shapes_a_bond_cannot_hold_need_consent_and_make_a_plain_bond() {
    let dir = tempfile::tempdir().unwrap();
    let application = app(dir.path());
    for display in ["Hash", "HollowWedgeBegin", "HollowWedgeEnd"] {
        let (_, created) = import(
            &application,
            "cdxml",
            &fan("", &format!("Display=\"{display}\"")),
        );
        let entry = loss(&created, "stereo_mark_unplaceable");
        assert_eq!(entry["disposition"], "dropped", "{display}");
        assert_eq!(entry["semantic_impact"], "stereo", "{display}");
        assert_eq!(
            entry["source_path"], "root.children[0].children[0].children[5].properties[3]",
            "{display}"
        );
        assert_eq!(
            created["result"]["document"]["bonds"][1]["mark"],
            Value::Null
        );
        assert!(stereo_atoms(&created).is_empty(), "{display}");
    }
    // A mark on a double bond, on either line.
    for extra in [
        "Display=\"WedgeBegin\"",
        "Display=\"Wavy\"",
        "Display2=\"WedgedHashEnd\"",
    ] {
        let source = cdxml(
            &[("C", 0.0, 0.0, ""), ("O", 30.0, 0.0, "")],
            &[(0, 1, 2, extra)],
        );
        let (_, created) = import(&application, "cdxml", &source);
        loss(&created, "stereo_mark_unplaceable");
        assert_eq!(
            created["result"]["document"]["bonds"][0]["mark"],
            Value::Null
        );
    }
}

#[test]
fn a_wavy_single_bond_is_held_and_derives_an_unknown_center() {
    let dir = tempfile::tempdir().unwrap();
    let application = app(dir.path());
    let (_, created) = import(&application, "cdxml", &fan("", "Display=\"Wavy\""));
    assert_eq!(
        created["result"]["document"]["bonds"][1]["mark"],
        json!({"kind":"wavy"})
    );
    assert_eq!(
        stereo_atoms(&created)[0]["configuration"]["kind"],
        "unknown"
    );
    assert!(!features(&created).contains(&"stereo_mark_unplaceable".to_owned()));
}

#[test]
fn a_display_value_the_public_list_lacks_refuses_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let application = app(dir.path());
    for display in [
        "Dot",
        "DashDot",
        "WavyWedgeBegin",
        "WavyWedgeEnd",
        "wedgebegin",
        "Future",
    ] {
        for attribute in ["Display", "Display2"] {
            let source = fan("", &format!("{attribute}=\"{display}\""));
            let (preview, created) = import(&application, "cdxml", &source);
            assert_eq!(
                preview["diagnostics"][0]["code"], "unsupported_bond",
                "{attribute}={display}: {preview}"
            );
            assert!(preview["result"].is_null());
            assert!(created.is_null());
        }
    }
    assert_eq!(count(&application), 0, "a refused file creates nothing");
}

#[test]
fn attributes_without_a_public_meaning_stay_refused() {
    let dir = tempfile::tempdir().unwrap();
    let application = app(dir.path());
    for extra in ["DoublePosition=\"Right\"", "CIP=\"E\"", "Unheard=\"1\""] {
        let source = fan("", extra);
        let (preview, _) = import(&application, "cdxml", &source);
        assert_eq!(
            preview["diagnostics"][0]["code"], "unsupported_bond",
            "{extra}"
        );
    }
    let (preview, _) = import(
        &application,
        "cdxml",
        &fan("ShowAtomEnhancedStereo=\"yes\"", ""),
    );
    assert_eq!(preview["diagnostics"][0]["code"], "unsupported_node");
    assert_eq!(count(&application), 0);
}

// ---- atom hints -------------------------------------------------------------------------

#[test]
fn geometry_bond_ordering_and_the_cip_label_are_noted_and_never_stored() {
    let dir = tempfile::tempdir().unwrap();
    let application = app(dir.path());
    let hints = "Geometry=\"Tetrahedral\" BondOrdering=\"100 101 0 102\" AS=\"S\"";
    let (_, with) = import(&application, "cdxml", &fan(hints, "Display=\"WedgeBegin\""));
    let (_, without) = import(&application, "cdxml", &fan("", "Display=\"WedgeBegin\""));
    // The configuration comes from the marks alone; the hints change nothing stored.
    assert_eq!(stereo_atoms(&with), stereo_atoms(&without));
    assert_eq!(stereo_atoms(&with).len(), 1);
    for feature in ["stereo_geometry", "bond_ordering", "cip_label_not_stored"] {
        let entry = loss(&with, feature);
        assert_eq!(entry["disposition"], "dropped", "{feature}");
        assert!(
            entry["source_path"]
                .as_str()
                .unwrap()
                .starts_with("root.children[0].children[0].children[0].properties["),
            "{feature}: {entry}"
        );
    }
    assert!(!features(&without).contains(&"cip_label_not_stored".to_owned()));
    // Read properties are accounted by their own entries, not also as unprojected.
    for reply in [&with, &without] {
        assert!(
            !features(reply).contains(&"unprojected_property".to_owned()),
            "{reply}"
        );
    }
}

#[test]
fn a_cip_label_that_disagrees_with_the_drawing_is_not_trusted() {
    // Both labels on the same drawing: the stored configuration is the drawing's, whichever
    // label the file carries.
    let dir = tempfile::tempdir().unwrap();
    let application = app(dir.path());
    let (_, r) = import(
        &application,
        "cdxml",
        &fan("AS=\"R\"", "Display=\"WedgeBegin\""),
    );
    let (_, s) = import(
        &application,
        "cdxml",
        &fan("AS=\"S\"", "Display=\"WedgeBegin\""),
    );
    assert_eq!(stereo_atoms(&r), stereo_atoms(&s));
}

#[test]
fn a_geometry_or_ordering_that_cannot_be_read_refuses_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let application = app(dir.path());
    let (preview, _) = import(&application, "cdxml", &fan("Geometry=\"Planar\"", ""));
    assert_eq!(preview["diagnostics"][0]["code"], "unsupported_node");
    for ordering in ["a b", "1 -1", ""] {
        let (preview, _) = import(
            &application,
            "cdxml",
            &fan(&format!("BondOrdering=\"{ordering}\""), ""),
        );
        assert_eq!(
            preview["diagnostics"][0]["code"], "invalid_stereo_attribute",
            "{ordering:?}"
        );
        assert_eq!(
            preview["diagnostics"][0]["details"]["family"],
            "malformed_input"
        );
    }
    assert_eq!(count(&application), 0);
}

// ---- enhanced stereo groups -------------------------------------------------------------

/// `count` disjoint wedge fans (carbon, F, Cl, Br each) whose carbons carry `attributes[i]`.
fn fans(attributes: &[&str]) -> String {
    let mut atoms = Vec::new();
    let mut bonds = Vec::new();
    for (i, extra) in attributes.iter().enumerate() {
        let x = 200.0 * i as f64;
        let base = 4 * i;
        atoms.push(("C", x + 100.0, 100.0, *extra));
        atoms.push(("F", x + 100.0, 70.0, ""));
        atoms.push(("Cl", x + 126.0, 115.0, ""));
        atoms.push(("Br", x + 74.0, 115.0, ""));
        bonds.push((base, base + 1, 1, ""));
        bonds.push((base, base + 2, 1, "Display=\"WedgeBegin\""));
        bonds.push((base, base + 3, 1, ""));
    }
    cdxml(&atoms, &bonds)
}

fn groups(created: &Value) -> Vec<(String, Value, Value, Vec<String>)> {
    created["result"]["document"]["stereo"]["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|group| {
            (
                group["kind"].as_str().unwrap().to_owned(),
                group["stored_number"].clone(),
                group["number"].clone(),
                group["members"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|m| m.as_str().unwrap().to_owned())
                    .collect(),
            )
        })
        .collect()
}

fn typed(kind: &str, number: Option<i64>) -> String {
    match number {
        Some(n) => format!("EnhancedStereoType=\"{kind}\" EnhancedStereoGroupNum=\"{n}\""),
        None => format!("EnhancedStereoType=\"{kind}\""),
    }
}

#[test]
fn groups_import_with_their_kind_and_number_as_stated() {
    let dir = tempfile::tempdir().unwrap();
    let application = app(dir.path());
    let source = fans(&[
        &typed("Absolute", None),
        &typed("Or", Some(1)),
        &typed("And", Some(1)),
        &typed("Or", Some(1)),
    ]);
    let (_, created) = import(&application, "cdxml", &source);
    assert!(!created.is_null());
    let found = groups(&created);
    // Or 1 and And 1 stay separate; the two Or 1 atoms are one group.
    assert_eq!(found.len(), 3, "{found:?}");
    assert!(found.contains(&(
        "absolute".into(),
        Value::Null,
        Value::Null,
        vec!["a1".into()]
    )));
    assert!(found.contains(&("and".into(), json!(1), json!(1), vec!["a9".into()])));
    assert!(found.contains(&(
        "or".into(),
        json!(1),
        json!(1),
        vec!["a5".into(), "a13".into()]
    )));
    assert_eq!(stereo_atoms(&created).len(), 4);
}

#[test]
fn an_omitted_number_is_zero_and_joins_a_stated_zero() {
    let dir = tempfile::tempdir().unwrap();
    let application = app(dir.path());
    let source = fans(&[
        &typed("Or", None),
        &typed("Or", Some(0)),
        &typed("And", None),
    ]);
    let (_, created) = import(&application, "cdxml", &source);
    let found = groups(&created);
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found.contains(&(
        "or".into(),
        json!(0),
        json!(0),
        vec!["a1".into(), "a5".into()]
    )));
    assert!(found.contains(&("and".into(), json!(0), json!(0), vec!["a9".into()])));
    let note = loss(&created, "stereo_group_number");
    assert_eq!(note["disposition"], "normalized");
}

#[test]
fn numbers_above_127_and_beyond_258_stay_as_stated_and_never_merge() {
    let dir = tempfile::tempdir().unwrap();
    let application = app(dir.path());
    let source = fans(&[
        &typed("Or", Some(2)),
        &typed("Or", Some(258)),
        &typed("Or", Some(128)),
        &typed("Or", Some(-128)),
        &typed("And", Some(i64::from(i32::MAX))),
    ]);
    let (_, created) = import(&application, "cdxml", &source);
    let found = groups(&created);
    assert_eq!(found.len(), 5, "Or2 and Or258 are two groups: {found:?}");
    for (kind, number, atom) in [
        ("or", 2, "a1"),
        ("or", 258, "a5"),
        ("or", 128, "a9"),
        ("or", -128, "a13"),
        ("and", i64::from(i32::MAX), "a17"),
    ] {
        assert!(
            found.contains(&(kind.into(), json!(number), json!(number), vec![atom.into()])),
            "{kind}{number}: {found:?}"
        );
    }
}

#[test]
fn an_absolute_atom_with_a_number_and_a_none_type_are_noted_not_grouped() {
    let dir = tempfile::tempdir().unwrap();
    let application = app(dir.path());
    let source = fans(&[
        &typed("Absolute", Some(7)),
        "EnhancedStereoType=\"None\"",
        "EnhancedStereoType=\"Unspecified\"",
    ]);
    let (_, created) = import(&application, "cdxml", &source);
    let found = groups(&created);
    assert_eq!(
        found,
        vec![(
            "absolute".into(),
            Value::Null,
            Value::Null,
            vec!["a1".into()]
        )]
    );
    assert_eq!(
        loss(&created, "stereo_group_number")["disposition"],
        "dropped"
    );
    assert!(
        loss(&created, "enhanced_stereo_none")["source_path"]
            .as_str()
            .unwrap()
            .ends_with(".children[4]"),
        "{created}"
    );
}

#[test]
fn a_group_member_that_is_not_a_derived_center_refuses_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let application = app(dir.path());
    // The first fan's center is a stereocenter; the Br atom of the second states a group but
    // has no wedge, so no configuration is derived for it.
    let mut source = fans(&[&typed("Or", Some(1)), ""]);
    source = source.replacen(
        "<n id=\"17\"",
        &format!("<n {} id=\"17\"", typed("Or", Some(1))),
        1,
    );
    let (preview, created) = import(&application, "cdxml", &source);
    assert_eq!(
        preview["diagnostics"][0]["code"],
        "stereo_group_member_not_stereocenter"
    );
    assert_eq!(
        preview["diagnostics"][0]["details"]["family"],
        "malformed_input"
    );
    assert!(created.is_null());
    // The same refusal on the create path, which has its own entry.
    let digest = "unavailable";
    let create = call(
        &application,
        "document.import",
        json!({"mode":"create","format":"cdxml","source":source,"title":"S","page_id":1,"fragment_id":2,"preview_digest":digest,"accept_loss":true}),
    );
    assert_eq!(
        create["diagnostics"][0]["code"],
        "stereo_group_member_not_stereocenter"
    );
    assert_eq!(count(&application), 0, "nothing is created");
}

#[test]
fn a_group_member_without_any_mark_and_a_wavy_member_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let application = app(dir.path());
    // No wedge at all: nothing derives, the atom is no stereocenter.
    let plain = cdxml(
        &[
            ("C", 100.0, 100.0, "EnhancedStereoType=\"Absolute\""),
            ("F", 100.0, 70.0, ""),
            ("Cl", 126.0, 115.0, ""),
            ("Br", 74.0, 115.0, ""),
        ],
        &[(0, 1, 1, ""), (0, 2, 1, ""), (0, 3, 1, "")],
    );
    let (preview, _) = import(&application, "cdxml", &plain);
    assert_eq!(
        preview["diagnostics"][0]["code"],
        "stereo_group_member_not_stereocenter"
    );
    // A wavy center derives `unknown`, which a group cannot hold.
    let wavy = fan("EnhancedStereoType=\"Absolute\"", "Display=\"Wavy\"");
    let (preview, _) = import(&application, "cdxml", &wavy);
    assert_eq!(
        preview["diagnostics"][0]["code"],
        "stereo_group_member_not_stereocenter"
    );
    assert_eq!(count(&application), 0);
}

#[test]
fn a_malformed_group_number_or_type_refuses_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let application = app(dir.path());
    for extra in [
        "EnhancedStereoType=\"Or\" EnhancedStereoGroupNum=\"2147483648\"",
        "EnhancedStereoType=\"Or\" EnhancedStereoGroupNum=\"x\"",
        "EnhancedStereoType=\"Maybe\"",
        "EnhancedStereoType=\"or\"",
    ] {
        let (preview, _) = import(&application, "cdxml", &fan(extra, "Display=\"WedgeBegin\""));
        assert_eq!(
            preview["diagnostics"][0]["code"], "invalid_stereo_attribute",
            "{extra}: {preview}"
        );
    }
    assert_eq!(count(&application), 0);
}

#[test]
fn the_group_number_spelling_of_the_public_files_is_the_one_read() {
    // `EnhancedStereoGroupNumber` is the COM property name, not the file attribute: it is an
    // unknown attribute and the file is refused rather than read as number 0.
    let dir = tempfile::tempdir().unwrap();
    let application = app(dir.path());
    let source = fan(
        "EnhancedStereoType=\"Or\" EnhancedStereoGroupNumber=\"3\"",
        "Display=\"WedgeBegin\"",
    );
    let (preview, _) = import(&application, "cdxml", &source);
    assert_eq!(preview["diagnostics"][0]["code"], "unsupported_node");
}

#[test]
fn the_root_level_enhanced_stereo_display_flag_is_reported_and_stereo_still_imports() {
    let dir = tempfile::tempdir().unwrap();
    let application = app(dir.path());
    let source = fans(&[&typed("Or", Some(1))]).replacen(
        "<CDXML>",
        "<CDXML ShowAtomEnhancedStereo=\"yes\">",
        1,
    );
    let (_, created) = import(&application, "cdxml", &source);
    assert_eq!(groups(&created).len(), 1);
    let entry = created["loss_report"]
        .as_array()
        .unwrap()
        .iter()
        .find(|loss| loss["source_path"] == "root.properties[0]")
        .unwrap_or_else(|| panic!("{created}"));
    assert_eq!(entry["feature"], "unprojected_property");
    assert_eq!(entry["disposition"], "dropped");
}

// ---- binary CDX -------------------------------------------------------------------------

/// A CDX fragment with a carbon at the origin and three neighbours; `center` are extra node
/// properties of the carbon, `bond` extra properties of its first bond.
fn cdx(center: &[Vec<u8>], bond: &[Vec<u8>]) -> String {
    use common::{i16le, obj, point, prop, u16le, u32le};
    let node = |id: u32, x: f64, y: f64, element: i16, extra: &[Vec<u8>]| {
        let mut parts = vec![prop(0x0200, &point(x, y)), prop(0x0402, &i16le(element))];
        parts.extend(extra.iter().cloned());
        obj(0x8004, id, &parts)
    };
    let edge = |id: u32, end: u32, extra: &[Vec<u8>]| {
        let mut parts = vec![
            prop(0x0604, &u32le(3)),
            prop(0x0605, &u32le(end)),
            prop(0x0600, &u16le(1)),
        ];
        parts.extend(extra.iter().cloned());
        obj(0x8005, id, &parts)
    };
    let fragment = obj(
        0x8003,
        2,
        &[
            node(3, 100.0, 100.0, 6, center),
            node(4, 100.0, 70.0, 9, &[]),
            node(5, 126.0, 115.0, 17, &[]),
            node(6, 74.0, 115.0, 35, &[]),
            edge(7, 4, bond),
            edge(8, 5, &[]),
            edge(9, 6, &[]),
        ],
    );
    let page = obj(0x8001, 1, &[fragment]);
    transport_base64::encode(&common::file(0, &[page]))
}

#[test]
fn cdx_atom_hints_are_noted_and_a_group_member_without_a_readable_mark_is_refused() {
    use common::prop;
    let dir = tempfile::tempdir().unwrap();
    let application = app(dir.path());
    let ordering: Vec<u8> = [7_u32, 8, 0, 9]
        .iter()
        .flat_map(|n| n.to_le_bytes())
        .collect();
    let hints = [prop(0x0430, &[7]), prop(0x0431, &ordering)];
    let (_, created) = import(&application, "cdx", &cdx(&hints, &[]));
    assert!(!created.is_null());
    let found = features(&created);
    assert!(found.contains(&"stereo_geometry".to_owned()), "{found:?}");
    assert!(found.contains(&"bond_ordering".to_owned()), "{found:?}");
    assert!(stereo_atoms(&created).is_empty());
    // An enhanced stereo type on an atom: the binary bond marks are not read, so no
    // configuration exists for the member and the file is refused, not guessed.
    let member = [prop(0x0446, &[3]), prop(0x0447, &[0x02, 0x01])];
    let (preview, created) = import(&application, "cdx", &cdx(&member, &[]));
    assert_eq!(
        preview["diagnostics"][0]["code"],
        "stereo_group_member_not_stereocenter"
    );
    assert!(created.is_null());
    assert_eq!(
        count(&application),
        1,
        "only the hints-only file was created"
    );
}

#[test]
fn a_binary_bond_display_tag_is_not_guessed_and_stays_refused() {
    use common::prop;
    let dir = tempfile::tempdir().unwrap();
    let application = app(dir.path());
    // 0601, 0602 and 0603 are the tags of other tools' notes; the public sources used here do
    // not give them, so the property is unread and the bond refused as before.
    for tag in [0x0601_u16, 0x0602, 0x0603] {
        let (preview, _) = import(&application, "cdx", &cdx(&[], &[prop(tag, &[6])]));
        assert_eq!(
            preview["diagnostics"][0]["code"], "unsupported_bond",
            "{tag:#06x}: {preview}"
        );
    }
    assert_eq!(count(&application), 0);
}

#[test]
fn cdx_carriers_of_the_wrong_shape_are_refused() {
    use common::prop;
    let dir = tempfile::tempdir().unwrap();
    let application = app(dir.path());
    for property in [
        prop(0x0430, &[6]),
        prop(0x0431, &[1, 2, 3]),
        prop(0x0446, &[9]),
        prop(0x0447, &[1]),
    ] {
        let (preview, _) = import(&application, "cdx", &cdx(&[property], &[]));
        assert!(
            ["unsupported_node", "invalid_stereo_attribute"]
                .contains(&preview["diagnostics"][0]["code"].as_str().unwrap()),
            "{preview}"
        );
        assert!(preview["result"].is_null());
    }
}
