//! Explicit native-template anchors preserve source statements or refuse before any edit.
use chemifree_domain::clipboard::{
    Fragment, FragmentAtom, FragmentBond, FragmentError, copy_fragment,
};
use chemifree_domain::document::{
    Atom, AtomId, AtomRef, Bond, BondId, BondOrder, BondRef, Command, Document, DocumentId,
    EditError, NewAtomPositions, ObjectId, Point, Vector,
};
use chemifree_domain::element::Element;
use chemifree_domain::scene::{Selection, TransactionSelection};
use chemifree_domain::settings::Settings;
use chemifree_domain::stereo::{BondConfiguration, BondEnd, StereoKey, StereoMark};
use chemifree_domain::template::{self, TemplateError};
use std::collections::BTreeSet;
fn atom(id: u32) -> AtomId {
    AtomId::from_serial(id).unwrap()
}
fn bond(id: u32) -> BondId {
    BondId::from_serial(id).unwrap()
}
fn point(x: f64, y: f64) -> Point {
    Point::new(x, y).unwrap()
}
fn drawing(id: &str) -> Document {
    Document::new(DocumentId::parse(id).unwrap(), "Template").unwrap()
}
fn add(element: &str, x: f64, y: f64) -> Command {
    Command::AddAtom {
        atom: Atom::from(Element::from_symbol(element).unwrap()),
        positions: NewAtomPositions::Single(point(x, y)),
        bond_to: None,
    }
}
fn source() -> (Document, Fragment) {
    let mut source = drawing("source");
    source
        .apply(&[
            add("C", 100.0, 200.0),
            add("O", 130.0, 200.0),
            add("N", 160.0, 200.0),
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
            Command::SetBondMark {
                bond: BondRef::Id(bond(1)),
                mark: Some(StereoMark::Wedge {
                    narrow: BondEnd::Begin,
                }),
            },
            Command::SetBondConfiguration {
                bond: BondRef::Id(bond(2)),
                configuration: Some(BondConfiguration::Unknown),
            },
        ])
        .unwrap();
    let selected = Selection {
        objects: BTreeSet::new(),
        atoms: BTreeSet::from([atom(1), atom(2), atom(3)]),
        bonds: BTreeSet::new(),
    };
    let fragment = copy_fragment(&source, &selected, source.first_instance())
        .unwrap()
        .fragment;
    (source, fragment)
}
fn destination() -> Document {
    let mut dest = drawing("dest");
    dest.apply(&[add("C", 10.0, 20.0)]).unwrap();
    dest
}

#[test]
fn translation_connection_mapping_and_preserved_source_stereo_survive_history() {
    let (source, fragment) = source();
    let original_source = source.to_parts();
    let original_fragment = fragment.clone();
    assert_eq!(
        template::inspect(&fragment)
            .unwrap()
            .iter()
            .map(|anchor| (anchor.atom, anchor.supported))
            .collect::<Vec<_>>(),
        vec![(atom(1), true), (atom(2), false), (atom(3), false)]
    );
    let mut dest = destination();
    let before = dest.to_parts();
    let plan = template::attach(
        &fragment,
        &dest,
        dest.first_instance(),
        atom(1),
        atom(1),
        Vector { dx: 3.0, dy: 4.0 },
    )
    .unwrap();
    assert_eq!(
        dest.to_parts(),
        before,
        "planning must not reserve IDs or history"
    );
    assert_eq!(plan.connection, bond(3));
    assert_eq!(plan.paste.selection.atoms, vec![atom(2), atom(3), atom(4)]);
    assert_eq!(
        plan.paste.selection.bonds,
        vec![bond(1), bond(2)],
        "connector and target are not selected"
    );
    dest.apply_selecting(
        &plan.paste.commands,
        &TransactionSelection {
            before: None,
            after: Some(plan.paste.selection),
        },
    )
    .unwrap();
    assert_eq!(dest.atoms().len(), 4);
    assert_eq!(dest.bonds().len(), 3);
    assert_eq!(
        dest.position(dest.first_instance(), atom(1)),
        Some(point(10.0, 20.0))
    );
    for (id, x) in [(2, 28.0), (3, 58.0), (4, 88.0)] {
        assert_eq!(
            dest.position(dest.first_instance(), atom(id)),
            Some(point(x, 44.0))
        );
    }
    let edges: Vec<_> = dest
        .bonds()
        .map(|(_, edge)| (edge.begin, edge.end, edge.order.clone(), edge.mark))
        .collect();
    assert_eq!(
        edges[0],
        (
            atom(2),
            atom(3),
            BondOrder::Single,
            Some(StereoMark::Wedge {
                narrow: BondEnd::Begin
            })
        )
    );
    assert_eq!(edges[2], (atom(1), atom(2), BondOrder::Single, None));
    assert_eq!(dest.to_parts().stereo.len(), 1);
    assert_eq!(dest.to_parts().stereo[0].0, StereoKey::DoubleBond(bond(2)));
    let after = dest.to_parts();
    dest.undo().unwrap();
    assert_eq!(dest.to_parts().atoms, before.atoms);
    assert_eq!(dest.to_parts().bonds, before.bonds);
    assert_eq!(dest.to_parts().selection, before.selection);
    dest.redo().unwrap();
    assert_eq!(dest.to_parts().positions, after.positions);
    assert_eq!(dest.to_parts().stereo, after.stereo);
    assert_eq!(dest.to_parts().selection, after.selection);
    assert_eq!(source.to_parts(), original_source);
    assert_eq!(fragment, original_fragment);
}

