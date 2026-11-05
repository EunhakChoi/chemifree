//! Geometry-only rigid alignment/distribution, including hierarchical selection and history.

use std::collections::BTreeSet;

use chemifree_domain::document::{
    Atom, AtomId, AtomRef, BondOrder, Command, Document, DocumentId, EditError, NewAtomPositions,
    ObjectId, Outcome, Point, Vector,
};
use chemifree_domain::element::Element;
use chemifree_domain::scene::{
    AlignMode, Bounds, DistributionAxis, Graphic, GraphicShape, ObjectArrangement, ObjectRef,
    ReferenceTarget, SceneObject, SceneReference, Selection, SelectionRequest,
    TransactionSelection,
};
use chemifree_domain::stereo::{AtomConfiguration, SlotRef, Turn};

fn object(serial: u32) -> ObjectId {
    ObjectId::from_serial(serial).unwrap()
}
fn atom(serial: u32) -> AtomId {
    AtomId::from_serial(serial).unwrap()
}
fn point(x: f64, y: f64) -> Point {
    Point::new(x, y).unwrap()
}
fn ids(values: &[u32]) -> BTreeSet<ObjectId> {
    values.iter().copied().map(object).collect()
}
fn empty() -> Document {
    Document::new(DocumentId::parse("arrangement").unwrap(), "Rigid objects").unwrap()
}
fn graphic(left: f64, top: f64, right: f64, bottom: f64) -> Command {
    Command::AddGraphic {
        graphic: Graphic {
            shape: GraphicShape::Rectangle,
            points: [point(left, top), point(right, bottom)],
        },
        parent: None,
        index: None,
    }
}
fn arrange(objects: &[u32], arrangement: ObjectArrangement) -> Command {
    Command::ArrangeObjects {
        objects: objects.iter().copied().map(object).collect(),
        arrangement,
    }
}
fn bounds(document: &Document, id: u32) -> Bounds {
    match document.object(object(id)).unwrap() {
        SceneObject::Graphic(graphic) => {
            let [a, b] = graphic.points;
            Bounds {
                left: a.x().min(b.x()),
                right: a.x().max(b.x()),
                top: a.y().min(b.y()),
                bottom: a.y().max(b.y()),
            }
        }
        other => panic!("s{id} is not a graphic: {other:?}"),
    }
}
fn drawings(rectangles: &[(f64, f64, f64, f64)]) -> Document {
    let mut document = empty();
    document
        .apply(
            &rectangles
                .iter()
                .map(|(left, top, right, bottom)| graphic(*left, *top, *right, *bottom))
                .collect::<Vec<_>>(),
        )
        .unwrap();
    document
}

#[test]
fn all_six_alignments_use_union_bounds_and_preserve_each_unit_shape() {
    for (mode, deltas) in [
        (AlignMode::Left, [(0.0, 0.0), (-30.0, 0.0)]),
        (AlignMode::HorizontalCenter, [(20.0, 0.0), (-15.0, 0.0)]),
        (AlignMode::Right, [(40.0, 0.0), (0.0, 0.0)]),
        (AlignMode::Top, [(0.0, 0.0), (0.0, -40.0)]),
        (AlignMode::VerticalCenter, [(0.0, 20.0), (0.0, -20.0)]),
        (AlignMode::Bottom, [(0.0, 40.0), (0.0, 0.0)]),
    ] {
        let mut document = drawings(&[
            (0.0, 0.0, 10.0, 20.0),
            (30.0, 40.0, 50.0, 60.0),
            (80.0, 80.0, 90.0, 90.0),
        ]);
        let before = document.clone();
        let plan = document
            .object_arrangement(&ids(&[3, 2]), ObjectArrangement::Align(mode))
            .unwrap();
        assert_eq!(document, before, "planning is read-only");
        assert_eq!(
            plan.units
                .iter()
                .map(|unit| unit.object)
                .collect::<Vec<_>>(),
            vec![object(2), object(3)]
        );
        assert_eq!(
            plan.units
                .iter()
                .map(|unit| (unit.delta.dx, unit.delta.dy))
                .collect::<Vec<_>>(),
            deltas,
            "{mode:?}"
        );
        document
            .apply(&[arrange(&[2, 3], ObjectArrangement::Align(mode))])
            .unwrap();
        for (serial, (dx, dy)) in [(2, deltas[0]), (3, deltas[1])] {
            let original = bounds(&before, serial);
            assert_eq!(
                bounds(&document, serial),
                Bounds {
                    left: original.left + dx,
                    right: original.right + dx,
                    top: original.top + dy,
                    bottom: original.bottom + dy
                },
                "{mode:?},s{serial}"
            );
        }
        assert_eq!(
            document.object(object(4)),
            before.object(object(4)),
            "unselected objects stay untouched"
        );
    }
}

