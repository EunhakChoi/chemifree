//! Stereo that a document already holds, seen from the operations that cannot read it.
//!
//! Every document here is built with the domain API, saved with the real file store, and then
//! read by the Application, so that these tests do not depend on the write commands (those
//! are covered in `stereo_wire.rs`). The tests pin what the snapshot shows and that comparison
//! and export never present such a drawing as a plain structure: `partial` with a loss entry
//! for comparisons, consent for SMILES, and a refusal for V2000.

use chemifree_application::{Application, DocumentRepository};
use chemifree_domain::document::{
    Atom, AtomId, AtomRef, BondId, BondOrder, BondRef, Command, Document, DocumentId,
    NewAtomPositions, Point,
};
use chemifree_domain::element::Element;
use chemifree_domain::stereo::{
    AtomConfiguration, BondConfiguration, BondEnd, Relation, SlotRef, StereoMark, Turn,
};
use chemifree_providers::interchange_codec::LocalInterchangeCodec;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities, OpenSmilesCodec, V2000Codec};
use serde_json::{Value, json};
use tempfile::TempDir;

type App = Application<LocalCapabilities, FileDocumentRepository>;

fn id(serial: u32) -> DocumentId {
    DocumentId::parse(&format!("doc-{serial:016x}")).unwrap()
}

fn name(serial: u32) -> String {
    id(serial).to_string()
}

fn atom(serial: u32) -> AtomId {
    AtomId::from_serial(serial).unwrap()
}

fn bond(serial: u32) -> BondId {
    BondId::from_serial(serial).unwrap()
}

fn s(serial: u32) -> SlotRef {
    SlotRef::Atom(AtomRef::Id(atom(serial)))
}

fn new_atom(symbol: &str) -> Command {
    Command::AddAtom {
        atom: Atom::from(Element::from_symbol(symbol).unwrap()),
        positions: NewAtomPositions::Single(Point::new(0.0, 0.0).unwrap()),
        bond_to: None,
    }
}

fn add_bond(begin: u32, end: u32, order: BondOrder) -> Command {
    Command::AddBond {
        begin: AtomRef::Id(atom(begin)),
        end: AtomRef::Id(atom(end)),
        order,
    }
}

fn mark(bond_serial: u32, mark: StereoMark) -> Command {
    Command::SetBondMark {
        bond: BondRef::Id(bond(bond_serial)),
        mark: Some(mark),
    }
}

fn tetra(center: u32, neighbors: [SlotRef; 4], turn: Turn) -> Command {
    Command::SetAtomConfiguration {
        atom: AtomRef::Id(atom(center)),
        configuration: Some(AtomConfiguration::Tetrahedral { neighbors, turn }),
    }
}

/// `a1` bonded to F `a2`, Cl `a3`, Br `a4`, N `a5` (bonds `b1`-`b4`), with the stereo given.
fn halo_center(serial: u32, stereo: &[Command]) -> Document {
    let mut document = Document::new(id(serial), "Center").unwrap();
    let mut commands: Vec<Command> = ["C", "F", "Cl", "Br", "N"]
        .into_iter()
        .map(new_atom)
        .collect();
    commands.extend((2..=5).map(|n| add_bond(1, n, BondOrder::Single)));
    document.apply(&commands).unwrap();
    if !stereo.is_empty() {
        document.apply(stereo).unwrap();
    }
    document
}

const WEDGE_BEGIN: StereoMark = StereoMark::Wedge {
    narrow: BondEnd::Begin,
};

/// Two drawings of the same graph that differ only in the stated turn: mirror images.
fn mirror_pair() -> [Document; 2] {
    let drawn = |serial, turn| {
        halo_center(
            serial,
            &[
                mark(1, WEDGE_BEGIN),
                tetra(1, [s(2), s(3), s(4), s(5)], turn),
            ],
        )
    };
    [drawn(1, Turn::Clockwise), drawn(2, Turn::Anticlockwise)]
}

