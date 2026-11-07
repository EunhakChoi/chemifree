//! Nullable per-graphic width is presentation state with ordinary atomic history.

use chemifree_domain::document::{
    Atom, Command, Document, DocumentId, EditError, NewAtomPositions, ObjectId, Outcome, Point,
    Vector,
};
use chemifree_domain::element::Element;
use chemifree_domain::scene::{
    Graphic, GraphicShape, ReferenceTarget, SceneObject, SceneReference, Selection,
    SelectionTransform,
};
use chemifree_domain::scene_clipboard;
use chemifree_domain::settings::{SettingsEdit, SettingsError};

fn object(n: u32) -> ObjectId {
    ObjectId::from_serial(n).unwrap()
}
fn point(x: f64, y: f64) -> Point {
    Point::new(x, y).unwrap()
}
fn set(n: u32, width: Option<f64>) -> Command {
    Command::SetGraphicLineWidth {
        object: object(n),
        line_width_pt: width,
    }
}
fn graphic(doc: &Document, n: u32) -> &Graphic {
    match doc.object(object(n)).unwrap() {
        SceneObject::Graphic(graphic) => graphic,
        other => panic!("expected graphic: {other:?}"),
    }
}
fn drawing() -> Document {
    let mut doc = Document::new(DocumentId::parse("graphic-width").unwrap(), "Width").unwrap();
    let mut commands = vec![Command::AddAtom {
        atom: Atom::from(Element::CARBON),
        positions: NewAtomPositions::Single(point(0.0, 0.0)),
        bond_to: None,
    }];
    for shape in [
        GraphicShape::Line,
        GraphicShape::Arrow,
        GraphicShape::Rectangle,
    ] {
        commands.push(Command::AddGraphic {
            graphic: Graphic {
                line_width_pt: None,
                shape,
                points: vec![point(0.0, 0.0), point(10.0, 20.0)],
            },
            parent: None,
            index: None,
        });
    }
    doc.apply(&commands).unwrap();
    doc.apply(&[
        Command::Group {
            objects: vec![object(2), object(3)],
        },
        Command::AddReference {
            reference: SceneReference {
                source: object(2),
                target: ReferenceTarget::Object(object(3)),
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
fn widths_and_inheritance_are_undoable_noop_aware_and_preserve_shape_graph_tree_and_links() {
    let mut doc = drawing();
    for (id, width) in [(2, 0.1), (3, 10.0), (4, 1.2)] {
        let before = doc.to_parts();
        let old = graphic(&doc, id).clone();
        assert_eq!(
            doc.apply(&[set(id, Some(width))]).unwrap(),
            Outcome::Changed
        );
        assert_eq!(graphic(&doc, id).line_width_pt, Some(width));
        assert_eq!(graphic(&doc, id).shape, old.shape);
        assert_eq!(graphic(&doc, id).points, old.points);
        let after = doc.to_parts();
        assert_eq!(after.atoms, before.atoms);
        assert_eq!(after.bonds, before.bonds);
        assert_eq!(after.positions, before.positions);
        assert_eq!(after.stereo, before.stereo);
        assert_eq!(after.stereo_groups, before.stereo_groups);
        assert_eq!(after.roots, before.roots);
        assert_eq!(after.references, before.references);
        assert_eq!(after.next_object, before.next_object);
        assert_eq!(after.selection, before.selection);
        let exact = doc.clone();
        assert_eq!(
            doc.apply(&[set(id, Some(width))]).unwrap(),
            Outcome::Unchanged
        );
        assert_eq!(doc, exact);
        doc.apply(&[set(id, None)]).unwrap();
        assert_eq!(graphic(&doc, id).line_width_pt, None);
        doc.undo().unwrap();
        assert_eq!(graphic(&doc, id).line_width_pt, Some(width));
        doc.redo().unwrap();
        assert_eq!(graphic(&doc, id).line_width_pt, None);
    }
    doc.apply(&[set(2, Some(doc.settings().line_width_pt))])
        .unwrap();
    doc.apply(&[Command::UpdateSettings(SettingsEdit {
        line_width_pt: Some(4.0),
        ..SettingsEdit::default()
    })])
    .unwrap();
    assert_eq!(graphic(&doc, 2).line_width_pt, Some(1.2));
    assert_eq!(graphic(&doc, 3).line_width_pt, None);
    assert_eq!(doc.settings().line_width_pt, 4.0);
    assert_eq!(Document::restore(doc.to_parts()).unwrap(), doc);
}

#[test]
fn invalid_widths_and_wrong_targets_fail_atomically_without_allocating_ids() {
    let mut doc = drawing();
    for width in [
        0.0,
        0.099,
        10.001,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
    ] {
        let before = doc.clone();
        let error = doc
            .apply(&[
                Command::Rename {
                    title: "rollback".into(),
                },
                set(2, Some(width)),
            ])
            .unwrap_err();
        assert_eq!(
            error.error,
            EditError::Settings(SettingsError::LineWidthOutOfRange)
        );
        assert_eq!(doc, before);
    }
    for id in [1, 5, 99] {
        let before = doc.clone();
        assert!(doc.apply(&[set(2, Some(2.0)), set(id, Some(3.0))]).is_err());
        assert_eq!(doc, before);
    }
    let before = doc.clone();
    let error = doc
        .apply(&[Command::AddGraphic {
            graphic: Graphic {
                line_width_pt: Some(f64::NAN),
                shape: GraphicShape::Line,
                points: vec![point(0.0, 0.0), point(1.0, 1.0)],
            },
            parent: None,
            index: None,
        }])
        .unwrap_err();
    assert_eq!(
        error.error,
        EditError::Settings(SettingsError::LineWidthOutOfRange)
    );
    assert_eq!(doc, before);
}

#[test]
fn point_updates_moves_and_transforms_preserve_explicit_widths() {
    let mut doc = drawing();
    doc.apply(&[set(2, Some(2.75))]).unwrap();
    for command in [
        Command::MoveObjects {
            objects: vec![object(5)],
            delta: Vector { dx: 5.0, dy: 6.0 },
        },
        Command::TransformSelection {
            selection: Selection {
                objects: [object(5)].into(),
                ..Selection::default()
            },
            instance: None,
            pivot: point(0.0, 0.0),
            transform: SelectionTransform::Rotate { degrees: 90.0 },
        },
        Command::UpdateGraphicPoints {
            object: object(2),
            points: [point(4.0, 5.0), point(6.0, 7.0)],
        },
    ] {
        doc.apply(&[command]).unwrap();
        assert_eq!(graphic(&doc, 2).line_width_pt, Some(2.75));
        doc.undo().unwrap();
        assert_eq!(graphic(&doc, 2).line_width_pt, Some(2.75));
        doc.redo().unwrap();
        assert_eq!(graphic(&doc, 2).line_width_pt, Some(2.75));
    }
}

#[test]
fn graphics_clipboard_preserves_styled_members_and_group_descendants_without_mutation() {
    let mut doc = drawing();
    // Keep each selected subtree closed so copy exercises width preservation
    // independently of outgoing-reference refusals.
    let references: Vec<_> = doc.references().collect();
    doc.apply(
        &references
            .into_iter()
            .map(|reference| Command::RemoveReference { reference })
            .collect::<Vec<_>>(),
    )
    .unwrap();
    doc.apply(&[set(2, Some(1.2))]).unwrap();
    for id in [2, 5] {
        let before = doc.clone();
        let selected = Selection {
            objects: [object(id)].into(),
            ..Selection::default()
        };
        let copied = scene_clipboard::copy_fragment(&doc, &selected).unwrap();
        assert_eq!(copied.roots, [object(id)]);
        assert_eq!(
            copied
                .objects
                .iter()
                .find(|entry| entry.id == object(2))
                .unwrap()
                .object,
            SceneObject::Graphic(graphic(&doc, 2).clone())
        );
        assert_eq!(copied.objects.len(), if id == 2 { 1 } else { 3 });
        assert_eq!(doc, before);
    }
    let outside = Selection {
        objects: [object(3)].into(),
        ..Selection::default()
    };
    assert_eq!(
        scene_clipboard::copy_fragment(&doc, &outside)
            .unwrap()
            .objects
            .len(),
        1
    );
    doc.apply(&[set(2, None)]).unwrap();
    let selected = Selection {
        objects: [object(5)].into(),
        ..Selection::default()
    };
    assert_eq!(
        scene_clipboard::copy_fragment(&doc, &selected)
            .unwrap()
            .objects
            .len(),
        3
    );
}

#[test]
fn current_and_discarded_history_cannot_hide_invalid_graphic_widths() {
    let mut doc = drawing();
    doc.apply(&[set(2, Some(2.0))]).unwrap();
    let mut parts = doc.to_parts();
    if let SceneObject::Graphic(graphic) = &mut parts
        .objects
        .iter_mut()
        .find(|(id, _)| *id == object(2))
        .unwrap()
        .1
    {
        graphic.line_width_pt = Some(f64::NAN);
    }
    assert!(
        Document::restore(parts)
            .unwrap_err()
            .to_string()
            .contains("line width")
    );
    let mut parts = doc.to_parts();
    let mut bad = parts.undo.last().unwrap().clone();
    if let Some(SceneObject::Graphic(graphic)) = &mut bad.objects[0].1.after {
        graphic.line_width_pt = Some(100.0);
    }
    parts.undo.splice(0..0, std::iter::repeat_n(bad, 101));
    assert!(
        Document::restore(parts)
            .unwrap_err()
            .to_string()
            .contains("invalid graphic history")
    );
}
