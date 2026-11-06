//! File format version 7: enhanced-stereo groups in documents, history, and recovery drafts;
//! version 6 and older files still read, with none.

use std::fs;
use std::path::Path;

use chemifree_application::{Application, DocumentRepository, Draft, StorageError};
use chemifree_domain::document::{
    Atom, AtomId, AtomRef, BondOrder, Command, Document, DocumentId, GroupId, GroupRef,
    NewAtomPositions, Point,
};
use chemifree_domain::element::Element;
use chemifree_domain::stereo::{AtomConfiguration, SlotRef, Turn};
use chemifree_domain::stereo_group::GroupKind;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};

const ID: &str = "doc-00000000000000f1";

fn id() -> DocumentId {
    DocumentId::parse(ID).unwrap()
}

fn atom(serial: u32) -> AtomId {
    AtomId::from_serial(serial).unwrap()
}

fn fixture(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name);
    fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn stored(
    bytes: &[u8],
) -> (
    tempfile::TempDir,
    FileDocumentRepository,
    std::path::PathBuf,
) {
    let root = tempfile::tempdir().unwrap();
    let repository = FileDocumentRepository::new(root.path());
    let path = root.path().join("documents").join(format!("{ID}.json"));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, bytes).unwrap();
    (root, repository, path)
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
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

/// Centers `a1..=a4`, each with four leaf neighbors and a stored configuration, in groups:
/// `e1` absolute `{a1}`, `e2` and (`i32::MIN`) `{a2}`, `e3` or (no number) `{a3}`, `e4` or (-3)
/// `{a4}`. The history ends with the cascade that removed `e2` when `a2`'s configuration was
/// cleared, undone, so that the redo stack holds it.
fn grouped_document() -> Document {
    let mut document = Document::new(id(), "Groups").unwrap();
    let mut commands: Vec<Command> = (0..20)
        .map(|_| Command::AddAtom {
            atom: Atom::from(Element::CARBON),
            positions: NewAtomPositions::Single(Point::new(1.0, 2.0).unwrap()),
            bond_to: None,
        })
        .collect();
    let leaf = |k: u32, j: u32| 5 + 4 * (k - 1) + j;
    for k in 1..=4 {
        for j in 0..4 {
            commands.push(Command::AddBond {
                begin: AtomRef::Id(atom(k)),
                end: AtomRef::Id(atom(leaf(k, j))),
                order: BondOrder::Single,
            });
        }
    }
    for k in 1..=4 {
        commands.push(Command::SetAtomConfiguration {
            atom: AtomRef::Id(atom(k)),
            configuration: Some(AtomConfiguration::Tetrahedral {
                neighbors: [0, 1, 2, 3].map(|j| SlotRef::Atom(AtomRef::Id(atom(leaf(k, j))))),
                turn: Turn::Clockwise,
            }),
        });
    }
    document.apply(&commands).unwrap();
    let create = |kind, number, member: u32| Command::CreateStereoGroup {
        kind,
        number,
        atoms: vec![AtomRef::Id(atom(member))],
    };
    document
        .apply(&[
            create(GroupKind::Absolute, None, 1),
            create(GroupKind::And, Some(i32::MIN), 2),
            create(GroupKind::Or, None, 3),
            create(GroupKind::Or, Some(-3), 4),
        ])
        .unwrap();
    document
        .apply(&[Command::SetAtomConfiguration {
            atom: AtomRef::Id(atom(2)),
            configuration: None,
        }])
        .unwrap();
    document.undo().unwrap();
    document
}

fn saved_file() -> Value {
    let root = tempfile::tempdir().unwrap();
    FileDocumentRepository::new(root.path())
        .save(&grouped_document(), None)
        .unwrap();
    read_json(&root.path().join("documents").join(format!("{ID}.json")))
}

