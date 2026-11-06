//! Independent ring insertion uses ordinary document history and refuses atomically.

use chemifree_domain::document::{
    Atom, AtomId, BondOrder, Command, Document, DocumentId, EditError, MAX_ATOMS, NewAtomPositions,
    ObjectId, Point, RingPattern, RingSize, Vector,
};
use chemifree_domain::element::Element;
use chemifree_domain::settings::SettingsEdit;

fn drawing() -> Document {
    Document::new(DocumentId::parse("ring-test").unwrap(), "Independent ring").unwrap()
}

fn ring(x: f64, y: f64) -> Command {
    Command::InsertRing {
        instance: ObjectId::from_serial(1).unwrap(),
        center: Point::new(x, y).unwrap(),
        size: RingSize::Six,
        pattern: RingPattern::Single,
    }
}

#[test]
fn ring_is_one_independent_six_cycle_with_current_bond_length_and_stable_history_ids() {
    let mut document = drawing();
    document
        .apply(&[
            Command::AddAtom {
                atom: Atom::from(Element::from_symbol("O").unwrap()),
                positions: NewAtomPositions::Single(Point::new(1.0, 2.0).unwrap()),
                bond_to: None,
            },
            Command::UpdateSettings(SettingsEdit {
                bond_length_pt: Some(40.0),
                ..SettingsEdit::default()
            }),
        ])
        .unwrap();
    let before = document.to_parts();
    document.apply(&[ring(100.0, 200.0)]).unwrap();
    let inserted = document.to_parts();
    assert_eq!(document.revision(), 2);
    assert_eq!(document.undo_depth(), 2);
    assert_eq!(document.atoms().len(), 7);
    let ids: Vec<_> = document
        .atoms()
        .skip(1)
        .map(|(id, atom)| {
            assert_eq!(atom, &Atom::from(Element::CARBON));
            id
        })
        .collect();
    let edges: Vec<_> = document
        .bonds()
        .map(|(_, bond)| {
            assert_eq!(bond.order, BondOrder::Single);
            assert_eq!(bond.mark, None);
            (bond.begin, bond.end)
        })
        .collect();
    assert_eq!(
        edges,
        vec![
            (ids[0], ids[1]),
            (ids[1], ids[2]),
            (ids[2], ids[3]),
            (ids[3], ids[4]),
            (ids[4], ids[5]),
            (ids[5], ids[0])
        ]
    );
    let points: Vec<_> = document
        .positions(document.first_instance())
        .skip(1)
        .map(|(_, point)| point)
        .collect();
    assert_eq!(points[0], Point::new(140.0, 200.0).unwrap());
    assert_eq!(points[3], Point::new(60.0, 200.0).unwrap());
    assert!((points[1].y() - 234.64101615137756).abs() < 1e-12);
    for index in 0..6 {
        let next = points[(index + 1) % 6];
        let length = (points[index].x() - next.x()).hypot(points[index].y() - next.y());
        assert!((length - 40.0).abs() < 1e-12, "edge {index}: {length}");
    }
    document.undo().unwrap();
    assert_eq!(document.to_parts().atoms, before.atoms);
    assert_eq!(document.to_parts().bonds, before.bonds);
    assert_eq!(document.to_parts().positions, before.positions);
    assert_eq!(document.selection(), &before.selection);
    document.redo().unwrap();
    assert_eq!(document.to_parts().atoms, inserted.atoms);
    assert_eq!(document.to_parts().bonds, inserted.bonds);
    assert_eq!(document.to_parts().positions, inserted.positions);
    document.undo().unwrap();
    document.apply(&[ring(0.0, 0.0)]).unwrap();
    assert!(
        document
            .atoms()
            .skip(1)
            .all(|(id, _)| id.serial() > ids[5].serial()),
        "Undo never recycles fresh IDs"
    );
}

#[test]
fn late_coordinate_failure_rolls_back_prior_commands_counters_and_redo() {
    for size in [RingSize::Five, RingSize::Six] {
        let insert = |x, y| Command::InsertRing {
            instance: ObjectId::from_serial(1).unwrap(),
            center: Point::new(x, y).unwrap(),
            size,
            pattern: RingPattern::Single,
        };
        let mut document = drawing();
        document.apply(&[insert(0.0, 0.0)]).unwrap();
        document.undo().unwrap();
        let before = document.to_parts();
        let error = document
            .apply(&[
                Command::Rename {
                    title: "Must roll back".into(),
                },
                insert(-99_990.0, 0.0),
            ])
            .unwrap_err();
        assert_eq!(error.index, Some(1));
        assert_eq!(error.error, EditError::CoordinateOutOfRange);
        assert_eq!(document.to_parts(), before);
    }
}

