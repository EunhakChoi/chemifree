//! Styled graphic forests retain width, grouping, references and atomic graph-independent history.

use chemifree_domain::document::{
    Atom, Command, Document, DocumentId, EditError, NewAtomPositions, ObjectId, Point,
    TransactionError, Vector,
};
use chemifree_domain::element::Element;
use chemifree_domain::scene::{
    DeleteMode, Graphic, GraphicShape, ReferenceTarget, SceneObject, SceneReference, Selection,
    TransactionSelection,
};
use chemifree_domain::scene_clipboard::{self, SceneFragmentError};
use chemifree_domain::settings::{SettingsEdit, SettingsError};

fn object(n: u32) -> ObjectId {
    ObjectId::from_serial(n).unwrap()
}
fn document(id: &str) -> Document {
    Document::new(DocumentId::parse(id).unwrap(), id).unwrap()
}
fn source() -> Document {
    let mut doc = document("source");
    doc.apply(&[
        Command::AddAtom {
            atom: Atom::from(Element::CARBON),
            positions: NewAtomPositions::Single(Point::new(1.0, 2.0).unwrap()),
            bond_to: None,
        },
        Command::AddGraphic {
            graphic: Graphic {
                line_width_pt: Some(1.2),
                shape: GraphicShape::Line,
                points: [
                    Point::new(0.0, 0.0).unwrap(),
                    Point::new(10.0, 5.0).unwrap(),
                ],
            },
            parent: None,
            index: None,
        },
        Command::AddGraphic {
            graphic: Graphic {
                line_width_pt: None,
                shape: GraphicShape::Arrow,
                points: [
                    Point::new(20.0, 0.0).unwrap(),
                    Point::new(30.0, 5.0).unwrap(),
                ],
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
fn selection() -> Selection {
    Selection {
        objects: [object(4)].into(),
        ..Selection::default()
    }
}
fn width(doc: &Document, id: ObjectId) -> Option<f64> {
    match doc.object(id).unwrap() {
        SceneObject::Graphic(graphic) => graphic.line_width_pt,
        other => panic!("expected graphic: {other:?}"),
    }
}

#[test]
fn styled_copy_paste_keeps_nullable_width_offsets_refs_and_source_graph_unchanged() {
    let source = source();
    let exact = source.clone();
    let fragment = scene_clipboard::copy_fragment(&source, &selection()).unwrap();
    assert_eq!(source, exact);
    let mut destination = document("destination");
    destination
        .apply(&[
            Command::AddAtom {
                atom: Atom::from(Element::CARBON),
                positions: NewAtomPositions::Single(Point::new(7.0, 8.0).unwrap()),
                bond_to: None,
            },
            Command::AddGraphic {
                graphic: Graphic {
                    line_width_pt: Some(10.0),
                    shape: GraphicShape::Rectangle,
                    points: [Point::new(100.0, 100.0).unwrap(); 2],
                },
                parent: None,
                index: None,
            },
        ])
        .unwrap();
    let original = destination.to_parts();
    let plan = fragment
        .paste_commands(
            &destination,
            Vector {
                dx: 50.0,
                dy: -20.0,
            },
        )
        .unwrap();
    let targets: Vec<_> = plan.objects.iter().map(|mapping| mapping.target).collect();
    assert_eq!(targets, vec![object(5), object(3), object(4)]);
    destination
        .apply_selecting(
            &plan.commands,
            &TransactionSelection {
                before: None,
                after: Some(plan.selection),
            },
        )
        .unwrap();
    assert_eq!(width(&destination, object(3)), Some(1.2));
    assert_eq!(width(&destination, object(4)), None);
    assert_eq!(width(&destination, object(2)), Some(10.0));
    assert!(
        matches!(destination.object(object(3)),Some(SceneObject::Graphic(graphic)) if graphic.points==[Point::new(50.0,-20.0).unwrap(),Point::new(60.0,-15.0).unwrap()])
    );
    assert_eq!(destination.to_parts().atoms, original.atoms);
    assert_eq!(destination.to_parts().positions, original.positions);
    assert_eq!(
        destination.to_parts().references,
        vec![SceneReference {
            source: object(3),
            target: ReferenceTarget::Object(object(4))
        }]
    );
    destination.undo().unwrap();
    assert_eq!(destination.to_parts().objects, original.objects);
    destination.redo().unwrap();
    assert_eq!(width(&destination, object(3)), Some(1.2));
    assert_eq!(
        Document::restore(destination.to_parts()).unwrap(),
        destination
    );
}

#[test]
fn invalid_styles_settings_and_overflow_fail_before_any_destination_change() {
    let source = source();
    let fragment = scene_clipboard::copy_fragment(&source, &selection()).unwrap();
    let mut destination = document("refusals");
    let exact = destination.clone();
    for bad in [0.0, 10.01, f64::NAN, f64::INFINITY] {
        let mut invalid = fragment.clone();
        if let SceneObject::Graphic(graphic) = &mut invalid
            .objects
            .iter_mut()
            .find(|entry| entry.id == object(2))
            .unwrap()
            .object
        {
            graphic.line_width_pt = Some(bad);
        }
        assert_eq!(
            invalid
                .paste_commands(&destination, Vector { dx: 0.0, dy: 0.0 })
                .unwrap_err(),
            SceneFragmentError::Invalid(EditError::Settings(SettingsError::LineWidthOutOfRange))
        );
        assert_eq!(destination, exact);
    }
    assert!(matches!(
        fragment.paste_commands(
            &destination,
            Vector {
                dx: 100001.0,
                dy: 0.0
            }
        ),
        Err(SceneFragmentError::Invalid(EditError::CoordinateOutOfRange))
    ));
    assert_eq!(destination, exact);
    destination
        .apply(&[Command::UpdateSettings(SettingsEdit {
            line_width_pt: Some(2.0),
            ..SettingsEdit::default()
        })])
        .unwrap();
    let before = destination.clone();
    assert_eq!(
        fragment
            .paste_commands(&destination, Vector { dx: 0.0, dy: 0.0 })
            .unwrap_err(),
        SceneFragmentError::StyleMismatch
    );
    assert_eq!(destination, before);
}

#[test]
fn copying_styled_forest_is_readonly_and_cut_rollback_and_undo_restore_exact_style() {
    let mut doc = source();
    let before = doc.clone();
    let fragment = scene_clipboard::copy_fragment(&doc, &selection()).unwrap();
    assert_eq!(doc, before);
    let failed: TransactionError = doc
        .apply(&[
            Command::DeleteObjects {
                objects: vec![object(4)],
                mode: DeleteMode::Subtree,
            },
            Command::UpdateGraphicPoints {
                object: object(99),
                points: [Point::new(0.0, 0.0).unwrap(); 2],
            },
        ])
        .unwrap_err();
    assert_eq!(failed.error, EditError::UnknownObject(object(99)));
    assert_eq!(doc, before);
    doc.apply(&[Command::DeleteObjects {
        objects: vec![object(4)],
        mode: DeleteMode::Subtree,
    }])
    .unwrap();
    assert_eq!(doc.to_parts().atoms, before.to_parts().atoms);
    doc.undo().unwrap();
    assert_eq!(doc.to_parts().objects, before.to_parts().objects);
    assert_eq!(
        scene_clipboard::copy_fragment(&doc, &selection()).unwrap(),
        fragment
    );
}
