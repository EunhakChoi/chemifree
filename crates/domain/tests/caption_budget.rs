//! Large strings must not turn accepted caption edits into documents that cannot be saved.

use chemifree_domain::document::{
    CAPTION_CONTENT_WEIGHT_LIMIT, CAPTION_HISTORY_WEIGHT_LIMIT, Command, Document, DocumentId,
    EditError, ObjectId, Point,
};
use chemifree_domain::scene::{Caption, CaptionRun, CaptionScript, SceneObject};

fn widest(text: char) -> Caption {
    Caption {
        alignment: Default::default(),
        anchor: Point::new(0.0, 0.0).unwrap(),
        runs: vec![
            CaptionRun {
                underline: false,
                color: None,
                size_pt: None,
                font: None,
                text: text.to_string().repeat(512),
                script: CaptionScript::Superscript,
                weight: Default::default(),
                style: Default::default(),
            };
            32
        ],
    }
}
fn document(count: u32) -> Document {
    let mut doc = Document::new(DocumentId::parse("caption-budget").unwrap(), "Budget").unwrap();
    doc.apply(
        &(0..count)
            .map(|_| Command::AddCaption {
                caption: widest('"'),
                parent: None,
                index: None,
            })
            .collect::<Vec<_>>(),
    )
    .unwrap();
    doc
}
fn updates(count: u32, text: char) -> Vec<Command> {
    (2..count + 2)
        .map(|serial| Command::UpdateCaption {
            object: ObjectId::from_serial(serial).unwrap(),
            caption: widest(text),
        })
        .collect()
}
fn first(doc: &Document) -> &str {
    let SceneObject::Caption(caption) = doc.scene_objects()[1].2 else {
        panic!("caption fixture")
    };
    &caption.runs[0].text
}

#[test]
fn largest_single_caption_transaction_keeps_undo_and_the_next_size_refuses_atomically() {
    let mut doc = document(110);
    let before = doc.clone();
    let refused = doc.apply(&updates(110, '\\')).unwrap_err();
    assert_eq!(
        refused.index, None,
        "this is a whole-transaction reservation"
    );
    assert_eq!(refused.error, EditError::CaptionPersistenceLimit);
    assert_eq!(refused.error.reason(), Some("caption_persistence_limit"));
    assert_eq!(doc, before);
    // At 160 metadata bytes per run, each complete before/after update reserves
    // 76,352 bytes. With 576 bytes of entry overhead, 109 fit 8 MiB;
    // the 110th does not.
    doc.apply(&updates(109, '\\')).unwrap();
    assert_eq!(
        doc.undo_depth(),
        1,
        "old creation history makes room, but the accepted edit remains undoable"
    );
    assert!(
        doc.persistence_weight() <= CAPTION_CONTENT_WEIGHT_LIMIT + CAPTION_HISTORY_WEIGHT_LIMIT
    );
    assert_eq!(Document::restore(doc.to_parts()).unwrap(), doc);
    doc.undo().unwrap();
    assert_eq!(first(&doc), "\"".repeat(512));
    assert_eq!(
        Document::restore(doc.to_parts()).unwrap(),
        doc,
        "moving the same entry into redo does not change its reservation"
    );
    doc.redo().unwrap();
    assert_eq!(first(&doc), "\\".repeat(512));
}

#[test]
fn maximum_command_count_does_not_bypass_caption_transaction_size() {
    let mut doc = Document::new(DocumentId::parse("caption-budget").unwrap(), "Budget").unwrap();
    let before = doc.clone();
    let commands: Vec<_> = (0..500)
        .map(|_| Command::AddCaption {
            caption: widest('"'),
            parent: None,
            index: None,
        })
        .collect();
    let refused = doc.apply(&commands).unwrap_err();
    assert_eq!(refused.error, EditError::CaptionPersistenceLimit);
    assert_eq!(
        doc, before,
        "no ID, content or history can be committed without a persistable undo entry"
    );
}

#[test]
fn restoring_looser_history_trims_oldest_undo_then_farthest_redo() {
    let mut doc = document(100);
    let initial = doc.to_parts();
    let mut changes = Vec::new();
    for text in ['\\', '"', '\\'] {
        doc.apply(&updates(100, text)).unwrap();
        changes.push(doc.to_parts().undo.last().unwrap().clone());
    }
    let mut parts = doc.to_parts();
    parts.undo = changes.clone();
    let restored = Document::restore(parts).unwrap();
    assert_eq!(
        restored, doc,
        "only the nearest undo survives the 8 MiB whole-history budget"
    );
    let mut parts = initial;
    parts.undo.clear();
    parts.redo = changes.into_iter().rev().collect();
    let mut restored = Document::restore(parts).unwrap();
    assert_eq!((restored.undo_depth(), restored.redo_depth()), (0, 1));
    assert_eq!(first(&restored), "\"".repeat(512));
    restored.redo().unwrap();
    assert_eq!(first(&restored), "\\".repeat(512));
}

#[test]
fn reservations_count_utf8_bytes_and_escape_overhead_without_changing_text_limits() {
    let mut ascii = Document::new(DocumentId::parse("caption-budget").unwrap(), "Budget").unwrap();
    let mut unicode = ascii.clone();
    let caption = |text: String| Command::AddCaption {
        caption: Caption {
            alignment: Default::default(),
            anchor: Point::new(0.0, 0.0).unwrap(),
            runs: vec![CaptionRun {
                underline: false,
                color: None,
                size_pt: None,
                font: None,
                text,
                script: CaptionScript::Baseline,
                weight: Default::default(),
                style: Default::default(),
            }],
        },
        parent: None,
        index: None,
    };
    ascii.apply(&[caption("\"".repeat(16_384))]).unwrap();
    unicode
        .apply(&[caption(format!("{}a", "한".repeat(5461)))])
        .unwrap();
    assert_eq!(
        ascii.persistence_weight(),
        unicode.persistence_weight(),
        "equal UTF-8 payload sizes get equal conservative escape reservations"
    );
    assert_eq!(first(&unicode).len(), 16_384);
    let before = unicode.clone();
    let refused = unicode.apply(&[caption("한".repeat(5462))]).unwrap_err();
    assert_eq!(
        refused.error,
        EditError::CaptionInvalid,
        "the existing 16 KiB per-caption limit still applies first"
    );
    assert_eq!(unicode, before);
}
