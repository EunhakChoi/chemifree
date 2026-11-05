//! Geometry-only rotation/reflection: exact target scope, representability, atomicity/history.

use std::collections::BTreeSet;

use chemifree_domain::document::{
    Atom, AtomId, AtomRef, BondId, BondOrder, BondRef, Command, Document, DocumentId, EditError,
    MAX_COORDINATE, NewAtomPositions, ObjectId, Outcome, Point, Vector,
};
use chemifree_domain::element::Element;
use chemifree_domain::scene::{
    Graphic, GraphicShape, ObjectRef, ReferenceTarget, SceneObject, SceneReference, Selection,
    SelectionRequest, SelectionTransform, TransactionSelection,
};
use chemifree_domain::stereo::{AtomConfiguration, BondEnd, SlotRef, StereoMark, Turn};
use chemifree_domain::stereo_group::GroupKind;

fn atom(serial: u32) -> AtomId {
    AtomId::from_serial(serial).unwrap()
}
fn object(serial: u32) -> ObjectId {
    ObjectId::from_serial(serial).unwrap()
}
fn bond(serial: u32) -> BondId {
    BondId::from_serial(serial).unwrap()
}
fn point(x: f64, y: f64) -> Point {
    Point::new(x, y).unwrap()
}
fn ids(values: &[u32]) -> BTreeSet<ObjectId> {
    values.iter().copied().map(object).collect()
}
fn selection(objects: &[u32], atoms: &[u32], bonds: &[u32]) -> Selection {
    Selection {
        objects: ids(objects),
        atoms: atoms.iter().copied().map(atom).collect(),
        bonds: bonds.iter().copied().map(bond).collect(),
    }
}
fn command(
    selected: Selection,
    instance: Option<u32>,
    pivot: Point,
    transform: SelectionTransform,
) -> Command {
    Command::TransformSelection {
        selection: selected,
        instance: instance.map(object),
        pivot,
        transform,
    }
}
fn rotate(degrees: f64) -> SelectionTransform {
    SelectionTransform::Rotate { degrees }
}
fn add_graphic(shape: GraphicShape, points: [Point; 2]) -> Command {
    Command::AddGraphic {
        graphic: Graphic { shape, points },
        parent: None,
        index: None,
    }
}
fn empty() -> Document {
    Document::new(
        DocumentId::parse("selection-transform").unwrap(),
        "Geometry transform",
    )
    .unwrap()
}

