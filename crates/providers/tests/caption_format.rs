//! Native v8 caption data and lossless migration from the pre-caption v7 schema.

use std::fs;

use chemifree_application::{Application, DocumentRepository, Draft, RecoveryState, StorageError};
use chemifree_domain::document::{Command, Document, DocumentId, Point};
use chemifree_domain::scene::{Caption, CaptionRun, CaptionScript};
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};

fn document() -> Document {
    let mut doc = Document::new(DocumentId::parse("caption-native").unwrap(), "Before").unwrap();
    doc.apply(&[Command::Rename {
        title: "After".into(),
    }])
    .unwrap();
    doc.apply(&[Command::Rename {
        title: "Redo boundary".into(),
    }])
    .unwrap();
    doc.undo().unwrap();
    doc
}
fn caption() -> Command {
    Command::AddCaption {
        caption: Caption {
            anchor: Point::new(-0.0, 4.0).unwrap(),
            runs: vec![
                CaptionRun {
                    text: "한글 👩‍🔬 e\u{301}".into(),
                    script: CaptionScript::Baseline,
                },
                CaptionRun {
                    text: "2".into(),
                    script: CaptionScript::Subscript,
                },
            ],
        },
        parent: None,
        index: None,
    }
}

#[test]
fn old_document_bytes_and_history_survive_read_and_first_v8_save() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let doc = document();
    repo.save(&doc, None).unwrap();
    let path = root.path().join("documents/caption-native.json");
    let mut stored: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(stored["format_version"], 9);
    strip_abbreviations(&mut stored);
    stored["format_version"] = json!(7);
    let original = serde_json::to_vec_pretty(&stored).unwrap();
    fs::write(&path, &original).unwrap();
    let mut loaded = repo.load(doc.id()).unwrap().unwrap();
    assert_eq!(loaded, doc);
    assert_eq!(
        fs::read(&path).unwrap(),
        original,
        "reading must not rewrite v7"
    );
    loaded.redo().unwrap();
    assert_eq!(loaded.title(), "Redo boundary");
    loaded.undo().unwrap();
    loaded.apply(&[caption()]).unwrap();
    repo.save(&loaded, Some(doc.revision())).unwrap();
    assert_eq!(fs::read(repo.migrated_path(doc.id(), 7)).unwrap(), original);
    assert_eq!(repo.load(doc.id()).unwrap(), Some(loaded));
}

#[test]
fn old_draft_bytes_are_preserved_before_recovery_writes_v8() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let mut doc = document();
    repo.save(&doc, None).unwrap();
    let base_revision = doc.revision();
    doc.apply(&[Command::Rename {
        title: "Draft only".into(),
    }])
    .unwrap();
    repo.write_draft(&Draft {
        document: doc.clone(),
        base_revision,
        saved_at_ms: 123,
    })
    .unwrap();
    repo.release_draft(doc.id());
    let path = root.path().join("drafts/caption-native.json");
    let mut stored: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    strip_abbreviations(&mut stored);
    stored["format_version"] = json!(7);
    let original = serde_json::to_vec(&stored).unwrap();
    fs::write(&path, &original).unwrap();
    let app = Application::new(LocalCapabilities, FileDocumentRepository::new(root.path()));
    assert_eq!(
        app.document("caption-native")
            .unwrap()
            .recovery
            .unwrap()
            .state,
        RecoveryState::Recoverable
    );
    assert_eq!(fs::read(&path).unwrap(), original);
    let recovered = app
        .recover("caption-native", Some("recover-old-caption-format"))
        .unwrap();
    assert_eq!(recovered.document.title, "Draft only");
    assert_eq!(
        fs::read(repo.migrated_draft_path(doc.id(), 7)).unwrap(),
        original
    );
    app.autosave("caption-native", Some("draft-v8")).unwrap();
    let rewritten: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(rewritten["format_version"], 9);
}

#[test]
fn v7_rejects_history_only_captions_and_v8_rejects_invalid_caption_states() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let mut doc = document();
    doc.apply(&[caption()]).unwrap();
    doc.undo().unwrap();
    repo.save(&doc, None).unwrap();
    let path = root.path().join("documents/caption-native.json");
    let original: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let mut old = original.clone();
    strip_abbreviations(&mut old);
    old["format_version"] = json!(7);
    fs::write(&path, serde_json::to_vec(&old).unwrap()).unwrap();
    assert!(
        matches!(repo.load(doc.id()), Err(StorageError::Corrupt(message)) if message.contains("version 8"))
    );
    let mut invalid = original;
    invalid["history"]["redo"][0]["objects"][0]["after"]["runs"][0]["script"] = json!("auto");
    fs::write(&path, serde_json::to_vec(&invalid).unwrap()).unwrap();
    assert!(
        matches!(repo.load(doc.id()), Err(StorageError::Corrupt(message)) if message.contains("auto"))
    );
}

// Build the exact pre-abbreviation schema instead of putting v9 fields behind an old header.
fn strip_abbreviations(value: &mut Value) {
    value["document"]
        .as_object_mut()
        .unwrap()
        .remove("abbreviations");
    for stack in ["undo", "redo"] {
        for entry in value["history"][stack].as_array_mut().unwrap() {
            entry.as_object_mut().unwrap().remove("abbreviations");
        }
    }
}
