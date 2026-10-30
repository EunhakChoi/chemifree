use std::collections::BTreeMap;

use chemifree_domain::document::{
    Atom, AtomEdit, AtomId, AtomRef, BondId, BondOrder, BondTo, Change, ChangeSet, Command,
    Document, DocumentId, EditError, HISTORY_LIMIT, HistoryError, MAX_ATOMS, MAX_COMMANDS,
    MAX_EDIT_REVISION, MAX_INSTANCES, MAX_REVISION, NewAtomPositions, ObjectId, Outcome, Point,
    PositionKey, RevisionsExhausted, TransactionError, Vector,
};
use chemifree_domain::element::Element;
use chemifree_domain::states::AtomKind;

fn element(symbol: &str) -> Element {
    Element::from_symbol(symbol).unwrap()
}

fn point(x: f64, y: f64) -> Point {
    Point::new(x, y).unwrap()
}

fn atom(serial: u32) -> AtomId {
    AtomId::from_serial(serial).unwrap()
}

fn bond(serial: u32) -> BondId {
    BondId::from_serial(serial).unwrap()
}

fn instance(serial: u32) -> ObjectId {
    ObjectId::from_serial(serial).unwrap()
}

fn here(x: f64, y: f64) -> NewAtomPositions {
    NewAtomPositions::Single(point(x, y))
}

fn move_atoms(atoms: Vec<AtomId>, dx: f64, dy: f64) -> Command {
    Command::MoveAtoms {
        atoms,
        delta: Vector { dx, dy },
        instance: None,
    }
}

/// Positions of `instance` as `(atom, x, y)`.
fn positions(document: &Document, instance: ObjectId) -> Vec<(String, f64, f64)> {
    document
        .positions(instance)
        .map(|(atom, point)| (atom.to_string(), point.x(), point.y()))
        .collect()
}

fn empty() -> Document {
    Document::new(DocumentId::parse("doc-1").unwrap(), "  Ethanol sketch ").unwrap()
}

fn add(symbol: &str, x: f64, y: f64) -> Command {
    Command::AddAtom {
        atom: Atom::from(element(symbol)),
        positions: here(x, y),
        bond_to: None,
    }
}

/// C1–C2–O3 drawn in one transaction.
fn ethanol() -> Document {
    let mut document = empty();
    document
        .apply(&[
            add("C", 0.0, 0.0),
            Command::AddAtom {
                atom: Atom::from(Element::CARBON),
                positions: here(26.0, 15.0),
                bond_to: Some(BondTo {
                    atom: AtomRef::Id(atom(1)),
                    order: BondOrder::Single,
                }),
            },
            Command::AddAtom {
                atom: Atom::from(element("O")),
                positions: here(52.0, 0.0),
                bond_to: Some(BondTo {
                    atom: AtomRef::Id(atom(2)),
                    order: BondOrder::Single,
                }),
            },
        ])
        .unwrap();
    document
}

fn command_error(document: &mut Document, commands: &[Command]) -> TransactionError {
    let before = document.clone();
    let error = document.apply(commands).unwrap_err();
    assert_eq!(
        *document, before,
        "a rejected transaction must not change the document"
    );
    error
}

#[test]
fn new_documents_are_empty_at_revision_zero_with_a_trimmed_title() {
    let document = empty();
    assert_eq!(document.revision(), 0);
    assert_eq!(document.title(), "Ethanol sketch");
    assert_eq!(document.atoms().len(), 0);
    assert_eq!((document.undo_depth(), document.redo_depth()), (0, 0));
    for title in ["", "   ", "line\nbreak", &"x".repeat(201)] {
        assert_eq!(
            Document::new(DocumentId::parse("d").unwrap(), title),
            Err(EditError::InvalidTitle)
        );
    }
}

#[test]
fn one_transaction_is_one_revision_and_one_undo_entry() {
    let document = ethanol();
    assert_eq!(document.revision(), 1);
    assert_eq!(document.undo_depth(), 1);
    let atoms: Vec<_> = document
        .atoms()
        .map(|(id, atom)| (id.to_string(), atom.kind.element().unwrap().symbol()))
        .collect();
    assert_eq!(
        atoms,
        [("a1".into(), "C"), ("a2".into(), "C"), ("a3".into(), "O")]
    );
    let bonds: Vec<_> = document
        .bonds()
        .map(|(id, bond)| (id.to_string(), bond.begin.to_string(), bond.end.to_string()))
        .collect();
    assert_eq!(
        bonds,
        [
            ("b1".into(), "a1".into(), "a2".into()),
            ("b2".into(), "a2".into(), "a3".into())
        ]
    );
}

