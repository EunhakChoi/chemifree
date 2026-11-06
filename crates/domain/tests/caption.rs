//! Independent annotations must never change graph semantics or silently disappear in edits.

use chemifree_domain::document::{
    Command, Document, DocumentId, EditError, ObjectId, Outcome, Point, Vector,
};
use chemifree_domain::scene::{
    Caption, CaptionRun, CaptionScript, CaptionStyle, CaptionWeight, DeleteMode, ReferenceTarget,
    SceneObject, SceneReference, Selection, SelectionTransform,
};

fn object(serial: u32) -> ObjectId {
    ObjectId::from_serial(serial).unwrap()
}
fn caption(text: &str) -> Caption {
    Caption {
        alignment: Default::default(),
        anchor: Point::new(5.0, 7.0).unwrap(),
        runs: vec![CaptionRun {
            font: None,
            text: text.into(),
            script: CaptionScript::Baseline,
            weight: Default::default(),
            style: Default::default(),
        }],
    }
}
fn add(value: Caption) -> Command {
    Command::AddCaption {
        caption: value,
        parent: None,
        index: None,
    }
}
fn document() -> Document {
    Document::new(DocumentId::parse("captions").unwrap(), "Caption").unwrap()
}

#[test]
fn caption_unicode_and_explicit_scripts_are_verbatim_and_one_undo_step() {
    let mut doc = document();
    let value = Caption {
        alignment: Default::default(),
        anchor: Point::new(5.0, 7.0).unwrap(),
        runs: vec![
            CaptionRun {
                font: None,
                text: "한글 e\u{301} 👩‍🔬 H".into(),
                script: CaptionScript::Baseline,
                weight: Default::default(),
                style: Default::default(),
            },
            CaptionRun {
                font: None,
                text: "2".into(),
                script: CaptionScript::Subscript,
                weight: Default::default(),
                style: Default::default(),
            },
            CaptionRun {
                font: None,
                text: "+".into(),
                script: CaptionScript::Superscript,
                weight: Default::default(),
                style: Default::default(),
            },
        ],
    };
    assert_eq!(doc.apply(&[add(value.clone())]).unwrap(), Outcome::Changed);
    assert_eq!(
        doc.scene_objects()[1].2,
        &SceneObject::Caption(value.clone())
    );
    assert_eq!(doc.atoms().count(), 0);
    assert_eq!(doc.undo_depth(), 1);
    assert_eq!(
        doc.apply(&[Command::UpdateCaption {
            object: object(2),
            caption: value.clone()
        }])
        .unwrap(),
        Outcome::Unchanged
    );
    doc.undo().unwrap();
    assert_eq!(doc.scene_objects().len(), 1);
    doc.redo().unwrap();
    assert_eq!(doc.scene_objects()[1].2, &SceneObject::Caption(value));
}

#[test]
fn nested_caption_moves_once_and_transform_refuses_the_entire_union() {
    let mut doc = document();
    doc.apply(&[
        add(caption("A")),
        add(caption("B")),
        Command::Group {
            objects: vec![object(2), object(3)],
        },
    ])
    .unwrap();
    doc.apply(&[Command::MoveObjects {
        objects: vec![object(4), object(2)],
        delta: Vector { dx: 10.0, dy: -2.0 },
    }])
    .unwrap();
    let mut expected = caption("A");
    expected.anchor = Point::new(15.0, 5.0).unwrap();
    assert_eq!(doc.scene_objects()[2].2, &SceneObject::Caption(expected));
    let before = doc.clone();
    let error = doc
        .apply(&[Command::TransformSelection {
            selection: Selection {
                objects: [object(4)].into(),
                ..Selection::default()
            },
            instance: None,
            pivot: Point::new(0.0, 0.0).unwrap(),
            transform: SelectionTransform::Rotate { degrees: 90.0 },
        }])
        .unwrap_err();
    assert_eq!(error.error, EditError::CaptionUnsupported(object(2)));
    assert_eq!(doc, before);
}