#[test]
fn distribution_equalizes_actual_gaps_for_unequal_extents_and_keeps_endpoints_fixed() {
    for axis in [DistributionAxis::Horizontal, DistributionAxis::Vertical] {
        let rectangles = match axis {
            DistributionAxis::Horizontal => vec![
                (0.0, 4.0, 10.0, 9.0),
                (20.0, 7.0, 40.0, 12.0),
                (90.0, 10.0, 100.0, 15.0),
            ],
            DistributionAxis::Vertical => vec![
                (4.0, 0.0, 9.0, 10.0),
                (7.0, 20.0, 12.0, 40.0),
                (10.0, 90.0, 15.0, 100.0),
            ],
        };
        let mut document = drawings(&rectangles);
        let before = document.clone();
        let plan = document
            .object_arrangement(&ids(&[4, 2, 3]), ObjectArrangement::Distribute(axis))
            .unwrap();
        assert_eq!(
            plan.units
                .iter()
                .map(|unit| unit.object)
                .collect::<Vec<_>>(),
            vec![object(2), object(3), object(4)]
        );
        assert_eq!(plan.units[0].delta, Vector { dx: 0.0, dy: 0.0 });
        assert_eq!(plan.units[2].delta, Vector { dx: 0.0, dy: 0.0 });
        assert_eq!(
            plan.units[1].delta,
            match axis {
                DistributionAxis::Horizontal => Vector { dx: 20.0, dy: 0.0 },
                DistributionAxis::Vertical => Vector { dx: 0.0, dy: 20.0 },
            }
        );
        document
            .apply(&[arrange(&[4, 3, 2], ObjectArrangement::Distribute(axis))])
            .unwrap();
        let (a, b, c) = (
            bounds(&document, 2),
            bounds(&document, 3),
            bounds(&document, 4),
        );
        match axis {
            DistributionAxis::Horizontal => {
                assert_eq!(b.left - a.right, 30.0);
                assert_eq!(c.left - b.right, 30.0);
                assert_eq!(b.top, 7.0);
            }
            DistributionAxis::Vertical => {
                assert_eq!(b.top - a.bottom, 30.0);
                assert_eq!(c.top - b.bottom, 30.0);
                assert_eq!(b.left, 7.0);
            }
        }
        assert_eq!(document.object(object(2)), before.object(object(2)));
        assert_eq!(document.object(object(4)), before.object(object(4)));
    }
}

#[test]
fn tied_leading_bounds_use_identifier_order_even_with_signed_zero_and_initial_overlap() {
    let mut document = drawings(&[
        (0.0, 0.0, 5.0, 1.0),
        (-0.0, 2.0, 10.0, 3.0),
        (30.0, 4.0, 35.0, 5.0),
    ]);
    let plan = document
        .object_arrangement(
            &ids(&[4, 3, 2]),
            ObjectArrangement::Distribute(DistributionAxis::Horizontal),
        )
        .unwrap();
    assert_eq!(
        plan.units
            .iter()
            .map(|unit| unit.object)
            .collect::<Vec<_>>(),
        vec![object(2), object(3), object(4)]
    );
    assert_eq!(plan.units[1].delta.dx, 12.5);
    document
        .apply(&[arrange(
            &[3, 4, 2],
            ObjectArrangement::Distribute(DistributionAxis::Horizontal),
        )])
        .unwrap();
    assert_eq!(bounds(&document, 3).left, 12.5);
    assert_eq!(bounds(&document, 3).right, 22.5);
    assert_eq!(bounds(&document, 2).right, 5.0);
    assert_eq!(bounds(&document, 4).left, 30.0);
}