#[test]
fn atoms_and_bonds_can_be_edited() {
    let mut document = ethanol();
    document
        .apply(&[
            Command::UpdateAtom {
                atom: atom(3),
                edit: AtomEdit {
                    kind: Some(AtomKind::Element(element("N"))),
                    charge: Some(1),
                    isotope: Some(Some(15)),
                    ..AtomEdit::default()
                },
            },
            Command::UpdateBond {
                bond: bond(1),
                order: BondOrder::Double,
            },
            move_atoms(vec![atom(1), atom(1), atom(2)], 10.0, -5.0),
            Command::Rename {
                title: "Edited".into(),
            },
        ])
        .unwrap();
    let atoms: Vec<_> = document.atoms().map(|(_, atom)| atom.clone()).collect();
    let s1 = instance(1);
    assert_eq!(
        document.position(s1, atom(1)),
        Some(point(10.0, -5.0)),
        "duplicate targets move once"
    );
    assert_eq!(document.position(s1, atom(2)), Some(point(36.0, 10.0)));
    assert_eq!(document.position(s1, atom(3)), Some(point(52.0, 0.0)));
    assert_eq!(
        (
            atoms[2].kind.element().unwrap().symbol(),
            atoms[2].charge,
            atoms[2].isotope
        ),
        ("N", 1, Some(15))
    );
    assert_eq!(document.bonds().next().unwrap().1.order, BondOrder::Double);
    assert_eq!(document.title(), "Edited");

    document
        .apply(&[Command::UpdateAtom {
            atom: atom(3),
            edit: AtomEdit {
                isotope: Some(None),
                ..AtomEdit::default()
            },
        }])
        .unwrap();
    assert_eq!(document.atoms().nth(2).unwrap().1.isotope, None);
    assert_eq!(
        document.atoms().nth(2).unwrap().1.charge,
        1,
        "absent fields stay"
    );
}

#[test]
fn deleting_an_atom_removes_its_incident_bonds() {
    let mut document = ethanol();
    document
        .apply(&[Command::DeleteAtoms {
            atoms: vec![atom(2)],
        }])
        .unwrap();
    assert_eq!(document.atoms().len(), 2);
    assert_eq!(document.bonds().len(), 0);
}

#[test]
fn invalid_commands_reject_the_whole_transaction_with_the_failing_index() {
    let mut document = ethanol();
    let cases = [
        (
            Command::AddBond {
                begin: AtomRef::Id(atom(1)),
                end: AtomRef::Id(atom(1)),
                order: BondOrder::Single,
            },
            EditError::SelfBond(atom(1)),
        ),
        (
            Command::AddBond {
                begin: AtomRef::Id(atom(2)),
                end: AtomRef::Id(atom(1)),
                order: BondOrder::Double,
            },
            EditError::DuplicateBond { existing: bond(1) },
        ),
        (
            Command::AddBond {
                begin: AtomRef::Id(atom(1)),
                end: AtomRef::Id(atom(9)),
                order: BondOrder::Single,
            },
            EditError::UnknownAtom(atom(9)),
        ),
        (
            Command::UpdateBond {
                bond: bond(7),
                order: BondOrder::Triple,
            },
            EditError::UnknownBond(bond(7)),
        ),
        (
            Command::UpdateAtom {
                atom: atom(1),
                edit: AtomEdit {
                    charge: Some(9),
                    ..AtomEdit::default()
                },
            },
            EditError::ChargeOutOfRange(9),
        ),
        (
            Command::UpdateAtom {
                atom: atom(1),
                edit: AtomEdit {
                    isotope: Some(Some(0)),
                    ..AtomEdit::default()
                },
            },
            EditError::IsotopeOutOfRange(0),
        ),
        (
            move_atoms(vec![atom(1)], 200_000.0, 0.0),
            EditError::CoordinateOutOfRange,
        ),
        (
            move_atoms(vec![atom(1)], f64::NAN, 0.0),
            EditError::CoordinateOutOfRange,
        ),
        (
            Command::DeleteAtoms { atoms: vec![] },
            EditError::EmptyTargetList,
        ),
        (
            Command::DeleteAtoms {
                atoms: vec![atom(1), atom(8)],
            },
            EditError::UnknownAtom(atom(8)),
        ),
        (
            Command::Rename {
                title: "tab\there".into(),
            },
            EditError::InvalidTitle,
        ),
    ];
    for (command, expected) in cases {
        // A valid first command proves earlier effects are rolled back too.
        let error = command_error(&mut document, &[add("S", 1.0, 1.0), command]);
        assert_eq!(
            error,
            TransactionError {
                index: Some(1),
                error: expected
            }
        );
    }
    assert_eq!(
        command_error(&mut document, &[]).error,
        EditError::EmptyTransaction
    );
    let too_many = vec![add("C", 0.0, 0.0); MAX_COMMANDS + 1];
    assert_eq!(
        command_error(&mut document, &too_many),
        TransactionError {
            index: None,
            error: EditError::TooManyCommands
        }
    );
}

