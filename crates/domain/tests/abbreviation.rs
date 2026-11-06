//! Et hides a checked existing graph and round-trips that display through ordinary history.

use chemifree_domain::clipboard::{self, FragmentError};
use chemifree_domain::document::{
    Atom, AtomEdit, AtomId, AtomRef, BondId, BondOrder, BondRef, BondTo, Command, Document,
    DocumentId, EditError, NewAtomPositions, ObjectId, Outcome, Point, PositionKey, Vector,
};
use chemifree_domain::element::Element;
use chemifree_domain::scene::{Selection, SelectionTransform};
use chemifree_domain::stereo::StereoMark;

fn atom(serial: u32) -> AtomId {
    AtomId::from_serial(serial).unwrap()
}
fn instance() -> ObjectId {
    ObjectId::from_serial(1).unwrap()
}
fn point(x: f64, y: f64) -> Point {
    Point::new(x, y).unwrap()
}
fn collapse() -> Command {
    Command::CollapseEthyl {
        instance: instance(),
        anchor: atom(2),
        terminal: atom(3),
    }
}
fn drawing() -> Document {
    let mut doc = Document::new(DocumentId::parse("ethyl").unwrap(), "Et").unwrap();
    doc.apply(&[
        Command::AddAtom {
            atom: Atom::from(Element::from_symbol("O").unwrap()),
            positions: NewAtomPositions::Single(point(0.0, 0.0)),
            bond_to: None,
        },
        Command::AddAtom {
            atom: Atom::from(Element::from_symbol("C").unwrap()),
            positions: NewAtomPositions::Single(point(30.0, 0.0)),
            bond_to: Some(BondTo {
                atom: AtomRef::Created(0),
                order: BondOrder::Single,
            }),
        },
        Command::AddAtom {
            atom: Atom::from(Element::from_symbol("C").unwrap()),
            positions: NewAtomPositions::Single(point(45.0, 25.0)),
            bond_to: Some(BondTo {
                atom: AtomRef::Created(1),
                order: BondOrder::Single,
            }),
        },
    ])
    .unwrap();
    doc
}
fn selection() -> Selection {
    Selection {
        atoms: [atom(2), atom(3)].into(),
        bonds: [BondId::from_serial(2).unwrap()].into(),
        ..Selection::default()
    }
}

#[test]
fn collapse_and_expand_change_only_display_and_each_have_one_undo_step() {
    let mut doc = drawing();
    let original = doc.to_parts();
    assert_eq!(doc.apply(&[collapse()]).unwrap(), Outcome::Changed);
    let collapsed = doc.to_parts();
    assert_eq!(
        collapsed.abbreviations,
        vec![(
            PositionKey {
                instance: instance(),
                atom: atom(2)
            },
            atom(3)
        )]
    );
    assert_eq!(
        (
            &collapsed.atoms,
            &collapsed.bonds,
            &collapsed.positions,
            &collapsed.stereo,
            &collapsed.stereo_groups
        ),
        (
            &original.atoms,
            &original.bonds,
            &original.positions,
            &original.stereo,
            &original.stereo_groups
        )
    );
    assert_eq!(
        (
            collapsed.next_atom,
            collapsed.next_bond,
            collapsed.next_object
        ),
        (original.next_atom, original.next_bond, original.next_object)
    );
    assert_eq!(doc.undo_depth(), 2);
    assert_eq!(doc.apply(&[collapse()]).unwrap(), Outcome::Unchanged);
    assert_eq!(doc.to_parts(), collapsed);
    doc.undo().unwrap();
    assert_eq!(doc.abbreviations().count(), 0);
    doc.redo().unwrap();
    assert_eq!(doc.abbreviations().count(), 1);
    doc.apply(&[Command::ExpandEthyl {
        instance: instance(),
        anchor: atom(2),
    }])
    .unwrap();
    assert_eq!(doc.to_parts().positions, original.positions);
    doc.undo().unwrap();
    assert_eq!(doc.abbreviations().count(), 1);
    assert_eq!(Document::restore(doc.to_parts()).unwrap(), doc);
}