/// Group s5 contains molecular instance s1 and graphic s2; s3 is another unit and
/// instance s4 is outside the selection. A reference and stored tetrahedral state survive.
fn grouped() -> Document {
    let mut document = empty();
    let mut commands = Vec::new();
    for (element, x, y) in [
        ("C", 0.0, 0.0),
        ("F", 10.0, 0.0),
        ("Cl", 0.0, 10.0),
        ("Br", 10.0, 10.0),
    ] {
        commands.push(Command::AddAtom {
            atom: Atom::from(Element::from_symbol(element).unwrap()),
            positions: NewAtomPositions::Single(point(x, y)),
            bond_to: None,
        });
    }
    for end in 1..4 {
        commands.push(Command::AddBond {
            begin: AtomRef::Created(0),
            end: AtomRef::Created(end),
            order: BondOrder::Single,
        });
    }
    commands.extend([
        Command::SetAtomConfiguration {
            atom: AtomRef::Created(0),
            configuration: Some(AtomConfiguration::Tetrahedral {
                neighbors: [
                    SlotRef::Atom(AtomRef::Created(1)),
                    SlotRef::Atom(AtomRef::Created(2)),
                    SlotRef::Atom(AtomRef::Created(3)),
                    SlotRef::ImplicitHydrogen,
                ],
                turn: Turn::Clockwise,
            }),
        },
        graphic(20.0, 20.0, 30.0, 20.0),
        graphic(100.0, 80.0, 120.0, 90.0),
        Command::AddInstance {
            source: Some(object(1)),
            offset: Vector { dx: 50.0, dy: 50.0 },
        },
        Command::Group {
            objects: vec![object(1), object(2)],
        },
        Command::AddReference {
            reference: SceneReference {
                source: object(2),
                target: ReferenceTarget::Atom(atom(1)),
            },
        },
    ]);
    document.apply(&commands).unwrap();
    document
}

#[test]
fn ancestor_selected_descendants_are_one_rigid_unit_and_do_not_move_other_placements() {
    let mut document = grouped();
    let before = document.clone();
    let plan = document
        .object_arrangement(
            &ids(&[5, 2, 1, 3]),
            ObjectArrangement::Align(AlignMode::Right),
        )
        .unwrap();
    assert_eq!(
        plan.units
            .iter()
            .map(|unit| unit.object)
            .collect::<Vec<_>>(),
        vec![object(3), object(5)]
    );
    assert_eq!(
        plan.units[1].bounds,
        Bounds {
            left: 0.0,
            top: 0.0,
            right: 30.0,
            bottom: 20.0
        }
    );
    assert_eq!(plan.units[1].delta, Vector { dx: 90.0, dy: 0.0 });
    assert_eq!(plan.scope.objects, ids(&[1, 2, 3, 5]));
    assert_eq!(plan.scope.positions.len(), 4);
    assert_eq!(plan.scope.graphics, ids(&[2, 3]));
    document
        .apply(&[arrange(
            &[5, 2, 1, 3, 5],
            ObjectArrangement::Align(AlignMode::Right),
        )])
        .unwrap();
    for serial in 1..=4 {
        let original = before.position(object(1), atom(serial)).unwrap();
        assert_eq!(
            document.position(object(1), atom(serial)),
            Some(point(original.x() + 90.0, original.y()))
        );
        assert_eq!(
            document.position(object(4), atom(serial)),
            before.position(object(4), atom(serial))
        );
    }
    assert_eq!(
        bounds(&document, 2),
        Bounds {
            left: 110.0,
            top: 20.0,
            right: 120.0,
            bottom: 20.0
        }
    );
    assert_eq!(document.roots(), before.roots());
    assert_eq!(
        document
            .scene_objects()
            .iter()
            .map(|(id, parent, value)| (*id, *parent, value.kind()))
            .collect::<Vec<_>>(),
        before
            .scene_objects()
            .iter()
            .map(|(id, parent, value)| (*id, *parent, value.kind()))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        document.references().collect::<Vec<_>>(),
        before.references().collect::<Vec<_>>()
    );
    assert_eq!(document.stereo(), before.stereo());
    assert_eq!(
        document.atoms().collect::<Vec<_>>(),
        before.atoms().collect::<Vec<_>>()
    );
    assert_eq!(
        document.bonds().collect::<Vec<_>>(),
        before.bonds().collect::<Vec<_>>()
    );
}