#[test]
fn groups_and_their_history_round_trip_in_version_7_with_deterministic_bytes() {
    let document = grouped_document();
    let ids: Vec<String> = document
        .stereo_groups()
        .iter()
        .map(|(id, _)| id.to_string())
        .collect();
    assert_eq!(ids, ["e1", "e2", "e3", "e4"]);
    assert!(document.redo_depth() > 0, "the cascade entry is redoable");
    let first = tempfile::tempdir().unwrap();
    let repository = FileDocumentRepository::new(first.path());
    repository.save(&document, None).unwrap();
    let reopened = repository.load(&id()).unwrap().unwrap();
    assert_eq!(reopened, document);

    // The same content written again gives the same bytes.
    let second = tempfile::tempdir().unwrap();
    FileDocumentRepository::new(second.path())
        .save(&reopened, None)
        .unwrap();
    let path = |root: &Path| root.join("documents").join(format!("{ID}.json"));
    assert_eq!(
        fs::read(path(first.path())).unwrap(),
        fs::read(path(second.path())).unwrap()
    );

    // Redo still removes the member after reopening; undo brings it back with the same id.
    let mut walked = reopened;
    walked.redo().unwrap();
    assert_eq!(walked.stereo_groups().len(), 3);
    assert!(
        walked
            .stereo_groups()
            .get(GroupId::from_serial(2).unwrap())
            .is_none()
    );
    walked.undo().unwrap();
    assert_eq!(walked.stereo_groups(), document.stereo_groups());
}

#[test]
fn the_stored_shape_keeps_numbers_as_stated_in_identifier_order() {
    let file = saved_file();
    assert_eq!(file["format_version"], 22);
    assert_eq!(file["document"]["next_group"], 5);
    assert_eq!(
        file["document"]["stereo_groups"],
        json!([
            {"id": "e1", "state": {"kind": "absolute", "number": null, "members": ["a1"]}},
            {"id": "e2", "state": {"kind": "and", "number": i32::MIN, "members": ["a2"]}},
            {"id": "e3", "state": {"kind": "or", "number": null, "members": ["a3"]}},
            {"id": "e4", "state": {"kind": "or", "number": -3, "members": ["a4"]}},
        ])
    );
    // The cascade is a history entry that records the whole group before and after.
    let redo = &file["history"]["redo"][0]["stereo_groups"];
    assert_eq!(redo[0]["id"], "e2");
    assert_eq!(redo[0]["after"], Value::Null);
    assert_eq!(redo[0]["before"]["number"], i32::MIN);
}

#[test]
fn a_version_6_file_reads_with_no_groups_and_its_first_save_keeps_the_original_bytes() {
    let original = fixture("v6-stereo-document.json");
    let (root, repository, path) = stored(&original);
    let mut document = repository.load(&id()).unwrap().unwrap();
    assert!(document.stereo_groups().is_empty());
    assert_eq!(document.numbering().next_group, 1);
    assert!(!document.stereo().is_empty(), "the stereo survives");
    assert_eq!(fs::read(&path).unwrap(), original, "reading writes nothing");

    // History is intact and a group can be made after the migration.
    let revision = repository.stored_revision(&id()).unwrap().unwrap();
    document.undo().unwrap();
    document.redo().unwrap();
    document
        .apply(&[Command::CreateStereoGroup {
            kind: GroupKind::Or,
            number: Some(0),
            atoms: vec![AtomRef::Id(atom(7))],
        }])
        .unwrap();
    repository.save(&document, Some(revision)).unwrap();
    assert_eq!(
        fs::read(repository.migrated_path(&id(), 6)).unwrap(),
        original
    );
    assert_eq!(
        repository
            .migrated_path(&id(), 6)
            .file_name()
            .unwrap()
            .to_str()
            .unwrap(),
        format!("{ID}.v6.json")
    );
    assert_eq!(read_json(&path)["format_version"], 22);
    assert_eq!(repository.load(&id()).unwrap(), Some(document));
    assert!(root.path().join("migrated").is_dir());
}