#[test]
fn full_pair_moves_and_transforms_keep_display_but_partial_moves_expand_with_undo() {
    let mut doc = drawing();
    doc.apply(&[collapse()]).unwrap();
    doc.apply(&[Command::MoveSelection {
        selection: selection(),
        instance: Some(instance()),
        delta: Vector { dx: 7.0, dy: -2.0 },
    }])
    .unwrap();
    assert_eq!(doc.abbreviations().count(), 1);
    assert_eq!(doc.position(instance(), atom(2)), Some(point(37.0, -2.0)));
    assert_eq!(doc.position(instance(), atom(3)), Some(point(52.0, 23.0)));
    doc.apply(&[Command::TransformSelection {
        selection: selection(),
        instance: Some(instance()),
        pivot: point(0.0, 0.0),
        transform: SelectionTransform::Rotate { degrees: 90.0 },
    }])
    .unwrap();
    assert_eq!(doc.abbreviations().count(), 1);
    let before = doc.to_parts();
    doc.apply(&[Command::MoveAtoms {
        atoms: vec![atom(3)],
        instance: Some(instance()),
        delta: Vector { dx: 1.0, dy: 0.0 },
    }])
    .unwrap();
    assert_eq!(doc.abbreviations().count(), 0);
    doc.undo().unwrap();
    assert_eq!(doc.to_parts().positions, before.positions);
    assert_eq!(doc.to_parts().abbreviations, before.abbreviations);
}

#[test]
fn edits_to_members_boundary_and_local_marks_expand_atomically_without_stereo_loss() {
    let mut doc = drawing();
    doc.apply(&[collapse()]).unwrap();
    let before = doc.clone();
    let mark = Command::SetBondMark {
        bond: BondRef::Id(BondId::from_serial(1).unwrap()),
        mark: Some(StereoMark::Wavy),
    };
    doc.apply(&[mark]).unwrap();
    assert_eq!(doc.abbreviations().count(), 0);
    assert_eq!(
        doc.bond(BondId::from_serial(1).unwrap()).unwrap().mark,
        Some(StereoMark::Wavy)
    );
    doc.undo().unwrap();
    assert_eq!(
        doc.to_parts().abbreviations,
        before.to_parts().abbreviations
    );
    doc.apply(&[Command::UpdateAtom {
        atom: atom(3),
        edit: AtomEdit {
            charge: Some(1),
            ..AtomEdit::default()
        },
    }])
    .unwrap();
    assert_eq!(doc.abbreviations().count(), 0);
    assert_eq!(
        doc.atoms()
            .find(|(id, _)| *id == atom(3))
            .map(|(_, state)| state.charge),
        Some(1)
    );
    doc.undo().unwrap();
    assert_eq!(
        doc.atoms()
            .find(|(id, _)| *id == atom(3))
            .map(|(_, state)| state.charge),
        Some(0)
    );
    assert_eq!(doc.abbreviations().count(), 1);
    let before = doc.clone();
    assert_eq!(
        doc.apply(&[
            Command::DeleteAtoms {
                atoms: vec![atom(3)]
            },
            collapse()
        ])
        .unwrap_err()
        .error,
        EditError::AbbreviationInvalid
    );
    assert_eq!(
        doc, before,
        "failed later command rolls back graph, display, IDs and history"
    );
}

