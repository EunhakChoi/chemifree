//! Native v8 caption data and lossless migration from the pre-caption v7 schema.

use std::fs;

use chemifree_application::{
    Application, CommandDto, DocumentRepository, Draft, RecoveryState, StorageError,
};
use chemifree_domain::document::{Command, Document, DocumentId, Point};
use chemifree_domain::scene::{Caption, CaptionRun, CaptionScript, SceneObject};
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
    assert_eq!(stored["format_version"], 8);
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
fn old_draft_bytes_are_preserved_before_the_first_changed_v8_draft() {
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
    // Recovery already owns a complete durable draft. With no edit, autosave intentionally
    // writes nothing; migrating in memory does not authorize a needless file replacement.
    let unchanged = app
        .autosave("caption-native", Some("already-drafted"))
        .unwrap();
    assert_eq!(
        unchanged.session.draft_revision,
        Some(recovered.document.revision)
    );
    assert_eq!(fs::read(&path).unwrap(), original);
    let command: CommandDto = serde_json::from_value(json!({
        "op":"add_caption","anchor":{"x":-0.0,"y":4.0},"runs":[
            {"text":"한글 👩‍🔬 e\u{301}","script":"baseline"},
            {"text":"2","script":"subscript"}
        ]
    }))
    .unwrap();
    let edited = app
        .apply_transaction(
            "caption-native",
            Some("caption-after-recovery"),
            recovered.document.revision,
            vec![command],
        )
        .unwrap();
    let drafted = app.autosave("caption-native", Some("draft-v8")).unwrap();
    assert_eq!(
        drafted.session.draft_revision,
        Some(edited.document.revision)
    );
    let rewritten: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(rewritten["format_version"], 8);
    assert_eq!(rewritten["document"]["revision"], edited.document.revision);
    assert_eq!(
        fs::read(repo.migrated_draft_path(doc.id(), 7)).unwrap(),
        original
    );
    drop(app);
    let mut restored = repo
        .claim_draft(doc.id())
        .unwrap()
        .unwrap()
        .unwrap()
        .document;
    let Command::AddCaption {
        caption: expected, ..
    } = caption()
    else {
        panic!("fixture must create a caption")
    };
    assert_eq!(
        restored.scene_objects()[1].2,
        &SceneObject::Caption(expected.clone())
    );
    restored.undo().unwrap();
    assert_eq!(restored.scene_objects().len(), 1);
    assert_eq!(restored.title(), "Draft only");
    restored.undo().unwrap();
    assert_eq!(
        restored.title(),
        "After",
        "the old v7 draft history must remain usable"
    );
    restored.redo().unwrap();
    assert_eq!(restored.title(), "Draft only");
    restored.redo().unwrap();
    assert_eq!(
        restored.scene_objects()[1].2,
        &SceneObject::Caption(expected)
    );
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
