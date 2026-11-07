//! Ordered polygon vertices are geometry, including degenerate and crossing paths.
use chemifree_domain::document::{
    Command, Document, DocumentId, EditError, ObjectId, Outcome, Point, Vector,
};
use chemifree_domain::scene::{
    DeleteMode, Graphic, GraphicShape, SceneObject, Selection, SelectionTransform,
};

fn point(x: f64, y: f64) -> Point {
    Point::new(x, y).unwrap()
}
fn object(n: u32) -> ObjectId {
    ObjectId::from_serial(n).unwrap()
}
fn document() -> Document {
    Document::new(DocumentId::parse("polygon").unwrap(), "Polygon").unwrap()
}
fn add(vertices: Vec<Point>) -> Command {
    Command::AddPolygon {
        vertices,
        line_width_pt: Some(2.5),
        parent: None,
        index: None,
    }
}
fn graphic(doc: &Document) -> &Graphic {
    let SceneObject::Graphic(graphic) = doc.object(object(2)).unwrap() else {
        panic!("graphic")
    };
    graphic
}

#[test]
fn cardinality_boundaries_preserve_order_duplicates_crossings_and_atomic_failures() {
    for vertices in [
        vec![point(0., 0.); 3],
        vec![point(-100000., 100000.); 64],
        vec![
            point(0., 0.),
            point(2., 2.),
            point(0., 2.),
            point(2., 0.),
            point(0., 0.),
        ],
    ] {
        let mut doc = document();
        doc.apply(&[add(vertices.clone())]).unwrap();
        assert_eq!(graphic(&doc).points, vertices);
        assert_eq!(graphic(&doc).shape, GraphicShape::Polygon);
        let before = doc.clone();
        assert_eq!(
            doc.apply(&[Command::UpdatePolygonVertices {
                object: object(2),
                vertices: vertices.clone()
            }])
            .unwrap(),
            Outcome::Unchanged
        );
        assert_eq!(doc, before);
        for count in [0, 1, 2, 65] {
            let err = doc
                .apply(&[
                    Command::Rename {
                        title: "rollback".into(),
                    },
                    Command::UpdatePolygonVertices {
                        object: object(2),
                        vertices: vec![point(0., 0.); count],
                    },
                ])
                .unwrap_err();
            assert_eq!(err.error, EditError::GraphicGeometryInvalid);
            assert_eq!(doc, before);
        }
        assert_eq!(
            doc.apply(&[Command::UpdateGraphicPoints {
                object: object(2),
                points: [point(1., 2.); 2]
            }])
            .unwrap_err()
            .error,
            EditError::GraphicCommandMismatch
        );
        assert_eq!(doc, before);
    }
    let mut doc = document();
    assert_eq!(
        doc.apply(&[Command::AddGraphic {
            graphic: Graphic {
                shape: GraphicShape::Polygon,
                points: vec![point(0., 0.); 3],
                line_width_pt: None
            },
            parent: None,
            index: None
        }])
        .unwrap_err()
        .error,
        EditError::GraphicCommandMismatch
    );
    doc.apply(&[Command::AddGraphic {
        graphic: Graphic {
            shape: GraphicShape::Line,
            points: vec![point(0., 0.); 2],
            line_width_pt: None,
        },
        parent: None,
        index: None,
    }])
    .unwrap();
    let before = doc.clone();
    assert_eq!(
        doc.apply(&[Command::UpdatePolygonVertices {
            object: object(2),
            vertices: vec![point(0., 0.); 3]
        }])
        .unwrap_err()
        .error,
        EditError::GraphicCommandMismatch
    );
    assert_eq!(doc, before);
}

