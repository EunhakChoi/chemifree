//! Graphic point edits retain object kind, containment, references and the chemical graph.

use chemifree_domain::document::{
    Atom, AtomId, AtomRef, BondOrder, BondTo, Command, Document, DocumentId, EditError,
    NewAtomPositions, ObjectId, Outcome, Point,
};
use chemifree_domain::element::Element;
use chemifree_domain::scene::{
    Caption, CaptionRun, CaptionScript, Graphic, GraphicShape, ObjectKind, ReferenceTarget,
    SceneObject, SceneReference, Selection, TransactionSelection,
};

fn object(id: u32) -> ObjectId {
    ObjectId::from_serial(id).unwrap()
}
fn point(x: f64, y: f64) -> Point {
    Point::new(x, y).unwrap()
}
fn points(doc: &Document, id: u32) -> [Point; 2] {
    match doc.object(object(id)).unwrap() {
        SceneObject::Graphic(graphic) => graphic.points.clone().try_into().unwrap(),
        other => panic!("expected graphic, got {other:?}"),
    }
}
fn update(id: u32, points: [Point; 2]) -> Command {
    Command::UpdateGraphicPoints {
        object: object(id),
        points,
    }
}
fn drawing() -> Document {
    let mut doc = Document::new(
        DocumentId::parse("graphic-points").unwrap(),
        "Graphic points",
    )
    .unwrap();
    doc.apply(&[
        Command::AddAtom {
            atom: Atom::from(Element::CARBON),
            positions: NewAtomPositions::Single(point(2.0, 3.0)),
            bond_to: None,
        },
        Command::AddAtom {
            atom: Atom::from(Element::from_symbol("O").unwrap()),
            positions: NewAtomPositions::Single(point(32.0, 3.0)),
            bond_to: Some(BondTo {
                atom: AtomRef::Created(0),
                order: BondOrder::Single,
            }),
        },
        Command::AddGraphic {
            graphic: Graphic {
                line_width_pt: None,
                shape: GraphicShape::Line,
                points: vec![point(0.0, 0.0), point(10.0, 0.0)],
            },
            parent: None,
            index: None,
        },
        Command::AddGraphic {
            graphic: Graphic {
                line_width_pt: None,
                shape: GraphicShape::Arrow,
                points: vec![point(20.0, 0.0), point(30.0, 0.0)],
            },
            parent: None,
            index: None,
        },
        Command::AddGraphic {
            graphic: Graphic {
                line_width_pt: None,
                shape: GraphicShape::Rectangle,
                points: vec![point(40.0, 0.0), point(50.0, 10.0)],
            },
            parent: None,
            index: None,
        },
        Command::AddCaption {
            caption: Caption {
                alignment: Default::default(),
                anchor: point(0.0, 20.0),
                runs: vec![CaptionRun {
                    text: "caption".into(),
                    script: CaptionScript::Baseline,
                    weight: Default::default(),
                    style: Default::default(),
                    font: None,
                    size_pt: None,
                    color: None,
                    underline: false,
                    outline: false,
                    shadow: false,
                }],
            },
            parent: None,
            index: None,
        },
    ])
    .unwrap();
    doc.apply(&[
        Command::Group {
            objects: vec![object(2), object(3)],
        },
        Command::AddReference {
            reference: SceneReference {
                source: object(2),
                target: ReferenceTarget::Atom(AtomId::from_serial(1).unwrap()),
            },
        },
        Command::AddReference {
            reference: SceneReference {
                source: object(4),
                target: ReferenceTarget::Object(object(2)),
            },
        },
    ])
    .unwrap();
    doc
}

