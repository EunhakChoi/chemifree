//! Version 1 documents and drafts, written before graph and scene were separated, and version
//! 2 ones, written before recorded atom and bond states, are migrated when read and keep their
//! identifiers, revision, and undo/redo history; the first save that replaces one keeps its
//! exact bytes. Version 3 keeps every recorded state, and version 4 adds the scene tree and references.

use std::fs;
use std::path::{Path, PathBuf};

use chemifree_application::{
    Application, DocumentRepository, Draft, DraftStatus, ErrorCode, MAX_OPEN_SESSIONS,
    RecoveryState, StorageError,
};
use chemifree_domain::document::{
    Atom, AtomEdit, AtomId, AtomRef, BondId, BondOrder, BondTo, Command, Document, DocumentId,
    NewAtomPositions, ObjectId, Point, Vector,
};
use chemifree_domain::element::Element;
use chemifree_domain::scene::{
    DeleteMode, Graphic, GraphicShape, ObjectRef, ReferenceTarget, SceneReference, Selection,
    SelectionRequest, TransactionSelection,
};
use chemifree_domain::states::{
    AtomKind, AttachmentPoints, BondQuery, Hydrogens, QueryAtom, QueryBondOrder, Radical,
};
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};

const ID: &str = "doc-00000000000000f1";

fn fixture(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name);
    fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn id() -> DocumentId {
    DocumentId::parse(ID).unwrap()
}

fn atom(serial: u32) -> AtomId {
    AtomId::from_serial(serial).unwrap()
}

fn s1() -> ObjectId {
    ObjectId::from_serial(1).unwrap()
}

fn point(x: f64, y: f64) -> Point {
    Point::new(x, y).unwrap()
}

/// A data directory holding `bytes` as the stored document `ID`.
fn stored(bytes: &[u8]) -> (tempfile::TempDir, FileDocumentRepository, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let repository = FileDocumentRepository::new(root.path());
    let path = root.path().join("documents").join(format!("{ID}.json"));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, bytes).unwrap();
    (root, repository, path)
}

/// The fixture's history made with today's commands: draw C–O⁻(¹⁸O), move the oxygen,
/// change carbon to nitrogen with a double bond and a new title, then undo that.
fn drawn_today() -> Document {
    let mut document = drawn_and_edited();
    document.undo().unwrap();
    document
}

/// The fixture's three edits, none undone.
fn drawn_and_edited() -> Document {
    let mut document = Document::new(id(), "Fixture").unwrap();
    document
        .apply(&[
            Command::AddAtom {
                atom: Atom::from(Element::CARBON),
                positions: NewAtomPositions::Single(point(0.0, 0.0)),
                bond_to: None,
            },
            Command::AddAtom {
                atom: Atom::from(Element::from_symbol("O").unwrap()),
                positions: NewAtomPositions::Single(point(30.0, 0.0)),
                bond_to: Some(BondTo {
                    atom: AtomRef::Id(atom(1)),
                    order: BondOrder::Single,
                }),
            },
            Command::UpdateAtom {
                atom: atom(2),
                edit: AtomEdit {
                    charge: Some(-1),
                    isotope: Some(Some(18)),
                    ..AtomEdit::default()
                },
            },
        ])
        .unwrap();
    document
        .apply(&[Command::MoveAtoms {
            atoms: vec![atom(2)],
            delta: Vector { dx: 0.0, dy: 10.5 },
            instance: None,
        }])
        .unwrap();
    document
        .apply(&[
            Command::UpdateAtom {
                atom: atom(1),
                edit: AtomEdit {
                    kind: Some(AtomKind::Element(Element::from_symbol("N").unwrap())),
                    ..AtomEdit::default()
                },
            },
            Command::UpdateBond {
                bond: BondId::from_serial(1).unwrap(),
                order: BondOrder::Double,
            },
            Command::Rename {
                title: "Fixture edited".into(),
            },
        ])
        .unwrap();
    document
}

#[test]
fn a_version_1_file_migrates_with_its_graph_positions_and_history() {
    let (_root, repository, _path) = stored(&fixture("v1-document.json"));
    let document = repository.load(&id()).unwrap().unwrap();
    assert_eq!(
        document,
        drawn_today(),
        "the same graph, s1 positions, counters, revision, and history"
    );
    assert_eq!(document.revision(), 4);
    assert_eq!(document.instances().collect::<Vec<_>>(), [s1()]);
    assert_eq!(document.position(s1(), atom(2)), Some(point(30.0, 10.5)));
    assert_eq!((document.undo_depth(), document.redo_depth()), (2, 1));
    assert_eq!(repository.stored_revision(&id()), Ok(Some(4)));
}

#[test]
fn undo_and_redo_work_after_migration() {
    let (_root, repository, _path) = stored(&fixture("v1-document.json"));
    let mut document = repository.load(&id()).unwrap().unwrap();

    document.undo().unwrap();
    assert_eq!(
        document.position(s1(), atom(2)),
        Some(point(30.0, 0.0)),
        "a move-only entry undoes as a position change"
    );
    assert_eq!(document.atoms().len(), 2, "and leaves the graph alone");

    document.redo().unwrap();
    document.redo().unwrap();
    assert_eq!(document.title(), "Fixture edited");
    assert_eq!(
        document.atoms().next().unwrap().1.kind,
        AtomKind::Element(Element::from_symbol("N").unwrap())
    );
    assert_eq!(document.bonds().next().unwrap().1.order, BondOrder::Double);
    assert_eq!(document.revision(), 7);

    document.undo().unwrap();
    document.undo().unwrap();
    document.undo().unwrap();
    assert_eq!(document.atoms().len(), 0);
    assert_eq!(document.positions(s1()).count(), 0);
}