#[test]
fn no_effective_change_keeps_revision_history_and_identifier_counters() {
    let mut document = ethanol();
    let before = document.clone();
    let outcome = document
        .apply(&[
            Command::Rename {
                title: "Ethanol sketch".into(),
            },
            move_atoms(vec![atom(1)], 0.0, 0.0),
            add("C", 5.0, 5.0),
            Command::DeleteAtoms {
                atoms: vec![atom(4)],
            },
        ])
        .unwrap();
    assert_eq!(outcome, Outcome::Unchanged);
    assert_eq!(document, before);
    document.apply(&[add("C", 5.0, 5.0)]).unwrap();
    assert_eq!(document.atoms().last().unwrap().0.to_string(), "a4");
}

#[test]
fn undo_and_redo_restore_exact_states_as_new_revisions() {
    let mut document = ethanol();
    let first = document.clone();
    document
        .apply(&[
            Command::UpdateBond {
                bond: bond(2),
                order: BondOrder::Double,
            },
            Command::Rename {
                title: "Acetaldehyde-like".into(),
            },
        ])
        .unwrap();
    let second = document.clone();

    document.undo().unwrap();
    assert_eq!(
        document.revision(),
        3,
        "undo is a new revision, never a rewind"
    );
    assert_eq!(document.to_parts().atoms, first.to_parts().atoms);
    assert_eq!(document.to_parts().bonds, first.to_parts().bonds);
    assert_eq!(document.title(), first.title());
    assert_eq!((document.undo_depth(), document.redo_depth()), (1, 1));

    document.redo().unwrap();
    assert_eq!(document.revision(), 4);
    assert_eq!(document.to_parts().bonds, second.to_parts().bonds);
    assert_eq!(document.title(), second.title());

    document.undo().unwrap();
    document.undo().unwrap();
    assert_eq!(document.atoms().len(), 0);
    assert_eq!(document.undo(), Err(HistoryError::NothingToUndo));

    document.redo().unwrap();
    document.apply(&[add("N", 0.0, 0.0)]).unwrap();
    assert_eq!(
        document.redo_depth(),
        0,
        "a new edit discards the redo branch"
    );
    assert_eq!(document.redo(), Err(HistoryError::NothingToRedo));
    assert_eq!(
        document.atoms().last().unwrap().0.to_string(),
        "a4",
        "identifiers are not reused after undo"
    );
}

#[test]
fn history_keeps_the_most_recent_entries() {
    let mut document = empty();
    for index in 0..HISTORY_LIMIT + 5 {
        let x = f64::from(u32::try_from(index).unwrap());
        document.apply(&[add("C", x, 0.0)]).unwrap();
    }
    assert_eq!(document.undo_depth(), HISTORY_LIMIT);
    while document.undo().is_ok() {}
    assert_eq!(document.atoms().len(), 5);
}