/// A drawing that holds every kind of mark and configuration. Atoms `a1`-`a5` are a center,
/// `a6=a7` (with `a8`, `a9`) an alkene, `a10`-`a13` a three-bond center, `a14=a15` a bare
/// double bond. The graph is the same with or without the stereo.
fn full(serial: u32, with_stereo: bool) -> Document {
    let mut document = Document::new(id(serial), "Full").unwrap();
    let mut commands: Vec<Command> = (0..15).map(|_| new_atom("C")).collect();
    commands.extend([
        add_bond(1, 2, BondOrder::Single),
        add_bond(1, 3, BondOrder::Single),
        add_bond(1, 4, BondOrder::Single),
        add_bond(1, 5, BondOrder::Single),
        add_bond(6, 7, BondOrder::Double),
        add_bond(6, 8, BondOrder::Single),
        add_bond(7, 9, BondOrder::Single),
        add_bond(10, 11, BondOrder::Single),
        add_bond(10, 12, BondOrder::Single),
        add_bond(10, 13, BondOrder::Single),
        add_bond(14, 15, BondOrder::Double),
    ]);
    document.apply(&commands).unwrap();
    if with_stereo {
        document
            .apply(&[
                mark(1, WEDGE_BEGIN),
                mark(
                    2,
                    StereoMark::Hash {
                        narrow: BondEnd::End,
                    },
                ),
                mark(3, StereoMark::Wavy),
                mark(5, StereoMark::Crossed),
                tetra(1, [s(3), s(2), s(4), s(5)], Turn::Clockwise),
                Command::SetAtomConfiguration {
                    atom: AtomRef::Id(atom(10)),
                    configuration: Some(AtomConfiguration::Unknown {
                        neighbors: [s(13), SlotRef::LonePair, s(11), s(12)],
                    }),
                },
                Command::SetBondConfiguration {
                    bond: BondRef::Id(bond(5)),
                    configuration: Some(BondConfiguration::Relative {
                        begin_neighbor: s(8),
                        end_neighbor: SlotRef::ImplicitHydrogen,
                        relation: Relation::SameSide,
                    }),
                },
                Command::SetBondConfiguration {
                    bond: BondRef::Id(bond(11)),
                    configuration: Some(BondConfiguration::Unknown),
                },
            ])
            .unwrap();
    }
    document
}

/// An application over a data directory that already holds `documents`.
fn app_with(documents: &[Document]) -> (App, TempDir) {
    let directory = tempfile::tempdir().unwrap();
    let repository = FileDocumentRepository::new(directory.path());
    for document in documents {
        repository.save(document, None).unwrap();
    }
    let app = Application::new(LocalCapabilities, repository)
        .with_ctfile_codec(V2000Codec)
        .with_interchange_codec(LocalInterchangeCodec)
        .with_smiles_codec(OpenSmilesCodec);
    (app, directory)
}

/// The response schema's coupling of outcome and loss list, checked on every reply: `ok` never
/// carries a dropped, rejected, or unknown entry, and `partial` always carries one.
fn assert_outcome_matches_losses(reply: &Value) {
    let losses = reply["loss_report"]
        .as_array()
        .expect("loss_report is a list");
    match reply["outcome"].as_str().expect("an outcome") {
        "ok" => {
            for loss in losses {
                assert!(
                    matches!(
                        loss["disposition"].as_str(),
                        Some("retained" | "normalized")
                    ),
                    "an ok reply carries a {} loss: {reply}",
                    loss["disposition"]
                );
            }
        }
        "partial" => assert!(!losses.is_empty(), "a partial reply with no loss: {reply}"),
        _ => {}
    }
}

fn call(app: &App, request: &str, operation: &str, parameters: Value) -> Value {
    let body = json!({"contract":1,"request_id":request,"profile":"preserve-semantics","operation":operation,"parameters":parameters});
    let reply = serde_json::to_value(app.execute(body.to_string().as_bytes())).unwrap();
    assert_outcome_matches_losses(&reply);
    reply
}