#[test]
fn reading_a_version_1_file_writes_nothing() {
    let original = fixture("v1-document.json");
    let (root, repository, path) = stored(&original);
    repository.load(&id()).unwrap().unwrap();
    repository.stored_revision(&id()).unwrap();
    assert_eq!(repository.list_ids().unwrap(), [id()]);
    assert_eq!(fs::read(&path).unwrap(), original);
    assert!(!root.path().join("migrated").exists());
}

#[test]
fn the_first_save_over_a_version_1_file_keeps_its_exact_bytes() {
    let original = fixture("v1-document.json");
    let (_root, repository, path) = stored(&original);
    let mut document = repository.load(&id()).unwrap().unwrap();
    document.redo().unwrap();
    repository.save(&document, Some(4)).unwrap();

    let kept = repository.migrated_path(&id(), 1);
    assert_eq!(
        kept.file_name().unwrap(),
        format!("{ID}.v1.json").as_str(),
        "{}",
        kept.display()
    );
    assert_eq!(fs::read(&kept).unwrap(), original);
    let saved = String::from_utf8(fs::read(&path).unwrap()).unwrap();
    assert!(
        saved.contains("\"format_version\":4"),
        "the document is stored as version 4"
    );
    assert_eq!(repository.load(&id()).unwrap(), Some(document.clone()));
    assert_eq!(
        repository.list_ids().unwrap(),
        [id()],
        "the kept copy is not a document"
    );

    // A second save replaces version 2 and leaves the kept copy alone.
    let base = document.revision();
    document.undo().unwrap();
    repository.save(&document, Some(base)).unwrap();
    assert_eq!(fs::read(&kept).unwrap(), original);
}