#[test]
fn a_version_6_draft_is_recovered_and_its_bytes_are_kept() {
    let document = fixture("v6-stereo-document.json");
    let original = fixture("v6-stereo-draft.json");
    let (root, repository, _) = stored(&document);
    let draft_path = root.path().join("drafts").join(format!("{ID}.json"));
    fs::create_dir_all(draft_path.parent().unwrap()).unwrap();
    fs::write(&draft_path, &original).unwrap();
    let archive = repository.migrated_draft_path(&id(), 6);
    let app = Application::new(LocalCapabilities, repository);

    let recovered = app.recover(ID, Some("recover-v6")).unwrap();
    assert_eq!(recovered.session.redo_depth, 1);
    assert_eq!(fs::read(&archive).unwrap(), original);
    let revision = recovered.document.revision;
    app.redo(ID, None, revision).unwrap();
    app.autosave(ID, None).unwrap();
    assert_eq!(read_json(&draft_path)["format_version"], 22);
    assert_eq!(fs::read(archive).unwrap(), original, "never overwritten");
}

#[test]
fn a_draft_in_version_7_carries_groups_and_recovers_them() {
    let document = grouped_document();
    let root = tempfile::tempdir().unwrap();
    let repository = FileDocumentRepository::new(root.path());
    repository.save(&document, None).unwrap();
    let draft = Draft {
        document: document.clone(),
        base_revision: document.revision(),
        saved_at_ms: 1_767_225_600_000,
    };
    repository.write_draft(&draft).unwrap();
    let file = read_json(&root.path().join("drafts").join(format!("{ID}.json")));
    assert_eq!(file["format_version"], 22);
    assert_eq!(
        file["document"]["stereo_groups"].as_array().unwrap().len(),
        4
    );
    repository.release_draft(&id());
    assert_eq!(repository.claim_draft(&id()), Ok(Some(Ok(draft))));
}

#[test]
fn version_6_cannot_hold_groups_and_version_7_cannot_omit_them() {
    let file = saved_file();

    let mut v6_with_groups = file.clone();
    strip_label_weights(&mut v6_with_groups);
    v6_with_groups["format_version"] = json!(6);
    // Isolate forbidden group fields from later Et metadata, which v6 also forbids.
    v6_with_groups["document"]
        .as_object_mut()
        .unwrap()
        .remove("abbreviations");
    for stack in ["undo", "redo"] {
        for set in v6_with_groups["history"][stack].as_array_mut().unwrap() {
            set.as_object_mut().unwrap().remove("abbreviations");
        }
    }
    assert_unreadable(&v6_with_groups, "version 6 with groups");
    let mut v6_with_counter = fixture_json("v6-stereo-document.json");
    v6_with_counter["document"]["next_group"] = json!(1);
    assert_unreadable(&v6_with_counter, "version 6 with a group counter");
    let mut v6_with_history = fixture_json("v6-stereo-document.json");
    v6_with_history["history"]["undo"][0]["stereo_groups"] = json!([]);
    assert_unreadable(&v6_with_history, "version 6 with a group history list");

    for (what, remove) in [
        ("no group list", ("document", "stereo_groups")),
        ("no group counter", ("document", "next_group")),
    ] {
        let mut broken = file.clone();
        broken[remove.0].as_object_mut().unwrap().remove(remove.1);
        assert_unreadable(&broken, what);
    }
    let mut no_history = file;
    no_history["history"]["undo"][0]
        .as_object_mut()
        .unwrap()
        .remove("stereo_groups");
    assert_unreadable(&no_history, "no group history list");
}

fn fixture_json(name: &str) -> Value {
    serde_json::from_slice(&fixture(name)).unwrap()
}