#[test]
fn unsupported_boundary_states_and_multiple_placements_refuse_without_changing_anything() {
    let mut doc = drawing();
    let before = doc.clone();
    assert_eq!(
        doc.apply(&[Command::CollapseEthyl {
            instance: instance(),
            anchor: atom(3),
            terminal: atom(2)
        }])
        .unwrap_err()
        .error,
        EditError::AbbreviationInvalid
    );
    assert_eq!(doc, before);
    doc.apply(&[Command::SetBondMark {
        bond: BondRef::Id(BondId::from_serial(2).unwrap()),
        mark: Some(StereoMark::Wavy),
    }])
    .unwrap();
    let marked = doc.clone();
    assert_eq!(
        doc.apply(&[collapse()]).unwrap_err().error,
        EditError::AbbreviationInvalid
    );
    assert_eq!(doc, marked);
    doc.undo().unwrap();
    doc.apply(&[collapse()]).unwrap();
    doc.apply(&[Command::AddInstance {
        source: Some(instance()),
        offset: Vector { dx: 100.0, dy: 0.0 },
    }])
    .unwrap();
    assert_eq!(doc.abbreviations().count(), 0);
    let multiple = doc.clone();
    assert_eq!(
        doc.apply(&[collapse()]).unwrap_err().error,
        EditError::AbbreviationInvalid
    );
    assert_eq!(doc, multiple);
    doc.undo().unwrap();
    assert_eq!(doc.abbreviations().count(), 1);
}

#[test]
fn native_copy_refuses_selected_display_but_other_fragments_remain_available() {
    let mut doc = drawing();
    doc.apply(&[collapse()]).unwrap();
    let before = doc.clone();
    assert_eq!(
        clipboard::copy_fragment(&doc, &selection(), instance()),
        Err(FragmentError::AbbreviationDisplay)
    );
    let outside = Selection {
        atoms: [atom(1)].into(),
        ..Selection::default()
    };
    let copied = clipboard::copy_fragment(&doc, &outside, instance()).unwrap();
    assert_eq!(copied.fragment.atoms.len(), 1);
    assert_eq!(copied.fragment.atoms[0].id, atom(1));
    assert_eq!(doc, before);
    let mut corrupt = doc.to_parts();
    corrupt.abbreviations.push(corrupt.abbreviations[0]);
    assert!(
        Document::restore(corrupt).is_err(),
        "duplicate display keys are not silently deduplicated"
    );
    let mut corrupt = doc.to_parts();
    corrupt.undo.last_mut().unwrap().abbreviations.clear();
    assert!(
        Document::restore(corrupt).is_err(),
        "history must replay display changes exactly"
    );
}

#[test]
fn unrelated_marked_fragment_is_unchanged_and_member_selection_replays_with_display() {
    use chemifree_domain::scene::{SelectionRequest, TransactionSelection};
    let mut doc = drawing();
    doc.apply(&[
        Command::AddAtom {
            atom: Atom::from(Element::CARBON),
            positions: NewAtomPositions::Single(point(100.0, 0.0)),
            bond_to: None,
        },
        Command::AddAtom {
            atom: Atom::from(Element::CARBON),
            positions: NewAtomPositions::Single(point(130.0, 0.0)),
            bond_to: Some(BondTo {
                atom: AtomRef::Created(0),
                order: BondOrder::Single,
            }),
        },
        Command::SetBondMark {
            bond: BondRef::Id(BondId::from_serial(3).unwrap()),
            mark: Some(StereoMark::Wavy),
        },
    ])
    .unwrap();
    let before = doc.to_parts();
    doc.apply_selecting(
        &[collapse()],
        &TransactionSelection {
            before: Some(Selection::default()),
            after: Some(SelectionRequest {
                atoms: vec![atom(2), atom(3)],
                bonds: vec![BondId::from_serial(2).unwrap()],
                ..SelectionRequest::default()
            }),
        },
    )
    .unwrap();
    assert_eq!(doc.to_parts().selection, selection());
    assert_eq!(doc.to_parts().bonds, before.bonds);
    doc.undo().unwrap();
    assert_eq!(doc.to_parts().selection, Selection::default());
    assert_eq!(doc.to_parts().abbreviations, vec![]);
    doc.redo().unwrap();
    assert_eq!(doc.to_parts().selection, selection());
    assert_eq!(doc.to_parts().bonds, before.bonds);
    assert_eq!(doc.abbreviations().count(), 1);
}
