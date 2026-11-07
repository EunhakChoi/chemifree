//! Axis-aligned ellipses reuse bounded corner geometry and preserve graph-independent history.

use chemifree_domain::document::{
    Atom, AtomId, Command, Document, DocumentId, EditError, NewAtomPositions, ObjectId, Outcome,
    Point, Vector,
};
use chemifree_domain::element::Element;
use chemifree_domain::scene::{
    DeleteMode, Graphic, GraphicShape, ReferenceTarget, SceneObject, SceneReference, Selection,
    SelectionTransform, TransactionSelection,
};
use chemifree_domain::scene_clipboard;

fn object(id: u32) -> ObjectId {
    ObjectId::from_serial(id).unwrap()
}
fn point(x: f64, y: f64) -> Point {
    Point::new(x, y).unwrap()
}
fn ellipse(doc: &Document, id: u32) -> &Graphic {
    match doc.object(object(id)).unwrap() {
        SceneObject::Graphic(graphic) => graphic,
        other => panic!("not graphic: {other:?}"),
    }
}
fn drawing(points: [Point; 2]) -> Document {
    let mut doc = Document::new(DocumentId::parse("ellipse").unwrap(), "Ellipse").unwrap();
    doc.apply(&[
        Command::AddAtom {
            atom: Atom::from(Element::CARBON),
            positions: NewAtomPositions::Single(point(2.0, 3.0)),
            bond_to: None,
        },
        Command::AddGraphic {
            graphic: Graphic {
                shape: GraphicShape::Ellipse,
                line_width_pt: Some(2.75),
                points,
            },
            parent: None,
            index: None,
        },
        Command::AddGraphic {
            graphic: Graphic {
                shape: GraphicShape::Line,
                line_width_pt: None,
                points: [point(0.0, 0.0), point(20.0, 10.0)],
            },
            parent: None,
            index: None,
        },
        Command::Group {
            objects: vec![object(2), object(3)],
        },
        Command::AddReference {
            reference: SceneReference {
                source: object(2),
                target: ReferenceTarget::Object(object(3)),
            },
        },
    ])
    .unwrap();
    doc
}
fn group() -> Selection {
    Selection {
        objects: [object(4)].into(),
        ..Selection::default()
    }
}
fn transform(degrees: f64) -> Command {
    Command::TransformSelection {
        selection: group(),
        instance: None,
        pivot: point(0.0, 0.0),
        transform: SelectionTransform::Rotate { degrees },
    }
}

#[test]
fn reversed_zero_axis_and_coincident_ellipses_keep_exact_corners_and_noop_history() {
    for points in [
        [point(-100000.0, 100000.0), point(100000.0, -100000.0)],
        [point(6.0, 8.0), point(-4.0, -2.0)],
        [point(1.0, 2.0), point(1.0, 12.0)],
        [point(1.0, 2.0); 2],
    ] {
        let mut doc = drawing(points);
        assert_eq!(ellipse(&doc, 2).shape, GraphicShape::Ellipse);
        assert_eq!(ellipse(&doc, 2).points, points);
        let before = doc.clone();
        assert_eq!(
            doc.apply(&[Command::UpdateGraphicPoints {
                object: object(2),
                points
            }])
            .unwrap(),
            Outcome::Unchanged
        );
        assert_eq!(doc, before);
        assert_eq!(Document::restore(doc.to_parts()).unwrap(), doc);
    }
}

