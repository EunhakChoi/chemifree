//! Attached rings add a carbon cycle and one exocyclic single bond, atomically.

use chemifree_domain::document::{
    Atom, AtomId, BondOrder, Command, Document, DocumentId, EditError, NewAtomPositions, ObjectId,
    Point, RingSize, Vector,
};
use chemifree_domain::element::Element;
use chemifree_domain::settings::SettingsEdit;

fn drawing() -> Document {
    let mut document = Document::new(DocumentId::parse("attach-ring").unwrap(), "Attach").unwrap();
    document
        .apply(&[Command::AddAtom {
            atom: Atom::from(Element::CARBON),
            positions: NewAtomPositions::Single(Point::new(10.0, 20.0).unwrap()),
            bond_to: None,
        }])
        .unwrap();
    document
}

fn attach(size: RingSize, dx: f64, dy: f64) -> Command {
    Command::AttachRing {
        instance: ObjectId::from_serial(1).unwrap(),
        atom: AtomId::from_serial(1).unwrap(),
        direction: Vector { dx, dy },
        size,
    }
}

#[test]
fn attached_cycles_follow_direction_and_current_length_without_reusing_the_target() {
    for (size, count) in [(RingSize::Five, 5), (RingSize::Six, 6)] {
        for (dx, dy, ux, uy) in [
            (1.0, 0.0, 1.0, 0.0),
            (0.0, 1.0, 0.0, 1.0),
            (-1.0, 0.0, -1.0, 0.0),
            (0.0, -1.0, 0.0, -1.0),
            (3.0, 4.0, 0.6, 0.8),
        ] {
            let mut document = drawing();
            document
                .apply(&[Command::UpdateSettings(SettingsEdit {
                    bond_length_pt: Some(40.0),
                    ..SettingsEdit::default()
                })])
                .unwrap();
            let before = document.to_parts();
            document.apply(&[attach(size, dx, dy)]).unwrap();
            assert_eq!(document.atoms().len(), count + 1);
            assert_eq!(document.bonds().len(), count + 1);
            assert_eq!(
                document.atoms().next().map(|(_, atom)| atom),
                Some(&before.atoms[0].1)
            );
            let points: Vec<_> = document
                .positions(document.first_instance())
                .skip(1)
                .map(|(_, point)| point)
                .collect();
            // Independent regular-polygon trigonometry; production uses fixed radicals.
            let radius = 40.0 / (2.0 * (std::f64::consts::PI / count as f64).sin());
            for index in 0..count {
                let angle = 2.0 * std::f64::consts::PI * index as f64 / count as f64;
                let axial = 40.0 + radius - radius * angle.cos();
                let tangent = radius * angle.sin();
                assert!(
                    (points[index].x() - (10.0 + axial * ux + tangent * uy)).abs() < 1e-10,
                    "vertex {index} x"
                );
                assert!(
                    (points[index].y() - (20.0 + axial * uy - tangent * ux)).abs() < 1e-10,
                    "vertex {index} y"
                );
                let next = points[(index + 1) % count];
                assert!(
                    ((next.x() - points[index].x()).hypot(next.y() - points[index].y()) - 40.0)
                        .abs()
                        < 1e-10
                );
            }
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
            for index in 0..count {
                assert_eq!(edges[index], (ids[index], ids[(index + 1) % count]));
            }
            assert_eq!(edges[count], (AtomId::from_serial(1).unwrap(), ids[0]));
            let after = document.to_parts();
            assert_eq!(document.undo_depth(), 3);
            document.undo().unwrap();
            assert_eq!(document.to_parts().atoms, before.atoms);
            assert_eq!(document.to_parts().bonds, before.bonds);
            assert_eq!(document.to_parts().positions, before.positions);
            document.redo().unwrap();
            assert_eq!(document.to_parts().atoms, after.atoms);
            assert_eq!(document.to_parts().bonds, after.bonds);
            assert_eq!(document.to_parts().positions, after.positions);
            document.undo().unwrap();
            document.apply(&[attach(size, dx, dy)]).unwrap();
            assert!(
                document
                    .atoms()
                    .skip(1)
                    .all(|(id, _)| id.serial() > ids[count - 1].serial()),
                "fresh IDs after Undo"
            );
        }
    }
}

#[test]
fn finite_huge_and_subnormal_directions_normalize_without_overflow_or_underflow() {
    for magnitude in [f64::MAX, f64::from_bits(1)] {
        let mut document = drawing();
        document
            .apply(&[attach(RingSize::Six, magnitude, 0.0)])
            .unwrap();
        assert_eq!(
            document.position(document.first_instance(), AtomId::from_serial(2).unwrap()),
            Some(Point::new(40.0, 20.0).unwrap())
        );
    }
}

#[test]
fn invalid_direction_late_geometry_and_link_id_failures_preserve_redo_and_prior_commands() {
    for (dx, dy, next_bond, expected) in [
        (0.0, 0.0, None, EditError::RingDirectionInvalid),
        (f64::NAN, 1.0, None, EditError::RingDirectionInvalid),
        (f64::INFINITY, 1.0, None, EditError::RingDirectionInvalid),
        (
            1.0,
            0.0,
            Some(u32::MAX - 6),
            EditError::IdentifierSpaceExhausted,
        ),
    ] {
        let mut document = drawing();
        document.apply(&[attach(RingSize::Six, -1.0, 0.0)]).unwrap();
        document.undo().unwrap();
        if let Some(next_bond) = next_bond {
            let mut parts = document.to_parts();
            parts.next_bond = next_bond;
            document = Document::restore(parts).unwrap();
        }
        let before = document.to_parts();
        let error = document
            .apply(&[
                Command::Rename {
                    title: "Rollback".into(),
                },
                attach(RingSize::Six, dx, dy),
            ])
            .unwrap_err();
        assert_eq!(error.index, Some(1));
        assert_eq!(error.error, expected);
        assert_eq!(document.to_parts(), before);
    }
    let mut document = drawing();
    document
        .apply(&[Command::MoveAtoms {
            atoms: vec![AtomId::from_serial(1).unwrap()],
            instance: None,
            delta: Vector {
                dx: 99_950.0,
                dy: 0.0,
            },
        }])
        .unwrap();
    let before = document.to_parts();
    assert_eq!(
        document
            .apply(&[attach(RingSize::Six, 1.0, 0.0)])
            .unwrap_err()
            .error,
        EditError::CoordinateOutOfRange
    );
    assert_eq!(document.to_parts(), before);
}

#[test]
fn explicit_target_and_only_placement_are_required() {
    let mut document = drawing();
    let before = document.to_parts();
    let error = document
        .apply(&[Command::AttachRing {
            instance: document.first_instance(),
            atom: AtomId::from_serial(99).unwrap(),
            direction: Vector { dx: 1.0, dy: 0.0 },
            size: RingSize::Five,
        }])
        .unwrap_err();
    assert_eq!(
        error.error,
        EditError::UnknownAtom(AtomId::from_serial(99).unwrap())
    );
    assert_eq!(document.to_parts(), before);
    document
        .apply(&[Command::AddInstance {
            source: None,
            offset: Vector { dx: 10.0, dy: 0.0 },
        }])
        .unwrap();
    let before = document.to_parts();
    assert_eq!(
        document
            .apply(&[attach(RingSize::Five, 1.0, 0.0)])
            .unwrap_err()
            .error,
        EditError::PositionsRequired { instances: 2 }
    );
    assert_eq!(document.to_parts(), before);
}
