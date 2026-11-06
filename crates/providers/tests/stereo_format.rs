//! File format version 6: stereo marks on bonds and stated configurations, in documents,
//! history, and recovery drafts; version 5 and older files still read, with none.

use std::fs;
use std::path::{Path, PathBuf};

use chemifree_application::{Application, DocumentRepository, Draft, StorageError};
use chemifree_domain::document::{
    Atom, AtomId, AtomRef, BondId, BondOrder, BondRef, Command, Document, DocumentId,
    NewAtomPositions, Point,
};
use chemifree_domain::element::Element;
use chemifree_domain::settings::{CarbonLabels, DisplayUnit, LabelFont};
use chemifree_domain::stereo::{
    AtomConfiguration, BondConfiguration, BondEnd, Relation, SlotRef, StereoKey, StereoMark, Turn,
};
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};

const ID: &str = "doc-00000000000000f1";

fn id() -> DocumentId {
    DocumentId::parse(ID).unwrap()
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

fn fixture(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name);
    fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn stored(bytes: &[u8]) -> (tempfile::TempDir, FileDocumentRepository, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let repository = FileDocumentRepository::new(root.path());
    let path = root.path().join("documents").join(format!("{ID}.json"));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, bytes).unwrap();
    (root, repository, path)
}

fn new_atom() -> Command {
    Command::AddAtom {
        atom: Atom::from(Element::CARBON),
        positions: NewAtomPositions::Single(Point::new(1.5, -2.5).unwrap()),
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

/// A center `a1` with four neighbors, a center `a7` with three, and an alkene `a10=a11`, with
/// every kind of mark and configuration. The history has an undo entry that cleared the record
/// of `a1` (deleting its neighbor `a5`) and a redo entry that cleared the record of `a7`.
fn stereo_document() -> Document {
    let mut document = Document::new(id(), "Stereo").unwrap();
    let mut commands: Vec<Command> = (0..13).map(|_| new_atom()).collect();
    commands.extend([
        add_bond(1, 2, BondOrder::Single),
        add_bond(1, 3, BondOrder::Single),
        add_bond(1, 4, BondOrder::Single),
        add_bond(1, 5, BondOrder::Single),
        add_bond(7, 8, BondOrder::Single),
        add_bond(7, 9, BondOrder::Single),
        add_bond(7, 6, BondOrder::Single),
        add_bond(10, 11, BondOrder::Double),
        add_bond(10, 12, BondOrder::Single),
        add_bond(11, 13, BondOrder::Single),
    ]);
    document.apply(&commands).unwrap();
    document
        .apply(&[
            Command::SetBondMark {
                bond: BondRef::Id(bond(1)),
                mark: Some(StereoMark::Wedge {
                    narrow: BondEnd::End,
                }),
            },
            Command::SetBondMark {
                bond: BondRef::Id(bond(2)),
                mark: Some(StereoMark::Hash {
                    narrow: BondEnd::Begin,
                }),
            },
            Command::SetBondMark {
                bond: BondRef::Id(bond(5)),
                mark: Some(StereoMark::Wavy),
            },
            Command::SetBondMark {
                bond: BondRef::Id(bond(8)),
                mark: Some(StereoMark::Crossed),
            },
            Command::SetAtomConfiguration {
                atom: AtomRef::Id(atom(1)),
                configuration: Some(AtomConfiguration::Tetrahedral {
                    neighbors: [s(3), s(2), s(4), s(5)],
                    turn: Turn::Clockwise,
                }),
            },
            Command::SetAtomConfiguration {
                atom: AtomRef::Id(atom(7)),
                configuration: Some(AtomConfiguration::Tetrahedral {
                    neighbors: [SlotRef::ImplicitHydrogen, s(6), s(9), s(8)],
                    turn: Turn::Anticlockwise,
                }),
            },
            Command::SetBondConfiguration {
                bond: BondRef::Id(bond(8)),
                configuration: Some(BondConfiguration::Relative {
                    begin_neighbor: s(12),
                    end_neighbor: s(13),
                    relation: Relation::OppositeSides,
                }),
            },
        ])
        .unwrap();
    // A neighbor's deletion clears the center `a1` (and takes its bond away): an undo entry
    // that records a cascade-cleared record.
    document
        .apply(&[Command::DeleteAtoms {
            atoms: vec![atom(5)],
        }])
        .unwrap();
    // The same for `a7`, then undone: a redo entry that clears a record.
    document
        .apply(&[Command::DeleteAtoms {
            atoms: vec![atom(8)],
        }])
        .unwrap();
    document.undo().unwrap();
    document
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

#[test]
fn marks_and_configurations_round_trip_with_their_history_in_version_6() {
    let root = tempfile::tempdir().unwrap();
    let repository = FileDocumentRepository::new(root.path());
    let document = stereo_document();
    assert!(document.undo_depth() > 0 && document.redo_depth() > 0);
    assert!(!document.stereo().is_empty());
    repository.save(&document, None).unwrap();

    let reopened = repository.load(&id()).unwrap().unwrap();
    assert_eq!(reopened, document);
    // Undo and redo still work after reopening, through the cascade-cleared entry.
    let mut walked = reopened.clone();
    let mut undone = 0;
    while walked.undo().is_ok() {
        undone += 1;
    }
    assert_eq!(undone, document.undo_depth());
    assert!(walked.stereo().is_empty());
    assert!(walked.bonds().all(|(_, bond)| bond.mark.is_none()));
    for _ in 0..undone {
        walked.redo().unwrap();
    }
    assert_eq!(walked.stereo(), document.stereo());
    assert_eq!(
        walked.bonds().collect::<Vec<_>>(),
        document.bonds().collect::<Vec<_>>()
    );
    // One more redo reaches the entry that cleared the record of a7.
    walked.redo().unwrap();
    assert!(walked.stereo().get(StereoKey::Center(atom(7))).is_none());

    let file = read_json(&root.path().join("documents").join(format!("{ID}.json")));
    assert_eq!(file["format_version"], 23);
    let stereo = &file["document"]["stereo"];
    assert!(
        stereo.as_array().is_some_and(|records| records.len() == 2),
        "{stereo}"
    );
    assert_eq!(
        stereo[0],
        json!({"id": "a7", "state": {"kind": "center", "state": {
            "mode": "tetrahedral",
            "neighbors": ["a6", "a8", "a9", "implicit_hydrogen"],
            "turn": "anticlockwise"
        }}}),
        "[H,a6,a9,a8] is a 3-cycle from the sorted order: the same turn"
    );
    assert_eq!(
        stereo[1],
        json!({"id": "b8", "state": {"kind": "double_bond", "state": {
            "mode": "relative",
            "begin_ref": "a12",
            "end_ref": "a13",
            "relation": "opposite_sides"
        }}})
    );
    // The marks are in the bond states, with the narrow end named.
    let bonds = file["document"]["molecule"]["bonds"].as_array().unwrap();
    assert_eq!(
        bonds[0]["state"]["mark"],
        json!({"kind": "wedge", "narrow": "end"})
    );
    assert_eq!(bonds[0]["state"]["begin"], "a1", "the ends are not swapped");
    assert_eq!(
        bonds[1]["state"]["mark"],
        json!({"kind": "hash", "narrow": "begin"})
    );
    assert_eq!(bonds[3]["state"]["mark"], json!({"kind": "wavy"}), "b5");
    assert_eq!(bonds[6]["state"]["mark"], json!({"kind": "crossed"}), "b8");
    assert!(
        bonds[2]["state"].get("mark").is_none(),
        "a plain bond has no mark field"
    );
    // The history keeps the records the edits changed, cleared ones included: the entry that
    // deleted `a5` took away the record of `a1`.
    let cleared_a1 = file["history"]["undo"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|set| set["stereo"].as_array().unwrap())
        .find(|change| change["id"] == "a1" && change["after"].is_null())
        .expect("an undo entry clears the record of a1");
    assert_eq!(cleared_a1["before"]["state"]["mode"], "tetrahedral");
    let redo = &file["history"]["redo"][0]["stereo"];
    assert_eq!(redo[0]["id"], "a7", "the redo entry clears a7: {redo}");
    assert!(redo[0]["after"].is_null());
}

#[test]
fn a_recovery_draft_carries_marks_configurations_and_history() {
    let root = tempfile::tempdir().unwrap();
    let repository = FileDocumentRepository::new(root.path());
    let document = stereo_document();
    repository.save(&document, None).unwrap();
    let draft = Draft {
        document: document.clone(),
        base_revision: document.revision(),
        saved_at_ms: 1_767_225_600_000,
    };
    repository.write_draft(&draft).unwrap();
    let draft_path = root.path().join("drafts").join(format!("{ID}.json"));
    let file = read_json(&draft_path);
    assert_eq!(file["format"], "chemifree.draft");
    assert_eq!(file["format_version"], 23);
    assert_eq!(file["document"]["stereo"].as_array().unwrap().len(), 2);
    repository.release_draft(&id());
    assert_eq!(repository.claim_draft(&id()), Ok(Some(Ok(draft))));
}

// ---------------------------------------------------------------------------------------------
// Older files
// ---------------------------------------------------------------------------------------------

#[test]
fn a_crossed_mark_and_a_double_bond_configuration_on_one_bond_survive_a_save_in_either_order() {
    let crossed = Command::SetBondMark {
        bond: BondRef::Id(bond(1)),
        mark: Some(StereoMark::Crossed),
    };
    let configuration = Command::SetBondConfiguration {
        bond: BondRef::Id(bond(1)),
        configuration: Some(BondConfiguration::Relative {
            begin_neighbor: s(3),
            end_neighbor: s(5),
            relation: Relation::SameSide,
        }),
    };
    for (name, first, second) in [
        ("mark first", &crossed, &configuration),
        ("configuration first", &configuration, &crossed),
    ] {
        let mut document = Document::new(id(), "Alkene").unwrap();
        let mut commands: Vec<Command> = (0..5).map(|_| new_atom()).collect();
        commands.extend([
            add_bond(1, 2, BondOrder::Double),
            add_bond(1, 3, BondOrder::Single),
            add_bond(1, 4, BondOrder::Single),
            add_bond(2, 5, BondOrder::Single),
        ]);
        document.apply(&commands).unwrap();
        document.apply(std::slice::from_ref(first)).unwrap();
        document.apply(std::slice::from_ref(second)).unwrap();

        let root = tempfile::tempdir().unwrap();
        let repository = FileDocumentRepository::new(root.path());
        repository.save(&document, None).unwrap();
        let reopened = repository.load(&id()).unwrap().unwrap();
        assert_eq!(reopened, document, "{name}");
        let bonds: Vec<_> = reopened.bonds().collect();
        assert_eq!(bonds[0].1.mark, Some(StereoMark::Crossed), "{name}");
        assert!(
            reopened
                .stereo()
                .get(StereoKey::DoubleBond(bond(1)))
                .is_some(),
            "{name}: the configuration is stored beside the mark"
        );
    }
}

#[test]
fn a_version_5_file_reads_with_no_stereo_and_its_first_save_keeps_the_original_bytes() {
    let original = fixture("v5-scene-document.json");
    let (root, repository, path) = stored(&original);
    let mut document = repository.load(&id()).unwrap().unwrap();
    assert!(document.stereo().is_empty());
    assert!(document.bonds().all(|(_, bond)| bond.mark.is_none()));
    assert_eq!(document.references().count(), 1, "the scene survives");
    assert_eq!(fs::read(&path).unwrap(), original, "reading writes nothing");

    document.undo().unwrap();
    document.redo().unwrap();
    repository.save(&document, Some(2)).unwrap();
    assert_eq!(
        fs::read(repository.migrated_path(&id(), 5)).unwrap(),
        original,
        "the version 5 bytes are kept before they are replaced"
    );
    assert_eq!(
        repository
            .migrated_path(&id(), 5)
            .file_name()
            .unwrap()
            .to_str()
            .unwrap(),
        format!("{ID}.v5.json")
    );
    assert_eq!(read_json(&path)["format_version"], 23);
    assert_eq!(repository.load(&id()).unwrap(), Some(document));
    assert!(root.path().join("migrated").is_dir());
}

#[test]
fn a_version_5_file_keeps_its_stored_settings_when_it_is_read() {
    // Only versions 1-4 receive the built-in settings; version 5 already stores its own.
    let default_settings = r#""settings":{"bond_length_pt":30.0,"line_width_pt":1.2,"label_font":"sans-serif","label_size_pt":12.0,"carbon_labels":"default","display_unit":"pt"}"#;
    let own_settings = r#""settings":{"bond_length_pt":42.5,"line_width_pt":2.5,"label_font":"monospace","label_size_pt":14.0,"carbon_labels":"all","display_unit":"mm"}"#;
    let text = String::from_utf8(fixture("v5-scene-document.json")).unwrap();
    assert_eq!(text.matches(default_settings).count(), 1, "fixture shape");
    let (_root, repository, _path) =
        stored(text.replace(default_settings, own_settings).as_bytes());

    let document = repository.load(&id()).unwrap().unwrap();
    let settings = document.settings();
    assert_eq!(settings.bond_length_pt, 42.5);
    assert_eq!(settings.line_width_pt, 2.5);
    assert_eq!(settings.label_font, LabelFont::Monospace);
    assert_eq!(settings.label_size_pt, 14.0);
    assert_eq!(settings.carbon_labels, CarbonLabels::All);
    assert_eq!(settings.display_unit, DisplayUnit::Mm);
}

#[test]
fn a_version_5_draft_is_recovered_and_its_bytes_are_kept() {
    let document = fixture("v5-scene-document.json");
    let original = fixture("v5-scene-draft.json");
    let (root, repository, _) = stored(&document);
    let draft_path = root.path().join("drafts").join(format!("{ID}.json"));
    fs::create_dir_all(draft_path.parent().unwrap()).unwrap();
    fs::write(&draft_path, &original).unwrap();
    let archive = repository.migrated_draft_path(&id(), 5);
    let app = Application::new(LocalCapabilities, repository);

    let recovered = app.recover(ID, Some("recover-v5-scene")).unwrap();
    assert_eq!(recovered.document.revision, 4);
    assert_eq!(recovered.session.redo_depth, 1);
    assert_eq!(fs::read(&archive).unwrap(), original);
    let redone = app.redo(ID, None, 4).unwrap();
    assert_eq!(redone.document.revision, 5);
    app.autosave(ID, None).unwrap();
    assert_eq!(read_json(&draft_path)["format_version"], 23);
    assert_eq!(fs::read(archive).unwrap(), original, "never overwritten");
}

/// The version 5 file a stereo-free document would have been: the current version without
/// `stereo`, groups, marks, Et display fields, or the version number.
fn as_version_5(bytes: &[u8]) -> Vec<u8> {
    let mut file: Value = serde_json::from_slice(bytes).unwrap();
    strip_label_weights(&mut file);
    file["format_version"] = json!(5);
    for field in ["stereo", "stereo_groups", "next_group", "abbreviations"] {
        file["document"].as_object_mut().unwrap().remove(field);
    }
    for side in ["undo", "redo"] {
        for set in file["history"][side].as_array_mut().unwrap() {
            set.as_object_mut().unwrap().remove("stereo");
            set.as_object_mut().unwrap().remove("stereo_groups");
            set.as_object_mut().unwrap().remove("abbreviations");
        }
    }
    serde_json::to_vec(&file).unwrap()
}

fn stereo_free_document() -> Document {
    let mut document = Document::new(id(), "Plain").unwrap();
    document
        .apply(&[
            new_atom(),
            new_atom(),
            new_atom(),
            add_bond(1, 2, BondOrder::Single),
            add_bond(2, 3, BondOrder::Double),
        ])
        .unwrap();
    document
        .apply(&[Command::UpdateBond {
            bond: bond(1),
            order: BondOrder::Aromatic,
        }])
        .unwrap();
    document.undo().unwrap();
    document
}

#[test]
fn a_stereo_free_document_written_as_version_5_reads_back_identically() {
    let root = tempfile::tempdir().unwrap();
    let writer = FileDocumentRepository::new(root.path());
    let document = stereo_free_document();
    writer.save(&document, None).unwrap();
    let path = root.path().join("documents").join(format!("{ID}.json"));
    let version_6 = fs::read(&path).unwrap();
    let version_5 = as_version_5(&version_6);

    let (_root, repository, _) = stored(&version_5);
    assert_eq!(repository.load(&id()).unwrap(), Some(document));
}

#[test]
fn version_5_cannot_hold_a_mark_or_stereo_records() {
    let root = tempfile::tempdir().unwrap();
    FileDocumentRepository::new(root.path())
        .save(&stereo_free_document(), None)
        .unwrap();
    let version_6 = fs::read(root.path().join("documents").join(format!("{ID}.json"))).unwrap();
    let version_5 = as_version_5(&version_6);

    let mut with_mark: Value = serde_json::from_slice(&version_5).unwrap();
    with_mark["document"]["molecule"]["bonds"][0]["state"]["mark"] = json!({"kind": "wavy"});
    let mut with_records: Value = serde_json::from_slice(&version_5).unwrap();
    with_records["document"]["stereo"] = json!([]);
    let mut with_history_records: Value = serde_json::from_slice(&version_5).unwrap();
    with_history_records["history"]["undo"][0]["stereo"] = json!([]);
    for (what, file) in [
        ("a mark", with_mark),
        ("a stereo list", with_records),
        ("a stereo history list", with_history_records),
    ] {
        let bytes = serde_json::to_vec(&file).unwrap();
        let (_root, repository, path) = stored(&bytes);
        assert!(
            matches!(repository.load(&id()), Err(StorageError::Corrupt(_))),
            "version 5 with {what} is unreadable"
        );
        assert_eq!(fs::read(path).unwrap(), bytes, "and left untouched");
    }
}

// ---------------------------------------------------------------------------------------------
// Unreadable version 6 files
// ---------------------------------------------------------------------------------------------

fn saved_stereo_file() -> (Value, Document) {
    let root = tempfile::tempdir().unwrap();
    let document = stereo_document();
    FileDocumentRepository::new(root.path())
        .save(&document, None)
        .unwrap();
    (
        read_json(&root.path().join("documents").join(format!("{ID}.json"))),
        document,
    )
}

fn assert_unreadable(file: &Value, why: &str) {
    let bytes = serde_json::to_vec(file).unwrap();
    let (_root, repository, path) = stored(&bytes);
    assert!(
        matches!(repository.load(&id()), Err(StorageError::Corrupt(_))),
        "{why}"
    );
    assert_eq!(fs::read(path).unwrap(), bytes, "{why}: left untouched");
}

#[test]
fn version_6_requires_the_stereo_lists() {
    // A document without stereo, so that nothing but the missing field can make it unreadable.
    let root = tempfile::tempdir().unwrap();
    FileDocumentRepository::new(root.path())
        .save(&stereo_free_document(), None)
        .unwrap();
    let file = read_json(&root.path().join("documents").join(format!("{ID}.json")));
    assert_eq!(file["document"]["stereo"], json!([]));
    let mut without_document = file.clone();
    without_document["document"]
        .as_object_mut()
        .unwrap()
        .remove("stereo");
    assert_unreadable(&without_document, "no document stereo list");
    let mut without_history = file.clone();
    without_history["history"]["undo"][0]
        .as_object_mut()
        .unwrap()
        .remove("stereo");
    assert_unreadable(&without_history, "no history stereo list");
}

#[test]
fn damaged_stereo_records_make_the_file_unreadable() {
    let (file, _) = saved_stereo_file();
    let damage = |what: &str, change: &dyn Fn(&mut Value)| {
        let mut broken = file.clone();
        change(&mut broken);
        assert_unreadable(&broken, what);
    };
    damage("a record for an atom that does not exist", &|file| {
        file["document"]["stereo"][0]["id"] = json!("a99");
    });
    damage("slots out of canonical order", &|file| {
        file["document"]["stereo"][0]["state"]["state"]["neighbors"] =
            json!(["a8", "a6", "a9", "implicit_hydrogen"]);
    });
    damage("a neighbor set that is not the bonded one", &|file| {
        file["document"]["stereo"][0]["state"]["state"]["neighbors"] =
            json!(["a6", "a8", "a9", "a10"]);
    });
    damage("a slot that is not an atom or a named slot", &|file| {
        file["document"]["stereo"][0]["state"]["state"]["neighbors"] =
            json!(["a6", "a8", "a9", "hydrogen"]);
    });
    damage("a record id that is neither an atom nor a bond", &|file| {
        file["document"]["stereo"][0]["id"] = json!("s1");
    });
    damage("a double bond record under an atom id", &|file| {
        file["document"]["stereo"][0]["state"] =
            json!({"kind": "double_bond", "state": {"mode": "unknown"}});
    });
    damage("a repeated record", &|file| {
        let first = file["document"]["stereo"][0].clone();
        file["document"]["stereo"]
            .as_array_mut()
            .unwrap()
            .push(first);
    });
    damage("a mark on a bond it does not fit", &|file| {
        file["document"]["molecule"]["bonds"][0]["state"]["mark"] = json!({"kind": "crossed"});
    });
    damage("an unknown mark kind", &|file| {
        file["document"]["molecule"]["bonds"][0]["state"]["mark"] = json!({"kind": "bold"});
    });
    damage("a wedge without its narrow end", &|file| {
        file["document"]["molecule"]["bonds"][0]["state"]["mark"] = json!({"kind": "wedge"});
    });
    damage("a wavy mark with a narrow end", &|file| {
        file["document"]["molecule"]["bonds"][3]["state"]["mark"] =
            json!({"kind": "wavy", "narrow": "begin"});
    });
    damage("a history record that does not replay", &|file| {
        let undo = file["history"]["undo"].as_array_mut().unwrap();
        let set = undo
            .iter_mut()
            .find(|set| !set["stereo"].as_array().unwrap().is_empty())
            .unwrap();
        set["stereo"][0]["after"] = json!({"kind": "double_bond", "state": {"mode": "unknown"}});
    });
}

// ---------------------------------------------------------------------------------------------
// The form version 6 first stored for a bare double-bond end
// ---------------------------------------------------------------------------------------------

/// A lone double bond `a1=a2`: both ends are bare, so each end's two positions are an implicit
/// hydrogen and a lone pair. Three edits give the document, an undo entry and a redo entry that
/// each carry a record of the bond:
/// - undo `b1`'s record: none -> same side (the document's own record);
/// - redo: same side -> opposite sides.
fn bare_double_bond() -> Document {
    let mut document = Document::new(id(), "Imine").unwrap();
    document
        .apply(&[new_atom(), new_atom(), add_bond(1, 2, BondOrder::Double)])
        .unwrap();
    let relative = |relation| Command::SetBondConfiguration {
        bond: BondRef::Id(bond(1)),
        configuration: Some(BondConfiguration::Relative {
            begin_neighbor: SlotRef::ImplicitHydrogen,
            end_neighbor: SlotRef::ImplicitHydrogen,
            relation,
        }),
    };
    document.apply(&[relative(Relation::SameSide)]).unwrap();
    document
        .apply(&[relative(Relation::OppositeSides)])
        .unwrap();
    document.undo().unwrap();
    document
}

/// Rewrites a stored double-bond state into the form the first version 6 build accepted: the
/// named ends' implicit hydrogen replaced by the lone pair, the relation flipped once for each.
fn as_legacy_lone_pairs(state: &mut Value, ends: &[&str]) {
    let inner = &mut state["state"];
    assert_eq!(inner["mode"], "relative", "{inner}");
    for end in ends {
        assert_eq!(inner[*end], "implicit_hydrogen", "{end} of {inner}");
        inner[*end] = json!("lone_pair");
        inner["relation"] = json!(match inner["relation"].as_str().unwrap() {
            "same_side" => "opposite_sides",
            _ => "same_side",
        });
    }
}

/// The stored forms of one document and its history, all in the legacy form.
fn with_legacy_references(file: &mut Value) {
    as_legacy_lone_pairs(&mut file["document"]["stereo"][0]["state"], &["begin_ref"]);
    let undo = file["history"]["undo"].as_array_mut().unwrap();
    let step = undo
        .iter_mut()
        .find(|set| !set["stereo"].as_array().unwrap().is_empty())
        .expect("an undo entry with the record");
    as_legacy_lone_pairs(&mut step["stereo"][0]["after"], &["begin_ref", "end_ref"]);
    let redo = &mut file["history"]["redo"][0]["stereo"][0];
    as_legacy_lone_pairs(&mut redo["before"], &["end_ref"]);
    as_legacy_lone_pairs(&mut redo["after"], &["begin_ref", "end_ref"]);
}

#[test]
fn a_lone_pair_stored_at_a_bare_double_bond_end_reads_back_as_the_implicit_hydrogen() {
    let document = bare_double_bond();
    assert!(document.undo_depth() > 0 && document.redo_depth() > 0);
    assert_eq!(document.stereo().len(), 1);

    // A document with its history.
    let root = tempfile::tempdir().unwrap();
    FileDocumentRepository::new(root.path())
        .save(&document, None)
        .unwrap();
    let mut file = read_json(&root.path().join("documents").join(format!("{ID}.json")));
    assert_eq!(
        file["document"]["stereo"][0]["state"]["state"]["begin_ref"], "implicit_hydrogen",
        "the writer stores the canonical form"
    );
    with_legacy_references(&mut file);
    let (_root, repository, path) = stored(&serde_json::to_vec(&file).unwrap());
    let reopened = repository.load(&id()).unwrap().unwrap();
    assert_eq!(reopened, document, "the same state, read in canonical form");
    assert_eq!(
        reopened.stereo().get(StereoKey::DoubleBond(bond(1))),
        document.stereo().get(StereoKey::DoubleBond(bond(1)))
    );
    // Undo and redo walk the normalized history.
    let mut walked = reopened.clone();
    let mut expected = document.clone();
    while walked.undo().is_ok() {
        expected.undo().unwrap();
        assert_eq!(walked.stereo(), expected.stereo());
    }
    assert!(walked.stereo().is_empty());
    while walked.redo().is_ok() {
        expected.redo().unwrap();
        assert_eq!(walked.stereo(), expected.stereo());
    }
    assert_eq!(walked, expected);
    assert!(
        fs::read(&path).unwrap() == serde_json::to_vec(&file).unwrap(),
        "reading does not rewrite the file"
    );

    // A recovery draft of the same document.
    let root = tempfile::tempdir().unwrap();
    let repository = FileDocumentRepository::new(root.path());
    repository.save(&document, None).unwrap();
    let draft = Draft {
        document: document.clone(),
        base_revision: document.revision(),
        saved_at_ms: 1_767_225_600_000,
    };
    repository.write_draft(&draft).unwrap();
    repository.release_draft(&id());
    let draft_path = root.path().join("drafts").join(format!("{ID}.json"));
    let mut draft_file = read_json(&draft_path);
    with_legacy_references(&mut draft_file);
    fs::write(&draft_path, serde_json::to_vec(&draft_file).unwrap()).unwrap();
    assert_eq!(repository.claim_draft(&id()), Ok(Some(Ok(draft))));
}

#[test]
fn a_lone_pair_at_an_end_with_a_neighbor_is_still_unreadable() {
    // Only the first build's one form is normalized: here the end has a neighbor, so a lone
    // pair never was storable.
    let (file, _) = saved_stereo_file();
    let mut broken = file.clone();
    broken["document"]["stereo"][1]["state"]["state"]["begin_ref"] = json!("lone_pair");
    assert_unreadable(&broken, "a lone pair at an end with an explicit neighbor");
}

#[test]
fn a_version_6_document_keeps_its_bond_ends_when_marks_change() {
    // The ends are in the file exactly as drawn, whichever end the mark is narrow at.
    let (file, document) = saved_stereo_file();
    for (index, (_, bond)) in document.bonds().enumerate() {
        let state = &file["document"]["molecule"]["bonds"][index]["state"];
        assert_eq!(state["begin"], bond.begin.to_string());
        assert_eq!(state["end"], bond.end.to_string());
    }
    assert!(document.stereo().get(StereoKey::Center(atom(7))).is_some());
}

fn strip_label_weights(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(object) => {
            object.remove("label_weight");
            object.remove("label_style");
            object.remove("label_underline");
            object.remove("label_outline");
            object.remove("label_shadow");
            object.remove("display_labels");
            for child in object.values_mut() {
                strip_label_weights(child);
            }
        }
        serde_json::Value::Array(values) => {
            for child in values {
                strip_label_weights(child);
            }
        }
        _ => {}
    }
}