#[test]
fn empty_geometry_unknown_objects_and_insufficient_units_have_explicit_errors() {
    let document = grouped();
    assert_eq!(
        document
            .object_arrangement(&ids(&[5, 1, 2]), ObjectArrangement::Align(AlignMode::Left))
            .unwrap_err(),
        EditError::ArrangementTooFewUnits {
            required: 2,
            found: 1
        }
    );
    assert_eq!(
        document
            .object_arrangement(&BTreeSet::new(), ObjectArrangement::Align(AlignMode::Left))
            .unwrap_err(),
        EditError::ArrangementTooFewUnits {
            required: 2,
            found: 0
        }
    );
    assert_eq!(
        document
            .object_arrangement(
                &ids(&[3, 5]),
                ObjectArrangement::Distribute(DistributionAxis::Vertical)
            )
            .unwrap_err(),
        EditError::ArrangementTooFewUnits {
            required: 3,
            found: 2
        }
    );
    assert_eq!(
        document
            .object_arrangement(&ids(&[999]), ObjectArrangement::Align(AlignMode::Left))
            .unwrap_err(),
        EditError::UnknownObject(object(999))
    );
    let document = drawings(&[(0.0, 0.0, 5.0, 5.0)]);
    assert_eq!(
        document
            .object_arrangement(&ids(&[1, 2]), ObjectArrangement::Align(AlignMode::Left))
            .unwrap_err(),
        EditError::ArrangementEmptyGeometry(object(1))
    );
    assert_eq!(
        EditError::ArrangementNegativeGap.reason(),
        Some("arrangement_negative_gap")
    );
    let mut document = document;
    let before = document.clone();
    let error = document
        .apply(&[arrange(&[], ObjectArrangement::Align(AlignMode::Left))])
        .unwrap_err();
    assert_eq!(
        error.error,
        EditError::ArrangementTooFewUnits {
            required: 2,
            found: 0
        }
    );
    assert_eq!(document, before);
}

#[test]
fn impossible_nonnegative_distribution_refuses_atomically_including_earlier_commands() {
    let mut document = drawings(&[
        (0.0, 0.0, 20.0, 10.0),
        (10.0, 20.0, 30.0, 30.0),
        (20.0, 40.0, 40.0, 50.0),
    ]);
    let before = document.clone();
    let arrangement = ObjectArrangement::Distribute(DistributionAxis::Horizontal);
    assert_eq!(
        document
            .object_arrangement(&ids(&[2, 3, 4]), arrangement)
            .unwrap_err(),
        EditError::ArrangementNegativeGap
    );
    let error = document
        .apply(&[
            Command::Rename {
                title: "Must roll back".into(),
            },
            arrange(&[2, 3, 4], arrangement),
        ])
        .unwrap_err();
    assert_eq!(
        (error.index, error.error),
        (Some(1), EditError::ArrangementNegativeGap)
    );
    assert_eq!(
        document, before,
        "entire transaction, history and selection must roll back"
    );
}