#[test]
fn restore_round_trips_and_rejects_impossible_states() {
    let mut document = ethanol();
    document
        .apply(&[Command::DeleteBonds {
            bonds: vec![bond(2)],
        }])
        .unwrap();
    document.undo().unwrap();
    assert_eq!(Document::restore(document.to_parts()).unwrap(), document);

    let mut dangling = document.to_parts();
    dangling.atoms.retain(|(id, _)| *id != atom(3));
    assert!(Document::restore(dangling).is_err());

    let mut reused = document.to_parts();
    reused.next_atom = 3;
    assert!(Document::restore(reused).is_err());

    let mut duplicate = document.to_parts();
    let first = duplicate.atoms[0].clone();
    duplicate.atoms.push(first);
    assert!(Document::restore(duplicate).is_err());

    let mut mismatched_history = document.to_parts();
    mismatched_history.undo[0].atoms[0].1.after = None;
    assert!(Document::restore(mismatched_history).is_err());

    let mut mismatched_redo = document.to_parts();
    mismatched_redo.redo[0].bonds[0].1.before = mismatched_redo.redo[0].bonds[0].1.after.clone();
    assert!(Document::restore(mismatched_redo).is_err());
}

#[test]
fn restore_trims_history_beyond_what_editing_keeps() {
    let mut document = empty();
    for index in 0..HISTORY_LIMIT {
        document
            .apply(&[Command::Rename {
                title: format!("Title {index}"),
            }])
            .unwrap();
    }
    // One redo entry more than editing can produce: the oldest undo entry is dropped, so a
    // later redo cannot grow undo past the limit.
    let mut parts = document.to_parts();
    let oldest = parts.undo[0].clone();
    parts.redo.push(ChangeSet {
        title: Some(Change {
            before: parts.title.clone(),
            after: "One more".into(),
        }),
        ..ChangeSet::default()
    });
    let mut restored = Document::restore(parts).unwrap();
    assert_eq!(
        (restored.undo_depth(), restored.redo_depth()),
        (HISTORY_LIMIT - 1, 1)
    );
    assert_ne!(restored.to_parts().undo[0], oldest);
    restored.redo().unwrap();
    assert_eq!(restored.title(), "One more");
    assert!(Document::restore(restored.to_parts()).is_ok());
}

#[test]
fn revisions_stop_at_the_largest_exact_client_number() {
    let mut beyond = ethanol().to_parts();
    beyond.revision = MAX_REVISION + 1;
    assert!(Document::restore(beyond).is_err());

    let mut parts = ethanol().to_parts();
    parts.revision = MAX_EDIT_REVISION - 1;
    let mut document = Document::restore(parts).unwrap();
    document.apply(&[add("N", 90.0, 0.0)]).unwrap();
    assert_eq!(document.revision(), MAX_EDIT_REVISION);

    let before = document.clone();
    let error = document.apply(&[add("N", 120.0, 0.0)]).unwrap_err();
    assert_eq!(error.error, EditError::RevisionLimit);
    assert_eq!(document.undo(), Err(HistoryError::RevisionLimit));
    assert_eq!(document.redo(), Err(HistoryError::RevisionLimit));
    assert_eq!(document, before, "nothing changed");
    assert_eq!(
        document.apply(&[move_atoms(vec![atom(1)], 0.0, 0.0)]),
        Ok(Outcome::Unchanged),
        "a no-op still succeeds"
    );
}

#[test]
fn continuing_after_discarded_edits_keeps_a_fresh_revision_or_refuses() {
    let stored = ethanol();
    let mut discarded = stored.clone();
    let mut parts = discarded.to_parts();
    parts.revision = MAX_EDIT_REVISION;
    discarded = Document::restore(parts).unwrap();

    // Edits stop below the last revision, so a copy reopened after discarding them has one.
    let mut reopened = stored.clone();
    reopened.continue_after(discarded.numbering()).unwrap();
    assert_eq!(reopened.revision(), MAX_REVISION);
    assert_eq!(reopened.atoms().len(), stored.atoms().len());

    // Past that, nothing is renumbered rather than a used revision named again.
    let mut again = stored.clone();
    assert_eq!(
        again.continue_after(reopened.numbering()),
        Err(RevisionsExhausted)
    );
    assert_eq!(again, stored);
}