/// s6 contains primary instance s1, line s2, rectangle s3 and arrow s4. Secondary
/// placement s5 is outside the group. Stored stereo, its wedge and absolute group coexist.
fn drawing() -> Document {
    let mut document = empty();
    let mut commands = Vec::new();
    for (symbol, x, y) in [
        ("C", 0.0, 0.0),
        ("F", 0.0, -3.0),
        ("Cl", 2.6, 1.5),
        ("Br", -2.6, 1.5),
    ] {
        commands.push(Command::AddAtom {
            atom: Atom::from(Element::from_symbol(symbol).unwrap()),
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
        Command::SetBondMark {
            bond: BondRef::Created(5),
            mark: Some(StereoMark::Wedge {
                narrow: BondEnd::Begin,
            }),
        },
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
        Command::CreateStereoGroup {
            kind: GroupKind::Absolute,
            number: None,
            atoms: vec![AtomRef::Created(0)],
        },
        add_graphic(GraphicShape::Line, [point(2.0, 1.0), point(4.0, 1.0)]),
        add_graphic(GraphicShape::Rectangle, [point(1.0, 1.0), point(3.0, 2.0)]),
        add_graphic(GraphicShape::Arrow, [point(5.0, 1.0), point(6.0, 2.0)]),
        Command::AddInstance {
            source: Some(object(1)),
            offset: Vector { dx: 100.0, dy: 0.0 },
        },
        Command::Group {
            objects: vec![object(1), object(2), object(3), object(4)],
        },
        Command::AddReference {
            reference: SceneReference {
                source: object(2),
                target: ReferenceTarget::Bond(bond(1)),
            },
        },
    ]);
    document.apply(&commands).unwrap();
    document
}

fn positions(document: &Document) -> Vec<(ObjectId, AtomId, Point)> {
    document
        .instances()
        .flat_map(|instance| {
            document
                .positions(instance)
                .map(move |(atom, position)| (instance, atom, position))
        })
        .collect()
}
fn graphics(document: &Document) -> Vec<(ObjectId, Graphic)> {
    document
        .scene_objects()
        .into_iter()
        .filter_map(|(id, _, value)| match value {
            SceneObject::Graphic(graphic) => Some((id, graphic.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn overlapping_group_child_atom_and_bond_targets_rotate_once_with_correct_rectangle_corners() {
    let mut document = drawing();
    let before = document.clone();
    let selected = selection(&[6, 1, 2, 3, 4], &[1, 2], &[1, 2]);
    let plan = document
        .selection_transform(&selected, Some(object(1)), point(0.0, 0.0), rotate(90.0))
        .unwrap();
    assert_eq!(document, before, "plan is read-only");
    assert_eq!(plan.scope.objects, ids(&[1, 2, 3, 4, 6]));
    assert_eq!(plan.scope.positions.len(), 4);
    assert_eq!(plan.scope.graphics, ids(&[2, 3, 4]));
    assert!(plan.drawing_stereo_may_change);
    for position in &plan.positions {
        assert_eq!(
            position.after,
            point(-position.before.y(), position.before.x())
        );
    }
    document
        .apply(&[command(selected, Some(1), point(0.0, 0.0), rotate(90.0))])
        .unwrap();
    for serial in 1..=4 {
        let original = before.position(object(1), atom(serial)).unwrap();
        assert_eq!(
            document.position(object(1), atom(serial)),
            Some(point(-original.y(), original.x()))
        );
        assert_eq!(
            document.position(object(5), atom(serial)),
            before.position(object(5), atom(serial))
        );
    }
    assert_eq!(
        document.object(object(3)),
        Some(&SceneObject::Graphic(Graphic {
            shape: GraphicShape::Rectangle,
            points: [point(-1.0, 1.0), point(-2.0, 3.0)]
        })),
        "opposite-corner identities are transformed directly, not replaced by an enclosing AABB"
    );
    assert_eq!(
        document.object(object(4)),
        Some(&SceneObject::Graphic(Graphic {
            shape: GraphicShape::Arrow,
            points: [point(-1.0, 5.0), point(-2.0, 6.0)]
        })),
        "arrow endpoint order remains intact"
    );
}

#[test]
fn explicit_bond_selection_uses_just_its_endpoints_in_the_named_placement() {
    let mut document = drawing();
    let before = document.clone();
    let selected = selection(&[], &[1], &[1, 2]);
    let plan = document
        .selection_transform(&selected, Some(object(5)), point(100.0, 0.0), rotate(180.0))
        .unwrap();
    assert_eq!(plan.positions.len(), 3);
    assert!(plan.graphics.is_empty());
    assert!(plan.scope.objects.is_empty());
    document
        .apply(&[command(selected, Some(5), point(100.0, 0.0), rotate(180.0))])
        .unwrap();
    assert_eq!(
        document.position(object(5), atom(2)),
        Some(point(100.0, 3.0))
    );
    assert_eq!(
        document.position(object(5), atom(3)),
        Some(point(97.4, -1.5))
    );
    assert_eq!(
        document.position(object(5), atom(4)),
        before.position(object(5), atom(4))
    );
    assert_eq!(
        document.positions(object(1)).collect::<Vec<_>>(),
        before.positions(object(1)).collect::<Vec<_>>()
    );
    assert_eq!(graphics(&document), graphics(&before));
}

#[test]
fn axis_reflections_use_explicit_pivot_lines_and_preserve_all_stored_chemical_and_scene_state() {
    for transform in [
        SelectionTransform::ReflectHorizontal,
        SelectionTransform::ReflectVertical,
    ] {
        let mut document = drawing();
        let before = document.clone();
        let pivot = point(0.5, 1.25);
        let selected = selection(&[6], &[], &[]);
        let plan = document
            .selection_transform(&selected, None, pivot, transform)
            .unwrap();
        assert!(plan.drawing_stereo_may_change);
        document
            .apply(&[command(selected, None, pivot, transform)])
            .unwrap();
        for serial in 1..=4 {
            let original = before.position(object(1), atom(serial)).unwrap();
            let expected = match transform {
                SelectionTransform::ReflectHorizontal => point(original.x(), 2.5 - original.y()),
                SelectionTransform::ReflectVertical => point(1.0 - original.x(), original.y()),
                _ => unreachable!(),
            };
            assert_eq!(document.position(object(1), atom(serial)), Some(expected));
        }
        assert_eq!(
            document.atoms().collect::<Vec<_>>(),
            before.atoms().collect::<Vec<_>>()
        );
        assert_eq!(
            document.bonds().collect::<Vec<_>>(),
            before.bonds().collect::<Vec<_>>(),
            "wedge is not flipped"
        );
        assert_eq!(
            document.stereo(),
            before.stereo(),
            "stored turns are not mirrored or derived"
        );
        assert_eq!(document.stereo_groups(), before.stereo_groups());
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
    }
}

#[test]
fn arbitrary_rotations_support_atoms_lines_and_arrows_with_an_advisory_only_for_changed_graph_positions()
 {
    let mut document = drawing();
    let selected = selection(&[2, 4], &[2], &[]);
    let plan = document
        .selection_transform(&selected, Some(object(1)), point(0.0, 0.0), rotate(30.0))
        .unwrap();
    assert!(plan.drawing_stereo_may_change);
    let fluorine = plan
        .positions
        .iter()
        .find(|position| position.key.atom == atom(2))
        .unwrap();
    assert!((fluorine.after.x() - 1.5).abs() < 1e-14);
    assert!((fluorine.after.y() + 2.598076211353316).abs() < 1e-14);
    document
        .apply(&[command(selected, Some(1), point(0.0, 0.0), rotate(30.0))])
        .unwrap();
    assert_eq!(
        document.object(object(3)),
        Some(&SceneObject::Graphic(Graphic {
            shape: GraphicShape::Rectangle,
            points: [point(1.0, 1.0), point(3.0, 2.0)]
        }))
    );
    let only_graphics = document
        .selection_transform(
            &selection(&[2, 4], &[], &[]),
            None,
            point(0.0, 0.0),
            SelectionTransform::ReflectVertical,
        )
        .unwrap();
    assert!(!only_graphics.drawing_stereo_may_change);
}

#[test]
fn any_selected_rectangle_including_a_descendant_or_degenerate_shape_refuses_arbitrary_rotation_atomically()
 {
    let mut document = drawing();
    let before = document.clone();
    for degrees in [45.0, 90.00000000000001, -30.0] {
        let error = document
            .apply(&[
                Command::Rename {
                    title: "Must roll back".into(),
                },
                command(
                    selection(&[6], &[], &[]),
                    None,
                    point(0.0, 0.0),
                    rotate(degrees),
                ),
            ])
            .unwrap_err();
        assert_eq!(
            (error.index, error.error),
            (Some(1), EditError::TransformRectangleUnsupported(object(3)))
        );
        assert_eq!(document, before);
    }
    let mut document = empty();
    document
        .apply(&[add_graphic(GraphicShape::Rectangle, [point(1.0, 1.0); 2])])
        .unwrap();
    assert_eq!(
        document
            .selection_transform(
                &selection(&[2], &[], &[]),
                None,
                point(0.0, 0.0),
                rotate(45.0)
            )
            .unwrap_err(),
        EditError::TransformRectangleUnsupported(object(2))
    );
}

#[test]
fn invalid_refs_placements_and_nonfinite_angles_are_checked_even_for_identity_or_empty_targets() {
    let mut document = drawing();
    let before = document.clone();
    for degrees in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let error = document
            .apply(&[command(
                Selection::default(),
                None,
                point(0.0, 0.0),
                rotate(degrees),
            )])
            .unwrap_err();
        assert_eq!(error.error, EditError::TransformInvalidAngle);
        assert_eq!(document, before);
    }
    for selected in [
        selection(&[999], &[], &[]),
        selection(&[], &[999], &[]),
        selection(&[], &[], &[999]),
    ] {
        assert!(matches!(
            document.selection_transform(
                &selected,
                Some(object(1)),
                point(0.0, 0.0),
                rotate(360.0)
            ),
            Err(EditError::SelectionUnknown(_))
        ));
    }
    assert_eq!(
        document
            .selection_transform(
                &Selection::default(),
                Some(object(999)),
                point(0.0, 0.0),
                rotate(0.0)
            )
            .unwrap_err(),
        EditError::UnknownObject(object(999))
    );
    assert_eq!(
        document
            .selection_transform(
                &selection(&[], &[1], &[]),
                None,
                point(0.0, 0.0),
                rotate(0.0)
            )
            .unwrap_err(),
        EditError::InstanceRequired { instances: 2 }
    );
    assert_eq!(
        Point::new(f64::NAN, 0.0),
        Err(EditError::CoordinateOutOfRange)
    );
    assert_eq!(
        Point::new(0.0, MAX_COORDINATE + 1.0),
        Err(EditError::CoordinateOutOfRange)
    );
}

#[test]
fn late_out_of_range_graphic_rolls_back_all_transformed_positions_and_earlier_commands() {
    let mut document = drawing();
    document
        .apply(&[add_graphic(
            GraphicShape::Line,
            [point(-MAX_COORDINATE, 0.0), point(0.0, 0.0)],
        )])
        .unwrap();
    let before = document.clone();
    let error = document
        .apply(&[
            Command::Rename {
                title: "Must roll back".into(),
            },
            command(
                selection(&[1, 2, 7], &[], &[]),
                None,
                point(0.5, 0.0),
                SelectionTransform::ReflectVertical,
            ),
        ])
        .unwrap_err();
    assert_eq!(
        (error.index, error.error),
        (Some(1), EditError::CoordinateOutOfRange)
    );
    assert_eq!(document, before);
}

fn with_redo(mut document: Document) -> Document {
    document
        .apply(&[Command::Rename {
            title: "Redo branch".into(),
        }])
        .unwrap();
    document.undo().unwrap();
    document
}

#[test]
fn full_turn_identities_and_axis_fixed_geometry_are_exact_noops_preserving_redo() {
    let mut document = with_redo(drawing());
    let before = document.clone();
    for degrees in [0.0, -0.0, 360.0, -720.0, 1080.0] {
        let plan = document
            .selection_transform(
                &selection(&[6], &[], &[]),
                None,
                point(0.1, 0.2),
                rotate(degrees),
            )
            .unwrap();
        assert!(!plan.drawing_stereo_may_change);
        assert!(
            plan.positions
                .iter()
                .all(|position| position.before == position.after)
        );
        assert!(
            plan.graphics
                .iter()
                .all(|graphic| graphic.before == graphic.after)
        );
        assert_eq!(
            document
                .apply(&[command(
                    selection(&[6], &[], &[]),
                    None,
                    point(0.1, 0.2),
                    rotate(degrees)
                )])
                .unwrap(),
            Outcome::Unchanged
        );
        assert_eq!(document, before);
    }
    assert_eq!(
        document
            .apply(&[command(
                Selection::default(),
                None,
                point(0.1, 0.2),
                rotate(33.0)
            )])
            .unwrap(),
        Outcome::Unchanged
    );
    assert_eq!(document, before);
    let fixed = document
        .selection_transform(
            &selection(&[], &[1], &[]),
            Some(object(1)),
            point(0.0, 0.0),
            SelectionTransform::ReflectHorizontal,
        )
        .unwrap();
    assert!(!fixed.drawing_stereo_may_change);
    assert_eq!(fixed.positions[0].before, fixed.positions[0].after);
}

#[test]
fn four_quarter_turns_about_origin_preserve_decimal_coordinates_without_trigonometric_drift() {
    let mut document = drawing();
    let original_positions = positions(&document);
    let original_graphics = graphics(&document);
    for _ in 0..4 {
        document
            .apply(&[command(
                selection(&[6], &[], &[]),
                None,
                point(0.0, 0.0),
                rotate(90.0),
            )])
            .unwrap();
    }
    assert_eq!(positions(&document), original_positions);
    assert_eq!(graphics(&document), original_graphics);
    let plan = document
        .selection_transform(
            &selection(&[2], &[], &[]),
            None,
            point(0.5, 1.25),
            rotate(-270.0),
        )
        .unwrap();
    assert_eq!(
        plan.graphics[0].after.points,
        [point(0.75, 2.75), point(0.75, 4.75)]
    );
}

#[test]
fn transform_is_one_history_step_and_undo_redo_restore_geometry_and_transaction_selection() {
    let mut document = drawing();
    let before_positions = positions(&document);
    let before_graphics = graphics(&document);
    let selected = selection(&[6], &[2], &[1]);
    let after = selection(&[3], &[], &[]);
    let revision = document.revision();
    let depth = document.undo_depth();
    document
        .apply_selecting(
            &[command(
                selected.clone(),
                Some(1),
                point(0.0, 0.0),
                SelectionTransform::ReflectHorizontal,
            )],
            &TransactionSelection {
                before: Some(selected.clone()),
                after: Some(SelectionRequest {
                    objects: vec![ObjectRef::Id(object(3))],
                    ..SelectionRequest::default()
                }),
            },
        )
        .unwrap();
    assert_eq!(document.revision(), revision + 1);
    assert_eq!(document.undo_depth(), depth + 1);
    assert_eq!(document.selection(), &after);
    let transformed_positions = positions(&document);
    let transformed_graphics = graphics(&document);
    document.undo().unwrap();
    assert_eq!(positions(&document), before_positions);
    assert_eq!(graphics(&document), before_graphics);
    assert_eq!(document.selection(), &selected);
    document.redo().unwrap();
    assert_eq!(positions(&document), transformed_positions);
    assert_eq!(graphics(&document), transformed_graphics);
    assert_eq!(document.selection(), &after);
}

#[test]
fn quarter_turns_round_midpoint_tails_once_for_both_axes_and_signs() {
    // 1 + 2^-53 is exactly halfway between 1 and its successor. The
    // additional 2^-107 must select the successor even though summing the
    // two small terms first would round away that deciding tail.
    let half_ulp = 2.0_f64.powi(-53);
    let tail = 2.0_f64.powi(-107);
    let successor = 1.0_f64.next_up();
    for (degrees, input, pivot, expected) in [
        // The smaller pivot component is just below a half ULP at this
        // magnitude. Retaining its tail must select the lower x endpoint.
        (
            90.0,
            point(0.0, -2048.0),
            point(32768.00000000002, 3.6379788070917125e-12),
            point(34816.00000000002, -32768.00000000002),
        ),
        (
            90.0,
            point(0.0, -1.0),
            point(half_ulp, tail),
            point(successor, -half_ulp),
        ),
        (
            90.0,
            point(0.0, 1.0),
            point(-half_ulp, -tail),
            point(-successor, half_ulp),
        ),
        (
            90.0,
            point(1.0, 0.0),
            point(-tail, half_ulp),
            point(half_ulp, successor),
        ),
        (
            90.0,
            point(-1.0, 0.0),
            point(tail, -half_ulp),
            point(-half_ulp, -successor),
        ),
        (
            270.0,
            point(0.0, 1.0),
            point(half_ulp, -tail),
            point(successor, half_ulp),
        ),
        (
            270.0,
            point(0.0, -1.0),
            point(-half_ulp, tail),
            point(-successor, -half_ulp),
        ),
        (
            270.0,
            point(-1.0, 0.0),
            point(tail, half_ulp),
            point(-half_ulp, successor),
        ),
        (
            270.0,
            point(1.0, 0.0),
            point(-tail, -half_ulp),
            point(half_ulp, -successor),
        ),
        // An absent tail ties to the even endpoint; an opposite-sign tail
        // stays below the midpoint. Neither may be rounded upward.
        (
            90.0,
            point(0.0, -1.0),
            point(half_ulp, 0.0),
            point(1.0, -half_ulp),
        ),
        (
            90.0,
            point(0.0, -1.0),
            point(half_ulp, -tail),
            point(1.0, -half_ulp),
        ),
    ] {
        let mut document = empty();
        document
            .apply(&[
                Command::AddAtom {
                    atom: Atom::from(Element::from_symbol("C").unwrap()),
                    positions: NewAtomPositions::Single(input),
                    bond_to: None,
                },
                add_graphic(GraphicShape::Line, [input, input]),
            ])
            .unwrap();
        let selected = selection(&[1, 2], &[], &[]);
        let before = document.clone();
        let plan = document
            .selection_transform(&selected, None, pivot, rotate(degrees))
            .unwrap();
        assert_eq!(document, before, "preview must remain read-only");
        assert_eq!(
            plan.positions[0].after, expected,
            "{degrees} degrees, input {input:?}, pivot {pivot:?}"
        );
        assert_eq!(plan.graphics[0].after.points, [expected, expected]);
        assert_eq!(
            document
                .apply(&[command(selected.clone(), None, pivot, rotate(degrees))])
                .unwrap(),
            Outcome::Changed
        );
        let applied = document
            .selection_transform(&selected, None, pivot, rotate(0.0))
            .unwrap();
        assert_eq!(applied.positions[0].before, expected);
        assert_eq!(applied.graphics[0].before.points, [expected, expected]);
    }
}
