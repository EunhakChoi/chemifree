//! Version 1 documents and drafts, written before graph and scene were separated, and
//! version 2 ones, written before hydrogen, radical, query, and bond-kind states, are migrated
//! when read and keep their identifiers, revision, and undo/redo history; the first save that
//! replaces one keeps its exact bytes. Version 3 round-trips every state.

use std::fs;
use std::path::{Path, PathBuf};

use chemifree_application::{DocumentRepository, Draft, DraftStatus, StorageError};
use chemifree_domain::document::{
    Atom, AtomId, AtomKind, AtomQuery, AtomRef, AtomUpdate, AttachmentPoints, BondId, BondOrder,
    BondQuery, BondTo, Command, Document, DocumentId, Hydrogens, InstanceId, NewAtomPositions,
    Point, QueryBondOrder, Radical, Vector,
};
use chemifree_domain::element::Element;
use chemifree_providers::FileDocumentRepository;

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

fn s1() -> InstanceId {
    InstanceId::from_serial(1).unwrap()
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
                update: AtomUpdate {
                    charge: Some(-1),
                    isotope: Some(Some(18)),
                    ..AtomUpdate::default()
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
                update: AtomUpdate {
                    kind: Some(AtomKind::Element(Element::from_symbol("N").unwrap())),
                    ..AtomUpdate::default()
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
    document.undo().unwrap();
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
        document
            .atoms()
            .next()
            .unwrap()
            .1
            .element()
            .unwrap()
            .symbol(),
        "N"
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
        saved.contains("\"format_version\":3"),
        "the document is stored as version 3"
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
    let newer = fixture("v4-document.json");
    let (root, repository, path) = stored(&newer);
    let refused = |result: Result<_, StorageError>| match result {
        Err(StorageError::Corrupt(message)) => message,
        other => panic!("expected an unreadable file, got {other:?}"),
    };
    let message = refused(repository.load(&id()).map(|_| ()));
    assert!(
        message.contains("version 4") && message.contains("versions 1, 2, and 3"),
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

    // The recovered session's next draft is version 3.
    repository
        .write_draft(&Draft {
            saved_at_ms: 1_767_225_700_000,
            ..draft
        })
        .unwrap();
    let rewritten = String::from_utf8(fs::read(&draft_path).unwrap()).unwrap();
    assert!(rewritten.contains("\"format_version\":3"), "{rewritten}");
}

#[test]
fn several_scene_instances_round_trip_in_version_3() {
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
            instance: InstanceId::from_serial(2).unwrap(),
            offset: Vector { dx: 5.0, dy: 0.0 },
        }])
        .unwrap();
    document.undo().unwrap();
    repository.save(&document, None).unwrap();
    let loaded = repository.load(&id()).unwrap().unwrap();
    assert_eq!(loaded, document);
    assert_eq!(loaded.instances().len(), 2);
    assert_eq!(
        loaded.position(InstanceId::from_serial(2).unwrap(), atom(2)),
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

#[test]
fn a_version_2_file_migrates_with_default_states_and_its_history() {
    let original = fixture("v2-document.json");
    let (root, repository, path) = stored(&original);
    let mut document = repository.load(&id()).unwrap().unwrap();
    assert_eq!(
        document,
        drawn_today(),
        "the same graph, positions, counters, revision, and history"
    );
    for (_, atom) in document.atoms() {
        assert_eq!(
            atom.hydrogens,
            Hydrogens::Inferred,
            "not computed, not zero"
        );
        assert_eq!(atom.radical, Radical::None);
        assert!(atom.attachment_points.is_empty());
    }
    assert_eq!(document.bonds().next().unwrap().1.order, BondOrder::Single);
    assert_eq!(fs::read(&path).unwrap(), original, "reading wrote nothing");
    assert!(!root.path().join("migrated").exists());

    document.redo().unwrap();
    assert_eq!(document.bonds().next().unwrap().1.order, BondOrder::Double);
    for _ in 0..3 {
        document.undo().unwrap();
    }
    assert_eq!(document.atoms().len(), 0, "the whole history undoes");
}

#[test]
fn the_first_save_over_a_version_2_file_keeps_its_exact_bytes_once() {
    let original = fixture("v2-document.json");
    let (root, repository, path) = stored(&original);
    let mut document = repository.load(&id()).unwrap().unwrap();
    document.redo().unwrap();
    repository.save(&document, Some(4)).unwrap();

    let kept = repository.migrated_path(&id(), 2);
    assert_eq!(kept.file_name().unwrap(), format!("{ID}.v2.json").as_str());
    assert_eq!(fs::read(&kept).unwrap(), original);
    let saved = String::from_utf8(fs::read(&path).unwrap()).unwrap();
    assert!(saved.contains("\"format_version\":3"), "{saved}");
    assert_eq!(repository.load(&id()).unwrap(), Some(document.clone()));

    // Saving version 3 again keeps nothing more.
    let base = document.revision();
    document.undo().unwrap();
    repository.save(&document, Some(base)).unwrap();
    assert_eq!(migrated_names(root.path()), [format!("{ID}.v2.json")]);

    // Other version 2 bytes under the same ID get the next number; the same bytes, none.
    let mut changed = original.clone();
    changed.extend_from_slice(b"\n");
    for (bytes, expected) in [(&changed, 2), (&original, 2), (&changed, 2)] {
        fs::write(&path, bytes).unwrap();
        let mut document = repository.load(&id()).unwrap().unwrap();
        document.redo().unwrap();
        repository.save(&document, Some(4)).unwrap();
        assert_eq!(migrated_names(root.path()).len(), expected);
    }
    assert_eq!(
        fs::read(kept.with_file_name(format!("{ID}.v2.2.json"))).unwrap(),
        changed
    );
    assert_eq!(fs::read(&kept).unwrap(), original, "never overwritten");
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
    assert_eq!((info.revision, info.base_revision), (4, 2));
    let Some(Ok(draft)) = repository.claim_draft(&id()).unwrap() else {
        panic!("the version 2 draft is recovered");
    };
    assert_eq!(draft.document, drawn_today());
    assert_eq!(
        fs::read(&draft_path).unwrap(),
        original,
        "reading wrote nothing"
    );
    repository.write_draft(&draft).unwrap();
    let rewritten = String::from_utf8(fs::read(&draft_path).unwrap()).unwrap();
    assert!(rewritten.contains("\"format_version\":3"), "{rewritten}");
}

#[test]
fn a_version_1_file_saved_as_version_3_reopens_with_its_history() {
    let (root, repository, _path) = stored(&fixture("v1-document.json"));
    let document = repository.load(&id()).unwrap().unwrap();
    repository.save(&document, Some(4)).unwrap();

    let mut reopened = FileDocumentRepository::new(root.path())
        .load(&id())
        .unwrap()
        .unwrap();
    assert_eq!(reopened, drawn_today());
    reopened.undo().unwrap();
    assert_eq!(
        reopened.position(s1(), atom(2)),
        Some(point(30.0, 0.0)),
        "the move undoes after reopening"
    );
    reopened.redo().unwrap();
    reopened.redo().unwrap();
    assert_eq!(reopened.title(), "Fixture edited");
    assert_eq!(
        reopened.atoms().next().unwrap().1.hydrogens,
        Hydrogens::Inferred
    );
}

/// The fixture drawing with every state a version 3 file holds, set in one entry and partly
/// undone, so states appear in the document, the undo stack, and the redo stack.
fn with_every_state() -> Document {
    let mut document = drawn_today();
    let list = AtomQuery::List {
        elements: vec![
            Element::from_symbol("Cl").unwrap(),
            Element::from_symbol("Br").unwrap(),
        ],
        negated: true,
    };
    document
        .apply(&[
            Command::UpdateAtom {
                atom: atom(1),
                update: AtomUpdate {
                    hydrogens: Some(Hydrogens::Specified(2)),
                    radical: Some(Radical::Triplet),
                    attachment_points: Some(AttachmentPoints::try_new(&[3, 1]).unwrap()),
                    ..AtomUpdate::default()
                },
            },
            Command::UpdateAtom {
                atom: atom(2),
                update: AtomUpdate {
                    kind: Some(AtomKind::Query(list)),
                    charge: Some(0),
                    isotope: Some(None),
                    hydrogens: Some(Hydrogens::Unknown),
                    ..AtomUpdate::default()
                },
            },
            Command::UpdateBond {
                bond: BondId::from_serial(1).unwrap(),
                order: BondOrder::Query(BondQuery::List(vec![
                    QueryBondOrder::Aromatic,
                    QueryBondOrder::Double,
                ])),
            },
            Command::AddAtom {
                atom: Atom::from(AtomKind::Query(AtomQuery::Any)),
                positions: NewAtomPositions::Single(point(60.0, 0.0)),
                bond_to: Some(BondTo {
                    atom: AtomRef::Id(atom(1)),
                    order: BondOrder::Dative,
                }),
            },
        ])
        .unwrap();
    document
        .apply(&[Command::UpdateBond {
            bond: BondId::from_serial(2).unwrap(),
            order: BondOrder::Aromatic,
        }])
        .unwrap();
    document
        .apply(&[Command::UpdateBond {
            bond: BondId::from_serial(2).unwrap(),
            order: BondOrder::Unknown,
        }])
        .unwrap();
    document.undo().unwrap();
    document
}

#[test]
fn every_state_round_trips_in_version_3() {
    let root = tempfile::tempdir().unwrap();
    let repository = FileDocumentRepository::new(root.path());
    let document = with_every_state();
    repository.save(&document, None).unwrap();
    let mut loaded = FileDocumentRepository::new(root.path())
        .load(&id())
        .unwrap()
        .unwrap();
    assert_eq!(loaded, document);

    loaded.redo().unwrap();
    assert_eq!(loaded.bonds().last().unwrap().1.order, BondOrder::Unknown);
    loaded.undo().unwrap();
    loaded.undo().unwrap();
    loaded.undo().unwrap();
    assert_eq!(
        loaded
            .atoms()
            .map(|(_, atom)| atom.clone())
            .collect::<Vec<_>>(),
        drawn_today()
            .atoms()
            .map(|(_, atom)| atom.clone())
            .collect::<Vec<_>>(),
        "undo after reopening restores the states before"
    );
}

#[test]
fn version_3_states_that_do_not_hold_together_are_unreadable() {
    let root = tempfile::tempdir().unwrap();
    let repository = FileDocumentRepository::new(root.path());
    repository.save(&with_every_state(), None).unwrap();
    let path = root.path().join("documents").join(format!("{ID}.json"));
    let saved = fs::read_to_string(&path).unwrap();
    let edits = [
        // Both an element and a query.
        (
            r#"{"query":{"kind":"any"},"charge":0"#,
            r#"{"element":"C","query":{"kind":"any"},"charge":0"#,
        ),
        // A count with inferred hydrogens.
        (r#"{"mode":"inferred"}"#, r#"{"mode":"inferred","count":1}"#),
        // A query atom with a charge.
        (
            r#"{"query":{"kind":"any"},"charge":0"#,
            r#"{"query":{"kind":"any"},"charge":1"#,
        ),
        // A stated count above 16.
        (r#""count":2"#, r#""count":17"#),
        // Attachment points out of order.
        (
            r#""attachment_points":[1,3]"#,
            r#""attachment_points":[3,1]"#,
        ),
        // A query expression on a bond that is not a query.
        (
            r#""order":"dative""#,
            r#""order":"dative","query":{"kind":"any"}"#,
        ),
        // An empty element list.
        (r#""elements":["Cl","Br"]"#, r#""elements":[]"#),
    ];
    for (from, to) in edits {
        assert!(saved.contains(from), "the saved file contains {from}");
        fs::write(&path, saved.replacen(from, to, 1)).unwrap();
        assert!(
            matches!(repository.load(&id()), Err(StorageError::Corrupt(_))),
            "{to} is refused"
        );
    }
}
