//! Native fragments preserve stated graph data and refuse unsupported boundaries atomically.

use std::collections::BTreeSet;

use chemifree_domain::clipboard::{FragmentError, copy_fragment};
use chemifree_domain::document::{
    Atom, AtomId, AtomRef, BondId, BondOrder, BondRef, Command, Document, DocumentId, EditError,
    NewAtomPositions, ObjectId, Outcome, Point, Vector,
};
use chemifree_domain::element::Element;
use chemifree_domain::scene::{Selection, TransactionSelection};
use chemifree_domain::settings::Settings;
use chemifree_domain::states::{AtomKind, AttachmentPoints, Hydrogens, QueryAtom, Radical};
use chemifree_domain::stereo::{
    AtomConfiguration, BondConfiguration, BondEnd, CenterState, DoubleBondState, Relation, Slot,
    SlotRef, StereoKey, StereoMark, StereoState, Turn,
};
use chemifree_domain::stereo_group::GroupKind;

fn atom(serial: u32) -> AtomId {
    AtomId::from_serial(serial).unwrap()
}
fn bond(serial: u32) -> BondId {
    BondId::from_serial(serial).unwrap()
}
fn object(serial: u32) -> ObjectId {
    ObjectId::from_serial(serial).unwrap()
}
fn point(x: f64, y: f64) -> Point {
    Point::new(x, y).unwrap()
}
fn document(id: &str) -> Document {
    Document::new(DocumentId::parse(id).unwrap(), "Native fragments").unwrap()
}
fn selected(atoms: &[u32], bonds: &[u32]) -> Selection {
    Selection {
        objects: BTreeSet::new(),
        atoms: atoms.iter().copied().map(atom).collect(),
        bonds: bonds.iter().copied().map(bond).collect(),
    }
}
fn add(symbol: &str, x: f64, y: f64) -> Command {
    Command::AddAtom {
        atom: Atom::from(Element::from_symbol(symbol).unwrap()),
        positions: NewAtomPositions::Single(point(x, y)),
        bond_to: None,
    }
}
fn chain() -> Document {
    let mut source = document("source");
    source
        .apply(&[
            add("C", 0.0, 0.0),
            add("O", 30.0, 0.0),
            add("N", 60.0, 0.0),
            Command::AddBond {
                begin: AtomRef::Id(atom(1)),
                end: AtomRef::Id(atom(2)),
                order: BondOrder::Single,
            },
            Command::AddBond {
                begin: AtomRef::Id(atom(2)),
                end: AtomRef::Id(atom(3)),
                order: BondOrder::Double,
            },
            Command::AddInstance {
                source: Some(object(1)),
                offset: Vector {
                    dx: 100.0,
                    dy: 200.0,
                },
            },
        ])
        .unwrap();
    source
}
fn star() -> Document {
    let mut source = document("stereo-source");
    source
        .apply(&[
            add("C", 0.0, 0.0),
            add("F", 30.0, 0.0),
            add("Cl", 0.0, 30.0),
            add("Br", -30.0, 0.0),
            add("I", 0.0, -30.0),
            Command::AddBond {
                begin: AtomRef::Id(atom(1)),
                end: AtomRef::Id(atom(2)),
                order: BondOrder::Single,
            },
            Command::AddBond {
                begin: AtomRef::Id(atom(1)),
                end: AtomRef::Id(atom(3)),
                order: BondOrder::Single,
            },
            Command::AddBond {
                begin: AtomRef::Id(atom(1)),
                end: AtomRef::Id(atom(4)),
                order: BondOrder::Single,
            },
            Command::AddBond {
                begin: AtomRef::Id(atom(1)),
                end: AtomRef::Id(atom(5)),
                order: BondOrder::Single,
            },
            Command::SetBondMark {
                bond: BondRef::Id(bond(1)),
                mark: Some(StereoMark::Wedge {
                    narrow: BondEnd::Begin,
                }),
            },
            Command::SetAtomConfiguration {
                atom: AtomRef::Id(atom(1)),
                configuration: Some(AtomConfiguration::Tetrahedral {
                    neighbors: [
                        SlotRef::Atom(AtomRef::Id(atom(2))),
                        SlotRef::Atom(AtomRef::Id(atom(3))),
                        SlotRef::Atom(AtomRef::Id(atom(4))),
                        SlotRef::Atom(AtomRef::Id(atom(5))),
                    ],
                    turn: Turn::Clockwise,
                }),
            },
        ])
        .unwrap();
    source
}

