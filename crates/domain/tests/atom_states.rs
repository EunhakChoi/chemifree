//! Hydrogen states, radicals, query atoms and bonds, aromatic and dative bonds, and attachment
//! points: recorded through commands, undone and redone, and refused when they do not hold
//! together. Nothing here is computed.

use chemifree_domain::document::{
    Atom, AtomId, AtomKind, AtomQuery, AtomRef, AtomUpdate, AttachmentPoints, BondId, BondOrder,
    BondQuery, BondTo, Command, Document, DocumentId, EditError, Hydrogens, NewAtomPositions,
    Point, QueryBondOrder, Radical, TransactionError,
};
use chemifree_domain::element::Element;

fn element(symbol: &str) -> Element {
    Element::from_symbol(symbol).unwrap()
}

fn atom(serial: u32) -> AtomId {
    AtomId::from_serial(serial).unwrap()
}

fn bond(serial: u32) -> BondId {
    BondId::from_serial(serial).unwrap()
}

fn here(x: f64) -> NewAtomPositions {
    NewAtomPositions::Single(Point::new(x, 0.0).unwrap())
}

fn add(new: Atom, x: f64) -> Command {
    Command::AddAtom {
        atom: new,
        positions: here(x),
        bond_to: None,
    }
}

fn update(serial: u32, update: AtomUpdate) -> Command {
    Command::UpdateAtom {
        atom: atom(serial),
        update,
    }
}

fn points(numbers: &[u64]) -> AttachmentPoints {
    AttachmentPoints::try_new(numbers).unwrap()
}

/// C1–N2 with a single bond b1.
fn drawn() -> Document {
    let mut document = Document::new(DocumentId::parse("doc-1").unwrap(), "States").unwrap();
    document
        .apply(&[
            add(Atom::from(Element::CARBON), 0.0),
            Command::AddAtom {
                atom: Atom::from(element("N")),
                positions: here(30.0),
                bond_to: Some(BondTo {
                    atom: AtomRef::Created(0),
                    order: BondOrder::Single,
                }),
            },
        ])
        .unwrap();
    document
}

fn atom_state(document: &Document, serial: u32) -> Atom {
    document
        .atoms()
        .find(|(id, _)| *id == atom(serial))
        .unwrap()
        .1
        .clone()
}

fn order(document: &Document, serial: u32) -> BondOrder {
    document
        .bonds()
        .find(|(id, _)| *id == bond(serial))
        .unwrap()
        .1
        .order
        .clone()
}

/// Applies `commands`, which must be refused, and checks that nothing changed.
fn refused(document: &mut Document, commands: &[Command]) -> TransactionError {
    let before = document.clone();
    let error = document.apply(commands).unwrap_err();
    assert_eq!(*document, before, "a refused transaction changes nothing");
    error
}

#[test]
fn a_new_atom_starts_with_inferred_hydrogens_no_radical_and_no_attachment_points() {
    let document = drawn();
    let carbon = atom_state(&document, 1);
    assert_eq!(carbon.kind, AtomKind::Element(Element::CARBON));
    assert_eq!(carbon.element(), Some(Element::CARBON));
    assert_eq!(carbon.hydrogens, Hydrogens::Inferred);
    assert_eq!(carbon.radical, Radical::None);
    assert!(carbon.attachment_points.is_empty());
    assert_eq!((carbon.charge, carbon.isotope), (0, None));
}

#[test]
fn every_atom_state_is_recorded_and_undone_and_redone_as_one_step() {
    let mut document = drawn();
    let before = document.clone();
    document
        .apply(&[
            update(
                1,
                AtomUpdate {
                    hydrogens: Some(Hydrogens::Specified(3)),
                    radical: Some(Radical::Doublet),
                    attachment_points: Some(points(&[2, 1])),
                    ..AtomUpdate::default()
                },
            ),
            update(
                2,
                AtomUpdate {
                    kind: Some(AtomKind::Query(AtomQuery::List {
                        elements: vec![element("O"), element("N")],
                        negated: true,
                    })),
                    hydrogens: Some(Hydrogens::Unknown),
                    ..AtomUpdate::default()
                },
            ),
        ])
        .unwrap();
    let edited = document.clone();
    let carbon = atom_state(&document, 1);
    assert_eq!(carbon.hydrogens, Hydrogens::Specified(3));
    assert_eq!(carbon.radical, Radical::Doublet);
    assert_eq!(
        carbon.attachment_points.iter().collect::<Vec<_>>(),
        [1, 2],
        "attachment points are a set read in increasing order"
    );
    let query = atom_state(&document, 2);
    assert_eq!(query.element(), None);
    assert_eq!(
        query.kind,
        AtomKind::Query(AtomQuery::List {
            elements: vec![element("O"), element("N")],
            negated: true,
        }),
        "the element list keeps the order given"
    );
    assert_eq!(query.hydrogens, Hydrogens::Unknown);

    document.undo().unwrap();
    assert_eq!(
        (document.atoms().collect::<Vec<_>>(), document.revision()),
        (before.atoms().collect::<Vec<_>>(), 3),
        "undo restores every field"
    );
    document.redo().unwrap();
    assert_eq!(
        document.atoms().collect::<Vec<_>>(),
        edited.atoms().collect::<Vec<_>>(),
        "redo reapplies every field"
    );
}