#[test]
fn restore_rejects_counters_that_would_allocate_serial_zero() {
    for (next_atom, next_bond) in [(0, 1), (1, 0)] {
        let mut parts = empty().to_parts();
        parts.next_atom = next_atom;
        parts.next_bond = next_bond;
        assert!(
            Document::restore(parts).is_err(),
            "next_atom {next_atom}, next_bond {next_bond}"
        );
    }
}

#[test]
fn restore_rejects_change_sets_that_only_replay_one_way() {
    let mut document = empty();
    document.apply(&[add("C", 0.0, 0.0)]).unwrap();
    let carbon = document.atoms().next().unwrap().1.clone();
    let oxygen = Atom {
        kind: AtomKind::Element(element("O")),
        ..carbon.clone()
    };

    // a1 listed twice as C → O → absent walks backward cleanly but cannot be redone.
    let mut chained = document.to_parts();
    chained.undo[0].atoms = vec![
        (
            atom(1),
            Change {
                before: Some(oxygen.clone()),
                after: Some(carbon.clone()),
            },
        ),
        (
            atom(1),
            Change {
                before: None,
                after: Some(oxygen),
            },
        ),
    ];
    assert!(Document::restore(chained).is_err());

    let mut no_op = document.to_parts();
    no_op.undo[0].atoms.push((
        atom(2),
        Change {
            before: None,
            after: None,
        },
    ));
    assert!(Document::restore(no_op).is_err());
}

#[test]
fn a_transaction_can_bond_atoms_it_creates() {
    let mut document = ethanol();
    document
        .apply(&[
            add("N", 100.0, 0.0),
            Command::AddAtom {
                atom: Atom::from(element("O")),
                positions: here(126.0, 15.0),
                bond_to: Some(BondTo {
                    atom: AtomRef::Created(0),
                    order: BondOrder::Double,
                }),
            },
            Command::AddBond {
                begin: AtomRef::Id(atom(3)),
                end: AtomRef::Created(0),
                order: BondOrder::Single,
            },
        ])
        .unwrap();
    let bonds: Vec<_> = document
        .bonds()
        .skip(2)
        .map(|(_, bond)| {
            (
                bond.begin.to_string(),
                bond.end.to_string(),
                bond.order.clone(),
            )
        })
        .collect();
    assert_eq!(
        bonds,
        [
            ("a4".into(), "a5".into(), BondOrder::Double),
            ("a3".into(), "a4".into(), BondOrder::Single)
        ]
    );

    for reference in [0, 1, 7] {
        // Command 0 adds no atom, command 1 is the current command, 7 is out of range.
        let error = command_error(
            &mut document,
            &[
                Command::Rename { title: "x".into() },
                Command::AddBond {
                    begin: AtomRef::Id(atom(1)),
                    end: AtomRef::Created(reference),
                    order: BondOrder::Single,
                },
            ],
        );
        assert_eq!(error.error, EditError::UnresolvedReference(reference));
    }
}

#[test]
fn identifiers_have_one_canonical_form() {
    assert_eq!(AtomId::parse("a12"), AtomId::from_serial(12));
    for value in ["a0", "a01", "b1", "a", "a-1", "a1x", "A1"] {
        assert_eq!(AtomId::parse(value), None, "{value}");
    }
    assert!(DocumentId::parse("d-0123abc").is_some());
    for value in ["", "-d", "D1", "../x", "a/b", &"d".repeat(65)] {
        assert!(DocumentId::parse(value).is_none(), "{value}");
    }
}

#[test]
fn atom_limit_is_enforced() {
    let mut document = empty();
    let batch: Vec<_> = (0..MAX_COMMANDS).map(|_| add("C", 0.0, 0.0)).collect();
    for _ in 0..MAX_ATOMS / MAX_COMMANDS {
        document.apply(&batch).unwrap();
    }
    assert_eq!(
        command_error(&mut document, &[add("C", 0.0, 0.0)]).error,
        EditError::AtomLimit
    );
}