/// The names in `migrated/`, sorted.
fn migrated_names(root: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(root.join("migrated"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

#[test]
fn a_different_version_1_file_under_a_kept_id_gets_the_next_numbered_copy() {
    let original = fixture("v1-document.json");
    let (root, repository, _path) = stored(&original);
    let kept = repository.migrated_path(&id(), 1);
    fs::create_dir_all(kept.parent().unwrap()).unwrap();
    fs::write(&kept, b"an earlier copy").unwrap();
    fs::write(kept.with_file_name(format!("{ID}.v1.2.json")), b"another").unwrap();

    let mut document = repository.load(&id()).unwrap().unwrap();
    document.redo().unwrap();
    repository.save(&document, Some(4)).unwrap();

    assert_eq!(
        fs::read(&kept).unwrap(),
        b"an earlier copy",
        "never overwritten"
    );
    assert_eq!(
        fs::read(kept.with_file_name(format!("{ID}.v1.2.json"))).unwrap(),
        b"another"
    );
    assert_eq!(
        fs::read(kept.with_file_name(format!("{ID}.v1.3.json"))).unwrap(),
        original,
        "the replaced bytes are kept under the next free name"
    );
    assert_eq!(
        migrated_names(root.path()),
        [
            format!("{ID}.v1.2.json"),
            format!("{ID}.v1.3.json"),
            format!("{ID}.v1.json"),
        ]
    );
}

#[test]
fn a_version_1_file_already_kept_byte_for_byte_is_not_copied_again() {
    let original = fixture("v1-document.json");
    let (root, repository, path) = stored(&original);
    let kept = repository.migrated_path(&id(), 1);
    fs::create_dir_all(kept.parent().unwrap()).unwrap();
    fs::write(&kept, b"an earlier copy").unwrap();
    fs::write(kept.with_file_name(format!("{ID}.v1.2.json")), &original).unwrap();

    let mut document = repository.load(&id()).unwrap().unwrap();
    document.redo().unwrap();
    repository.save(&document, Some(4)).unwrap();

    assert_eq!(
        migrated_names(root.path()),
        [format!("{ID}.v1.2.json"), format!("{ID}.v1.json")],
        "identical bytes are already kept"
    );
    assert_eq!(fs::read(&kept).unwrap(), b"an earlier copy");

    // The same version 1 bytes stored again under this ID are recognised as kept too.
    fs::write(&path, &original).unwrap();
    let mut document = repository.load(&id()).unwrap().unwrap();
    document.redo().unwrap();
    repository.save(&document, Some(4)).unwrap();
    assert_eq!(migrated_names(root.path()).len(), 2);
}

#[test]
fn a_version_1_file_is_not_replaced_when_its_copy_cannot_be_kept() {
    let original = fixture("v1-document.json");
    let (root, repository, path) = stored(&original);
    // A file where the directory belongs fails the copy on every platform, even for root.
    fs::write(root.path().join("migrated"), b"not a directory").unwrap();
    let mut document = repository.load(&id()).unwrap().unwrap();
    document.redo().unwrap();
    assert!(matches!(
        repository.save(&document, Some(4)),
        Err(StorageError::Unavailable(_))
    ));
    assert_eq!(fs::read(&path).unwrap(), original, "the original stays");
}

#[test]
fn a_newer_format_version_is_refused_and_left_untouched() {
    let newer = String::from_utf8(fixture("v4-document.json"))
        .unwrap()
        .replace("\"format_version\": 4", "\"format_version\": 5")
        .into_bytes();
    let (root, repository, path) = stored(&newer);
    let refused = |result: Result<_, StorageError>| match result {
        Err(StorageError::Corrupt(message)) => message,
        other => panic!("expected an unreadable file, got {other:?}"),
    };
    let message = refused(repository.load(&id()).map(|_| ()));
    assert!(
        message.contains("version 5") && message.contains("versions 1, 2, 3, and 4"),
        "the reason names the version: {message}"
    );
    refused(repository.stored_revision(&id()).map(|_| ()));
    let replacement = Document::new(id(), "Replacement").unwrap();
    refused(repository.save(&replacement, Some(4)));
    refused(repository.save(&replacement, None));
    assert_eq!(fs::read(&path).unwrap(), newer);
    assert!(!root.path().join("migrated").exists());
}

#[test]
fn a_version_1_draft_migrates_when_it_is_recovered() {
    let (root, repository, _path) = stored(&fixture("v1-document.json"));
    let draft_path = root.path().join("drafts").join(format!("{ID}.json"));
    fs::create_dir_all(draft_path.parent().unwrap()).unwrap();
    let original = fixture("v1-draft.json");
    fs::write(&draft_path, &original).unwrap();

    let DraftStatus::Abandoned(Ok(info)) = repository.draft_status(&id()).unwrap() else {
        panic!("the version 1 draft is offered for recovery");
    };
    assert_eq!(
        (info.revision, info.base_revision, info.saved_at_ms),
        (4, 2, 1_767_225_600_000)
    );
    let Some(Ok(draft)) = repository.claim_draft(&id()).unwrap() else {
        panic!("the version 1 draft is recovered");
    };
    assert_eq!(draft.document, drawn_today());
    assert_eq!(
        fs::read(&draft_path).unwrap(),
        original,
        "reading wrote nothing"
    );

    // The recovered session's next draft is version 4.
    repository
        .write_draft(&Draft {
            saved_at_ms: 1_767_225_700_000,
            ..draft
        })
        .unwrap();
    let rewritten = String::from_utf8(fs::read(&draft_path).unwrap()).unwrap();
    assert!(rewritten.contains("\"format_version\":4"), "{rewritten}");
}

#[test]
fn several_scene_instances_round_trip_in_version_4() {
    let root = tempfile::tempdir().unwrap();
    let repository = FileDocumentRepository::new(root.path());
    let mut document = drawn_today();
    document
        .apply(&[Command::AddInstance {
            source: None,
            offset: Vector { dx: 0.1, dy: 80.0 },
        }])
        .unwrap();
    document
        .apply(&[Command::MoveInstance {
            instance: ObjectId::from_serial(2).unwrap(),
            offset: Vector { dx: 5.0, dy: 0.0 },
        }])
        .unwrap();
    document.undo().unwrap();
    repository.save(&document, None).unwrap();
    let loaded = repository.load(&id()).unwrap().unwrap();
    assert_eq!(loaded, document);
    assert_eq!(loaded.instances().count(), 2);
    assert_eq!(
        loaded.position(ObjectId::from_serial(2).unwrap(), atom(2)),
        Some(point(30.1, 90.5))
    );
    assert!(!repository.migrated_path(&id(), 1).exists());
}

/// A version 1 file whose history fits the version 1 limit but not version 2's: 60 entries
/// alternately adding and deleting 1000 atoms are 60 000 changes in version 1, where an atom
/// was one change, and 120 000 in version 2, where it is the atom plus its position.
fn version_1_with_long_history() -> Vec<u8> {
    use serde_json::{Value, json};
    let atom = |serial: u32| json!({ "element": "C", "charge": 0, "isotope": null, "position": { "x": f64::from(serial % 1000), "y": 0.0 } });
    let block = |block: u32, added: bool| -> Value {
        let atoms: Vec<Value> = (1000 * block + 1..=1000 * block + 1000)
            .map(|serial| {
                let (before, after) = if added {
                    (Value::Null, atom(serial))
                } else {
                    (atom(serial), Value::Null)
                };
                json!({ "id": format!("a{serial}"), "before": before, "after": after })
            })
            .collect();
        json!({ "title": null, "atoms": atoms, "bonds": [] })
    };
    let undo: Vec<Value> = (0..30)
        .flat_map(|k| [block(k, true), block(k, false)])
        .collect();
    let file = json!({
        "format": "chemifree.document",
        "format_version": 1,
        "document": {
            "id": ID, "revision": 60, "title": "Long history",
            "atoms": [], "bonds": [], "next_atom": 30_001, "next_bond": 1
        },
        "history": { "undo": undo, "redo": [] }
    });
    serde_json::to_vec(&file).unwrap()
}

#[test]
fn migrated_history_beyond_the_version_2_limit_is_trimmed_oldest_first_and_kept_in_full_on_save() {
    let original = version_1_with_long_history();
    let (_root, repository, _path) = stored(&original);
    let mut document = repository.load(&id()).unwrap().unwrap();

    // 50 entries of 2000 changes fill the 100 000-change limit; the oldest 10 are dropped.
    assert_eq!(document.undo_depth(), 50);
    for _ in 0..50 {
        document.undo().unwrap();
    }
    assert_eq!(document.atoms().len(), 0);
    document.redo().unwrap();
    let ids: Vec<String> = document.atoms().map(|(id, _)| id.to_string()).collect();
    assert_eq!(ids.len(), 1000);
    assert_eq!(
        (ids.first().unwrap().as_str(), ids.last().unwrap().as_str()),
        ("a5001", "a6000"),
        "the oldest remaining entry is the sixth block's addition"
    );

    // The first save keeps the version 1 bytes, full history included.
    repository.save(&document, Some(60)).unwrap();
    assert_eq!(
        fs::read(repository.migrated_path(&id(), 1)).unwrap(),
        original
    );
}

/// The version 2 fixture's history made with today's commands: the version 1 fixture's first
/// three edits, a second scene instance, a move in it, and an undo of that move.
fn drawn_in_version_2() -> Document {
    let mut document = drawn_and_edited();
    document
        .apply(&[Command::AddInstance {
            source: None,
            offset: Vector { dx: 0.1, dy: 80.0 },
        }])
        .unwrap();
    document
        .apply(&[Command::MoveAtoms {
            atoms: vec![atom(1)],
            delta: Vector { dx: 5.0, dy: 0.0 },
            instance: Some(ObjectId::from_serial(2).unwrap()),
        }])
        .unwrap();
    document.undo().unwrap();
    document
}

#[test]
fn a_version_2_file_migrates_with_default_states_and_its_history() {
    let (_root, repository, _path) = stored(&fixture("v2-document.json"));
    let mut document = repository.load(&id()).unwrap().unwrap();
    assert_eq!(
        document,
        drawn_in_version_2(),
        "the same graph, scene, counters, revision, and history"
    );
    assert_eq!(document.revision(), 6);
    for (_, atom) in document.atoms() {
        assert_eq!(atom.hydrogens, Hydrogens::Inferred);
        assert_eq!(atom.radical, Radical::None);
        assert!(atom.attachment_points.is_empty());
    }

    // Undo reaches back through the migrated history to the empty drawing.
    for _ in 0..4 {
        document.undo().unwrap();
    }
    assert_eq!(document.atoms().len(), 0);
    for _ in 0..5 {
        document.redo().unwrap();
    }
    assert_eq!(
        document.position(ObjectId::from_serial(2).unwrap(), atom(1)),
        Some(point(5.1, 80.0))
    );
}

/// The v2 fixture is an independent older drawing. Add the fields that version 3 wrote,
/// including a recorded radical on a1 in every state where that atom exists.
fn version_3_fixture(name: &str) -> Vec<u8> {
    use serde_json::{Value, json};

    fn add_states(state: &mut Value, atom: &str) {
        if let Some(fields) = state.as_object_mut() {
            fields.insert("hydrogens".into(), json!({ "mode": "inferred" }));
            fields.insert(
                "radical".into(),
                json!(if atom == "a1" { "doublet" } else { "none" }),
            );
            fields.insert("attachment_points".into(), json!([]));
        }
    }

    let mut file: Value = serde_json::from_slice(&fixture(name)).unwrap();
    file["format_version"] = json!(3);
    for atom in file["document"]["molecule"]["atoms"]
        .as_array_mut()
        .unwrap()
    {
        let id = atom["id"].as_str().unwrap().to_owned();
        add_states(&mut atom["state"], &id);
    }
    for stack in ["undo", "redo"] {
        for set in file["history"][stack].as_array_mut().unwrap() {
            for atom in set["atoms"].as_array_mut().unwrap() {
                let id = atom["id"].as_str().unwrap().to_owned();
                add_states(&mut atom["before"], &id);
                add_states(&mut atom["after"], &id);
            }
        }
    }
    serde_json::to_vec_pretty(&file).unwrap()
}

#[test]
fn recovering_an_older_draft_keeps_its_exact_bytes_before_rewriting_it() {
    let (root, repository, _) = stored(&version_3_fixture("v2-document.json"));
    let mut draft: serde_json::Value =
        serde_json::from_slice(&version_3_fixture("v2-draft.json")).unwrap();
    draft["base_revision"] = serde_json::json!(6);
    let original = serde_json::to_vec_pretty(&draft).unwrap();
    let draft_path = root.path().join("drafts").join(format!("{ID}.json"));
    let backup = root
        .path()
        .join("migrated")
        .join(format!("{ID}.draft.v3.json"));
    fs::create_dir_all(draft_path.parent().unwrap()).unwrap();
    fs::write(&draft_path, &original).unwrap();
    let app = Application::new(LocalCapabilities, repository);

    app.document(ID).unwrap();
    assert_eq!(
        fs::read(&draft_path).unwrap(),
        original,
        "inspection never rewrites a draft"
    );
    assert!(!backup.exists(), "inspection never archives a draft");
    let recovered = app.recover(ID, Some("recover-old")).unwrap();
    assert_eq!(
        fs::read(&backup).unwrap(),
        original,
        "recovery keeps the original bytes"
    );
    assert_eq!(
        fs::read(&draft_path).unwrap(),
        original,
        "recovery has not rewritten the draft"
    );
    let commands =
        serde_json::from_value(serde_json::json!([{ "op": "rename", "title": "After recovery" }]))
            .unwrap();
    app.apply_transaction(ID, None, recovered.document.revision, commands)
        .unwrap();
    assert!(app.draft_all().is_empty());
    assert_ne!(
        fs::read(&draft_path).unwrap(),
        original,
        "the next draft uses the current format"
    );
    assert_eq!(
        fs::read(&backup).unwrap(),
        original,
        "rewriting cannot replace the original copy"
    );
}

#[test]
fn recovering_an_older_draft_as_a_copy_keeps_its_original_bytes() {
    for (version, document, original) in [
        (1, fixture("v1-document.json"), fixture("v1-draft.json")),
        (2, fixture("v2-document.json"), fixture("v2-draft.json")),
        (
            3,
            version_3_fixture("v2-document.json"),
            version_3_fixture("v2-draft.json"),
        ),
    ] {
        let (root, repository, _) = stored(&document);
        let draft_path = root.path().join("drafts").join(format!("{ID}.json"));
        let backup = root
            .path()
            .join("migrated")
            .join(format!("{ID}.draft.v{version}.json"));
        fs::create_dir_all(draft_path.parent().unwrap()).unwrap();
        fs::write(&draft_path, &original).unwrap();
        let app = Application::new(LocalCapabilities, repository);

        app.recover_as_copy(ID, Some("copy-old"), None).unwrap();
        assert!(
            !draft_path.exists(),
            "version {version}: the draft is consumed"
        );
        assert_eq!(
            fs::read(&backup).unwrap(),
            original,
            "version {version}: original bytes"
        );
    }
}

#[test]
fn discarding_an_older_draft_does_not_archive_it() {
    let (root, repository, _) = stored(&version_3_fixture("v2-document.json"));
    let draft_path = root.path().join("drafts").join(format!("{ID}.json"));
    let backup = root
        .path()
        .join("migrated")
        .join(format!("{ID}.draft.v3.json"));
    fs::create_dir_all(draft_path.parent().unwrap()).unwrap();
    fs::write(&draft_path, version_3_fixture("v2-draft.json")).unwrap();
    let app = Application::new(LocalCapabilities, repository);

    app.discard_draft(ID, Some("discard-old")).unwrap();
    assert!(!draft_path.exists());
    assert!(
        !backup.exists(),
        "an explicit discard does not create an archive"
    );
}

#[test]
fn different_old_drafts_of_one_version_are_kept_without_overwriting_or_duplicating() {
    let (root, repository, _) = stored(&version_3_fixture("v2-document.json"));
    let original = version_3_fixture("v2-draft.json");
    let mut different = original.clone();
    different.push(b'\n');
    let draft_path = root.path().join("drafts").join(format!("{ID}.json"));
    fs::create_dir_all(draft_path.parent().unwrap()).unwrap();
    let app = Application::new(LocalCapabilities, repository);
    for (request, bytes) in [
        ("copy-1", &original),
        ("copy-2", &different),
        ("copy-3", &original),
    ] {
        fs::write(&draft_path, bytes).unwrap();
        app.recover_as_copy(ID, Some(request), None).unwrap();
    }
    let migrated = root.path().join("migrated");
    assert_eq!(
        fs::read(migrated.join(format!("{ID}.draft.v3.json"))).unwrap(),
        original
    );
    assert_eq!(
        fs::read(migrated.join(format!("{ID}.draft.v3.2.json"))).unwrap(),
        different
    );
    assert!(!migrated.join(format!("{ID}.draft.v3.3.json")).exists());
}

#[test]
fn failed_draft_archive_leaves_the_draft_and_creates_no_copy() {
    let (root, repository, _) = stored(&version_3_fixture("v2-document.json"));
    let original = version_3_fixture("v2-draft.json");
    let draft_path = root.path().join("drafts").join(format!("{ID}.json"));
    fs::create_dir_all(draft_path.parent().unwrap()).unwrap();
    fs::write(&draft_path, &original).unwrap();
    fs::write(root.path().join("migrated"), b"not a directory").unwrap();
    let app = Application::new(LocalCapabilities, repository);

    assert!(
        app.recover_as_copy(ID, Some("cannot-archive"), None)
            .is_err()
    );
    assert_eq!(fs::read(&draft_path).unwrap(), original);
    assert_eq!(
        app.document(ID).unwrap().recovery.unwrap().state,
        RecoveryState::Outdated
    );
    let another = FileDocumentRepository::new(root.path());
    assert!(matches!(
        another.draft_status(&id()),
        Ok(DraftStatus::Abandoned(Ok(_)))
    ));
    assert_eq!(another.list_ids().unwrap(), vec![id()]);
    fs::remove_file(root.path().join("migrated")).unwrap();
    let copied = app
        .recover_as_copy(ID, Some("cannot-archive"), None)
        .unwrap();
    assert_eq!(
        app.recover_as_copy(ID, Some("cannot-archive"), None)
            .unwrap(),
        copied
    );
    assert_eq!(
        fs::read(
            root.path()
                .join("migrated")
                .join(format!("{ID}.draft.v3.json"))
        )
        .unwrap(),
        original
    );
    assert_eq!(
        another.list_ids().unwrap().len(),
        2,
        "retry commits only one copy"
    );
}

#[test]
fn an_outdated_draft_is_reported_before_a_backup_would_fail() {
    let (root, repository, _) = stored(&version_3_fixture("v2-document.json"));
    let original = version_3_fixture("v2-draft.json");
    let draft_path = root.path().join("drafts").join(format!("{ID}.json"));
    fs::create_dir_all(draft_path.parent().unwrap()).unwrap();
    fs::write(&draft_path, &original).unwrap();
    fs::write(root.path().join("migrated"), b"not a directory").unwrap();
    let app = Application::new(LocalCapabilities, repository);

    let error = app.recover(ID, Some("outdated-before-backup")).unwrap_err();
    assert_eq!(error.code, ErrorCode::DraftOutdated);
    assert_eq!(error.current_revision, Some(6));
    assert_eq!(fs::read(&draft_path).unwrap(), original);
}

#[test]
fn previously_used_identifiers_are_reported_before_a_backup_would_fail() {
    let (root, repository, _) = stored(&version_3_fixture("v2-document.json"));
    let mut draft: serde_json::Value =
        serde_json::from_slice(&version_3_fixture("v2-draft.json")).unwrap();
    draft["base_revision"] = serde_json::json!(6);
    let original = serde_json::to_vec_pretty(&draft).unwrap();
    let draft_path = root.path().join("drafts").join(format!("{ID}.json"));
    fs::create_dir_all(draft_path.parent().unwrap()).unwrap();
    fs::write(&draft_path, &original).unwrap();
    let app = Application::new(LocalCapabilities, repository);
    let commands = serde_json::from_value(serde_json::json!([
        { "op": "add_graphic", "shape": "line", "points": [{ "x": 0, "y": 0 }, { "x": 1, "y": 1 }] }
    ]))
    .unwrap();
    let edited = app.apply_transaction(ID, None, 6, commands).unwrap();
    app.close(ID, None, true, Some(edited.document.revision))
        .unwrap();
    fs::write(root.path().join("migrated"), b"not a directory").unwrap();

    let error = app
        .recover(ID, Some("identifiers-before-backup"))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::DraftOutdated);
    assert_eq!(fs::read(&draft_path).unwrap(), original);
}

#[test]
fn full_dirty_sessions_are_reported_before_a_backup_would_fail() {
    enum Recovery {
        InPlace,
        Copy,
    }
    for mode in [Recovery::InPlace, Recovery::Copy] {
        let (root, repository, _) = stored(&version_3_fixture("v2-document.json"));
        let mut draft: serde_json::Value =
            serde_json::from_slice(&version_3_fixture("v2-draft.json")).unwrap();
        draft["base_revision"] = serde_json::json!(6);
        let original = serde_json::to_vec_pretty(&draft).unwrap();
        let draft_path = root.path().join("drafts").join(format!("{ID}.json"));
        fs::create_dir_all(draft_path.parent().unwrap()).unwrap();
        fs::write(&draft_path, &original).unwrap();
        let app = Application::new(LocalCapabilities, repository);
        for _ in 0..MAX_OPEN_SESSIONS {
            let id = app.create_document(None, None).unwrap().document.id;
            let commands =
                serde_json::from_value(serde_json::json!([{ "op": "rename", "title": "Unsaved" }]))
                    .unwrap();
            app.apply_transaction(&id, None, 0, commands).unwrap();
        }
        fs::write(root.path().join("migrated"), b"not a directory").unwrap();
        let error = match mode {
            Recovery::InPlace => app.recover(ID, Some("full-before-backup")).unwrap_err(),
            Recovery::Copy => app
                .recover_as_copy(ID, Some("full-before-backup"), None)
                .unwrap_err(),
        };
        assert_eq!(error.code, ErrorCode::ResourceLimit);
        assert_eq!(fs::read(&draft_path).unwrap(), original);
    }
}

#[test]
fn invalid_copy_title_is_reported_before_a_backup_would_fail() {
    let (root, repository, _) = stored(&version_3_fixture("v2-document.json"));
    let original = version_3_fixture("v2-draft.json");
    let draft_path = root.path().join("drafts").join(format!("{ID}.json"));
    fs::create_dir_all(draft_path.parent().unwrap()).unwrap();
    fs::write(&draft_path, &original).unwrap();
    fs::write(root.path().join("migrated"), b"not a directory").unwrap();
    let app = Application::new(LocalCapabilities, repository);

    let error = app
        .recover_as_copy(ID, Some("invalid-title-before-backup"), Some("   "))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    assert_eq!(fs::read(&draft_path).unwrap(), original);
}

#[test]
fn recovery_archives_the_bytes_actually_claimed_even_if_the_path_changes() {
    let (root, repository, _) = stored(&version_3_fixture("v2-document.json"));
    let original = version_3_fixture("v2-draft.json");
    let draft_path = root.path().join("drafts").join(format!("{ID}.json"));
    fs::create_dir_all(draft_path.parent().unwrap()).unwrap();
    fs::write(&draft_path, &original).unwrap();
    assert!(matches!(repository.claim_draft(&id()), Ok(Some(Ok(_)))));
    let mut replacement = original.clone();
    replacement.push(b'\n');
    fs::write(&draft_path, replacement).unwrap();
    repository.prepare_draft_for_recovery(&id()).unwrap();
    assert_eq!(
        fs::read(repository.migrated_draft_path(&id(), 3)).unwrap(),
        original
    );
    repository.release_draft(&id());
}

#[test]
fn an_interrupted_old_draft_commit_preserves_its_bytes_before_cleanup() {
    let (root, repository, _) = stored(&version_3_fixture("v2-document.json"));
    let original = version_3_fixture("v2-draft.json");
    let interrupted = root.path().join("drafts").join(format!("{ID}.committing"));
    fs::create_dir_all(interrupted.parent().unwrap()).unwrap();
    fs::write(&interrupted, &original).unwrap();
    // The stored revision differs from the draft's base, so the interrupted commit finished.
    assert!(matches!(repository.claim_draft(&id()), Ok(None)));
    assert!(!interrupted.exists());
    assert_eq!(
        fs::read(repository.migrated_draft_path(&id(), 3)).unwrap(),
        original
    );
}

#[test]
fn failed_interrupted_draft_archive_leaves_the_original_for_retry() {
    let (root, repository, _) = stored(&version_3_fixture("v2-document.json"));
    let original = version_3_fixture("v2-draft.json");
    let interrupted = root.path().join("drafts").join(format!("{ID}.committing"));
    fs::create_dir_all(interrupted.parent().unwrap()).unwrap();
    fs::write(&interrupted, &original).unwrap();
    fs::write(root.path().join("migrated"), b"not a directory").unwrap();
    assert!(repository.claim_draft(&id()).is_err());
    assert_eq!(fs::read(&interrupted).unwrap(), original);
    fs::remove_file(root.path().join("migrated")).unwrap();
    assert!(matches!(repository.claim_draft(&id()), Ok(None)));
    assert_eq!(
        fs::read(repository.migrated_draft_path(&id(), 3)).unwrap(),
        original
    );
}

#[test]
fn version_3_file_and_draft_migrate_with_history_and_keep_original_bytes() {
    let original = version_3_fixture("v2-document.json");
    let (root, repository, path) = stored(&original);
    let mut document = repository.load(&id()).unwrap().unwrap();
    assert_eq!(document.atoms().next().unwrap().1.radical, Radical::Doublet);
    assert_eq!(document.instances().count(), 2);
    assert_eq!(
        fs::read(&path).unwrap(),
        original,
        "reading does not rewrite"
    );
    document.undo().unwrap();
    assert_eq!(document.instances().count(), 1);
    document.redo().unwrap();
    assert_eq!(document.instances().count(), 2);
    repository.save(&document, Some(6)).unwrap();
    assert_eq!(
        fs::read(repository.migrated_path(&id(), 3)).unwrap(),
        original
    );
    assert_eq!(repository.load(&id()).unwrap(), Some(document));

    let original_draft = version_3_fixture("v2-draft.json");
    let draft_path = root.path().join("drafts").join(format!("{ID}.json"));
    fs::create_dir_all(draft_path.parent().unwrap()).unwrap();
    fs::write(&draft_path, &original_draft).unwrap();
    let Some(Ok(draft)) = repository.claim_draft(&id()).unwrap() else {
        panic!("a version 3 draft is recoverable");
    };
    assert_eq!(
        draft.document.atoms().next().unwrap().1.radical,
        Radical::Doublet
    );
    assert_eq!(draft.document.instances().count(), 2);
    assert_eq!(fs::read(&draft_path).unwrap(), original_draft);
}

#[test]
fn scene_tree_selection_and_history_survive_file_and_draft_round_trips() {
    let root = tempfile::tempdir().unwrap();
    let repository = FileDocumentRepository::new(root.path());
    let mut document = Document::new(id(), "Scene").unwrap();
    let graphic = |shape| Command::AddGraphic {
        graphic: Graphic {
            shape,
            points: [point(0.0, 0.0), point(10.0, 10.0)],
        },
        parent: None,
        index: None,
    };
    document
        .apply(&[graphic(GraphicShape::Line), graphic(GraphicShape::Arrow)])
        .unwrap();
    let selected = Selection {
        objects: [
            ObjectId::from_serial(2).unwrap(),
            ObjectId::from_serial(3).unwrap(),
        ]
        .into(),
        ..Selection::default()
    };
    document
        .apply_selecting(
            &[Command::Group {
                objects: selected.objects.iter().copied().collect(),
            }],
            &TransactionSelection {
                before: Some(selected),
                after: Some(SelectionRequest {
                    objects: vec![ObjectRef::Created(0)],
                    ..SelectionRequest::default()
                }),
            },
        )
        .unwrap();
    document
        .apply(&[Command::AddReference {
            reference: SceneReference {
                source: ObjectId::from_serial(2).unwrap(),
                target: ReferenceTarget::Object(ObjectId::from_serial(3).unwrap()),
            },
        }])
        .unwrap();
    document
        .apply(&[Command::DeleteObjects {
            objects: vec![ObjectId::from_serial(2).unwrap()],
            mode: DeleteMode::Subtree,
        }])
        .unwrap();
    document.undo().unwrap();
    document.undo().unwrap();
    document.redo().unwrap();

    repository.save(&document, None).unwrap();
    let loaded = repository.load(&id()).unwrap().unwrap();
    assert_eq!(
        loaded, document,
        "tree, selection, both stacks, and counters round-trip"
    );
    let draft = Draft {
        document: document.clone(),
        base_revision: 0,
        saved_at_ms: 42,
    };
    repository.write_draft(&draft).unwrap();
    repository.release_draft(&id());
    assert_eq!(repository.claim_draft(&id()), Ok(Some(Ok(draft))));

    let path = root.path().join("documents").join(format!("{ID}.json"));
    let pristine = fs::read(&path).unwrap();
    let mut corrupt: serde_json::Value = serde_json::from_slice(&pristine).unwrap();
    corrupt["document"]["selection"]["objects"] = serde_json::json!(["s1"]);
    fs::write(&path, serde_json::to_vec(&corrupt).unwrap()).unwrap();
    assert!(
        matches!(repository.load(&id()), Err(StorageError::Corrupt(_))),
        "an existing object is still invalid if selection disagrees with the history boundary"
    );
    let mut dangling: serde_json::Value = serde_json::from_slice(&pristine).unwrap();
    dangling["document"]["references"][0]["target"]["id"] = serde_json::json!("s99");
    fs::write(&path, serde_json::to_vec(&dangling).unwrap()).unwrap();
    assert!(
        matches!(repository.load(&id()), Err(StorageError::Corrupt(_))),
        "dangling scene references are unreadable"
    );
}

#[test]
fn a_version_2_draft_migrates_when_it_is_recovered() {
    let (root, repository, _path) = stored(&fixture("v2-document.json"));
    let draft_path = root.path().join("drafts").join(format!("{ID}.json"));
    fs::create_dir_all(draft_path.parent().unwrap()).unwrap();
    let original = fixture("v2-draft.json");
    fs::write(&draft_path, &original).unwrap();

    let DraftStatus::Abandoned(Ok(info)) = repository.draft_status(&id()).unwrap() else {
        panic!("the version 2 draft is offered for recovery");
    };
    assert_eq!(
        (info.revision, info.base_revision, info.saved_at_ms),
        (6, 3, 1_767_225_600_000)
    );
    let Some(Ok(draft)) = repository.claim_draft(&id()).unwrap() else {
        panic!("the version 2 draft is recovered");
    };
    assert_eq!(draft.document, drawn_in_version_2());
    assert_eq!(
        fs::read(&draft_path).unwrap(),
        original,
        "reading wrote nothing"
    );
}

#[test]
fn the_first_save_over_a_version_2_file_keeps_its_exact_bytes_once() {
    let original = fixture("v2-document.json");
    let (root, repository, path) = stored(&original);
    let mut document = repository.load(&id()).unwrap().unwrap();
    document.redo().unwrap();
    repository.save(&document, Some(6)).unwrap();

    let kept = repository.migrated_path(&id(), 2);
    assert_eq!(kept.file_name().unwrap(), format!("{ID}.v2.json").as_str());
    assert_eq!(fs::read(&kept).unwrap(), original);
    let saved = String::from_utf8(fs::read(&path).unwrap()).unwrap();
    assert!(saved.contains("\"format_version\":4"), "{saved}");

    // The same version 2 bytes stored again are already kept; other bytes get the next name.
    fs::write(&path, &original).unwrap();
    let mut again = repository.load(&id()).unwrap().unwrap();
    again.redo().unwrap();
    repository.save(&again, Some(6)).unwrap();
    assert_eq!(migrated_names(root.path()), [format!("{ID}.v2.json")]);

    let edited = String::from_utf8(original.clone())
        .unwrap()
        .replace("Fixture edited", "Fixture edited elsewhere");
    fs::write(&path, &edited).unwrap();
    let mut other = repository.load(&id()).unwrap().unwrap();
    other.redo().unwrap();
    repository.save(&other, Some(6)).unwrap();
    assert_eq!(
        migrated_names(root.path()),
        [format!("{ID}.v2.2.json"), format!("{ID}.v2.json")]
    );
    assert_eq!(fs::read(&kept).unwrap(), original, "never overwritten");
    assert_eq!(
        fs::read(kept.with_file_name(format!("{ID}.v2.2.json"))).unwrap(),
        edited.as_bytes()
    );
}

#[test]
fn version_1_and_version_2_copies_of_one_document_are_kept_apart() {
    let (root, repository, path) = stored(&fixture("v1-document.json"));
    let mut document = repository.load(&id()).unwrap().unwrap();
    document.redo().unwrap();
    repository.save(&document, Some(4)).unwrap();

    fs::write(&path, fixture("v2-document.json")).unwrap();
    let mut document = repository.load(&id()).unwrap().unwrap();
    document.redo().unwrap();
    repository.save(&document, Some(6)).unwrap();
    assert_eq!(
        migrated_names(root.path()),
        [format!("{ID}.v1.json"), format!("{ID}.v2.json")]
    );
}

/// Every recorded state survives a save and a reopen in version 4, history included.
#[test]
fn recorded_states_round_trip_in_version_4() {
    let root = tempfile::tempdir().unwrap();
    let repository = FileDocumentRepository::new(root.path());
    let mut document = drawn_today();
    let query = AtomKind::Query(QueryAtom::List {
        elements: vec![
            Element::from_symbol("S").unwrap(),
            Element::from_symbol("Se").unwrap(),
        ],
        negated: true,
    });
    document
        .apply(&[
            Command::UpdateAtom {
                atom: atom(1),
                edit: AtomEdit {
                    hydrogens: Some(Hydrogens::Specified(3)),
                    radical: Some(Radical::Doublet),
                    attachment_points: Some(AttachmentPoints::new(&[2, 4]).unwrap()),
                    ..AtomEdit::default()
                },
            },
            Command::UpdateAtom {
                atom: atom(2),
                edit: AtomEdit {
                    kind: Some(query),
                    charge: Some(0),
                    isotope: Some(None),
                    hydrogens: Some(Hydrogens::Unknown),
                    ..AtomEdit::default()
                },
            },
            Command::UpdateBond {
                bond: BondId::from_serial(1).unwrap(),
                order: BondOrder::Query(BondQuery::OneOf(vec![
                    QueryBondOrder::Aromatic,
                    QueryBondOrder::Double,
                ])),
            },
            Command::AddAtom {
                atom: Atom {
                    radical: Radical::Singlet,
                    ..Atom::new(AtomKind::Query(QueryAtom::Heteroatom))
                },
                positions: NewAtomPositions::Single(point(60.0, 0.0)),
                bond_to: Some(BondTo {
                    atom: AtomRef::Id(atom(1)),
                    order: BondOrder::Dative,
                }),
            },
        ])
        .unwrap();
    document
        .apply(&[Command::AddBond {
            begin: AtomRef::Id(atom(2)),
            end: AtomRef::Id(atom(3)),
            order: BondOrder::Unknown,
        }])
        .unwrap();
    document
        .apply(&[Command::UpdateBond {
            bond: BondId::from_serial(2).unwrap(),
            order: BondOrder::Aromatic,
        }])
        .unwrap();
    document.undo().unwrap();
    repository.save(&document, None).unwrap();

    let mut loaded = repository.load(&id()).unwrap().unwrap();
    assert_eq!(loaded, document);
    loaded.redo().unwrap();
    assert_eq!(
        loaded.bonds().nth(1).unwrap().1.order,
        BondOrder::Aromatic,
        "the redo stack kept the aromatic bond"
    );
    for _ in 0..loaded.undo_depth() {
        loaded.undo().unwrap();
    }
    assert_eq!(loaded.atoms().len(), 0, "the whole history replays");
}

#[test]
fn a_stored_state_combination_an_edit_would_refuse_is_unreadable() {
    let root = tempfile::tempdir().unwrap();
    let repository = FileDocumentRepository::new(root.path());
    let mut document = drawn_today();
    document
        .apply(&[Command::UpdateAtom {
            atom: atom(1),
            edit: AtomEdit {
                hydrogens: Some(Hydrogens::Specified(16)),
                attachment_points: Some(AttachmentPoints::new(&[1]).unwrap()),
                ..AtomEdit::default()
            },
        }])
        .unwrap();
    repository.save(&document, None).unwrap();
    let path = root.path().join("documents").join(format!("{ID}.json"));
    let valid = String::from_utf8(fs::read(&path).unwrap()).unwrap();
    for (name, invalid) in [
        (
            "17 stated hydrogens",
            valid.replacen("\"count\":16", "\"count\":17", 1),
        ),
        (
            "an attachment point repeated",
            valid.replacen(
                "\"attachment_points\":[1]",
                "\"attachment_points\":[1,1]",
                1,
            ),
        ),
        (
            "a query atom with a charge",
            valid.replacen(
                "\"element\":\"C\",\"charge\":0",
                "\"query\":{\"kind\":\"any\"},\"charge\":1",
                1,
            ),
        ),
        (
            "a query bond without its query",
            valid.replacen("\"order\":\"single\"", "\"order\":\"query\"", 1),
        ),
    ] {
        assert_ne!(invalid, valid, "{name} changes the file");
        fs::write(&path, &invalid).unwrap();
        assert!(
            matches!(repository.load(&id()), Err(StorageError::Corrupt(_))),
            "{name} was accepted"
        );
    }
}