#[test]
fn a_new_atom_can_be_added_in_any_recorded_state() {
    let mut document = drawn();
    let query = Atom {
        hydrogens: Hydrogens::Specified(0),
        radical: Radical::Unknown,
        attachment_points: points(&[4]),
        ..Atom::from(AtomKind::Query(AtomQuery::Any))
    };
    document.apply(&[add(query.clone(), 60.0)]).unwrap();
    assert_eq!(atom_state(&document, 3), query);
    assert_eq!(AtomQuery::Any.label(), "*");
}

#[test]
fn bond_kinds_are_recorded_as_drawn_and_undone() {
    let mut document = drawn();
    let kinds = [
        BondOrder::Aromatic,
        BondOrder::Dative,
        BondOrder::Query(BondQuery::Any),
        BondOrder::Query(BondQuery::List(vec![
            QueryBondOrder::Aromatic,
            QueryBondOrder::Single,
        ])),
        BondOrder::Unknown,
    ];
    for kind in &kinds {
        document
            .apply(&[Command::UpdateBond {
                bond: bond(1),
                order: kind.clone(),
            }])
            .unwrap();
        assert_eq!(order(&document, 1), *kind);
    }
    for kind in kinds.iter().rev().skip(1) {
        document.undo().unwrap();
        assert_eq!(
            order(&document, 1),
            *kind,
            "undo walks back one kind at a time"
        );
    }
    document.undo().unwrap();
    assert_eq!(order(&document, 1), BondOrder::Single);
}

#[test]
fn a_dative_bond_to_a_new_atom_begins_at_the_donor_it_names() {
    let mut document = drawn();
    document
        .apply(&[Command::AddAtom {
            atom: Atom::from(element("B")),
            positions: here(60.0),
            bond_to: Some(BondTo {
                atom: AtomRef::Id(atom(2)),
                order: BondOrder::Dative,
            }),
        }])
        .unwrap();
    let (_, dative) = document.bonds().last().unwrap();
    assert_eq!(
        (dative.begin, dative.end, dative.order.clone()),
        (atom(2), atom(3), BondOrder::Dative)
    );
}

#[test]
fn atom_states_that_do_not_hold_together_are_refused_with_the_failing_command() {
    let query = |query: AtomQuery| AtomUpdate {
        kind: Some(AtomKind::Query(query)),
        ..AtomUpdate::default()
    };
    let list = |symbols: &[&str]| AtomQuery::List {
        elements: symbols.iter().map(|symbol| element(symbol)).collect(),
        negated: false,
    };
    let eleven = ["H", "B", "C", "N", "O", "F", "P", "S", "Cl", "Br", "I"];
    let cases: Vec<(Command, EditError)> = vec![
        (
            update(
                1,
                AtomUpdate {
                    hydrogens: Some(Hydrogens::Specified(17)),
                    ..AtomUpdate::default()
                },
            ),
            EditError::HydrogenCountOutOfRange(17),
        ),
        (
            add(
                Atom {
                    hydrogens: Hydrogens::Specified(17),
                    ..Atom::from(Element::CARBON)
                },
                60.0,
            ),
            EditError::HydrogenCountOutOfRange(17),
        ),
        (
            update(
                1,
                AtomUpdate {
                    charge: Some(1),
                    ..query(AtomQuery::Halogen)
                },
            ),
            EditError::QueryAtomCharge,
        ),
        (
            update(
                1,
                AtomUpdate {
                    isotope: Some(Some(13)),
                    ..query(AtomQuery::AnyHeavy)
                },
            ),
            EditError::QueryAtomIsotope,
        ),
        (update(1, query(list(&[]))), EditError::EmptyQueryList),
        (
            update(1, query(list(&["N", "O", "N"]))),
            EditError::RepeatedQueryEntry,
        ),
        (update(1, query(list(&eleven))), EditError::QueryListTooLong),
        (
            Command::UpdateBond {
                bond: bond(1),
                order: BondOrder::Query(BondQuery::List(Vec::new())),
            },
            EditError::EmptyQueryList,
        ),
        (
            Command::AddBond {
                begin: AtomRef::Id(atom(1)),
                end: AtomRef::Created(0),
                order: BondOrder::Query(BondQuery::List(vec![
                    QueryBondOrder::Double,
                    QueryBondOrder::Double,
                ])),
            },
            EditError::RepeatedQueryEntry,
        ),
    ];
    for (command, expected) in cases {
        let mut document = drawn();
        // A valid command first, so the failing one is not at index 0.
        let commands = [add(Atom::from(element("S")), 90.0), command.clone()];
        assert_eq!(
            refused(&mut document, &commands),
            TransactionError {
                index: Some(1),
                error: expected,
            },
            "{command:?}"
        );
    }
}