#[test]
fn a_copy_is_independent_and_keeps_identifiers_but_not_history() {
    let mut source = ethanol();
    source.undo().unwrap();
    source.redo().unwrap();
    let id = DocumentId::parse("doc-copy").unwrap();
    let mut copy = source.copy_as(id.clone(), " Copy ").unwrap();
    assert_eq!(copy.id(), &id);
    assert_eq!(copy.title(), "Copy");
    assert_eq!(copy.revision(), 0);
    assert_eq!((copy.undo_depth(), copy.redo_depth()), (0, 0));
    assert!(copy.atoms().eq(source.atoms()));
    assert!(copy.bonds().eq(source.bonds()));
    assert_eq!(copy.numbering().next_atom, source.numbering().next_atom);

    let before = source.clone();
    copy.apply(&[add("N", 90.0, 0.0)]).unwrap();
    assert_eq!(
        copy.atoms().last().unwrap().0,
        AtomId::from_serial(4).unwrap()
    );
    assert_eq!(source, before, "editing the copy leaves the source alone");
    assert_eq!(
        source.copy_as(DocumentId::parse("doc-x").unwrap(), "\u{7}"),
        Err(EditError::InvalidTitle)
    );
}

fn add_instance(offset_x: f64, offset_y: f64) -> Command {
    Command::AddInstance {
        source: None,
        offset: Vector {
            dx: offset_x,
            dy: offset_y,
        },
    }
}

fn move_instance(serial: u32, dx: f64, dy: f64) -> Command {
    Command::MoveInstance {
        instance: instance(serial),
        offset: Vector { dx, dy },
    }
}

/// Ethanol drawn twice: `s2` is `s1` moved 100 pt down.
fn ethanol_twice() -> Document {
    let mut document = ethanol();
    document.apply(&[add_instance(0.0, 100.0)]).unwrap();
    document
}

fn graph(document: &Document) -> (Vec<(AtomId, Atom)>, Vec<String>) {
    let parts = document.to_parts();
    (
        parts.atoms,
        parts
            .bonds
            .iter()
            .map(|(id, bond)| format!("{id}:{}-{}:{:?}", bond.begin, bond.end, bond.order))
            .collect(),
    )
}

#[test]
fn a_new_document_has_one_empty_scene_instance() {
    let document = empty();
    assert_eq!(document.instances().collect::<Vec<_>>(), [instance(1)]);
    assert_eq!(document.first_instance(), instance(1));
    assert_eq!(positions(&document, instance(1)), []);
    assert_eq!(document.numbering().next_object, 2);
}

#[test]
fn an_added_instance_copies_the_source_positions_with_the_offset() {
    let document = ethanol_twice();
    assert_eq!(
        document.instances().collect::<Vec<_>>(),
        [instance(1), instance(2)]
    );
    assert_eq!(
        positions(&document, instance(2)),
        [
            ("a1".to_owned(), 0.0, 100.0),
            ("a2".to_owned(), 26.0, 115.0),
            ("a3".to_owned(), 52.0, 100.0)
        ]
    );
    assert_eq!(document.revision(), 2, "one revision for the new instance");

    // A named source is copied instead of the first instance.
    let mut document = document;
    document
        .apply(&[Command::AddInstance {
            source: Some(instance(2)),
            offset: Vector { dx: 10.0, dy: 0.0 },
        }])
        .unwrap();
    assert_eq!(
        positions(&document, instance(3))[0],
        ("a1".to_owned(), 10.0, 100.0)
    );
}

#[test]
fn instances_move_independently_keeping_the_graph_and_the_other_instances() {
    let mut document = ethanol_twice();
    let graph_before = graph(&document);
    let first_before = positions(&document, instance(1));

    document.apply(&[move_instance(2, 50.0, -20.0)]).unwrap();
    assert_eq!(
        positions(&document, instance(2)),
        [
            ("a1".to_owned(), 50.0, 80.0),
            ("a2".to_owned(), 76.0, 95.0),
            ("a3".to_owned(), 102.0, 80.0)
        ],
        "every position of the instance moves exactly once"
    );
    assert_eq!(positions(&document, instance(1)), first_before);
    assert_eq!(graph(&document), graph_before, "chemistry is untouched");

    // Atom moves stay inside the named instance.
    document
        .apply(&[Command::MoveAtoms {
            atoms: vec![atom(1)],
            delta: Vector { dx: 1.0, dy: 1.0 },
            instance: Some(instance(1)),
        }])
        .unwrap();
    assert_eq!(
        document.position(instance(1), atom(1)),
        Some(point(1.0, 1.0))
    );
    assert_eq!(
        document.position(instance(2), atom(1)),
        Some(point(50.0, 80.0))
    );

    // Each scene change is one revision and one undo entry, undone as a whole.
    assert_eq!(document.revision(), 4);
    document.undo().unwrap();
    document.undo().unwrap();
    assert_eq!(positions(&document, instance(1)), first_before);
    assert_eq!(
        positions(&document, instance(2))[0],
        ("a1".to_owned(), 0.0, 100.0)
    );
    document.undo().unwrap();
    assert_eq!(document.instances().collect::<Vec<_>>(), [instance(1)]);
    assert_eq!(graph(&document), graph_before);
    document.redo().unwrap();
    assert_eq!(
        positions(&document, instance(2))[1],
        ("a2".to_owned(), 26.0, 115.0)
    );
}