#[test]
fn multiple_or_missing_placements_are_refused_without_editing() {
    let mut document = drawing();
    let before = document.to_parts();
    let error = document
        .apply(&[Command::InsertRing {
            instance: ObjectId::from_serial(99).unwrap(),
            center: Point::new(0.0, 0.0).unwrap(),
            size: RingSize::Six,
            pattern: RingPattern::Single,
        }])
        .unwrap_err();
    assert_eq!(error.error.reason(), Some("object_unknown"));
    assert_eq!(document.to_parts(), before);
    document
        .apply(&[Command::AddInstance {
            source: None,
            offset: Vector { dx: 50.0, dy: 0.0 },
        }])
        .unwrap();
    let before = document.to_parts();
    assert_eq!(
        document.apply(&[ring(0.0, 0.0)]).unwrap_err().error,
        EditError::PositionsRequired { instances: 2 }
    );
    assert_eq!(document.to_parts(), before);
}

#[test]
fn atom_limit_and_identifier_exhaustion_do_not_leave_partial_rings() {
    let mut document = drawing();
    let atom = Command::AddAtom {
        atom: Atom::from(Element::CARBON),
        positions: NewAtomPositions::Single(Point::new(0.0, 0.0).unwrap()),
        bond_to: None,
    };
    for chunk in vec![atom; MAX_ATOMS - 5].chunks(500) {
        document.apply(chunk).unwrap();
    }
    let before = document.to_parts();
    assert_eq!(
        document.apply(&[ring(0.0, 0.0)]).unwrap_err().error,
        EditError::AtomLimit
    );
    assert_eq!(document.to_parts(), before);
    let mut parts = drawing().to_parts();
    parts.next_atom = u32::MAX - 2;
    let mut document = Document::restore(parts).unwrap();
    let before = document.to_parts();
    assert_eq!(
        document.apply(&[ring(0.0, 0.0)]).unwrap_err().error,
        EditError::IdentifierSpaceExhausted
    );
    assert_eq!(document.to_parts(), before);
}

#[test]
fn ring_command_does_not_invent_a_transaction_local_atom_reference() {
    let mut document = drawing();
    let before = document.to_parts();
    let error = document
        .apply(&[
            ring(0.0, 0.0),
            Command::AddBond {
                begin: chemifree_domain::document::AtomRef::Created(0),
                end: chemifree_domain::document::AtomRef::Id(AtomId::from_serial(1).unwrap()),
                order: BondOrder::Single,
            },
        ])
        .unwrap_err();
    assert_eq!(error.error, EditError::UnresolvedReference(0));
    assert_eq!(document.to_parts(), before);
}

#[test]
fn late_bond_identifier_exhaustion_rolls_back_all_new_atoms_and_bonds() {
    let mut parts = drawing().to_parts();
    parts.next_bond = u32::MAX - 2;
    let mut document = Document::restore(parts).unwrap();
    let before = document.to_parts();
    assert_eq!(
        document.apply(&[ring(0.0, 0.0)]).unwrap_err().error,
        EditError::IdentifierSpaceExhausted
    );
    assert_eq!(document.to_parts(), before);
}