#[test]
fn transforms_visit_every_vertex_and_undo_restores_width_order_and_identity() {
    let vertices = vec![point(1., 0.), point(0., 1.), point(-3., 2.), point(5., -4.)];
    let mut doc = document();
    doc.apply(&[add(vertices.clone())]).unwrap();
    let original = doc.clone();
    let selection = Selection {
        objects: [object(2)].into(),
        ..Selection::default()
    };
    let plan = doc
        .selection_transform(
            &selection,
            None,
            point(0., 0.),
            SelectionTransform::Rotate { degrees: 37. },
        )
        .unwrap();
    assert_eq!(plan.graphics[0].after.points.len(), 4);
    assert_ne!(plan.graphics[0].after.points[3], vertices[3]);
    doc.apply(&[Command::TransformSelection {
        selection,
        instance: None,
        pivot: point(0., 0.),
        transform: SelectionTransform::Rotate { degrees: 37. },
    }])
    .unwrap();
    assert_eq!(graphic(&doc).points, plan.graphics[0].after.points);
    assert_eq!(graphic(&doc).line_width_pt, Some(2.5));
    assert_eq!(doc.atoms().count(), 0);
    doc.undo().unwrap();
    assert_eq!(doc.to_parts().objects, original.to_parts().objects);
    doc.redo().unwrap();
    assert_eq!(graphic(&doc).points, plan.graphics[0].after.points);
    let before = doc.clone();
    assert_eq!(
        doc.apply(&[Command::MoveObjects {
            objects: vec![object(2)],
            delta: Vector {
                dx: 100001.,
                dy: 0.
            }
        }])
        .unwrap_err()
        .error,
        EditError::CoordinateOutOfRange
    );
    assert_eq!(doc, before);
}

#[test]
fn polygon_history_activates_byte_budget_after_deletion_and_polygon_refusal_is_distinct() {
    let mut doc = document();
    for count in [500, 499] {
        doc.apply(
            &(0..count)
                .map(|_| add(vec![point(0., 0.); 64]))
                .collect::<Vec<_>>(),
        )
        .unwrap();
    }
    let before = doc.clone();
    let objects = (2..1001).map(object).collect::<Vec<_>>();
    let refusal = doc
        .apply(&[Command::MoveObjects {
            objects: objects.clone(),
            delta: Vector { dx: 1., dy: 1. },
        }])
        .unwrap_err();
    assert_eq!(refusal.index, None);
    assert_eq!(refusal.error, EditError::PolygonPersistenceLimit);
    assert_eq!(refusal.error.reason(), Some("polygon_persistence_limit"));
    assert_eq!(doc, before);
    doc.apply(&[Command::DeleteObjects {
        objects,
        mode: DeleteMode::Subtree,
    }])
    .unwrap();
    assert_eq!(
        doc.undo_depth(),
        1,
        "history-only polygons still activate the 8 MiB budget"
    );
    assert_eq!(Document::restore(doc.to_parts()).unwrap(), doc);
    doc.undo().unwrap();
    assert_eq!(doc.scene_objects().len(), 1000);
    doc.redo().unwrap();
    // Only the deletion history now contains polygons. Subsequent fixed-size-only
    // edits must still budget that older variable-sized entry until it is removed.
    for count in [500, 499] {
        doc.apply(
            &(0..count)
                .map(|_| Command::AddGraphic {
                    graphic: Graphic {
                        shape: GraphicShape::Line,
                        points: vec![point(0., 0.); 2],
                        line_width_pt: None,
                    },
                    parent: None,
                    index: None,
                })
                .collect::<Vec<_>>(),
        )
        .unwrap();
    }
    let fixed_objects = doc
        .scene_objects()
        .iter()
        .filter_map(|(id, _, object)| matches!(object, SceneObject::Graphic(_)).then_some(*id))
        .collect::<Vec<_>>();
    for _ in 0..3 {
        doc.apply(&[Command::MoveObjects {
            objects: fixed_objects.clone(),
            delta: Vector { dx: 1., dy: 1. },
        }])
        .unwrap();
    }
    assert!(doc.to_parts().undo.iter().all(|set| set.objects.iter().all(|(_, change)| {
        [&change.before, &change.after].into_iter().flatten().all(|object| {
            !matches!(object, SceneObject::Graphic(graphic) if graphic.shape == GraphicShape::Polygon)
        })
    })), "fixed-size edits must account for and eventually trim history-only polygons");
}