#[test]
fn a_new_atom_needs_a_position_in_every_instance_once_there_are_several() {
    let mut document = ethanol_twice();
    let per_instance = |entries: &[(u32, f64, f64)]| Command::AddAtom {
        atom: Atom::from(element("N")),
        positions: NewAtomPositions::PerInstance(
            entries
                .iter()
                .map(|&(serial, x, y)| (instance(serial), point(x, y)))
                .collect::<BTreeMap<_, _>>(),
        ),
        bond_to: None,
    };
    let cases = [
        (
            add("N", 0.0, 50.0),
            EditError::PositionsRequired { instances: 2 },
        ),
        (
            per_instance(&[(1, 0.0, 50.0)]),
            EditError::MissingPosition(instance(2)),
        ),
        (
            per_instance(&[(1, 0.0, 50.0), (2, 0.0, 150.0), (9, 0.0, 0.0)]),
            EditError::UnknownObject(instance(9)),
        ),
    ];
    for (command, expected) in cases {
        let error = command_error(
            &mut document,
            &[Command::Rename { title: "x".into() }, command],
        );
        assert_eq!(
            error,
            TransactionError {
                index: Some(1),
                error: expected
            }
        );
    }

    document
        .apply(&[per_instance(&[(1, 0.0, 50.0), (2, 70.0, 150.0)])])
        .unwrap();
    assert_eq!(
        document.position(instance(1), atom(4)),
        Some(point(0.0, 50.0))
    );
    assert_eq!(
        document.position(instance(2), atom(4)),
        Some(point(70.0, 150.0))
    );
    assert_eq!(
        document
            .atoms()
            .last()
            .unwrap()
            .1
            .kind
            .element()
            .unwrap()
            .symbol(),
        "N",
        "the atom is created once, in the graph"
    );
}

#[test]
fn moving_atoms_names_the_instance_once_there_are_several() {
    let mut document = ethanol_twice();
    assert_eq!(
        command_error(&mut document, &[move_atoms(vec![atom(1)], 5.0, 0.0)]),
        TransactionError {
            index: Some(0),
            error: EditError::InstanceRequired { instances: 2 }
        }
    );
    let unknown = Command::MoveAtoms {
        atoms: vec![atom(1)],
        delta: Vector { dx: 5.0, dy: 0.0 },
        instance: Some(instance(7)),
    };
    assert_eq!(
        command_error(&mut document, &[unknown]).error,
        EditError::UnknownObject(instance(7))
    );
    assert_eq!(
        command_error(&mut document, &[move_instance(7, 1.0, 1.0)]).error,
        EditError::UnknownObject(instance(7))
    );
}

#[test]
fn deleting_atoms_removes_their_positions_everywhere_and_undo_restores_them() {
    let mut document = ethanol_twice();
    let before = document.clone();
    document
        .apply(&[Command::DeleteAtoms {
            atoms: vec![atom(2)],
        }])
        .unwrap();
    for serial in [1, 2] {
        assert_eq!(
            positions(&document, instance(serial))
                .iter()
                .map(|(id, _, _)| id.as_str())
                .collect::<Vec<_>>(),
            ["a1", "a3"],
            "s{serial}"
        );
    }
    assert_eq!(document.undo_depth(), before.undo_depth() + 1);
    document.undo().unwrap();
    assert_eq!(document.to_parts().atoms, before.to_parts().atoms);
    assert_eq!(document.to_parts().bonds, before.to_parts().bonds);
    assert_eq!(document.to_parts().positions, before.to_parts().positions);
}

