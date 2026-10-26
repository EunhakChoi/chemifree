use chemifree_domain::document::{
    AtomId, AtomRef, BondId, BondOrder, BondTo, Command, Document, DocumentId, EditError,
    HISTORY_LIMIT, HistoryError, MAX_ATOMS, MAX_COMMANDS, Outcome, Point, TransactionError, Vector,
};
use chemifree_domain::element::Element;

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

fn empty() -> Document {
    Document::new(DocumentId::parse("doc-1").unwrap(), "  Ethanol sketch ").unwrap()
}

fn add(symbol: &str, x: f64, y: f64) -> Command {
    Command::AddAtom {
        element: element(symbol),
        position: point(x, y),
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
                element: Element::CARBON,
                position: point(26.0, 15.0),
                bond_to: Some(BondTo {
                    atom: AtomRef::Id(atom(1)),
                    order: BondOrder::Single,
                }),
            },
            Command::AddAtom {
                element: element("O"),
                position: point(52.0, 0.0),
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
        .map(|(id, atom)| (id.to_string(), atom.element.symbol()))
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
                element: Some(element("N")),
                charge: Some(1),
                isotope: Some(Some(15)),
            },
            Command::UpdateBond {
                bond: bond(1),
                order: BondOrder::Double,
            },
            Command::MoveAtoms {
                atoms: vec![atom(1), atom(1), atom(2)],
                delta: Vector { dx: 10.0, dy: -5.0 },
            },
            Command::Rename {
                title: "Edited".into(),
            },
        ])
        .unwrap();
    let atoms: Vec<_> = document.atoms().map(|(_, atom)| atom.clone()).collect();
    assert_eq!(
        atoms[0].position,
        point(10.0, -5.0),
        "duplicate targets move once"
    );
    assert_eq!(atoms[1].position, point(36.0, 10.0));
    assert_eq!(atoms[2].position, point(52.0, 0.0));
    assert_eq!(
        (atoms[2].element.symbol(), atoms[2].charge, atoms[2].isotope),
        ("N", 1, Some(15))
    );
    assert_eq!(document.bonds().next().unwrap().1.order, BondOrder::Double);
    assert_eq!(document.title(), "Edited");

    document
        .apply(&[Command::UpdateAtom {
            atom: atom(3),
            element: None,
            charge: None,
            isotope: Some(None),
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
                element: None,
                charge: Some(9),
                isotope: None,
            },
            EditError::ChargeOutOfRange(9),
        ),
        (
            Command::UpdateAtom {
                atom: atom(1),
                element: None,
                charge: None,
                isotope: Some(Some(0)),
            },
            EditError::IsotopeOutOfRange(0),
        ),
        (
            Command::MoveAtoms {
                atoms: vec![atom(1)],
                delta: Vector {
                    dx: 200_000.0,
                    dy: 0.0,
                },
            },
            EditError::CoordinateOutOfRange,
        ),
        (
            Command::MoveAtoms {
                atoms: vec![atom(1)],
                delta: Vector {
                    dx: f64::NAN,
                    dy: 0.0,
                },
            },
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
            Command::MoveAtoms {
                atoms: vec![atom(1)],
                delta: Vector { dx: 0.0, dy: 0.0 },
            },
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
fn a_transaction_can_bond_atoms_it_creates() {
    let mut document = ethanol();
    document
        .apply(&[
            add("N", 100.0, 0.0),
            Command::AddAtom {
                element: element("O"),
                position: point(126.0, 15.0),
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
        .map(|(_, bond)| (bond.begin.to_string(), bond.end.to_string(), bond.order))
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
