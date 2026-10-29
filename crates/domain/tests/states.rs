//! Recorded hydrogen, radical, query, attachment-point, and bond-kind states: they survive
//! commands, undo, and redo; refused combinations name their code and change nothing; and
//! restored documents obey the same rules.

use chemifree_domain::document::{
    Atom, AtomEdit, AtomId, AtomRef, BondId, BondOrder, BondTo, Change, Command, Document,
    DocumentId, EditError, NewAtomPositions, Point, TransactionError,
};
use chemifree_domain::element::Element;
use chemifree_domain::states::{
    AtomKind, AttachmentPoints, BondQuery, Hydrogens, QueryAtom, QueryBondOrder, Radical,
    StateError,
};

fn element(symbol: &str) -> Element {
    Element::from_symbol(symbol).unwrap()
}

fn atom(serial: u32) -> AtomId {
    AtomId::from_serial(serial).unwrap()
}

fn bond(serial: u32) -> BondId {
    BondId::from_serial(serial).unwrap()
}

fn at(x: f64) -> NewAtomPositions {
    NewAtomPositions::Single(Point::new(x, 0.0).unwrap())
}

fn add(atom: Atom, x: f64) -> Command {
    Command::AddAtom {
        atom,
        positions: at(x),
        bond_to: None,
    }
}

fn update(serial: u32, edit: AtomEdit) -> Command {
    Command::UpdateAtom {
        atom: atom(serial),
        edit,
    }
}

fn list(symbols: &[&str], negated: bool) -> AtomKind {
    AtomKind::Query(QueryAtom::List {
        elements: symbols.iter().map(|symbol| element(symbol)).collect(),
        negated,
    })
}

fn points(values: &[u8]) -> AttachmentPoints {
    AttachmentPoints::new(values).unwrap()
}