#[test]
fn invalid_caption_rolls_back_prior_commands_and_does_not_reserve_ids() {
    for text in ["", " \u{2003}", "a\n", "a\r", "a\t", "\u{85}", "a\u{2028}"] {
        let mut doc = document();
        let before = doc.clone();
        let error = doc
            .apply(&[add(caption("valid")), add(caption(text))])
            .unwrap_err();
        assert_eq!(error.index, Some(1), "{text:?}");
        assert_eq!(error.error, EditError::CaptionInvalid, "{text:?}");
        assert_eq!(doc, before, "{text:?}");
    }
}

#[test]
fn byte_and_run_limits_are_not_character_or_utf16_limits() {
    assert_eq!(caption(&"한".repeat(5461)).validate(), Ok(()));
    assert_eq!(
        caption(&"한".repeat(5462)).validate(),
        Err(EditError::CaptionInvalid)
    );
    let mut value = caption("x");
    value.runs = vec![value.runs[0].clone(); 32];
    assert_eq!(value.validate(), Ok(()));
    value.runs.push(value.runs[0].clone());
    assert_eq!(value.validate(), Err(EditError::CaptionInvalid));
    assert_eq!(
        caption("\u{feff}").validate(),
        Ok(()),
        "Unicode White_Space excludes FEFF"
    );
}

#[test]
fn caption_deletion_obeys_references_and_undo_restores_the_same_id() {
    let mut doc = document();
    let reference = SceneReference {
        source: object(1),
        target: ReferenceTarget::Object(object(2)),
    };
    doc.apply(&[add(caption("caption")), Command::AddReference { reference }])
        .unwrap();
    let before = doc.clone();
    let delete = Command::DeleteObjects {
        objects: vec![object(2)],
        mode: DeleteMode::Subtree,
    };
    let refused = doc.apply(std::slice::from_ref(&delete)).unwrap_err();
    assert_eq!(refused.error, EditError::ReferenceInUse(reference));
    assert_eq!(doc, before);
    doc.apply(&[Command::RemoveReference { reference }, delete])
        .unwrap();
    assert_eq!(doc.scene_objects().len(), 1);
    doc.undo().unwrap();
    assert_eq!(doc.scene_objects()[1].0, object(2));
    assert_eq!(
        doc.scene_objects()[1].2,
        &SceneObject::Caption(caption("caption"))
    );
    assert_eq!(doc.references().collect::<Vec<_>>(), vec![reference]);
}

#[test]
fn weight_and_style_only_edits_combine_with_every_script_and_undo_independently() {
    let mut doc = document();
    doc.apply(&[add(caption("Et H₂ e\u{301}"))]).unwrap();
    let original = doc.scene_objects()[1].2.clone();
    for script in [
        CaptionScript::Baseline,
        CaptionScript::Subscript,
        CaptionScript::Superscript,
    ] {
        for (weight, style) in [
            (CaptionWeight::Normal, CaptionStyle::Italic),
            (CaptionWeight::Bold, CaptionStyle::Normal),
            (CaptionWeight::Bold, CaptionStyle::Italic),
        ] {
            let mut styled = caption("Et H₂ e\u{301}");
            styled.runs[0].script = script;
            styled.runs[0].weight = weight;
            styled.runs[0].style = style;
            let expected = SceneObject::Caption(styled.clone());
            assert_eq!(
                doc.apply(&[Command::UpdateCaption {
                    object: object(2),
                    caption: styled
                }])
                .unwrap(),
                Outcome::Changed
            );
            assert_eq!(doc.scene_objects()[1].2, &expected);
            assert_eq!(doc.atoms().count(), 0);
            assert_eq!(doc.bonds().count(), 0);
            assert_eq!(doc.abbreviations().count(), 0);
            doc.undo().unwrap();
            assert_eq!(doc.scene_objects()[1].2, &original);
            doc.redo().unwrap();
            assert_eq!(doc.scene_objects()[1].2, &expected);
            doc.undo().unwrap();
        }
    }
}