#[test]
fn five_ring_has_independent_geometry_cycle_and_one_step_history() {
    let mut document = drawing();
    document
        .apply(&[Command::UpdateSettings(SettingsEdit {
            bond_length_pt: Some(60.0),
            ..SettingsEdit::default()
        })])
        .unwrap();
    let command = Command::InsertRing {
        instance: document.first_instance(),
        center: Point::new(100.0, 100.0).unwrap(),
        size: RingSize::Five,
        pattern: RingPattern::Single,
    };
    let before = document.to_parts();
    document.apply(std::slice::from_ref(&command)).unwrap();
    let inserted = document.to_parts();
    let ids: Vec<_> = document
        .atoms()
        .map(|(id, atom)| {
            assert_eq!(atom, &Atom::from(Element::CARBON));
            id
        })
        .collect();
    assert_eq!(ids.len(), 5);
    let points: Vec<_> = document
        .positions(document.first_instance())
        .map(|(_, point)| point)
        .collect();
    // Independent decimal coordinates for a 60 pt pentagon, not the recipe's radicals.
    let expected = [
        (151.0390485011224, 100.0),
        (115.771933363574, 148.54101966249684),
        (58.7085423858648, 130.0),
        (58.7085423858648, 70.0),
        (115.771933363574, 51.45898033750316),
    ];
    let bonds: Vec<_> = document.bonds().map(|(_, bond)| bond).collect();
    assert_eq!(bonds.len(), 5);
    for index in 0..5 {
        assert!(
            (points[index].x() - expected[index].0).abs() < 1e-10,
            "x vertex {index}"
        );
        assert!(
            (points[index].y() - expected[index].1).abs() < 1e-10,
            "y vertex {index}"
        );
        let next = points[(index + 1) % 5];
        let length = (points[index].x() - next.x()).hypot(points[index].y() - next.y());
        assert!((length - 60.0).abs() < 1e-10, "edge {index}: {length}");
        assert_eq!(
            (bonds[index].begin, bonds[index].end),
            (ids[index], ids[(index + 1) % 5])
        );
        assert_eq!(bonds[index].order, BondOrder::Single);
        assert_eq!(bonds[index].mark, None);
    }
    assert_eq!(document.undo_depth(), 2);
    document.undo().unwrap();
    assert_eq!(document.to_parts().positions, before.positions);
    assert_eq!(document.to_parts().atoms, before.atoms);
    assert_eq!(document.to_parts().bonds, before.bonds);
    document.redo().unwrap();
    assert_eq!(document.to_parts().positions, inserted.positions);
    assert_eq!(document.to_parts().atoms, inserted.atoms);
    assert_eq!(document.to_parts().bonds, inserted.bonds);
    document.undo().unwrap();
    document.apply(&[command]).unwrap();
    assert!(
        document
            .atoms()
            .all(|(id, _)| id.serial() > ids[4].serial()),
        "Undo must not recycle pentagon IDs"
    );
}

#[test]
fn five_ring_late_coordinate_and_id_failures_roll_back_whole_transaction() {
    for (x, next_atom, next_bond, expected) in [
        (-99_990.0, 1, 1, EditError::CoordinateOutOfRange),
        (0.0, u32::MAX - 2, 1, EditError::IdentifierSpaceExhausted),
        (0.0, 1, u32::MAX - 2, EditError::IdentifierSpaceExhausted),
    ] {
        let mut parts = drawing().to_parts();
        parts.next_atom = next_atom;
        parts.next_bond = next_bond;
        let mut document = Document::restore(parts).unwrap();
        let before = document.to_parts();
        let error = document
            .apply(&[
                Command::Rename {
                    title: "Must roll back".into(),
                },
                Command::InsertRing {
                    instance: document.first_instance(),
                    center: Point::new(x, 0.0).unwrap(),
                    size: RingSize::Five,
                    pattern: RingPattern::Single,
                },
            ])
            .unwrap_err();
        assert_eq!(error.index, Some(1));
        assert_eq!(error.error, expected);
        assert_eq!(document.to_parts(), before);
    }
}

#[test]
fn five_ring_respects_atom_budget_at_and_over_boundary() {
    let mut document = drawing();
    let atom = Command::AddAtom {
        atom: Atom::from(Element::CARBON),
        positions: NewAtomPositions::Single(Point::new(0.0, 0.0).unwrap()),
        bond_to: None,
    };
    for chunk in vec![atom; MAX_ATOMS - 5].chunks(500) {
        document.apply(chunk).unwrap();
    }
    let command = Command::InsertRing {
        instance: document.first_instance(),
        center: Point::new(0.0, 0.0).unwrap(),
        size: RingSize::Five,
        pattern: RingPattern::Single,
    };
    document.apply(std::slice::from_ref(&command)).unwrap();
    assert_eq!(document.atoms().len(), MAX_ATOMS);
    let before = document.to_parts();
    assert_eq!(
        document.apply(&[command]).unwrap_err().error,
        EditError::AtomLimit
    );
    assert_eq!(document.to_parts(), before);
}