#[test]
fn unsupported_anchor_stereo_connectivity_and_directions_refuse_without_changing_destination() {
    let (_, fragment) = source();
    let dest = destination();
    let before = dest.to_parts();
    for (source_anchor, direction, error) in [
        (
            atom(99),
            Vector { dx: 1.0, dy: 0.0 },
            TemplateError::AnchorUnknown,
        ),
        (
            atom(2),
            Vector { dx: 1.0, dy: 0.0 },
            TemplateError::AnchorStereo,
        ),
        (
            atom(1),
            Vector { dx: 0.0, dy: 0.0 },
            TemplateError::Direction,
        ),
        (
            atom(1),
            Vector {
                dx: f64::NAN,
                dy: 0.0,
            },
            TemplateError::Direction,
        ),
    ] {
        assert_eq!(
            template::attach(
                &fragment,
                &dest,
                dest.first_instance(),
                source_anchor,
                atom(1),
                direction
            )
            .unwrap_err(),
            error
        );
        assert_eq!(dest.to_parts(), before);
    }
    let mut disconnected = fragment.clone();
    disconnected.bonds.remove(0);
    assert_eq!(
        template::inspect(&disconnected).unwrap_err(),
        TemplateError::Disconnected
    );
    assert_eq!(
        template::attach(
            &fragment,
            &dest,
            ObjectId::from_serial(99).unwrap(),
            atom(1),
            atom(1),
            Vector { dx: 1.0, dy: 0.0 }
        )
        .unwrap_err(),
        TemplateError::Instance
    );
}

#[test]
fn connector_counts_toward_the_exact_command_budget_and_late_id_failure_is_atomic() {
    let atoms = (1..=250)
        .map(|id| FragmentAtom {
            id: atom(id),
            atom: Atom::from(Element::CARBON),
            position: point(f64::from(id), 0.0),
        })
        .collect();
    let bonds = (1..250)
        .map(|id| FragmentBond {
            id: bond(id),
            bond: Bond {
                begin: atom(id),
                end: atom(id + 1),
                order: BondOrder::Single,
                mark: None,
            },
        })
        .collect();
    let mut fragment = Fragment {
        settings: Settings::default(),
        atoms,
        bonds,
        stereo: Vec::new(),
    };
    let dest = destination();
    let plan = template::attach(
        &fragment,
        &dest,
        dest.first_instance(),
        atom(1),
        atom(1),
        Vector { dx: 1.0, dy: 0.0 },
    )
    .unwrap();
    assert_eq!(plan.paste.commands.len(), 500);
    fragment.bonds[0].bond.mark = Some(StereoMark::Wedge {
        narrow: BondEnd::Begin,
    });
    assert_eq!(
        template::attach(
            &fragment,
            &dest,
            dest.first_instance(),
            atom(1),
            atom(1),
            Vector { dx: 1.0, dy: 0.0 }
        )
        .unwrap_err(),
        TemplateError::Fragment(FragmentError::CommandLimit)
    );
    let (_, fragment) = source();
    let mut parts = dest.to_parts();
    parts.next_bond = u32::MAX - 2;
    let dest = Document::restore(parts).unwrap();
    let before = dest.to_parts();
    assert_eq!(
        template::attach(
            &fragment,
            &dest,
            dest.first_instance(),
            atom(1),
            atom(1),
            Vector { dx: 1.0, dy: 0.0 }
        )
        .unwrap_err(),
        TemplateError::Fragment(FragmentError::Invalid(EditError::IdentifierSpaceExhausted))
    );
    assert_eq!(dest.to_parts(), before);
}