#[test]
fn copy_uses_induced_bonds_and_explicit_secondary_positions_without_changing_source() {
    let source = chain();
    let before = source.clone();
    let copied = copy_fragment(&source, &selected(&[1, 2], &[]), object(2)).unwrap();
    assert_eq!(
        copied
            .fragment
            .atoms
            .iter()
            .map(|item| (item.id, item.position))
            .collect::<Vec<_>>(),
        vec![
            (atom(1), point(100.0, 200.0)),
            (atom(2), point(130.0, 200.0))
        ]
    );
    assert_eq!(
        copied
            .fragment
            .bonds
            .iter()
            .map(|item| item.id)
            .collect::<Vec<_>>(),
        vec![bond(1)],
        "an internal bond is copied even if not explicitly selected"
    );
    assert_eq!(copied.boundary_bonds, vec![bond(2)]);
    assert_eq!(source, before);
    let by_bond = copy_fragment(&source, &selected(&[], &[1]), object(2)).unwrap();
    assert_eq!(
        by_bond, copied,
        "a bond selects its endpoints without following the rest of the component"
    );
}

#[test]
fn paste_allocates_fresh_ids_and_is_one_selection_aware_undo_redo_step() {
    let copied = copy_fragment(&chain(), &selected(&[1, 2], &[]), object(2)).unwrap();
    let mut destination = document("destination");
    destination.apply(&[add("He", -40.0, -20.0)]).unwrap();
    let before = destination.clone();
    let plan = copied
        .fragment
        .paste_commands(&destination, Vector { dx: 5.0, dy: 7.0 })
        .unwrap();
    assert_eq!(
        destination, before,
        "planning does not edit the destination"
    );
    assert_eq!(
        plan.atoms
            .iter()
            .map(|item| (item.source, item.target))
            .collect::<Vec<_>>(),
        vec![(atom(1), atom(2)), (atom(2), atom(3))]
    );
    assert_eq!(plan.selection.atoms, vec![atom(2), atom(3)]);
    assert_eq!(plan.selection.bonds, vec![bond(1)]);
    assert_eq!(
        destination
            .apply_selecting(
                &plan.commands,
                &TransactionSelection {
                    before: None,
                    after: Some(plan.selection)
                }
            )
            .unwrap(),
        Outcome::Changed
    );
    assert_eq!(destination.revision(), before.revision() + 1);
    assert_eq!(
        destination.position(object(1), atom(1)),
        Some(point(-40.0, -20.0))
    );
    assert_eq!(
        destination.position(object(1), atom(2)),
        Some(point(105.0, 207.0))
    );
    assert_eq!(
        destination.position(object(1), atom(3)),
        Some(point(135.0, 207.0))
    );
    assert_eq!(
        destination.selection().atoms,
        BTreeSet::from([atom(2), atom(3)])
    );
    let pasted = destination.to_parts();
    destination.undo().unwrap();
    assert_eq!(
        destination
            .atoms()
            .map(|(id, value)| (id, value.clone()))
            .collect::<Vec<_>>(),
        before
            .atoms()
            .map(|(id, value)| (id, value.clone()))
            .collect::<Vec<_>>()
    );
    assert_eq!(destination.bonds().len(), 0);
    destination.redo().unwrap();
    let redone = destination.to_parts();
    assert_eq!(
        (
            redone.atoms,
            redone.bonds,
            redone.positions,
            redone.selection
        ),
        (
            pasted.atoms,
            pasted.bonds,
            pasted.positions,
            pasted.selection
        )
    );
}

#[test]
fn stereo_remapping_preserves_handedness_when_fragment_atom_order_changes() {
    let source = star();
    let mut fragment = copy_fragment(&source, &selected(&[1, 2, 3, 4, 5], &[]), object(1))
        .unwrap()
        .fragment;
    fragment.atoms.swap(1, 2);
    let mut destination = document("stereo-destination");
    destination.apply(&[add("He", 100.0, 100.0)]).unwrap();
    let plan = fragment
        .paste_commands(&destination, Vector { dx: 10.0, dy: 20.0 })
        .unwrap();
    destination.apply(&plan.commands).unwrap();
    assert_eq!(
        destination.stereo().get(StereoKey::Center(atom(2))),
        Some(&StereoState::Center(CenterState::Tetrahedral {
            slots: [
                Slot::Atom(atom(3)),
                Slot::Atom(atom(4)),
                Slot::Atom(atom(5)),
                Slot::Atom(atom(6))
            ],
            turn: Turn::Anticlockwise,
        })),
        "mapping swaps the first two neighbor identifiers, so canonical parity must flip"
    );
    assert_eq!(
        destination.bond(bond(1)).unwrap().mark,
        Some(StereoMark::Wedge {
            narrow: BondEnd::Begin
        })
    );
    assert_eq!(
        source.stereo().get(StereoKey::Center(atom(1))),
        Some(&StereoState::Center(CenterState::Tetrahedral {
            slots: [
                Slot::Atom(atom(2)),
                Slot::Atom(atom(3)),
                Slot::Atom(atom(4)),
                Slot::Atom(atom(5))
            ],
            turn: Turn::Clockwise,
        }))
    );
}