#[test]
fn move_reflection_exact_quarter_turn_and_corner_edit_retain_width_graph_group_and_references() {
    let mut doc = drawing([point(-4.0, -2.0), point(6.0, 8.0)]);
    let original = doc.to_parts();
    doc.apply(&[Command::MoveObjects {
        objects: vec![object(4)],
        delta: Vector { dx: 10.0, dy: 20.0 },
    }])
    .unwrap();
    assert_eq!(
        ellipse(&doc, 2).points,
        [point(6.0, 18.0), point(16.0, 28.0)]
    );
    doc.apply(&[Command::TransformSelection {
        selection: group(),
        instance: None,
        pivot: point(0.0, 0.0),
        transform: SelectionTransform::ReflectHorizontal,
    }])
    .unwrap();
    assert_eq!(
        ellipse(&doc, 2).points,
        [point(6.0, -18.0), point(16.0, -28.0)]
    );
    doc.apply(&[transform(90.0)]).unwrap();
    assert_eq!(
        ellipse(&doc, 2).points,
        [point(18.0, 6.0), point(28.0, 16.0)]
    );
    let exact = doc.clone();
    assert_eq!(doc.apply(&[transform(360.0)]).unwrap(), Outcome::Unchanged);
    assert_eq!(doc, exact);
    doc.apply(&[Command::UpdateGraphicPoints {
        object: object(2),
        points: [point(0.0, 0.0); 2],
    }])
    .unwrap();
    doc.undo().unwrap();
    assert_eq!(ellipse(&doc, 2).points, ellipse(&exact, 2).points);
    doc.redo().unwrap();
    assert_eq!(ellipse(&doc, 2).points, [point(0.0, 0.0); 2]);
    let after = doc.to_parts();
    assert_eq!(ellipse(&doc, 2).line_width_pt, Some(2.75));
    assert_eq!(after.atoms, original.atoms);
    assert_eq!(after.bonds, original.bonds);
    assert_eq!(after.positions, original.positions);
    assert_eq!(after.stereo, original.stereo);
    assert_eq!(after.roots, original.roots);
    assert_eq!(after.references, original.references);
    assert_eq!(after.settings, original.settings);
    assert_eq!(after.next_object, original.next_object);
}

#[test]
fn arbitrary_angles_refuse_entire_mixed_transactions_including_circles_and_degenerate_ellipses() {
    for points in [
        [point(-4.0, -2.0), point(6.0, 8.0)],
        [point(-5.0, -5.0), point(5.0, 5.0)],
        [point(0.0, 0.0); 2],
    ] {
        let mut doc = drawing(points);
        let selected = Selection {
            atoms: [AtomId::from_serial(1).unwrap()].into(),
            ..group()
        };
        for degrees in [-45.0, 0.00001, 89.999, 90.00000000000001, 360.5] {
            let before = doc.clone();
            let error = doc
                .selection_transform(
                    &selected,
                    None,
                    point(0.0, 0.0),
                    SelectionTransform::Rotate { degrees },
                )
                .unwrap_err();
            assert_eq!(error, EditError::TransformEllipseUnsupported(object(2)));
            let error = doc
                .apply(&[
                    Command::Rename {
                        title: "rollback".into(),
                    },
                    Command::TransformSelection {
                        selection: selected.clone(),
                        instance: None,
                        pivot: point(0.0, 0.0),
                        transform: SelectionTransform::Rotate { degrees },
                    },
                ])
                .unwrap_err();
            assert_eq!(error.error.reason(), Some("transform_ellipse_unsupported"));
            assert_eq!(doc, before);
        }
    }
}

#[test]
fn ellipse_clipboard_forest_preserves_style_refs_offsets_and_cut_undo_without_touching_graph() {
    let mut source = drawing([point(-4.0, -2.0), point(6.0, 8.0)]);
    let before = source.to_parts();
    let fragment = scene_clipboard::copy_fragment(&source, &group()).unwrap();
    let mut target = Document::new(DocumentId::parse("target").unwrap(), "Target").unwrap();
    let plan = fragment
        .paste_commands(&target, Vector { dx: 10.0, dy: 20.0 })
        .unwrap();
    target
        .apply_selecting(
            &plan.commands,
            &TransactionSelection {
                before: None,
                after: Some(plan.selection),
            },
        )
        .unwrap();
    assert_eq!(ellipse(&target, 2).shape, GraphicShape::Ellipse);
    assert_eq!(ellipse(&target, 2).line_width_pt, Some(2.75));
    assert_eq!(
        ellipse(&target, 2).points,
        [point(6.0, 18.0), point(16.0, 28.0)]
    );
    assert_eq!(target.to_parts().references, before.references);
    source
        .apply(&[Command::DeleteObjects {
            objects: vec![object(4)],
            mode: DeleteMode::Subtree,
        }])
        .unwrap();
    assert_eq!(source.to_parts().atoms, before.atoms);
    source.undo().unwrap();
    assert_eq!(source.to_parts().objects, before.objects);
    assert_eq!(
        scene_clipboard::copy_fragment(&source, &group()).unwrap(),
        fragment
    );
}