#[test]
fn each_shape_accepts_exact_bounded_points_and_keeps_graph_tree_links_and_ids() {
    let mut doc = drawing();
    for (id, shape, new_points) in [
        (
            2,
            GraphicShape::Line,
            [point(-100000.0, 100000.0), point(100000.0, -100000.0)],
        ),
        // Coincident points are already supported by add_graphic; updates add no new restriction.
        (3, GraphicShape::Arrow, [point(7.0, 8.0), point(7.0, 8.0)]),
        (
            4,
            GraphicShape::Rectangle,
            [point(50.0, 10.0), point(40.0, 0.0)],
        ),
    ] {
        let before = doc.to_parts();
        let parents: Vec<_> = doc
            .scene_objects()
            .iter()
            .map(|(id, parent, _)| (*id, *parent))
            .collect();
        assert_eq!(
            doc.apply(&[update(id, new_points)]).unwrap(),
            Outcome::Changed
        );
        assert_eq!(points(&doc, id), new_points);
        assert!(
            matches!(doc.object(object(id)),Some(SceneObject::Graphic(graphic)) if graphic.shape==shape)
        );
        let after = doc.to_parts();
        assert_eq!(after.atoms, before.atoms);
        assert_eq!(after.bonds, before.bonds);
        assert_eq!(after.positions, before.positions);
        assert_eq!(after.stereo, before.stereo);
        assert_eq!(after.stereo_groups, before.stereo_groups);
        assert_eq!(after.roots, before.roots);
        assert_eq!(after.references, before.references);
        assert_eq!(after.settings, before.settings);
        assert_eq!(after.next_object, before.next_object);
        assert_eq!(after.selection, before.selection);
        assert_eq!(
            doc.scene_objects()
                .iter()
                .map(|(id, parent, _)| (*id, *parent))
                .collect::<Vec<_>>(),
            parents
        );
        assert_eq!(Document::restore(doc.to_parts()).unwrap(), doc);
    }
}

#[test]
fn identical_points_are_noop_and_changed_points_undo_redo_with_selection_as_one_step() {
    let mut doc = drawing();
    let original = doc.clone();
    let old = points(&doc, 2);
    assert_eq!(doc.apply(&[update(2, old)]).unwrap(), Outcome::Unchanged);
    assert_eq!(doc, original);
    let selected = Selection {
        objects: [object(2)].into(),
        ..Selection::default()
    };
    doc.apply_selecting(
        &[update(2, [point(1.0, 2.0), point(3.0, 4.0)])],
        &TransactionSelection {
            before: Some(selected.clone()),
            after: None,
        },
    )
    .unwrap();
    assert_eq!(doc.selection(), &selected);
    assert_eq!(doc.undo_depth(), original.undo_depth() + 1);
    doc.undo().unwrap();
    assert_eq!(points(&doc, 2), old);
    assert_eq!(doc.selection(), &selected);
    doc.redo().unwrap();
    assert_eq!(points(&doc, 2), [point(1.0, 2.0), point(3.0, 4.0)]);
    assert_eq!(doc.selection(), &selected);
}

#[test]
fn absent_and_wrong_kind_targets_roll_back_earlier_edits_and_identifier_allocation() {
    let mut doc = drawing();
    for (id, error) in [
        (99, EditError::UnknownObject(object(99))),
        (
            1,
            EditError::ObjectKindMismatch {
                object: object(1),
                expected: ObjectKind::Graphic,
            },
        ),
        (
            5,
            EditError::ObjectKindMismatch {
                object: object(5),
                expected: ObjectKind::Graphic,
            },
        ),
        (
            6,
            EditError::ObjectKindMismatch {
                object: object(6),
                expected: ObjectKind::Graphic,
            },
        ),
    ] {
        let before = doc.clone();
        let failure = doc
            .apply(&[
                Command::AddGraphic {
                    graphic: Graphic {
                        line_width_pt: None,
                        shape: GraphicShape::Line,
                        points: vec![point(0.0, 0.0), point(1.0, 1.0)],
                    },
                    parent: None,
                    index: None,
                },
                update(2, [point(9.0, 10.0), point(11.0, 12.0)]),
                update(id, [point(3.0, 4.0), point(5.0, 6.0)]),
            ])
            .unwrap_err();
        assert_eq!(failure.error, error);
        assert_eq!(doc, before);
    }
}