/// Two carbons joined by a single bond, b1 from a1 to a2.
fn pair() -> Document {
    let mut document = Document::new(DocumentId::parse("doc-states").unwrap(), "States").unwrap();
    document
        .apply(&[
            add(Atom::from(Element::CARBON), 0.0),
            Command::AddAtom {
                atom: Atom::from(Element::CARBON),
                positions: at(30.0),
                bond_to: Some(BondTo {
                    atom: AtomRef::Id(atom(1)),
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
        .map(|(_, atom)| atom.clone())
        .expect("the atom exists")
}

fn bond_order(document: &Document, serial: u32) -> BondOrder {
    document
        .bonds()
        .find(|(id, _)| *id == bond(serial))
        .map(|(_, bond)| bond.order.clone())
        .expect("the bond exists")
}

#[test]
fn a_new_atom_has_inferred_hydrogens_no_radical_and_no_attachment_points() {
    let document = pair();
    let carbon = atom_state(&document, 1);
    assert_eq!(carbon.kind, AtomKind::Element(Element::CARBON));
    assert_eq!(carbon.hydrogens, Hydrogens::Inferred);
    assert_eq!(carbon.radical, Radical::None);
    assert!(carbon.attachment_points.is_empty());
    assert_eq!((carbon.charge, carbon.isotope), (0, None));
}

#[test]
fn atom_states_set_on_creation_survive_undo_and_redo() {
    let mut document = pair();
    let stated = Atom {
        hydrogens: Hydrogens::Specified(16),
        radical: Radical::Triplet,
        attachment_points: points(&[1, 4]),
        ..Atom::new(list(&["N", "O", "S"], true))
    };
    document.apply(&[add(stated.clone(), 60.0)]).unwrap();
    assert_eq!(atom_state(&document, 3), stated);

    document.undo().unwrap();
    assert_eq!(document.atoms().len(), 2);
    document.redo().unwrap();
    assert_eq!(atom_state(&document, 3), stated);
}

#[test]
fn every_atom_state_can_be_updated_and_undone() {
    let mut document = pair();
    let before = atom_state(&document, 1);
    let cases = [
        AtomEdit {
            hydrogens: Some(Hydrogens::Specified(0)),
            ..AtomEdit::default()
        },
        AtomEdit {
            hydrogens: Some(Hydrogens::Unknown),
            ..AtomEdit::default()
        },
        AtomEdit {
            radical: Some(Radical::Doublet),
            ..AtomEdit::default()
        },
        AtomEdit {
            radical: Some(Radical::Unknown),
            ..AtomEdit::default()
        },
        AtomEdit {
            attachment_points: Some(points(&[2, 3])),
            ..AtomEdit::default()
        },
        AtomEdit {
            kind: Some(AtomKind::Query(QueryAtom::Halogen)),
            ..AtomEdit::default()
        },
    ];
    for edit in cases {
        document.apply(&[update(1, edit.clone())]).unwrap();
        let after = atom_state(&document, 1);
        assert_ne!(after, before, "{edit:?} changed the atom");
        document.undo().unwrap();
        assert_eq!(atom_state(&document, 1), before, "{edit:?} undone");
        document.redo().unwrap();
        assert_eq!(atom_state(&document, 1), after, "{edit:?} redone");
        document.undo().unwrap();
    }
}

#[test]
fn an_element_atom_becomes_a_query_and_back_with_undo() {
    let mut document = pair();
    document
        .apply(&[update(
            1,
            AtomEdit {
                charge: Some(-1),
                isotope: Some(Some(13)),
                ..AtomEdit::default()
            },
        )])
        .unwrap();
    let charged = atom_state(&document, 1);
    // Becoming a query clears charge and isotope in the same command.
    document
        .apply(&[update(
            1,
            AtomEdit {
                kind: Some(AtomKind::Query(QueryAtom::Any)),
                charge: Some(0),
                isotope: Some(None),
                ..AtomEdit::default()
            },
        )])
        .unwrap();
    assert_eq!(
        atom_state(&document, 1).kind,
        AtomKind::Query(QueryAtom::Any)
    );
    document
        .apply(&[update(
            1,
            AtomEdit {
                kind: Some(AtomKind::Element(element("N"))),
                ..AtomEdit::default()
            },
        )])
        .unwrap();
    assert_eq!(
        atom_state(&document, 1).kind,
        AtomKind::Element(element("N"))
    );
    document.undo().unwrap();
    assert_eq!(
        atom_state(&document, 1).kind,
        AtomKind::Query(QueryAtom::Any)
    );
    document.undo().unwrap();
    assert_eq!(atom_state(&document, 1), charged);
}

#[test]
fn bond_kinds_survive_undo_and_redo_and_a_dative_bond_starts_at_its_donor() {
    let mut document = pair();
    let kinds = [
        BondOrder::Aromatic,
        BondOrder::Dative,
        BondOrder::Unknown,
        BondOrder::Query(BondQuery::Any),
        BondOrder::Query(BondQuery::OneOf(vec![
            QueryBondOrder::Aromatic,
            QueryBondOrder::Single,
        ])),
    ];
    for order in kinds {
        document
            .apply(&[Command::UpdateBond {
                bond: bond(1),
                order: order.clone(),
            }])
            .unwrap();
        assert_eq!(bond_order(&document, 1), order);
        document.undo().unwrap();
        assert_eq!(bond_order(&document, 1), BondOrder::Single, "{order:?}");
        document.redo().unwrap();
        assert_eq!(bond_order(&document, 1), order);
        document.undo().unwrap();
    }

    // `bond_to` names the bond's begin: here a1, the donor of a new dative bond.
    document
        .apply(&[Command::AddAtom {
            atom: Atom::from(element("Fe")),
            positions: at(90.0),
            bond_to: Some(BondTo {
                atom: AtomRef::Id(atom(1)),
                order: BondOrder::Dative,
            }),
        }])
        .unwrap();
    let (_, dative) = document.bonds().last().unwrap();
    assert_eq!(
        (dative.begin, dative.end, dative.order.clone()),
        (atom(1), atom(3), BondOrder::Dative)
    );
}

/// Each refused combination names its code and the failing command, and leaves the document
/// exactly as it was.
#[test]
fn invalid_combinations_are_refused_with_their_code_and_change_nothing() {
    let charged = Command::UpdateAtom {
        atom: atom(2),
        edit: AtomEdit {
            charge: Some(1),
            ..AtomEdit::default()
        },
    };
    let cases: Vec<(&str, Command, StateError)> = vec![
        (
            "17 stated hydrogens",
            update(
                1,
                AtomEdit {
                    hydrogens: Some(Hydrogens::Specified(17)),
                    ..AtomEdit::default()
                },
            ),
            StateError::HydrogenCountOutOfRange,
        ),
        (
            "a query atom made from a charged atom",
            update(
                2,
                AtomEdit {
                    kind: Some(AtomKind::Query(QueryAtom::AnyHeavy)),
                    ..AtomEdit::default()
                },
            ),
            StateError::QueryAtomCharge,
        ),
        (
            "an isotope on a query atom",
            add(
                Atom {
                    isotope: Some(2),
                    ..Atom::new(AtomKind::Query(QueryAtom::Any))
                },
                90.0,
            ),
            StateError::QueryAtomIsotope,
        ),
        (
            "an empty element list",
            add(Atom::new(list(&[], false)), 90.0),
            StateError::QueryListInvalid,
        ),
        (
            "a repeated element",
            update(
                1,
                AtomEdit {
                    kind: Some(list(&["N", "N"], false)),
                    ..AtomEdit::default()
                },
            ),
            StateError::QueryListInvalid,
        ),
        (
            "eleven elements",
            update(
                1,
                AtomEdit {
                    kind: Some(list(
                        &["N", "O", "S", "P", "F", "Cl", "Br", "I", "B", "Si", "Se"],
                        false,
                    )),
                    ..AtomEdit::default()
                },
            ),
            StateError::QueryListInvalid,
        ),
        (
            "an empty bond query",
            Command::UpdateBond {
                bond: bond(1),
                order: BondOrder::Query(BondQuery::OneOf(Vec::new())),
            },
            StateError::BondQueryInvalid,
        ),
        (
            "a repeated bond kind",
            Command::AddBond {
                begin: AtomRef::Id(atom(1)),
                end: AtomRef::Created(0),
                order: BondOrder::Query(BondQuery::OneOf(vec![
                    QueryBondOrder::Double,
                    QueryBondOrder::Double,
                ])),
            },
            StateError::BondQueryInvalid,
        ),
    ];
    for (name, command, code) in cases {
        let mut document = pair();
        document.apply(std::slice::from_ref(&charged)).unwrap();
        let before = document.clone();
        // The failing command follows a valid one, so its index is 1.
        let result = document.apply(&[add(Atom::from(element("O")), 120.0), command]);
        assert_eq!(
            result,
            Err(TransactionError {
                index: Some(1),
                error: EditError::InvalidState(code),
            }),
            "{name}"
        );
        assert_eq!(document, before, "{name} changed nothing");
    }
}

#[test]
fn restored_documents_obey_the_same_rules() {
    let mut document = pair();
    document
        .apply(&[update(
            1,
            AtomEdit {
                kind: Some(list(&["Cl", "Br"], false)),
                hydrogens: Some(Hydrogens::Unknown),
                ..AtomEdit::default()
            },
        )])
        .unwrap();
    assert_eq!(Document::restore(document.to_parts()), Ok(document.clone()));

    // A query atom stored with a charge.
    let mut parts = document.to_parts();
    parts.atoms[0].1.charge = 2;
    let message = Document::restore(parts).unwrap_err().to_string();
    assert!(message.contains("no charge"), "{message}");

    // A stated count beyond the limit.
    let mut parts = document.to_parts();
    parts.atoms[1].1.hydrogens = Hydrogens::Specified(40);
    let message = Document::restore(parts).unwrap_err().to_string();
    assert!(message.contains("0 to 16"), "{message}");

    // A bond query that repeats a kind.
    let mut parts = document.to_parts();
    parts.bonds[0].1.order = BondOrder::Query(BondQuery::OneOf(vec![
        QueryBondOrder::Triple,
        QueryBondOrder::Triple,
    ]));
    assert!(Document::restore(parts).is_err());

    // History whose earlier state is an invalid atom does not replay.
    let mut parts = document.to_parts();
    let change = &mut parts.undo.last_mut().unwrap().atoms[0].1;
    *change = Change {
        before: Some(Atom {
            charge: 3,
            ..Atom::new(list(&["N"], false))
        }),
        after: change.after.clone(),
    };
    let message = Document::restore(parts).unwrap_err().to_string();
    assert!(message.contains("undo history"), "{message}");
}