fn snapshot(app: &App, document: u32) -> Value {
    let reply = call(
        app,
        "snapshot",
        "document.snapshot",
        json!({"document_id": name(document)}),
    );
    assert_eq!(reply["outcome"], "ok", "{reply}");
    reply["result"]["document"].clone()
}

fn losses(reply: &Value) -> Vec<(&str, &str)> {
    reply["loss_report"]
        .as_array()
        .unwrap()
        .iter()
        .map(|loss| {
            (
                loss["feature"].as_str().unwrap(),
                loss["source_path"].as_str().unwrap(),
            )
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Snapshots
// ---------------------------------------------------------------------------------------------

#[test]
fn a_snapshot_shows_every_mark_and_every_stated_configuration() {
    let (app, _directory) = app_with(&[full(1, true)]);
    let document = snapshot(&app, 1);
    let marks: Vec<Value> = document["bonds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|bond| bond["mark"].clone())
        .collect();
    assert_eq!(
        marks,
        [
            json!({"kind": "wedge", "narrow": "begin"}),
            json!({"kind": "hash", "narrow": "end"}),
            json!({"kind": "wavy"}),
            Value::Null,
            json!({"kind": "crossed"}),
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
        ],
        "every bond carries a mark key; b4 and the rest are plain"
    );
    // The bond's own ends are not rewritten by a mark's narrow end.
    assert_eq!(document["bonds"][1]["begin"], "a1");
    assert_eq!(document["bonds"][1]["end"], "a3");
    assert_eq!(
        document["stereo"],
        json!({
            "atoms": [
                {"atom": "a1", "configuration": {
                    "kind": "tetrahedral",
                    "neighbors": ["a2", "a3", "a4", "a5"],
                    "turn": "anticlockwise"}},
                {"atom": "a10", "configuration": {
                    "kind": "unknown",
                    "neighbors": ["a11", "a12", "a13", "lone_pair"]}}
            ],
            "bonds": [
                {"bond": "b5", "configuration": {
                    "kind": "relative",
                    "begin_neighbor": "a8",
                    "end_neighbor": "a9",
                    "relation": "opposite_sides"}},
                {"bond": "b11", "configuration": {"kind": "unknown"}}
            ],
            "groups": []
        }),
        "[a3,a2,a4,a5] is one swap from sorted, so the turn flips; the hydrogen at a7 is a9 \
         with the relation flipped"
    );
}

#[test]
fn a_drawing_without_stereo_says_nothing_about_it() {
    let (app, _directory) = app_with(&[full(1, false)]);
    let document = snapshot(&app, 1);
    assert!(
        document["bonds"]
            .as_array()
            .unwrap()
            .iter()
            .all(|bond| bond.get("mark") == Some(&Value::Null)),
        "a plain bond has an explicit null mark"
    );
    assert_eq!(
        document["stereo"],
        json!({"atoms": [], "bonds": [], "groups": []})
    );
}

#[test]
fn stereo_survives_a_service_restart() {
    let (app, directory) = app_with(&[full(1, true)]);
    let before = snapshot(&app, 1);
    assert_eq!(before["stereo"]["atoms"].as_array().unwrap().len(), 2);
    assert_eq!(before["stereo"]["bonds"].as_array().unwrap().len(), 2);
    drop(app);
    let restarted: App = Application::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    );
    let reply = call(
        &restarted,
        "after",
        "document.snapshot",
        json!({"document_id": name(1)}),
    );
    assert_eq!(reply["result"]["document"]["stereo"], before["stereo"]);
    assert_eq!(reply["result"]["document"]["bonds"], before["bonds"]);
}

// ---------------------------------------------------------------------------------------------
// Comparison
// ---------------------------------------------------------------------------------------------

#[test]
fn mirror_images_compare_equivalent_and_the_reply_says_stereo_was_not_read() {
    let [left, right] = mirror_pair();
    let (app, _directory) = app_with(&[left, right]);
    for mode in ["recorded", "chemical"] {
        let reply = call(
            &app,
            mode,
            "chem.identity",
            json!({"document_id": name(1), "other_document_id": name(2), "mode": mode}),
        );
        assert_eq!(reply["outcome"], "partial", "{mode}: {reply}");
        assert_eq!(
            reply["result"]["comparison"]["equivalent"], true,
            "{mode}: the graph axis is stereo-blind"
        );
        assert_eq!(
            reply["result"]["stereo"],
            json!({"compared": false, "source_present": true, "other_present": true}),
            "{mode}"
        );
        assert_eq!(
            losses(&reply),
            [("stereo", "snapshot"), ("stereo", "comparison.snapshot")],
            "{mode}"
        );
        for loss in reply["loss_report"].as_array().unwrap() {
            assert_eq!(loss["disposition"], "unknown");
            assert_eq!(loss["semantic_impact"], "stereo");
        }
    }
}

#[test]
fn identity_names_only_the_side_that_holds_stereo() {
    let [marked, _] = mirror_pair();
    let plain = halo_center(3, &[]);
    let (app, _directory) = app_with(&[marked, plain]);
    let parameters = |source, other: Option<u32>| json!({"document_id": name(source), "other_document_id": other.map(name), "mode": "recorded"});
    let reply = call(
        &app,
        "marked-plain",
        "chem.identity",
        parameters(1, Some(3)),
    );
    assert_eq!(reply["outcome"], "partial");
    assert_eq!(
        reply["result"]["stereo"],
        json!({"compared": false, "source_present": true, "other_present": false})
    );
    assert_eq!(losses(&reply), [("stereo", "snapshot")]);

    let reply = call(
        &app,
        "plain-marked",
        "chem.identity",
        parameters(3, Some(1)),
    );
    assert_eq!(reply["outcome"], "partial");
    assert_eq!(
        reply["result"]["stereo"],
        json!({"compared": false, "source_present": false, "other_present": true})
    );
    assert_eq!(losses(&reply), [("stereo", "comparison.snapshot")]);

    let reply = call(&app, "alone", "chem.identity", parameters(1, None));
    assert_eq!(reply["outcome"], "partial");
    assert_eq!(
        reply["result"]["stereo"],
        json!({"compared": false, "source_present": true, "other_present": null})
    );
    assert_eq!(losses(&reply), [("stereo", "snapshot")]);
}

#[test]
fn identity_of_stereo_free_drawings_stays_ok_with_the_model_property() {
    let (app, _directory) = app_with(&[halo_center(3, &[]), halo_center(4, &[])]);
    let reply = call(
        &app,
        "plain",
        "chem.identity",
        json!({"document_id": name(3), "other_document_id": name(4), "mode": "chemical"}),
    );
    assert_eq!(reply["outcome"], "ok", "{reply}");
    assert_eq!(reply["loss_report"], json!([]));
    assert_eq!(reply["diagnostics"], json!([]));
    assert_eq!(
        reply["result"]["stereo"],
        json!({"compared": false, "source_present": false, "other_present": false}),
        "compared is a property of the model, so it is false here too"
    );
    assert_eq!(reply["result"]["comparison"]["equivalent"], true);
}

#[test]
fn substructure_search_reports_which_side_holds_stereo_it_did_not_read() {
    let [left, right] = mirror_pair();
    let plain = halo_center(3, &[]);
    let (app, _directory) = app_with(&[left, right, plain]);
    let search = |query, target| {
        call(
            &app,
            "search",
            "chem.substructure",
            json!({"query_document_id": name(query), "target_document_id": name(target)}),
        )
    };
    let both = search(1, 2);
    assert_eq!(both["outcome"], "partial", "{both}");
    assert_eq!(
        both["result"]["stereo"],
        json!({"compared": false, "query_present": true, "target_present": true})
    );
    assert_eq!(losses(&both), [("stereo", "query"), ("stereo", "target")]);
    assert!(
        !both["result"]["mappings"].as_array().unwrap().is_empty(),
        "the mirror image is found in its mirror image: stereo is not read"
    );
    for loss in both["loss_report"].as_array().unwrap() {
        assert_eq!(loss["disposition"], "unknown");
        assert_eq!(loss["semantic_impact"], "stereo");
    }

    let target_only = search(3, 1);
    assert_eq!(target_only["outcome"], "partial");
    assert_eq!(
        target_only["result"]["stereo"],
        json!({"compared": false, "query_present": false, "target_present": true})
    );
    assert_eq!(losses(&target_only), [("stereo", "target")]);

    let query_only = search(1, 3);
    assert_eq!(losses(&query_only), [("stereo", "query")]);
}

#[test]
fn substructure_search_of_stereo_free_drawings_stays_ok() {
    let (app, _directory) = app_with(&[halo_center(3, &[]), halo_center(4, &[])]);
    let reply = call(
        &app,
        "plain",
        "chem.substructure",
        json!({"query_document_id": name(3), "target_document_id": name(4)}),
    );
    assert_eq!(reply["outcome"], "ok", "{reply}");
    assert_eq!(reply["loss_report"], json!([]));
    assert_eq!(
        reply["result"]["stereo"],
        json!({"compared": false, "query_present": false, "target_present": false})
    );
}

// ---------------------------------------------------------------------------------------------
// SMILES export
// ---------------------------------------------------------------------------------------------

fn export_smiles(
    app: &App,
    request: &str,
    document: u32,
    style: &str,
    digest: Option<&str>,
) -> Value {
    let mut parameters = json!({"format": "smiles", "document_id": name(document), "style": style});
    if let Some(digest) = digest {
        parameters["accepted_losses"] = json!(digest);
    }
    call(app, request, "document.export", parameters)
}

fn consent_features(refusal: &Value) -> Vec<&str> {
    refusal["diagnostics"][0]["details"]["features"]
        .as_array()
        .unwrap()
        .iter()
        .map(|feature| feature.as_str().unwrap())
        .collect()
}

#[test]
fn smiles_export_of_stereo_needs_consent_in_both_styles_and_then_writes_the_graph_alone() {
    let (app, _directory) = app_with(&[full(1, true), full(2, false)]);
    for style in ["recorded", "canonical"] {
        let refused = export_smiles(&app, "first", 1, style, None);
        assert_eq!(refused["outcome"], "error", "{style}: {refused}");
        assert_eq!(
            refused["diagnostics"][0]["code"], "loss_consent_required",
            "{style}"
        );
        assert_eq!(
            consent_features(&refused),
            ["stereo_marks", "stereo_configuration"],
            "{style}"
        );
        assert!(refused["result"].is_null());
        let digest = refused["diagnostics"][0]["details"]["loss_digest"]
            .as_str()
            .unwrap();
        let accepted = export_smiles(&app, "second", 1, style, Some(digest));
        assert_eq!(accepted["outcome"], "partial", "{style}: {accepted}");
        let reported = losses(&accepted);
        assert!(
            reported.contains(&("stereo_marks", "bonds[b1, b2, b3, b5]")),
            "{style}: {reported:?}"
        );
        assert!(
            reported.contains(&("stereo_configuration", "atoms[a1, a10], bonds[b5, b11]")),
            "{style}: {reported:?}"
        );
        for loss in accepted["loss_report"].as_array().unwrap() {
            if loss["feature"].as_str().unwrap().starts_with("stereo_") {
                assert_eq!(loss["disposition"], "dropped");
                assert_eq!(loss["semantic_impact"], "stereo");
            }
        }
        // The text is that of the same graph without any stereo.
        let plain = export_smiles(&app, "plain", 2, style, None);
        assert_eq!(plain["outcome"], "partial", "{style}: {plain}");
        assert_eq!(
            accepted["result"]["content"], plain["result"]["content"],
            "{style}: stereo changes nothing in the text"
        );
        assert!(
            !losses(&plain)
                .iter()
                .any(|(feature, _)| feature.starts_with("stereo_")),
            "a stereo-free drawing reports no stereo loss: {style}"
        );
    }
}

#[test]
fn a_digest_for_one_stereo_drawing_does_not_consent_to_another() {
    let only_mark = halo_center(1, &[mark(1, WEDGE_BEGIN)]);
    let (app, _directory) = app_with(&[only_mark, full(2, true)]);
    let refused = export_smiles(&app, "marks", 1, "recorded", None);
    assert_eq!(consent_features(&refused), ["stereo_marks"]);
    let digest = refused["diagnostics"][0]["details"]["loss_digest"]
        .as_str()
        .unwrap()
        .to_owned();
    let other = export_smiles(&app, "other", 2, "recorded", Some(&digest));
    assert_eq!(
        other["diagnostics"][0]["code"], "loss_consent_changed",
        "{other}"
    );
    assert!(other["result"].is_null());
}

#[test]
fn a_wavy_mark_alone_and_an_unknown_record_alone_each_need_consent() {
    let wavy = halo_center(1, &[mark(1, StereoMark::Wavy)]);
    let unknown = halo_center(
        2,
        &[Command::SetAtomConfiguration {
            atom: AtomRef::Id(atom(1)),
            configuration: Some(AtomConfiguration::Unknown {
                neighbors: [s(2), s(3), s(4), s(5)],
            }),
        }],
    );
    let (app, _directory) = app_with(&[wavy, unknown]);
    let wavy = export_smiles(&app, "wavy", 1, "recorded", None);
    assert_eq!(consent_features(&wavy), ["stereo_marks"], "{wavy}");
    let unknown = export_smiles(&app, "unknown", 2, "recorded", None);
    assert_eq!(
        consent_features(&unknown),
        ["stereo_configuration"],
        "{unknown}"
    );
}

// ---------------------------------------------------------------------------------------------
// V2000 export
// ---------------------------------------------------------------------------------------------

#[test]
fn v2000_export_refuses_a_drawing_that_holds_stereo_and_names_the_record() {
    let (app, _directory) = app_with(&[
        full(1, false),
        full(2, true),
        halo_center(3, &[mark(1, StereoMark::Wavy)]),
    ]);
    // An SDF names the record that holds stereo and returns no content for the clean one.
    let refused = call(
        &app,
        "refuse-sdf",
        "document.export",
        json!({"format": "sdf", "records": [{"document_id": name(1)}, {"document_id": name(2)}]}),
    );
    assert_eq!(refused["outcome"], "error", "{refused}");
    assert_eq!(refused["diagnostics"][0]["code"], "unsupported_stereo");
    assert_eq!(refused["diagnostics"][0]["details"]["record"], 2);
    assert!(refused["result"].is_null());
    let refused = call(
        &app,
        "refuse-mol",
        "document.export",
        json!({"format": "mol", "records": [{"document_id": name(2)}]}),
    );
    assert_eq!(refused["outcome"], "error", "{refused}");
    assert_eq!(refused["diagnostics"][0]["code"], "unsupported_stereo");
    assert_eq!(refused["diagnostics"][0]["details"]["record"], 1);
    assert!(refused["result"].is_null());
    // A wavy mark alone is enough, and a stereo-free record writes.
    let wavy = call(
        &app,
        "wavy",
        "document.export",
        json!({"format": "mol", "records": [{"document_id": name(3)}]}),
    );
    assert_eq!(
        wavy["diagnostics"][0]["code"], "unsupported_stereo",
        "{wavy}"
    );
    let clean = call(
        &app,
        "clean",
        "document.export",
        json!({"format": "mol", "records": [{"document_id": name(1)}]}),
    );
    assert_ne!(clean["outcome"], "error", "{clean}");
    assert!(!clean["result"].is_null(), "{clean}");
}