#[test]
fn the_last_instance_stays_and_instance_identifiers_are_never_reused() {
    let mut document = ethanol_twice();
    let delete = |serial| Command::DeleteInstance {
        instance: instance(serial),
    };
    document.apply(&[delete(1)]).unwrap();
    assert_eq!(document.instances().collect::<Vec<_>>(), [instance(2)]);
    assert_eq!(document.first_instance(), instance(2));
    assert_eq!(
        command_error(&mut document, &[delete(2)]),
        TransactionError {
            index: Some(0),
            error: EditError::LastInstance
        }
    );
    // With one instance left, a single position and an unnamed move are enough again.
    document
        .apply(&[add("S", 9.0, 9.0), move_atoms(vec![atom(1)], 1.0, 0.0)])
        .unwrap();
    assert_eq!(
        document.position(instance(2), atom(4)),
        Some(point(9.0, 9.0))
    );

    document.undo().unwrap();
    document.undo().unwrap();
    assert_eq!(
        document.instances().collect::<Vec<_>>(),
        [instance(1), instance(2)],
        "undo brings the deleted instance back in its place"
    );
    document.apply(&[add_instance(0.0, 0.0)]).unwrap();
    assert_eq!(
        document.instances().last(),
        Some(instance(3)),
        "s3, not a reused identifier"
    );
}

#[test]
fn scene_changes_that_change_nothing_add_no_revision() {
    let mut document = ethanol_twice();
    let before = document.clone();
    let outcome = document
        .apply(&[
            move_instance(2, 0.0, 0.0),
            add_instance(5.0, 5.0),
            Command::DeleteInstance {
                instance: instance(3),
            },
        ])
        .unwrap();
    assert_eq!(outcome, Outcome::Unchanged);
    assert_eq!(document, before);
    document.apply(&[add_instance(1.0, 1.0)]).unwrap();
    assert_eq!(document.instances().last(), Some(instance(3)));
}

#[test]
fn instance_offsets_and_count_are_bounded() {
    let mut document = ethanol();
    assert_eq!(
        command_error(&mut document, &[add_instance(100_000.0, 0.0)]).error,
        EditError::CoordinateOutOfRange
    );
    let more: Vec<_> = (1..MAX_INSTANCES).map(|_| add_instance(1.0, 1.0)).collect();
    document.apply(&more).unwrap();
    assert_eq!(document.instances().count(), MAX_INSTANCES);
    assert_eq!(
        command_error(&mut document, &[add_instance(1.0, 1.0)]).error,
        EditError::InstanceLimit
    );
}

#[test]
fn restore_rejects_scenes_commands_could_not_produce() {
    let mut document = ethanol_twice();
    document.apply(&[move_instance(2, 3.0, 0.0)]).unwrap();
    assert_eq!(Document::restore(document.to_parts()).unwrap(), document);

    let mut missing = document.to_parts();
    missing.positions.pop();
    assert!(
        Document::restore(missing).is_err(),
        "an atom without a position in s2"
    );

    let mut stray = document.to_parts();
    stray.positions.push((
        PositionKey {
            instance: instance(5),
            atom: atom(1),
        },
        point(0.0, 0.0),
    ));
    assert!(
        Document::restore(stray).is_err(),
        "a position in no instance"
    );

    let mut none = document.to_parts();
    none.roots.clear();
    none.objects.clear();
    none.positions.clear();
    assert!(Document::restore(none).is_err(), "no instance at all");

    let mut reused = document.to_parts();
    reused.next_object = 2;
    assert!(
        Document::restore(reused).is_err(),
        "s2 would be handed out again"
    );

    let mut zero = document.to_parts();
    zero.next_object = 0;
    assert!(Document::restore(zero).is_err());

    let mut mismatched = document.to_parts();
    let last = mismatched.undo.len() - 1;
    mismatched.undo[last].positions[0].1.after = Some(point(-1.0, -1.0));
    assert!(Document::restore(mismatched).is_err());
}