#[test]
fn restore_validates_polygon_and_old_shape_cardinality_before_trimming_history() {
    let mut doc = document();
    doc.apply(&[add(vec![point(0., 0.); 3])]).unwrap();
    let mut parts = doc.to_parts();
    let (_, change) = parts.undo[0]
        .objects
        .iter_mut()
        .find(|(_, change)| matches!(change.after, Some(SceneObject::Graphic(_))))
        .unwrap();
    let Some(SceneObject::Graphic(graphic)) = &mut change.after else {
        unreachable!()
    };
    graphic.points = vec![point(0., 0.); 65];
    let invalid = parts.undo[0].clone();
    parts.undo = vec![invalid; 101];
    assert!(
        Document::restore(parts)
            .unwrap_err()
            .to_string()
            .contains("invalid graphic history")
    );
    let mut parts = doc.to_parts();
    // The common model's public vectors cannot bypass shape-specific restore checks.
    for (_, entry) in &mut parts.objects {
        if let SceneObject::Graphic(g) = entry {
            g.shape = GraphicShape::Line;
        }
    }
    assert!(
        Document::restore(parts)
            .unwrap_err()
            .to_string()
            .contains("point count")
    );
}

#[test]
fn caption_only_overflow_in_polygon_document_preserves_the_existing_public_reason() {
    use chemifree_domain::scene::{Caption, CaptionRun};
    let mut doc = document();
    doc.apply(&[add(vec![point(0., 0.); 3])]).unwrap();
    let before = doc.clone();
    let caption = Caption {
        anchor: point(0., 0.),
        alignment: Default::default(),
        runs: vec![
            CaptionRun {
                text: "\"".repeat(512),
                script: chemifree_domain::scene::CaptionScript::Baseline,
                weight: Default::default(),
                style: Default::default(),
                font: None,
                size_pt: None,
                color: None,
                underline: false,
                outline: false,
                shadow: false
            };
            32
        ],
    };
    let commands = (0..300)
        .map(|_| Command::AddCaption {
            caption: caption.clone(),
            parent: None,
            index: None,
        })
        .collect::<Vec<_>>();
    let failure = doc.apply(&commands).unwrap_err();
    assert_eq!(failure.error, EditError::CaptionPersistenceLimit);
    assert_eq!(failure.error.reason(), Some("caption_persistence_limit"));
    assert_eq!(
        failure.error.to_string(),
        "The text edit exceeds the persistence budget; reduce text or transaction size."
    );
    assert_eq!(doc, before);
}

#[test]
fn arrangement_bounds_include_the_last_vertex_and_translate_the_complete_path() {
    use chemifree_domain::scene::{AlignMode, ObjectArrangement};
    let mut doc = document();
    let vertices = vec![
        point(0., 0.),
        point(1., 1.),
        point(-20., -30.),
        point(40., 8.),
    ];
    doc.apply(&[
        add(vertices.clone()),
        add(vec![
            point(100., 100.),
            point(101., 101.),
            point(102., 102.),
        ]),
    ])
    .unwrap();
    let objects = [object(2), object(3)].into();
    let plan = doc
        .object_arrangement(&objects, ObjectArrangement::Align(AlignMode::Left))
        .unwrap();
    assert_eq!(plan.units[0].bounds.left, -20.);
    assert_eq!(plan.units[0].bounds.right, 40.);
    assert_eq!(plan.units[0].bounds.top, -30.);
    assert_eq!(plan.units[0].bounds.bottom, 8.);
    doc.apply(&[Command::ArrangeObjects {
        objects: vec![object(2), object(3)],
        arrangement: ObjectArrangement::Align(AlignMode::Left),
    }])
    .unwrap();
    let SceneObject::Graphic(other) = doc.object(object(3)).unwrap() else {
        panic!("graphic")
    };
    assert_eq!(
        other.points,
        vec![point(-20., 100.), point(-19., 101.), point(-18., 102.)]
    );
    assert_eq!(graphic(&doc).points, vertices);
}
