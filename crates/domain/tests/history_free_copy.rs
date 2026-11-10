//! `Document::without_history`: the copy that analyses and save-as take under the session table.
//! It keeps the identifier, revision, content, selection and counters, and drops the undo and redo
//! entries.

use std::collections::BTreeSet;

use chemifree_domain::document::{
    Atom, AtomId, AtomRef, BondId, BondOrder, BondTo, Command, Document, DocumentId, HistoryError,
    NewAtomPositions, Numbering, Point, Vector,
};
use chemifree_domain::element::Element;
use chemifree_domain::scene::{Selection, SelectionRequest, TransactionSelection};

fn atom(serial: u32) -> AtomId {
    AtomId::from_serial(serial).unwrap()
}

fn bond(serial: u32) -> BondId {
    BondId::from_serial(serial).unwrap()
}

/// An atom drawn at `(x, y)`, singly bonded to atom `bonded_to` when one is given.
fn add_atom(symbol: &str, x: f64, y: f64, bonded_to: Option<u32>) -> Command {
    Command::AddAtom {
        atom: Atom::from(Element::from_symbol(symbol).unwrap()),
        positions: NewAtomPositions::Single(Point::new(x, y).unwrap()),
        bond_to: bonded_to.map(|serial| BondTo {
            atom: AtomRef::Id(atom(serial)),
            order: BondOrder::Single,
        }),
    }
}

/// The selection the first edit leaves: atoms 1 and 2 and bond 1.
fn first_edit_selection() -> Selection {
    Selection {
        objects: BTreeSet::new(),
        atoms: BTreeSet::from([atom(1), atom(2)]),
        bonds: BTreeSet::from([bond(1)]),
    }
}

/// Edit one draws C1–C2–O3 and selects C1, C2 and their bond. Edit two adds N4 on O3 and a
/// second instance `s2`. Edit two is then undone, leaving one undo entry, one redo entry and
/// revision 3. Undo never rewinds the counters, so they stay advanced (`next_atom` 5,
/// `next_bond` 4, `next_object` 3). The group counter is then set to 3, past the groups held
/// (none), as deleting a group leaves it; restore accepts that state.
fn edited_then_undone() -> Document {
    let mut document =
        Document::new(DocumentId::parse("doc-1").unwrap(), "  Ethanol sketch ").unwrap();
    document
        .apply_selecting(
            &[
                add_atom("C", 0.0, 0.0, None),
                add_atom("C", 26.0, 15.0, Some(1)),
                add_atom("O", 52.0, 0.0, Some(2)),
            ],
            &TransactionSelection {
                before: None,
                after: Some(SelectionRequest {
                    atoms: vec![atom(1), atom(2)],
                    bonds: vec![bond(1)],
                    ..SelectionRequest::default()
                }),
            },
        )
        .unwrap();
    document
        .apply(&[
            add_atom("N", 78.0, 0.0, Some(3)),
            Command::AddInstance {
                source: None,
                offset: Vector { dx: 0.0, dy: 100.0 },
            },
        ])
        .unwrap();
    document.undo().unwrap();
    let mut parts = document.to_parts();
    parts.next_group = 3;
    Document::restore(parts).unwrap()
}

#[test]
fn without_history_keeps_identity_revision_content_selection_and_counters() {
    let document = edited_then_undone();
    assert_eq!(document.revision(), 3, "two edits and one undo");
    assert_eq!((document.undo_depth(), document.redo_depth()), (1, 1));
    assert_eq!(document.selection(), &first_edit_selection());
    assert_eq!(
        document.numbering(),
        Numbering {
            revision: 3,
            next_atom: 5,
            next_bond: 4,
            next_object: 3,
            next_group: 3,
        },
        "every counter is off its starting value, so dropping any one is visible"
    );

    let copy = document.without_history();
    assert_eq!(copy.id(), document.id());
    assert_eq!(copy.id().as_str(), "doc-1");
    assert_eq!(copy.revision(), 3, "the copy is at the source's revision");
    assert_eq!(
        (copy.undo_depth(), copy.redo_depth()),
        (0, 0),
        "the copy has no undo or redo entries"
    );
    let mut expected = document.to_parts();
    expected.undo.clear();
    expected.redo.clear();
    assert_eq!(
        copy.to_parts(),
        expected,
        "the copy holds the source's content, selection and counters"
    );
    assert_eq!(copy.numbering(), document.numbering());
}

#[test]
fn copying_leaves_the_source_history_in_place() {
    let mut document = edited_then_undone();
    let before = document.to_parts();
    let mut copy = document.without_history();
    assert_eq!(
        document.to_parts(),
        before,
        "copying changes nothing in the source"
    );
    assert_eq!((document.undo_depth(), document.redo_depth()), (1, 1));
    assert_eq!(
        copy.undo(),
        Err(HistoryError::NothingToUndo),
        "the copy has nothing to undo"
    );

    document
        .undo()
        .expect("the source still undoes its first edit");
    assert_eq!(document.revision(), 4);
    assert_eq!((document.undo_depth(), document.redo_depth()), (0, 2));
    assert_eq!(
        copy.revision(),
        3,
        "the copy does not follow the source's later undo"
    );
}

#[test]
fn a_document_without_history_is_returned_unchanged() {
    let fresh = Document::new(DocumentId::parse("doc-2").unwrap(), "Empty").unwrap();
    assert_eq!(fresh.without_history(), fresh);

    // Content, a selection and revision 3, with the history removed.
    let mut parts = edited_then_undone().to_parts();
    parts.undo.clear();
    parts.redo.clear();
    let stored = Document::restore(parts).unwrap();
    assert_eq!(
        (stored.revision(), stored.undo_depth(), stored.redo_depth()),
        (3, 0, 0),
        "the stored document is at revision 3 with no history"
    );
    assert_eq!(
        stored.without_history(),
        stored,
        "a copy of a document without history keeps its revision and selection"
    );
}
