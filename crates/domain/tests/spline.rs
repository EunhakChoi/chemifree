//! One cubic stores its four defining points without interpolation or topology constraints.
use chemifree_domain::document::{
    Command, Document, DocumentId, EditError, ObjectId, Outcome, Point, Vector,
};
use chemifree_domain::scene::{
    AlignMode, Graphic, GraphicShape, ObjectArrangement, ReferenceTarget, SceneObject,
    SceneReference, Selection, SelectionTransform,
};
fn point(x: f64, y: f64) -> Point {
    Point::new(x, y).unwrap()
}
fn object(n: u32) -> ObjectId {
    ObjectId::from_serial(n).unwrap()
}
fn document() -> Document {
    Document::new(DocumentId::parse("spline").unwrap(), "Spline").unwrap()
}
fn add(points: [Point; 4]) -> Command {
    Command::AddSpline {
        points,
        line_width_pt: Some(2.5),
        parent: None,
        index: None,
    }
}
fn graphic(doc: &Document, id: u32) -> &Graphic {
    let SceneObject::Graphic(graphic) = doc.object(object(id)).unwrap() else {
        panic!("graphic")
    };
    graphic
}
#[test]
fn repeated_and_degenerate_defining_points_are_preserved_with_noop_and_distinct_commands() {
    for points in [
        [point(0., 0.); 4],
        [point(-100000., 100000.); 4],
        [point(0., 0.), point(9., 8.), point(-9., 8.), point(0., 0.)],
    ] {
        let mut doc = document();
        doc.apply(&[add(points)]).unwrap();
        assert_eq!(graphic(&doc, 2).points, points);
        assert_eq!(graphic(&doc, 2).shape, GraphicShape::Spline);
        let before = doc.clone();
        assert_eq!(
            doc.apply(&[Command::UpdateSplinePoints {
                object: object(2),
                points
            }])
            .unwrap(),
            Outcome::Unchanged
        );
        assert_eq!(doc, before);
        for command in [
            Command::UpdateGraphicPoints {
                object: object(2),
                points: [point(1., 1.); 2],
            },
            Command::UpdatePolygonVertices {
                object: object(2),
                vertices: vec![point(1., 1.); 4],
            },
            Command::AddGraphic {
                graphic: graphic(&doc, 2).clone(),
                parent: None,
                index: None,
            },
        ] {
            assert_eq!(
                doc.apply(&[
                    Command::Rename {
                        title: "rollback".into()
                    },
                    command
                ])
                .unwrap_err()
                .error,
                EditError::GraphicCommandMismatch
            );
            assert_eq!(doc, before);
        }
        assert_eq!(
            doc.apply(&[Command::SetGraphicLineWidth {
                object: object(2),
                line_width_pt: Some(10.1)
            }])
            .unwrap_err()
            .error,
            EditError::Settings(chemifree_domain::settings::SettingsError::LineWidthOutOfRange)
        );
        assert_eq!(doc, before);
    }
    let mut doc = document();
    doc.apply(&[Command::AddPolygon {
        vertices: vec![point(0., 0.); 4],
        line_width_pt: None,
        parent: None,
        index: None,
    }])
    .unwrap();
    let before = doc.clone();
    assert_eq!(
        doc.apply(&[Command::UpdateSplinePoints {
            object: object(2),
            points: [point(1., 1.); 4]
        }])
        .unwrap_err()
        .error,
        EditError::GraphicCommandMismatch
    );
    assert_eq!(doc, before);
}
#[test]
fn four_control_point_bounds_and_affine_transforms_preserve_group_refs_and_graph() {
    let points = [
        point(0., 0.),
        point(-20., 90.),
        point(40., -80.),
        point(2., 3.),
    ];
    let mut doc = document();
    doc.apply(&[
        add(points),
        add([point(100., 100.); 4]),
        Command::AddReference {
            reference: SceneReference {
                source: object(2),
                target: ReferenceTarget::Object(object(3)),
            },
        },
    ])
    .unwrap();
    let plan = doc
        .object_arrangement(
            &[object(2), object(3)].into(),
            ObjectArrangement::Align(AlignMode::Left),
        )
        .unwrap();
    assert_eq!(
        (
            plan.units[0].bounds.left,
            plan.units[0].bounds.right,
            plan.units[0].bounds.top,
            plan.units[0].bounds.bottom
        ),
        (-20., 40., -80., 90.)
    );
    doc.apply(&[Command::Group {
        objects: vec![object(2), object(3)],
    }])
    .unwrap();
    let before = doc.clone();
    let selection = Selection {
        objects: [object(4)].into(),
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
    assert_ne!(plan.graphics[0].after.points[2], points[2]);
    doc.apply(&[Command::TransformSelection {
        selection,
        instance: None,
        pivot: point(0., 0.),
        transform: SelectionTransform::Rotate { degrees: 37. },
    }])
    .unwrap();
    assert_eq!(graphic(&doc, 2).points, plan.graphics[0].after.points);
    assert_eq!(graphic(&doc, 2).line_width_pt, Some(2.5));
    assert_eq!(
        doc.references().collect::<Vec<_>>(),
        before.references().collect::<Vec<_>>()
    );
    assert_eq!(doc.atoms().count(), 0);
    doc.undo().unwrap();
    assert_eq!(doc.to_parts().objects, before.to_parts().objects);
    doc.redo().unwrap();
    assert_eq!(graphic(&doc, 2).points, plan.graphics[0].after.points);
}
#[test]
fn restoration_checks_exact_four_even_in_history_that_retention_would_remove() {
    let mut doc = document();
    doc.apply(&[add([point(0., 0.); 4])]).unwrap();
    for count in [0, 2, 3, 5, 64] {
        let mut parts = doc.to_parts();
        for (_, object) in &mut parts.objects {
            if let SceneObject::Graphic(graphic) = object {
                graphic.points = vec![point(0., 0.); count];
            }
        }
        assert!(
            Document::restore(parts)
                .unwrap_err()
                .to_string()
                .contains("exactly four")
        );
        let mut parts = doc.to_parts();
        for (_, change) in &mut parts.undo[0].objects {
            if let Some(SceneObject::Graphic(graphic)) = &mut change.after {
                graphic.points = vec![point(0., 0.); count];
            }
        }
        parts.undo = vec![parts.undo[0].clone(); 101];
        assert!(
            Document::restore(parts)
                .unwrap_err()
                .to_string()
                .contains("invalid graphic history")
        );
    }
}
#[test]
fn cubic_history_uses_shared_byte_retention_and_restores_all_four_points() {
    let mut doc = document();
    for count in [500, 499] {
        doc.apply(
            &(0..count)
                .map(|_| add([point(0., 0.); 4]))
                .collect::<Vec<_>>(),
        )
        .unwrap();
    }
    for _ in 0..10 {
        doc.apply(&[Command::MoveObjects {
            objects: (2..1001).map(object).collect(),
            delta: Vector { dx: 1., dy: 1. },
        }])
        .unwrap();
    }
    assert_eq!(
        doc.undo_depth(),
        6,
        "six full before/after cubic forests fit8MiB; seven do not"
    );
    assert_eq!(Document::restore(doc.to_parts()).unwrap(), doc);
    for _ in 0..6 {
        doc.undo().unwrap();
    }
    assert_eq!(graphic(&doc, 2).points, [point(4., 4.); 4]);
    for _ in 0..6 {
        doc.redo().unwrap();
    }
    assert_eq!(graphic(&doc, 2).points, [point(10., 10.); 4]);
}

#[test]
fn mixed_spline_overflow_has_its_own_reason_and_caption_only_overflow_stays_compatible() {
    use chemifree_domain::scene::{Caption, CaptionRun, CaptionScript};
    let caption = |text: char| Caption {
        anchor: point(0., 0.),
        alignment: Default::default(),
        runs: vec![
            CaptionRun {
                text: text.to_string().repeat(512),
                script: CaptionScript::Baseline,
                weight: Default::default(),
                style: Default::default(),
                font: None,
                size_pt: None,
                color: None,
                underline: false,
                outline: true,
                shadow: false
            };
            32
        ],
    };
    let mut doc = document();
    doc.apply(&[add([point(0., 0.); 4])]).unwrap();
    doc.apply(
        &(0..107)
            .map(|_| Command::AddCaption {
                caption: caption('"'),
                parent: None,
                index: None,
            })
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let before = doc.clone();
    let mut edits = (3..110)
        .map(|n| Command::UpdateCaption {
            object: object(n),
            caption: caption('\\'),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        doc.apply(&edits).unwrap_err().error,
        EditError::CaptionPersistenceLimit
    );
    assert_eq!(doc, before);
    edits.push(Command::SetGraphicLineWidth {
        object: object(2),
        line_width_pt: Some(9.),
    });
    let error = doc.apply(&edits).unwrap_err();
    assert_eq!(error.error, EditError::SplinePersistenceLimit);
    assert_eq!(error.error.reason(), Some("spline_persistence_limit"));
    assert_eq!(doc, before);
}