#[test]
fn copy_refuses_stereo_boundaries_and_enhanced_groups_without_discarding_them() {
    let mut source = star();
    let before = source.clone();
    assert_eq!(
        copy_fragment(&source, &selected(&[1, 2, 3, 4], &[]), object(1)).unwrap_err(),
        FragmentError::StereoBoundary
    );
    assert_eq!(source, before);
    source
        .apply(&[Command::CreateStereoGroup {
            kind: GroupKind::Absolute,
            number: None,
            atoms: vec![AtomRef::Id(atom(1))],
        }])
        .unwrap();
    let grouped = source.clone();
    assert_eq!(
        copy_fragment(&source, &selected(&[1, 2, 3, 4, 5], &[]), object(1)).unwrap_err(),
        FragmentError::EnhancedStereo
    );
    assert_eq!(source, grouped);
}

#[test]
fn unknown_copy_targets_scene_selection_and_absent_placement_are_refused() {
    let source = chain();
    assert_eq!(
        copy_fragment(&source, &selected(&[99], &[]), object(1)).unwrap_err(),
        FragmentError::UnknownSelection
    );
    assert_eq!(
        copy_fragment(&source, &selected(&[], &[99]), object(1)).unwrap_err(),
        FragmentError::UnknownSelection
    );
    assert_eq!(
        copy_fragment(&source, &selected(&[1], &[]), object(99)).unwrap_err(),
        FragmentError::InvalidInstance
    );
    assert_eq!(
        copy_fragment(
            &source,
            &Selection {
                objects: BTreeSet::from([object(1)]),
                ..Selection::default()
            },
            object(1)
        )
        .unwrap_err(),
        FragmentError::SceneSelection
    );
    assert_eq!(
        copy_fragment(&source, &Selection::default(), object(1)).unwrap_err(),
        FragmentError::EmptySelection
    );
}

#[test]
fn duplicate_and_dangling_fragment_ids_cannot_address_existing_destination_entities() {
    let mut fragment = copy_fragment(&chain(), &selected(&[1, 2], &[]), object(1))
        .unwrap()
        .fragment;
    let mut destination = document("invalid-destination");
    destination.apply(&[add("He", 1.0, 2.0)]).unwrap();
    let before = destination.clone();
    fragment.bonds[0].bond.end = atom(99);
    assert_eq!(
        fragment
            .paste_commands(&destination, Vector { dx: 0.0, dy: 0.0 })
            .unwrap_err(),
        FragmentError::Invalid(EditError::UnknownAtom(atom(99)))
    );
    fragment.bonds[0].bond.end = atom(2);
    fragment.atoms.push(fragment.atoms[0].clone());
    assert_eq!(
        fragment
            .paste_commands(&destination, Vector { dx: 0.0, dy: 0.0 })
            .unwrap_err(),
        FragmentError::DuplicateAtom
    );
    assert_eq!(destination, before);
}

#[test]
fn style_multi_placement_and_coordinate_refusals_leave_destination_exactly_unchanged() {
    let fragment = copy_fragment(&chain(), &selected(&[1], &[]), object(1))
        .unwrap()
        .fragment;
    let mut settings = Settings::default();
    settings.line_width_pt = 2.0;
    let changed_style = Document::new_with_settings(
        DocumentId::parse("changed-style").unwrap(),
        "Other style",
        settings,
    )
    .unwrap();
    assert_eq!(
        fragment
            .paste_commands(&changed_style, Vector { dx: 0.0, dy: 0.0 })
            .unwrap_err(),
        FragmentError::StyleMismatch
    );
    let multi = chain();
    let before = multi.clone();
    assert_eq!(
        fragment
            .paste_commands(&multi, Vector { dx: 0.0, dy: 0.0 })
            .unwrap_err(),
        FragmentError::DestinationInstances
    );
    assert_eq!(multi, before);
    let destination = document("coordinate-refusal");
    let before = destination.clone();
    assert_eq!(
        fragment
            .paste_commands(
                &destination,
                Vector {
                    dx: 100_001.0,
                    dy: 0.0
                }
            )
            .unwrap_err(),
        FragmentError::Invalid(EditError::CoordinateOutOfRange)
    );
    assert_eq!(
        fragment
            .paste_commands(
                &destination,
                Vector {
                    dx: f64::NAN,
                    dy: 0.0
                }
            )
            .unwrap_err(),
        FragmentError::Invalid(EditError::CoordinateOutOfRange)
    );
    assert_eq!(destination, before);
}

