//! Native v9 caption data (introduced in v8) and lossless migration from the pre-caption v7 schema.

use std::fs;

use chemifree_application::{
    Application, CommandDto, DocumentRepository, Draft, RecoveryState, StorageError,
};
use chemifree_domain::document::{
    CAPTION_CONTENT_WEIGHT_LIMIT, CAPTION_HISTORY_WEIGHT_LIMIT, Command, Document, DocumentId,
    ObjectId, Point,
};
use chemifree_domain::scene::{Caption, CaptionRun, CaptionScript, SceneObject};
use chemifree_providers::{FileDocumentRepository, LocalCapabilities, MAX_DOCUMENT_BYTES};
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
                    weight: Default::default(),
                    style: Default::default(),
                },
                CaptionRun {
                    text: "2".into(),
                    script: CaptionScript::Subscript,
                    weight: Default::default(),
                    style: Default::default(),
                },
            ],
        },
        parent: None,
        index: None,
    }
}

#[test]
fn old_document_bytes_and_history_survive_read_and_first_v9_save() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let doc = document();
    repo.save(&doc, None).unwrap();
    let path = root.path().join("documents/caption-native.json");
    let mut stored: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(stored["format_version"], 10);
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
fn old_draft_bytes_are_preserved_before_the_first_changed_v9_draft() {
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
    let drafted = app.autosave("caption-native", Some("draft-v9")).unwrap();
    assert_eq!(
        drafted.session.draft_revision,
        Some(edited.document.revision)
    );
    let rewritten: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(rewritten["format_version"], 10);
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
fn v7_rejects_history_only_captions_and_v9_rejects_invalid_caption_states() {
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

#[test]
fn repeated_maximal_caption_updates_save_reload_and_keep_the_latest_undo() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let mut doc =
        Document::new(DocumentId::parse("caption-budget-store").unwrap(), "Budget").unwrap();
    let value = |character: char| Caption {
        anchor: Point::new(0.0, 0.0).unwrap(),
        runs: vec![
            CaptionRun {
                text: character.to_string().repeat(512),
                script: CaptionScript::Superscript,
                weight: Default::default(),
                style: Default::default(),
            };
            32
        ],
    };
    let create: Vec<_> = (0..100)
        .map(|_| Command::AddCaption {
            caption: value('"'),
            parent: None,
            index: None,
        })
        .collect();
    doc.apply(&create).unwrap();
    repo.save(&doc, None).unwrap();
    let path = root.path().join("documents/caption-budget-store.json");
    let mut stored_revision = doc.revision();
    // Without the new whole-history reservation, these escaped before/after strings alone
    // would exceed 64 MiB. Saving/reopening each edit must not strand accepted work.
    for round in 0..12 {
        let character = if round % 2 == 0 { '\\' } else { '"' };
        let edits: Vec<_> = (2..102)
            .map(|serial| Command::UpdateCaption {
                object: ObjectId::from_serial(serial).unwrap(),
                caption: value(character),
            })
            .collect();
        doc.apply(&edits).unwrap();
        assert_eq!(
            doc.undo_depth(),
            1,
            "only the newest large edit fits; round {round}"
        );
        assert!(
            doc.persistence_weight() <= CAPTION_CONTENT_WEIGHT_LIMIT + CAPTION_HISTORY_WEIGHT_LIMIT
        );
        repo.save(&doc, Some(stored_revision)).unwrap();
        stored_revision = doc.revision();
        let size = fs::metadata(&path).unwrap().len();
        assert!(
            size <= doc.persistence_weight() as u64 && size < MAX_DOCUMENT_BYTES,
            "round {round}: {size} bytes"
        );
        let loaded = repo.load(doc.id()).unwrap().unwrap();
        assert_eq!(
            loaded, doc,
            "round {round}: native history must not be trimmed again on reopen"
        );
        doc = loaded;
    }
    doc.undo().unwrap();
    assert_eq!(doc.scene_objects()[1].2, &SceneObject::Caption(value('\\')));
    repo.save(&doc, Some(stored_revision)).unwrap();
    stored_revision = doc.revision();
    doc = repo.load(doc.id()).unwrap().unwrap();
    assert_eq!((doc.undo_depth(), doc.redo_depth()), (0, 1));
    doc.redo().unwrap();
    assert_eq!(doc.scene_objects()[1].2, &SceneObject::Caption(value('"')));
    repo.save(&doc, Some(stored_revision)).unwrap();
    assert_eq!(repo.load(doc.id()).unwrap(), Some(doc.clone()));
    let draft = Draft {
        document: doc.clone(),
        base_revision: doc.revision(),
        saved_at_ms: u64::MAX,
    };
    repo.write_draft(&draft).unwrap();
    let draft_size = fs::metadata(root.path().join("drafts/caption-budget-store.json"))
        .unwrap()
        .len();
    assert!(draft_size <= doc.persistence_weight() as u64 && draft_size < MAX_DOCUMENT_BYTES);
    repo.release_draft(doc.id());
    assert_eq!(repo.claim_draft(doc.id()).unwrap(), Some(Ok(draft)));
}

// Build the exact pre-abbreviation schema instead of putting v9 fields behind an old header.
fn strip_abbreviations(value: &mut Value) {
    strip_caption_styles(value);
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

fn strip_caption_styles(value: &mut Value) {
    match value {
        Value::Object(object) => {
            if object.contains_key("script") && object.contains_key("text") {
                object.remove("weight");
                object.remove("style");
            }
            for child in object.values_mut() {
                strip_caption_styles(child);
            }
        }
        Value::Array(values) => {
            for child in values {
                strip_caption_styles(child);
            }
        }
        _ => {}
    }
}

#[test]
fn v9_caption_current_history_and_draft_migrate_without_rewriting_and_archive_exact_bytes() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let mut doc = document();
    doc.apply(&[caption()]).unwrap();
    doc.apply(&[Command::UpdateCaption {
        object: ObjectId::from_serial(2).unwrap(),
        caption: Caption {
            anchor: Point::new(7.0, 8.0).unwrap(),
            runs: vec![CaptionRun {
                text: "changed".into(),
                script: CaptionScript::Superscript,
                weight: Default::default(),
                style: Default::default(),
            }],
        },
    }])
    .unwrap();
    doc.undo().unwrap(); // Both undo and redo contain captions.
    repo.save(&doc, None).unwrap();
    let path = root.path().join("documents/caption-native.json");
    let mut v9: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    strip_caption_styles(&mut v9);
    v9["format_version"] = json!(9);
    let original = serde_json::to_vec_pretty(&v9).unwrap();
    fs::write(&path, &original).unwrap();
    let mut loaded = repo.load(doc.id()).unwrap().unwrap();
    assert_eq!(loaded, doc);
    assert_eq!(fs::read(&path).unwrap(), original);
    loaded.redo().unwrap();
    loaded.undo().unwrap();
    assert_eq!(loaded.scene_objects(), doc.scene_objects());
    repo.save(&loaded, Some(doc.revision())).unwrap();
    assert_eq!(fs::read(repo.migrated_path(doc.id(), 9)).unwrap(), original);
    assert_eq!(repo.load(doc.id()).unwrap(), Some(loaded.clone()));
    repo.write_draft(&Draft {
        document: loaded.clone(),
        base_revision: loaded.revision(),
        saved_at_ms: 12,
    })
    .unwrap();
    repo.release_draft(doc.id());
    let draft_path = root.path().join("drafts/caption-native.json");
    let mut draft: Value = serde_json::from_slice(&fs::read(&draft_path).unwrap()).unwrap();
    strip_caption_styles(&mut draft);
    draft["format_version"] = json!(9);
    let old_draft = serde_json::to_vec_pretty(&draft).unwrap();
    fs::write(&draft_path, &old_draft).unwrap();
    let claimed = repo.claim_draft(doc.id()).unwrap().unwrap().unwrap();
    assert_eq!(claimed.document, loaded);
    assert_eq!(fs::read(&draft_path).unwrap(), old_draft);
    // Claiming is read-only. Recovery preparation preserves the exact claimed bytes
    // before recovery may expose the document for editing or overwrite its draft.
    assert!(!repo.migrated_draft_path(doc.id(), 9).exists());
    repo.prepare_draft_for_recovery(
        doc.id(),
        chemifree_application::DraftRecoveryTarget::Original {
            base_revision: claimed.base_revision,
        },
    )
    .unwrap();
    assert_eq!(
        fs::read(repo.migrated_draft_path(doc.id(), 9)).unwrap(),
        old_draft
    );
    assert_eq!(fs::read(&draft_path).unwrap(), old_draft);
    let mut changed = claimed;
    changed.document.redo().unwrap();
    repo.write_draft(&changed).unwrap();
    assert_eq!(
        fs::read(repo.migrated_draft_path(doc.id(), 9)).unwrap(),
        old_draft
    );
    repo.release_draft(doc.id());
    assert_eq!(repo.claim_draft(doc.id()).unwrap(), Some(Ok(changed)));
}

#[test]
fn v10_requires_style_fields_and_v9_refuses_them_even_in_history_only_captions() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let mut doc = document();
    doc.apply(&[caption()]).unwrap();
    doc.undo().unwrap();
    repo.save(&doc, None).unwrap();
    let path = root.path().join("documents/caption-native.json");
    let original: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for (version, field, value) in [
        (10, "weight", None),
        (10, "style", Some(json!(null))),
        (10, "weight", Some(json!("heavy"))),
        (9, "weight", Some(json!("bold"))),
    ] {
        let mut invalid = original.clone();
        if version == 9 {
            strip_caption_styles(&mut invalid);
        }
        invalid["format_version"] = json!(version);
        let run = invalid["history"]["redo"][0]["objects"][0]["after"]["runs"][0]
            .as_object_mut()
            .unwrap();
        if let Some(value) = value {
            run.insert(field.into(), value);
        } else {
            run.remove(field);
        }
        let bytes = serde_json::to_vec(&invalid).unwrap();
        fs::write(&path, &bytes).unwrap();
        assert!(
            matches!(repo.load(doc.id()), Err(StorageError::Corrupt(_))),
            "version {version}, field {field}"
        );
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
}