#[test]
fn arrangement_is_one_history_step_with_selection_undo_redo_and_zero_change_retains_redo() {
    let mut document = drawings(&[(0.0, 0.0, 10.0, 10.0), (30.0, 20.0, 40.0, 30.0)]);
    let before_points = [bounds(&document, 2), bounds(&document, 3)];
    let depth = document.undo_depth();
    let revision = document.revision();
    let selected = Selection {
        objects: ids(&[2, 3]),
        ..Selection::default()
    };
    let after = Selection {
        objects: ids(&[3]),
        ..Selection::default()
    };
    document
        .apply_selecting(
            &[arrange(&[2, 3], ObjectArrangement::Align(AlignMode::Left))],
            &TransactionSelection {
                before: Some(selected.clone()),
                after: Some(SelectionRequest {
                    objects: vec![ObjectRef::Id(object(3))],
                    ..SelectionRequest::default()
                }),
            },
        )
        .unwrap();
    assert_eq!(document.undo_depth(), depth + 1);
    assert_eq!(document.revision(), revision + 1);
    assert_eq!(document.selection(), &after);
    let moved = [bounds(&document, 2), bounds(&document, 3)];
    document.undo().unwrap();
    assert_eq!([bounds(&document, 2), bounds(&document, 3)], before_points);
    assert_eq!(document.selection(), &selected);
    document.redo().unwrap();
    assert_eq!([bounds(&document, 2), bounds(&document, 3)], moved);
    assert_eq!(document.selection(), &after);
    let unchanged = document.clone();
    assert_eq!(
        document
            .apply(&[arrange(&[2, 3], ObjectArrangement::Align(AlignMode::Left))])
            .unwrap(),
        Outcome::Unchanged
    );
    assert_eq!(
        document, unchanged,
        "already aligned geometry is a true no-op"
    );
    document
        .apply(&[Command::Rename {
            title: "Undo me".into(),
        }])
        .unwrap();
    document.undo().unwrap();
    let before_noop = document.clone();
    assert_eq!(
        document
            .apply(&[arrange(&[2, 3], ObjectArrangement::Align(AlignMode::Left))])
            .unwrap(),
        Outcome::Unchanged
    );
    assert_eq!(document, before_noop, "no-op must preserve the redo branch");
}

#[test]
fn already_equal_distribution_is_noop_and_point_sized_units_have_real_zero_extents() {
    let mut document = drawings(&[
        (0.0, 0.0, 0.0, 0.0),
        (15.0, 5.0, 15.0, 5.0),
        (30.0, 10.0, 30.0, 10.0),
    ]);
    let before = document.clone();
    assert_eq!(
        document
            .apply(&[arrange(
                &[4, 2, 3],
                ObjectArrangement::Distribute(DistributionAxis::Horizontal)
            )])
            .unwrap(),
        Outcome::Unchanged
    );
    assert_eq!(document, before);
}

fn with_redo(mut document: Document) -> Document {
    document
        .apply(&[Command::Rename {
            title: "Create redo branch".into(),
        }])
        .unwrap();
    document.undo().unwrap();
    assert_eq!(document.redo_depth(), 1);
    document
}

#[test]
fn identical_decimal_centers_are_exact_noops_including_revision_selection_and_redo() {
    for mode in [AlignMode::HorizontalCenter, AlignMode::VerticalCenter] {
        let mut document = with_redo(drawings(&[(0.1, 0.1, 0.2, 0.2), (0.1, 0.1, 0.2, 0.2)]));
        let before = document.clone();
        let arrangement = ObjectArrangement::Align(mode);
        let plan = document
            .object_arrangement(&ids(&[2, 3]), arrangement)
            .unwrap();
        assert_eq!(
            plan.units.iter().map(|unit| unit.delta).collect::<Vec<_>>(),
            vec![Vector { dx: 0.0, dy: 0.0 }; 2],
            "{mode:?}"
        );
        assert_eq!(
            document.apply(&[arrange(&[2, 3], arrangement)]).unwrap(),
            Outcome::Unchanged
        );
        assert_eq!(
            document, before,
            "{mode:?}: no numerical drift, revision, selection or history change"
        );
    }
}