#[test]
fn copy_and_paste_enforce_the_real_transaction_command_boundary() {
    let mut source = document("command-boundary");
    source
        .apply(
            &(0..500)
                .map(|index| add("C", f64::from(index), 0.0))
                .collect::<Vec<_>>(),
        )
        .unwrap();
    let selection = Selection {
        atoms: (1..=500).map(atom).collect(),
        ..Selection::default()
    };
    let fragment = copy_fragment(&source, &selection, object(1))
        .unwrap()
        .fragment;
    assert_eq!(
        fragment
            .paste_commands(&document("fits"), Vector { dx: 0.0, dy: 0.0 })
            .unwrap()
            .commands
            .len(),
        500
    );
    source.apply(&[add("C", 500.0, 0.0)]).unwrap();
    let too_many = Selection {
        atoms: (1..=501).map(atom).collect(),
        ..Selection::default()
    };
    let before = source.clone();
    assert_eq!(
        copy_fragment(&source, &too_many, object(1)).unwrap_err(),
        FragmentError::CommandLimit
    );
    assert_eq!(source, before);
}

#[test]
fn paste_preserves_recorded_atom_states_and_query_atoms_without_reinterpreting_them() {
    let mut source = document("recorded-source");
    let mut carbon = Atom::from(Element::from_symbol("C").unwrap());
    carbon.charge = 1;
    carbon.isotope = Some(13);
    carbon.hydrogens = Hydrogens::Specified(1);
    carbon.radical = Radical::Doublet;
    carbon.attachment_points = AttachmentPoints::new(&[1, 3]).unwrap();
    let query = Atom::new(AtomKind::Query(QueryAtom::Halogen));
    source
        .apply(&[
            Command::AddAtom {
                atom: carbon.clone(),
                positions: NewAtomPositions::Single(point(0.0, 0.0)),
                bond_to: None,
            },
            Command::AddAtom {
                atom: query.clone(),
                positions: NewAtomPositions::Single(point(30.0, 0.0)),
                bond_to: None,
            },
        ])
        .unwrap();
    let fragment = copy_fragment(&source, &selected(&[1, 2], &[]), object(1))
        .unwrap()
        .fragment;
    let mut destination = document("recorded-destination");
    let plan = fragment
        .paste_commands(
            &destination,
            Vector {
                dx: 10.0,
                dy: -10.0,
            },
        )
        .unwrap();
    destination.apply(&plan.commands).unwrap();
    assert_eq!(
        destination
            .atoms()
            .map(|(_, value)| value.clone())
            .collect::<Vec<_>>(),
        vec![carbon, query]
    );
}

#[test]
fn double_bond_configuration_and_mark_keep_their_references_after_paste() {
    let mut source = document("double-bond-source");
    source
        .apply(&[
            add("F", -30.0, 0.0),
            add("C", 0.0, 0.0),
            add("C", 30.0, 0.0),
            add("Cl", 60.0, 0.0),
            Command::AddBond {
                begin: AtomRef::Id(atom(1)),
                end: AtomRef::Id(atom(2)),
                order: BondOrder::Single,
            },
            Command::AddBond {
                begin: AtomRef::Id(atom(2)),
                end: AtomRef::Id(atom(3)),
                order: BondOrder::Double,
            },
            Command::AddBond {
                begin: AtomRef::Id(atom(3)),
                end: AtomRef::Id(atom(4)),
                order: BondOrder::Single,
            },
            Command::SetBondMark {
                bond: BondRef::Id(bond(2)),
                mark: Some(StereoMark::Crossed),
            },
            Command::SetBondConfiguration {
                bond: BondRef::Id(bond(2)),
                configuration: Some(BondConfiguration::Relative {
                    begin_neighbor: SlotRef::Atom(AtomRef::Id(atom(1))),
                    end_neighbor: SlotRef::Atom(AtomRef::Id(atom(4))),
                    relation: Relation::OppositeSides,
                }),
            },
        ])
        .unwrap();
    assert_eq!(
        copy_fragment(&source, &selected(&[2, 3], &[]), object(1)).unwrap_err(),
        FragmentError::StereoBoundary
    );
    let fragment = copy_fragment(&source, &selected(&[1, 2, 3, 4], &[]), object(1))
        .unwrap()
        .fragment;
    let mut destination = document("double-bond-destination");
    destination.apply(&[add("He", 200.0, 200.0)]).unwrap();
    let plan = fragment
        .paste_commands(&destination, Vector { dx: 0.0, dy: 10.0 })
        .unwrap();
    destination.apply(&plan.commands).unwrap();
    assert_eq!(
        destination.stereo().get(StereoKey::DoubleBond(bond(2))),
        Some(&StereoState::DoubleBond(DoubleBondState::Relative {
            begin_ref: Slot::Atom(atom(2)),
            end_ref: Slot::Atom(atom(5)),
            relation: Relation::OppositeSides,
        }))
    );
    assert_eq!(
        destination.bond(bond(2)).unwrap().mark,
        Some(StereoMark::Crossed)
    );
}