#[test]
fn damaged_group_records_make_the_file_unreadable() {
    let file = saved_file();
    let damage = |what: &str, change: &dyn Fn(&mut Value)| {
        let mut broken = file.clone();
        change(&mut broken);
        assert_unreadable(&broken, what);
    };
    let groups = |file: &mut Value| file["document"]["stereo_groups"].clone();
    damage("a member without a configuration", &|file| {
        file["document"]["stereo_groups"][0]["state"]["members"] = json!(["a5"]);
    });
    damage("a member that does not exist", &|file| {
        file["document"]["stereo_groups"][0]["state"]["members"] = json!(["a99"]);
    });
    damage("a member that is not an atom id", &|file| {
        file["document"]["stereo_groups"][0]["state"]["members"] = json!(["b1"]);
    });
    damage("a member listed twice", &|file| {
        file["document"]["stereo_groups"][0]["state"]["members"] = json!(["a1", "a1"]);
    });
    damage("a group with no members", &|file| {
        file["document"]["stereo_groups"][0]["state"]["members"] = json!([]);
    });
    damage("an atom in two groups", &|file| {
        file["document"]["stereo_groups"][1]["state"]["members"] = json!(["a1"]);
    });
    damage("a number on the absolute group", &|file| {
        file["document"]["stereo_groups"][0]["state"]["number"] = json!(1);
    });
    damage("a second absolute group", &|file| {
        file["document"]["stereo_groups"][1]["state"] =
            json!({"kind": "absolute", "number": null, "members": ["a2"]});
    });
    damage("the same number twice in one kind", &|file| {
        file["document"]["stereo_groups"][3]["state"]["number"] = json!(null);
    });
    damage("a group without the number key", &|file| {
        file["document"]["stereo_groups"][2]["state"]
            .as_object_mut()
            .unwrap()
            .remove("number");
    });
    damage("a number outside the 32-bit range", &|file| {
        file["document"]["stereo_groups"][3]["state"]["number"] = json!(2_147_483_648_i64);
    });
    damage("a number that is not an integer", &|file| {
        file["document"]["stereo_groups"][3]["state"]["number"] = json!(1.5);
    });
    damage("a kind spelled `abs`", &|file| {
        file["document"]["stereo_groups"][0]["state"]["kind"] = json!("abs");
    });
    damage("an unknown field in a group", &|file| {
        file["document"]["stereo_groups"][0]["state"]["ratio"] = json!("1:1");
    });
    damage("a group id that is not a group id", &|file| {
        file["document"]["stereo_groups"][0]["id"] = json!("a1");
    });
    damage("a repeated group id", &|file| {
        let first = groups(file)[0].clone();
        file["document"]["stereo_groups"]
            .as_array_mut()
            .unwrap()
            .push(first);
    });
    damage("a counter that would reuse an identifier", &|file| {
        file["document"]["next_group"] = json!(4);
    });
    damage("a zero counter", &|file| {
        file["document"]["next_group"] = json!(0);
    });
    damage("a history entry that does not replay", &|file| {
        file["history"]["redo"][0]["stereo_groups"][0]["before"]["number"] = json!(7);
    });
}

#[test]
fn lists_are_read_in_any_order_as_for_every_other_list_so_members_are_not_refused_unsorted() {
    // The atom and stereo lists are written in identifier order, and a hand-edited file with
    // them reversed loads to the same document, so a group's member list is no different.
    let mut document = grouped_document();
    document
        .apply(&[
            Command::DeleteStereoGroup {
                group: GroupRef::Id(GroupId::from_serial(4).unwrap()),
            },
            Command::AddStereoGroupMembers {
                group: GroupRef::Id(GroupId::from_serial(3).unwrap()),
                atoms: vec![AtomRef::Id(atom(4))],
            },
        ])
        .unwrap();
    let root = tempfile::tempdir().unwrap();
    FileDocumentRepository::new(root.path())
        .save(&document, None)
        .unwrap();
    let written = read_json(&root.path().join("documents").join(format!("{ID}.json")));
    let members = &written["document"]["stereo_groups"][2]["state"]["members"];
    assert_eq!(members, &json!(["a3", "a4"]), "written ascending");

    let mut reversed = written.clone();
    reversed["document"]["stereo_groups"][2]["state"]["members"] = json!(["a4", "a3"]);
    reversed["document"]["molecule"]["atoms"]
        .as_array_mut()
        .unwrap()
        .reverse();
    reversed["document"]["stereo"]
        .as_array_mut()
        .unwrap()
        .reverse();
    let (_root, repository, _path) = stored(&serde_json::to_vec(&reversed).unwrap());
    assert_eq!(repository.load(&id()).unwrap(), Some(document));

    let mut twice = written;
    twice["document"]["stereo_groups"][2]["state"]["members"] = json!(["a3", "a3"]);
    assert_unreadable(&twice, "a repeated member is still refused");
}

fn strip_label_weights(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(object) => {
            object.remove("label_weight");
            object.remove("label_style");
            object.remove("label_underline");
            object.remove("label_outline");
            object.remove("label_shadow");
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