#[test]
fn touching_decimal_intervals_have_exact_zero_gap_and_preserve_redo() {
    for axis in [DistributionAxis::Horizontal, DistributionAxis::Vertical] {
        let rectangles = match axis {
            DistributionAxis::Horizontal => vec![
                (0.1, 0.0, 0.2, 1.0),
                (0.2, 2.0, 0.3, 3.0),
                (0.3, 4.0, 2.4, 5.0),
            ],
            DistributionAxis::Vertical => vec![
                (0.0, 0.1, 1.0, 0.2),
                (2.0, 0.2, 3.0, 0.3),
                (4.0, 0.3, 5.0, 2.4),
            ],
        };
        let mut document = with_redo(drawings(&rectangles));
        let before = document.clone();
        let arrangement = ObjectArrangement::Distribute(axis);
        let plan = document
            .object_arrangement(&ids(&[2, 3, 4]), arrangement)
            .unwrap();
        assert_eq!(
            plan.units.iter().map(|unit| unit.delta).collect::<Vec<_>>(),
            vec![Vector { dx: 0.0, dy: 0.0 }; 3],
            "{axis:?}"
        );
        assert_eq!(
            document.apply(&[arrange(&[2, 3, 4], arrangement)]).unwrap(),
            Outcome::Unchanged
        );
        assert_eq!(
            document, before,
            "{axis:?}: exact touching endpoints must not invent a negative gap or history entry"
        );
    }
}

#[test]
fn rounding_enclosures_do_not_hide_real_small_displacements_and_repeated_plan_is_noop() {
    let mut document = drawings(&[
        (0.1, 0.0, 0.1, 0.0),
        (0.200000000001, 1.0, 0.200000000001, 1.0),
        (0.3, 2.0, 0.3, 2.0),
    ]);
    let arrangement = ObjectArrangement::Distribute(DistributionAxis::Horizontal);
    let plan = document
        .object_arrangement(&ids(&[2, 3, 4]), arrangement)
        .unwrap();
    assert!(
        plan.units[1].delta.dx < -0.0000000000009,
        "a real displacement must survive the floating-rounding enclosure: {:?}",
        plan.units[1].delta
    );
    assert_eq!(
        document.apply(&[arrange(&[2, 3, 4], arrangement)]).unwrap(),
        Outcome::Changed
    );
    let mut document = with_redo(document);
    let before = document.clone();
    let repeated = document
        .object_arrangement(&ids(&[2, 3, 4]), arrangement)
        .unwrap();
    assert_eq!(repeated.units[1].delta, Vector { dx: 0.0, dy: 0.0 });
    assert_eq!(
        document.apply(&[arrange(&[2, 3, 4], arrangement)]).unwrap(),
        Outcome::Unchanged
    );
    assert_eq!(
        document, before,
        "repeating decimal distribution must preserve revision and redo"
    );
}

#[test]
fn truly_negative_space_is_refused_even_when_naive_large_gap_subtractions_cancel_it() {
    for trailing in [0.1 + 0.000000001, 0.1_f64.next_up()] {
        // The first unit fills the complete anchor span. Even the one-ULP width of
        // the middle unit cannot fit. Directly rounded adjacent-gap differences can
        // lose it against 100000; exact endpoint sums must retain its negative sign.
        let mut document = with_redo(drawings(&[
            (0.0, 0.0, 100_000.0, 1.0),
            (0.1, 2.0, trailing, 3.0),
            (100_000.0, 4.0, 100_000.0, 5.0),
        ]));
        let before = document.clone();
        let arrangement = ObjectArrangement::Distribute(DistributionAxis::Horizontal);
        assert_eq!(
            document
                .object_arrangement(&ids(&[2, 3, 4]), arrangement)
                .unwrap_err(),
            EditError::ArrangementNegativeGap,
            "trailing={trailing:?}"
        );
        let error = document
            .apply(&[arrange(&[2, 3, 4], arrangement)])
            .unwrap_err();
        assert_eq!(error.error, EditError::ArrangementNegativeGap);
        assert_eq!(
            document, before,
            "negative space must remain an atomic refusal, without epsilon clamping"
        );
    }
}