#[test]
fn kekule_six_cycle_has_explicit_orders_and_ordinary_history() {
    let mut document = drawing();
    let command = Command::InsertRing {
        instance: document.first_instance(),
        center: Point::new(100.0, 200.0).unwrap(),
        size: RingSize::Six,
        pattern: RingPattern::Kekule,
    };
    let before = document.to_parts();
    document.apply(&[command]).unwrap();
    let inserted = document.to_parts();
    assert_eq!(document.atoms().len(), 6);
    assert!(
        document
            .atoms()
            .all(|(_, atom)| atom == &Atom::from(Element::CARBON))
    );
    let edges: Vec<_> = document
        .bonds()
        .map(|(_, bond)| {
            assert_eq!(bond.mark, None);
            (bond.begin.serial(), bond.end.serial(), bond.order.clone())
        })
        .collect();
    assert_eq!(
        edges,
        vec![
            (1, 2, BondOrder::Double),
            (2, 3, BondOrder::Single),
            (3, 4, BondOrder::Double),
            (4, 5, BondOrder::Single),
            (5, 6, BondOrder::Double),
            (6, 1, BondOrder::Single),
        ]
    );
    assert_eq!(
        document.position(document.first_instance(), AtomId::from_serial(1).unwrap()),
        Some(Point::new(130.0, 200.0).unwrap())
    );
    assert_eq!(
        document.position(document.first_instance(), AtomId::from_serial(4).unwrap()),
        Some(Point::new(70.0, 200.0).unwrap())
    );
    assert_eq!(document.undo_depth(), 1);
    document.undo().unwrap();
    assert_eq!(document.to_parts().atoms, before.atoms);
    assert_eq!(document.to_parts().bonds, before.bonds);
    document.redo().unwrap();
    assert_eq!(document.to_parts().atoms, inserted.atoms);
    assert_eq!(document.to_parts().bonds, inserted.bonds);
    assert_eq!(document.to_parts().positions, inserted.positions);
    assert_eq!(document.to_parts().stereo, inserted.stereo);
}

#[test]
fn kekule_refusals_rollback_prior_commands_counters_and_redo() {
    for (size, x, atom_serial, bond_serial, expected) in [
        (RingSize::Five, 0.0, 1, 1, EditError::RingPatternInvalid),
        (
            RingSize::Six,
            -99_990.0,
            1,
            1,
            EditError::CoordinateOutOfRange,
        ),
        (
            RingSize::Six,
            0.0,
            u32::MAX - 2,
            1,
            EditError::IdentifierSpaceExhausted,
        ),
        (
            RingSize::Six,
            0.0,
            1,
            u32::MAX - 2,
            EditError::IdentifierSpaceExhausted,
        ),
    ] {
        let mut parts = drawing().to_parts();
        parts.next_atom = atom_serial;
        parts.next_bond = bond_serial;
        let mut document = Document::restore(parts).unwrap();
        document
            .apply(&[Command::Rename {
                title: "Temporary".into(),
            }])
            .unwrap();
        document.undo().unwrap();
        let before = document.to_parts();
        let refused = document
            .apply(&[
                Command::Rename {
                    title: "Must roll back".into(),
                },
                Command::InsertRing {
                    instance: document.first_instance(),
                    center: Point::new(x, 0.0).unwrap(),
                    size,
                    pattern: RingPattern::Kekule,
                },
            ])
            .unwrap_err();
        assert_eq!(refused.index, Some(1));
        assert_eq!(refused.error, expected);
        assert_eq!(document.to_parts(), before);
    }
}

#[test]
fn kekule_requires_one_existing_placement_and_respects_atom_budget() {
    let mut document = drawing();
    let atom = Command::AddAtom {
        atom: Atom::from(Element::CARBON),
        positions: NewAtomPositions::Single(Point::new(0.0, 0.0).unwrap()),
        bond_to: None,
    };
    for chunk in vec![atom; MAX_ATOMS - 6].chunks(500) {
        document.apply(chunk).unwrap();
    }
    let command = Command::InsertRing {
        instance: document.first_instance(),
        center: Point::new(0.0, 0.0).unwrap(),
        size: RingSize::Six,
        pattern: RingPattern::Kekule,
    };
    document.apply(std::slice::from_ref(&command)).unwrap();
    assert_eq!(document.atoms().len(), MAX_ATOMS);
    let before = document.to_parts();
    assert_eq!(
        document.apply(&[command]).unwrap_err().error,
        EditError::AtomLimit
    );
    assert_eq!(document.to_parts(), before);

    let mut document = drawing();
    let missing = Command::InsertRing {
        instance: ObjectId::from_serial(99).unwrap(),
        center: Point::new(0.0, 0.0).unwrap(),
        size: RingSize::Six,
        pattern: RingPattern::Kekule,
    };
    let before = document.to_parts();
    assert_eq!(
        document.apply(&[missing]).unwrap_err().error.reason(),
        Some("object_unknown")
    );
    assert_eq!(document.to_parts(), before);
    document
        .apply(&[Command::AddInstance {
            source: None,
            offset: Vector { dx: 10.0, dy: 0.0 },
        }])
        .unwrap();
    let before = document.to_parts();
    let command = Command::InsertRing {
        instance: document.first_instance(),
        center: Point::new(0.0, 0.0).unwrap(),
        size: RingSize::Six,
        pattern: RingPattern::Kekule,
    };
    assert_eq!(
        document.apply(&[command]).unwrap_err().error,
        EditError::PositionsRequired { instances: 2 }
    );
    assert_eq!(document.to_parts(), before);
}