#[test]
fn a_tetrahedral_source_anchor_is_refused_but_a_neighbor_anchor_preserves_that_statement() {
    use chemifree_domain::stereo::{AtomConfiguration, SlotRef, Turn};
    let mut source = drawing("center-source");
    source
        .apply(&[
            add("C", 0.0, 0.0),
            add("F", 30.0, 0.0),
            add("Cl", 0.0, 30.0),
            add("Br", -30.0, 0.0),
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
            Command::SetAtomConfiguration {
                atom: AtomRef::Id(atom(1)),
                configuration: Some(AtomConfiguration::Tetrahedral {
                    neighbors: [
                        SlotRef::Atom(AtomRef::Id(atom(2))),
                        SlotRef::Atom(AtomRef::Id(atom(3))),
                        SlotRef::Atom(AtomRef::Id(atom(4))),
                        SlotRef::ImplicitHydrogen,
                    ],
                    turn: Turn::Clockwise,
                }),
            },
        ])
        .unwrap();
    let fragment = copy_fragment(
        &source,
        &Selection {
            objects: BTreeSet::new(),
            atoms: (1..=4).map(atom).collect(),
            bonds: BTreeSet::new(),
        },
        source.first_instance(),
    )
    .unwrap()
    .fragment;
    let mut dest = destination();
    assert_eq!(
        template::attach(
            &fragment,
            &dest,
            dest.first_instance(),
            atom(1),
            atom(1),
            Vector { dx: 1.0, dy: 0.0 }
        )
        .unwrap_err(),
        TemplateError::AnchorStereo
    );
    let plan = template::attach(
        &fragment,
        &dest,
        dest.first_instance(),
        atom(2),
        atom(1),
        Vector { dx: 1.0, dy: 0.0 },
    )
    .unwrap();
    dest.apply(&plan.paste.commands).unwrap();
    assert_eq!(dest.to_parts().stereo.len(), 1);
    assert_eq!(dest.to_parts().stereo[0].0, StereoKey::Center(atom(2)));
}

#[test]
fn direction_scaling_and_coordinate_limits_do_not_change_the_source_or_reserve_ids() {
    let (_, fragment) = source();
    let mut dest = destination();
    let before = dest.to_parts();
    for dx in [f64::MAX, f64::from_bits(1)] {
        let plan = template::attach(
            &fragment,
            &dest,
            dest.first_instance(),
            atom(1),
            atom(1),
            Vector { dx, dy: 0.0 },
        )
        .unwrap();
        let mut candidate = dest.clone();
        candidate.apply(&plan.paste.commands).unwrap();
        assert_eq!(
            candidate.position(candidate.first_instance(), atom(2)),
            Some(point(40.0, 20.0))
        );
        assert_eq!(dest.to_parts(), before);
    }
    dest.apply(&[Command::MoveAtoms {
        atoms: vec![atom(1)],
        instance: None,
        delta: Vector {
            dx: 99980.0,
            dy: 0.0,
        },
    }])
    .unwrap();
    let before = dest.to_parts();
    assert_eq!(
        template::attach(
            &fragment,
            &dest,
            dest.first_instance(),
            atom(1),
            atom(1),
            Vector { dx: 1.0, dy: 0.0 }
        )
        .unwrap_err(),
        TemplateError::Fragment(FragmentError::Invalid(EditError::CoordinateOutOfRange))
    );
    assert_eq!(dest.to_parts(), before);
}