#[test]
fn edge_alignment_cancellation_lands_on_exact_anchor_and_repeating_preserves_redo() {
    for (mode, vertical, upper) in [
        (AlignMode::Left, false, false),
        (AlignMode::Right, false, true),
        (AlignMode::Top, true, false),
        (AlignMode::Bottom, true, true),
    ] {
        let (target, source) = if upper {
            (90718.90246803382, -70813.75800974434)
        } else {
            (-90718.90246803382, 70813.75800974434)
        };
        let endpoints = |anchor: f64| {
            let other = if upper { anchor - 4.0 } else { anchor + 4.0 };
            if vertical {
                (0.0, anchor, 2.0, other)
            } else {
                (anchor, 0.0, other, 2.0)
            }
        };
        let mut document = drawings(&[endpoints(target), endpoints(source)]);
        let expected = bounds(&document, 2);
        let arrangement = ObjectArrangement::Align(mode);
        assert_eq!(
            document.apply(&[arrange(&[2, 3], arrangement)]).unwrap(),
            Outcome::Changed
        );
        assert_eq!(
            bounds(&document, 3),
            expected,
            "{mode:?}: one rounded translation must place both endpoints at the exact target"
        );
        let mut document = with_redo(document);
        let before = document.clone();
        let plan = document
            .object_arrangement(&ids(&[2, 3]), arrangement)
            .unwrap();
        assert_eq!(
            plan.units
                .iter()
                .map(|unit| unit.translation)
                .collect::<Vec<_>>(),
            vec![None, None],
            "{mode:?}"
        );
        assert_eq!(
            document.apply(&[arrange(&[2, 3], arrangement)]).unwrap(),
            Outcome::Unchanged
        );
        assert_eq!(
            document, before,
            "{mode:?}: repeated alignment must retain revision, selection, history and redo"
        );
    }
}

#[test]
fn edge_alignment_keeps_real_one_ulp_displacements_on_both_axes() {
    for (mode, vertical, upper) in [
        (AlignMode::Left, false, false),
        (AlignMode::Right, false, true),
        (AlignMode::Top, true, false),
        (AlignMode::Bottom, true, true),
    ] {
        let target = -90718.90246803382_f64;
        let source = if upper {
            target.next_down()
        } else {
            target.next_up()
        };
        let endpoints = |anchor| {
            if vertical {
                (0.0, anchor, 0.0, anchor)
            } else {
                (anchor, 0.0, anchor, 0.0)
            }
        };
        let mut document = drawings(&[endpoints(target), endpoints(source)]);
        let expected = bounds(&document, 2);
        let arrangement = ObjectArrangement::Align(mode);
        assert_eq!(
            document.apply(&[arrange(&[2, 3], arrangement)]).unwrap(),
            Outcome::Changed,
            "{mode:?}: representable movement must not be hidden by a tolerance"
        );
        assert_eq!(bounds(&document, 3), expected, "{mode:?}");
    }
}

#[test]
fn anchored_translation_rounds_once_when_residual_decides_a_halfway_case() {
    // The last endpoint's exact result is 1 - 2^-54 - 2^-110, just below the
    // midpoint between 1 and its predecessor. Rounding the delta first loses
    // 2^-110 and incorrectly chooses the even value 1.
    let target = -2.0_f64.powi(-54);
    let source = 2.0_f64.powi(-110);
    let mut document = drawings(&[(target, 0.0, target, 0.0), (source, 0.0, 1.0, 0.0)]);
    assert_eq!(
        document
            .apply(&[arrange(&[2, 3], ObjectArrangement::Align(AlignMode::Left))])
            .unwrap(),
        Outcome::Changed
    );
    assert_eq!(
        bounds(&document, 3),
        Bounds {
            left: target,
            top: 0.0,
            right: 1.0_f64.next_down(),
            bottom: 0.0,
        },
        "the residual must choose the correctly rounded endpoint, not a twice-rounded delta"
    );
}