#[test]
fn whole_caption_alignment_keeps_anchor_runs_graph_and_one_undo_step() {
    use chemifree_domain::scene::CaptionAlignment;

    let mut doc = document();
    let original = caption("  H e\u{301} 👩‍🔬  ");
    doc.apply(&[add(original.clone())]).unwrap();
    for alignment in [
        CaptionAlignment::Center,
        CaptionAlignment::Right,
        CaptionAlignment::Left,
    ] {
        let before = doc.clone();
        let mut next = original.clone();
        next.alignment = alignment;
        next.runs[0].script = CaptionScript::Superscript;
        next.runs[0].weight = CaptionWeight::Bold;
        next.runs[0].style = CaptionStyle::Italic;
        assert_eq!(
            doc.apply(&[Command::UpdateCaption {
                object: object(2),
                caption: next.clone()
            }])
            .unwrap(),
            Outcome::Changed
        );
        assert_eq!(
            doc.scene_objects()[1].2,
            &SceneObject::Caption(next.clone())
        );
        assert_eq!(next.anchor, original.anchor);
        assert_eq!(doc.atoms().count(), 0);
        assert_eq!(doc.bonds().count(), 0);
        assert_eq!(doc.abbreviations().count(), 0);
        let depth = doc.undo_depth();
        assert_eq!(
            doc.apply(&[Command::UpdateCaption {
                object: object(2),
                caption: next.clone()
            }])
            .unwrap(),
            Outcome::Unchanged
        );
        assert_eq!(doc.undo_depth(), depth);
        doc.undo().unwrap();
        assert_eq!(doc.scene_objects(), before.scene_objects());
        doc.redo().unwrap();
        assert_eq!(doc.scene_objects()[1].2, &SceneObject::Caption(next));
        doc.undo().unwrap();
    }
}

#[test]
fn font_only_replacement_is_one_edit_and_preserves_mixed_runs_and_graph() {
    use chemifree_domain::settings::LabelFont;
    let mut doc = document();
    let mut value = caption("  한글 e\u{301} 👩‍🔬 H  ");
    value.alignment = chemifree_domain::scene::CaptionAlignment::Right;
    value.runs[0].weight = CaptionWeight::Bold;
    value.runs[0].style = CaptionStyle::Italic;
    let mut script = value.runs[0].clone();
    script.script = CaptionScript::Subscript;
    script.text = "2".into();
    value.runs.push(script);
    doc.apply(&[add(value.clone())]).unwrap();
    for font in [
        Some(LabelFont::SansSerif),
        Some(LabelFont::Serif),
        Some(LabelFont::Monospace),
        None,
    ] {
        let before = value.clone();
        value.runs[0].font = font;
        let depth = doc.undo_depth();
        let command = Command::UpdateCaption {
            object: object(2),
            caption: value.clone(),
        };
        assert_eq!(doc.apply(&[command.clone()]).unwrap(), Outcome::Changed);
        assert_eq!(doc.undo_depth(), depth + 1);
        assert_eq!(
            doc.scene_objects()[1].2,
            &SceneObject::Caption(value.clone())
        );
        assert_eq!(doc.apply(&[command]).unwrap(), Outcome::Unchanged);
        doc.undo().unwrap();
        assert_eq!(doc.scene_objects()[1].2, &SceneObject::Caption(before));
        doc.redo().unwrap();
        assert_eq!(
            doc.scene_objects()[1].2,
            &SceneObject::Caption(value.clone())
        );
        assert_eq!(doc.atoms().count(), 0);
        assert_eq!(doc.bonds().count(), 0);
        assert_eq!(doc.abbreviations().count(), 0);
    }
}