#[test]
fn a_charged_or_isotopic_atom_becomes_a_query_only_once_those_are_cleared() {
    let mut document = drawn();
    document
        .apply(&[update(
            1,
            AtomUpdate {
                charge: Some(-1),
                isotope: Some(Some(13)),
                ..AtomUpdate::default()
            },
        )])
        .unwrap();
    let to_query = AtomUpdate {
        kind: Some(AtomKind::Query(AtomQuery::Heteroatom)),
        ..AtomUpdate::default()
    };
    assert_eq!(
        refused(&mut document, &[update(1, to_query.clone())]).error,
        EditError::QueryAtomCharge
    );
    document
        .apply(&[update(
            1,
            AtomUpdate {
                charge: Some(0),
                isotope: Some(None),
                ..to_query
            },
        )])
        .unwrap();
    assert_eq!(atom_state(&document, 1).element(), None);
    assert_eq!(
        refused(
            &mut document,
            &[update(
                1,
                AtomUpdate {
                    isotope: Some(Some(2)),
                    ..AtomUpdate::default()
                },
            )]
        )
        .error,
        EditError::QueryAtomIsotope,
        "a query atom stays without an isotope"
    );
}

#[test]
fn the_limits_themselves_are_accepted() {
    let mut document = drawn();
    let ten = ["H", "B", "C", "N", "O", "F", "P", "S", "Cl", "Br"];
    document
        .apply(&[update(
            1,
            AtomUpdate {
                kind: Some(AtomKind::Query(AtomQuery::List {
                    elements: ten.iter().map(|symbol| element(symbol)).collect(),
                    negated: false,
                })),
                hydrogens: Some(Hydrogens::Specified(16)),
                attachment_points: Some(points(&[4, 3, 2, 1])),
                ..AtomUpdate::default()
            },
        )])
        .unwrap();
    let state = atom_state(&document, 1);
    assert_eq!(state.hydrogens, Hydrogens::Specified(16));
    assert_eq!(
        state.attachment_points.iter().collect::<Vec<_>>(),
        [1, 2, 3, 4]
    );
}

#[test]
fn attachment_points_outside_one_to_four_or_repeated_are_refused() {
    assert_eq!(
        AttachmentPoints::try_new(&[0]),
        Err(EditError::AttachmentPointOutOfRange(0))
    );
    assert_eq!(
        AttachmentPoints::try_new(&[5]),
        Err(EditError::AttachmentPointOutOfRange(5))
    );
    assert_eq!(
        AttachmentPoints::try_new(&[u64::MAX]),
        Err(EditError::AttachmentPointOutOfRange(u64::MAX))
    );
    assert_eq!(
        AttachmentPoints::try_new(&[3, 1, 3]),
        Err(EditError::RepeatedAttachmentPoint(3))
    );
    assert!(AttachmentPoints::try_new(&[]).unwrap().is_empty());
}

#[test]
fn query_labels_are_never_element_symbols() {
    let labels = [
        (AtomQuery::Any, "*"),
        (AtomQuery::AnyHeavy, "A"),
        (AtomQuery::Heteroatom, "Q"),
        (AtomQuery::Halogen, "X"),
        (
            AtomQuery::List {
                elements: vec![element("N"), element("O")],
                negated: false,
            },
            "[N,O]",
        ),
        (
            AtomQuery::List {
                elements: vec![element("Cl")],
                negated: true,
            },
            "![Cl]",
        ),
    ];
    for (query, label) in labels {
        assert_eq!(query.label(), label);
        assert_eq!(
            Element::from_symbol(label),
            None,
            "{label} is not an element"
        );
    }
}

#[test]
fn restore_rejects_states_commands_refuse() {
    let document = drawn();
    let charged_query = Atom {
        charge: 2,
        ..Atom::from(AtomKind::Query(AtomQuery::Any))
    };
    let mut parts = document.to_parts();
    parts.atoms[0].1 = charged_query;
    assert!(
        Document::restore(parts)
            .unwrap_err()
            .to_string()
            .contains("cannot carry a charge")
    );
    let mut parts = document.to_parts();
    parts.atoms[0].1.hydrogens = Hydrogens::Specified(200);
    assert!(Document::restore(parts).is_err());
    let mut parts = document.to_parts();
    parts.bonds[0].1.order = BondOrder::Query(BondQuery::List(Vec::new()));
    assert!(
        Document::restore(parts)
            .unwrap_err()
            .to_string()
            .contains("at least one entry")
    );
}
