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
            alignment: Default::default(),
            anchor: Point::new(-0.0, 4.0).unwrap(),
            runs: vec![
                CaptionRun {
                    shadow: false,
                    outline: false,
                    underline: false,
                    color: None,
                    size_pt: None,
                    font: None,
                    text: "한글 👩‍🔬 e\u{301}".into(),
                    script: CaptionScript::Baseline,
                    weight: Default::default(),
                    style: Default::default(),
                },
                CaptionRun {
                    shadow: false,
                    outline: false,
                    underline: false,
                    color: None,
                    size_pt: None,
                    font: None,
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
    strip_caption_shadows(&mut stored);
    assert_eq!(stored["format_version"], 18);
    strip_abbreviations(&mut stored);
    strip_label_weights(&mut stored);
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
    strip_caption_shadows(&mut stored);
    strip_abbreviations(&mut stored);
    strip_label_weights(&mut stored);
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
    assert_eq!(rewritten["format_version"], 18);
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
    strip_label_weights(&mut old);
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
        alignment: Default::default(),
        anchor: Point::new(0.0, 0.0).unwrap(),
        runs: vec![
            CaptionRun {
                shadow: false,
                outline: true,
                underline: false,
                color: Some("#ABCDEF".into()),
                size_pt: Some(4.000_000_000_000_003_6),
                font: Some(chemifree_domain::settings::LabelFont::SansSerif),
                text: character.to_string().repeat(512),
                script: CaptionScript::Superscript,
                weight: Default::default(),
                style: chemifree_domain::scene::CaptionStyle::Italic,
            };
            32
        ],
    };
    let create: Vec<_> = (0..106)
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
        let edits: Vec<_> = (2..108)
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
            if object.get("kind").and_then(Value::as_str) == Some("caption") {
                object.remove("alignment");
            }
            if object.contains_key("script") && object.contains_key("text") {
                object.remove("outline");
                object.remove("shadow");
                object.remove("underline");
                object.remove("color");
                object.remove("size_pt");
                object.remove("font");
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
            alignment: Default::default(),
            anchor: Point::new(7.0, 8.0).unwrap(),
            runs: vec![CaptionRun {
                shadow: false,
                outline: false,
                underline: false,
                color: None,
                size_pt: None,
                font: None,
                text: "changed".into(),
                script: CaptionScript::Superscript,
                weight: Default::default(),
                // V9 has no style field, so both history directions must use its normal default.
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
    strip_label_weights(&mut v9);
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
    strip_label_weights(&mut draft);
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
        strip_caption_alignment(&mut invalid);
        if version == 9 {
            strip_caption_styles(&mut invalid);
        }
        if version < 18 {
            strip_label_weights(&mut invalid);
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

fn strip_caption_alignment(value: &mut Value) {
    strip_caption_fonts(value);
    match value {
        Value::Object(object) => {
            if object.get("kind").and_then(Value::as_str) == Some("caption") {
                object.remove("alignment");
            }
            for child in object.values_mut() {
                strip_caption_alignment(child);
            }
        }
        Value::Array(values) => {
            for child in values {
                strip_caption_alignment(child);
            }
        }
        _ => {}
    }
}

#[test]
fn v8_to_v15_caption_migration_keeps_current_history_and_exact_document_and_draft_bytes() {
    for version in 8..=15 {
        for draft in [false, true] {
            // Document migration commits advance the saved revision; draft recovery needs
            // its own unchanged base document and matching saved stamp.
            let root = tempfile::tempdir().unwrap();
            let repo = FileDocumentRepository::new(root.path());
            let mut doc = document();
            doc.apply(&[caption()]).unwrap();
            let mut changed = match caption() {
                Command::AddCaption { caption, .. } => caption,
                _ => unreachable!(),
            };
            changed.runs[0].text = "redo text".into();
            doc.apply(&[Command::UpdateCaption {
                object: ObjectId::from_serial(2).unwrap(),
                caption: changed,
            }])
            .unwrap();
            doc.undo().unwrap();
            repo.save(&doc, None).unwrap();
            repo.write_draft(&Draft {
                document: doc.clone(),
                base_revision: doc.revision(),
                saved_at_ms: 42,
            })
            .unwrap();
            repo.release_draft(doc.id());
            let path = root.path().join(if draft {
                "drafts/caption-native.json"
            } else {
                "documents/caption-native.json"
            });
            let mut stored: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            strip_caption_shadows(&mut stored);
            strip_caption_outlines(&mut stored);
            if version < 15 {
                strip_caption_underlines(&mut stored);
            }
            if version < 14 {
                strip_caption_colors(&mut stored);
            }
            if version < 13 {
                strip_caption_sizes(&mut stored);
            }
            if version < 12 {
                strip_caption_fonts(&mut stored);
            }
            if version < 11 {
                strip_caption_alignment(&mut stored);
            }
            if version < 10 {
                strip_caption_styles(&mut stored);
            }
            if version == 8 {
                stored["document"]
                    .as_object_mut()
                    .unwrap()
                    .remove("abbreviations");
                for stack in ["undo", "redo"] {
                    for entry in stored["history"][stack].as_array_mut().unwrap() {
                        entry.as_object_mut().unwrap().remove("abbreviations");
                    }
                }
            }
            if version < 18 {
                strip_label_weights(&mut stored);
            }
            stored["format_version"] = json!(version);
            let bytes = serde_json::to_vec_pretty(&stored).unwrap();
            fs::write(&path, &bytes).unwrap();
            let mut loaded = if draft {
                repo.claim_draft(doc.id())
                    .unwrap()
                    .unwrap()
                    .unwrap()
                    .document
            } else {
                repo.load(doc.id()).unwrap().unwrap()
            };
            assert_eq!(loaded, doc, "v{version}, draft={draft}");
            assert_eq!(fs::read(&path).unwrap(), bytes);
            loaded.redo().unwrap();
            loaded.undo().unwrap();
            assert_eq!(loaded.scene_objects(), doc.scene_objects());
            if draft {
                repo.prepare_draft_for_recovery(
                    doc.id(),
                    chemifree_application::DraftRecoveryTarget::Original {
                        base_revision: doc.revision(),
                    },
                )
                .unwrap();
                repo.write_draft(&Draft {
                    document: loaded,
                    base_revision: doc.revision(),
                    saved_at_ms: 43,
                })
                .unwrap();
                assert_eq!(
                    fs::read(repo.migrated_draft_path(doc.id(), version)).unwrap(),
                    bytes
                );
                repo.release_draft(doc.id());
            } else {
                repo.save(&loaded, Some(doc.revision())).unwrap();
                assert_eq!(
                    fs::read(repo.migrated_path(doc.id(), version)).unwrap(),
                    bytes
                );
            }
        }
    }
}

#[test]
fn alignment_is_strict_in_current_and_history_for_documents_and_drafts() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let mut doc = document();
    doc.apply(&[caption()]).unwrap();
    repo.save(&doc, None).unwrap();
    let path = root.path().join("documents/caption-native.json");
    let with_current: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    doc.undo().unwrap();
    repo.save(&doc, Some(doc.revision() - 1)).unwrap();
    let history_only: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for history in [false, true] {
        for version in [8, 9, 10, 11] {
            for value in [
                None,
                Some(json!(null)),
                Some(json!("middle")),
                Some(json!(3)),
                Some(json!("center")),
            ] {
                let mut stored = if history {
                    history_only.clone()
                } else {
                    with_current.clone()
                };
                strip_caption_shadows(&mut stored);
                strip_caption_alignment(&mut stored);
                if version < 10 {
                    strip_caption_styles(&mut stored);
                }
                if version == 8 {
                    stored["document"]
                        .as_object_mut()
                        .unwrap()
                        .remove("abbreviations");
                    for stack in ["undo", "redo"] {
                        for entry in stored["history"][stack].as_array_mut().unwrap() {
                            entry.as_object_mut().unwrap().remove("abbreviations");
                        }
                    }
                }
                if version < 18 {
                    strip_label_weights(&mut stored);
                }
                stored["format_version"] = json!(version);
                // v11 needs alignment in every caption, before changing the chosen state.
                if version == 11 {
                    fn fill(value: &mut Value) {
                        match value {
                            Value::Object(object) => {
                                if object.get("kind").and_then(Value::as_str) == Some("caption") {
                                    object.insert("alignment".into(), json!("center"));
                                }
                                for child in object.values_mut() {
                                    fill(child);
                                }
                            }
                            Value::Array(values) => {
                                for child in values {
                                    fill(child);
                                }
                            }
                            _ => {}
                        }
                    }
                    fill(&mut stored);
                }
                let state = if history {
                    &mut stored["history"]["redo"][0]["objects"][0]["after"]
                } else {
                    &mut stored["document"]["objects"][1]["state"]
                };
                if let Some(value) = &value {
                    state["alignment"] = value.clone();
                } else {
                    state.as_object_mut().unwrap().remove("alignment");
                }
                let valid = (version < 11 && value.is_none())
                    || (version == 11 && value == Some(json!("center")));
                let bytes = serde_json::to_vec(&stored).unwrap();
                fs::write(&path, &bytes).unwrap();
                assert_eq!(
                    repo.load(doc.id()).is_ok(),
                    valid,
                    "v{version}, history={history}, value={value:?}"
                );
                assert_eq!(fs::read(&path).unwrap(), bytes);
                stored["format"] = json!("chemifree.draft");
                stored["base_revision"] = json!(0);
                stored["saved_at_ms"] = json!(42);
                let draft_path = root.path().join("drafts/caption-native.json");
                fs::create_dir_all(root.path().join("drafts")).unwrap();
                let bytes = serde_json::to_vec(&stored).unwrap();
                fs::write(&draft_path, &bytes).unwrap();
                assert_eq!(
                    repo.claim_draft(doc.id()).unwrap().unwrap().is_ok(),
                    valid,
                    "draft v{version}, history={history}, value={value:?}"
                );
                repo.release_draft(doc.id());
                assert_eq!(fs::read(&draft_path).unwrap(), bytes);
            }
        }
    }
}

fn strip_caption_fonts(value: &mut Value) {
    match value {
        Value::Object(object) => {
            if object.contains_key("script") && object.contains_key("text") {
                object.remove("outline");
                object.remove("shadow");
                object.remove("underline");
                object.remove("color");
                object.remove("size_pt");
                object.remove("font");
            }
            for child in object.values_mut() {
                strip_caption_fonts(child);
            }
        }
        Value::Array(values) => {
            for child in values {
                strip_caption_fonts(child);
            }
        }
        _ => {}
    }
}

#[test]
fn font_fields_are_strict_in_current_and_history_in_documents_and_drafts() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let mut doc = document();
    doc.apply(&[caption()]).unwrap();
    repo.save(&doc, None).unwrap();
    let path = root.path().join("documents/caption-native.json");
    let mut current: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    current["history"]["undo"] = json!([]);
    current["history"]["redo"] = json!([]);
    doc.undo().unwrap();
    repo.save(&doc, Some(doc.revision() - 1)).unwrap();
    let history_only: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for history in [false, true] {
        for version in 8..=16 {
            for font in [
                None,
                Some(json!(null)),
                Some(json!("sans-serif")),
                Some(json!("serif")),
                Some(json!("monospace")),
                Some(json!("Arial")),
                Some(json!(3)),
                Some(json!({})),
                Some(json!([])),
            ] {
                let mut stored = if history {
                    history_only.clone()
                } else {
                    current.clone()
                };
                strip_caption_shadows(&mut stored);
                if version < 16 {
                    strip_caption_outlines(&mut stored);
                }
                if version < 15 {
                    strip_caption_underlines(&mut stored);
                }
                if version < 14 {
                    strip_caption_colors(&mut stored);
                }
                if version < 13 {
                    strip_caption_sizes(&mut stored);
                }
                if version < 12 {
                    strip_caption_fonts(&mut stored);
                }
                if version < 11 {
                    strip_caption_alignment(&mut stored);
                }
                if version < 10 {
                    strip_caption_styles(&mut stored);
                }
                if version == 8 {
                    stored["document"]
                        .as_object_mut()
                        .unwrap()
                        .remove("abbreviations");
                    for stack in ["undo", "redo"] {
                        for entry in stored["history"][stack].as_array_mut().unwrap() {
                            entry.as_object_mut().unwrap().remove("abbreviations");
                        }
                    }
                }
                if version < 18 {
                    strip_label_weights(&mut stored);
                }
                stored["format_version"] = json!(version);
                let state = if history {
                    &mut stored["history"]["redo"][0]["objects"][0]["after"]
                } else {
                    &mut stored["document"]["objects"][1]["state"]
                };
                let run = state["runs"][0].as_object_mut().unwrap();
                if let Some(font) = &font {
                    run.insert("font".into(), font.clone());
                } else {
                    run.remove("font");
                }
                let valid = if version < 12 {
                    font.is_none()
                } else {
                    matches!(&font, Some(Value::Null))
                        || [
                            Some(json!("sans-serif")),
                            Some(json!("serif")),
                            Some(json!("monospace")),
                        ]
                        .contains(&font)
                };
                let bytes = serde_json::to_vec(&stored).unwrap();
                fs::write(&path, &bytes).unwrap();
                assert_eq!(
                    repo.load(doc.id()).is_ok(),
                    valid,
                    "v{version}, history={history}, font={font:?}"
                );
                assert_eq!(fs::read(&path).unwrap(), bytes);
                stored["format"] = json!("chemifree.draft");
                stored["base_revision"] = json!(0);
                stored["saved_at_ms"] = json!(42);
                let draft_path = root.path().join("drafts/caption-native.json");
                fs::create_dir_all(root.path().join("drafts")).unwrap();
                let bytes = serde_json::to_vec(&stored).unwrap();
                fs::write(&draft_path, &bytes).unwrap();
                assert_eq!(
                    repo.claim_draft(doc.id()).unwrap().unwrap().is_ok(),
                    valid,
                    "draft v{version}, history={history}, font={font:?}"
                );
                repo.release_draft(doc.id());
                assert_eq!(fs::read(&draft_path).unwrap(), bytes);
            }
        }
    }
}

fn strip_caption_sizes(value: &mut Value) {
    strip_caption_underlines(value);
    match value {
        Value::Object(object) => {
            if object.contains_key("script") && object.contains_key("text") {
                object.remove("size_pt");
            }
            for child in object.values_mut() {
                strip_caption_sizes(child);
            }
        }
        Value::Array(values) => {
            for child in values {
                strip_caption_sizes(child);
            }
        }
        _ => {}
    }
}

#[test]
fn size_fields_are_strict_in_current_history_documents_and_drafts_and_never_rewrite_refusals() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let mut doc = document();
    doc.apply(&[caption()]).unwrap();
    repo.save(&doc, None).unwrap();
    let path = root.path().join("documents/caption-native.json");
    let mut current: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    current["history"]["undo"] = json!([]);
    current["history"]["redo"] = json!([]);
    doc.undo().unwrap();
    repo.save(&doc, Some(doc.revision() - 1)).unwrap();
    let history_only: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for history in [false, true] {
        for version in 8..=16 {
            for size in [
                None,
                Some(json!(null)),
                Some(json!(4)),
                Some(json!(72)),
                Some(json!(12.5)),
                Some(json!(3.999)),
                Some(json!(72.001)),
                Some(json!("12")),
                Some(json!(false)),
                Some(json!([])),
                Some(json!({})),
            ] {
                let mut stored = if history {
                    history_only.clone()
                } else {
                    current.clone()
                };
                strip_caption_shadows(&mut stored);
                if version < 16 {
                    strip_caption_outlines(&mut stored);
                }
                if version < 15 {
                    strip_caption_underlines(&mut stored);
                }
                if version < 14 {
                    strip_caption_colors(&mut stored);
                }
                if version < 13 {
                    strip_caption_sizes(&mut stored);
                }
                if version < 12 {
                    strip_caption_fonts(&mut stored);
                }
                if version < 11 {
                    strip_caption_alignment(&mut stored);
                }
                if version < 10 {
                    strip_caption_styles(&mut stored);
                }
                if version == 8 {
                    strip_abbreviations(&mut stored);
                }
                if version < 18 {
                    strip_label_weights(&mut stored);
                }
                stored["format_version"] = json!(version);
                let state = if history {
                    &mut stored["history"]["redo"][0]["objects"][0]["after"]
                } else {
                    &mut stored["document"]["objects"][1]["state"]
                };
                state["runs"][0].as_object_mut().unwrap().remove("size_pt");
                if let Some(size) = &size {
                    state["runs"][0]["size_pt"] = size.clone();
                }
                // All runs in v13 need an explicit nullable size, including untargeted runs.
                if version >= 13 {
                    state["runs"][1]["size_pt"] = Value::Null;
                }
                let valid = if version < 13 {
                    size.is_none()
                } else {
                    matches!(size, Some(Value::Null))
                        || size
                            .as_ref()
                            .and_then(Value::as_f64)
                            .is_some_and(|size| (4.0..=72.0).contains(&size))
                };
                let bytes = serde_json::to_vec(&stored).unwrap();
                fs::write(&path, &bytes).unwrap();
                assert_eq!(
                    repo.load(doc.id()).is_ok(),
                    valid,
                    "v{version} history={history} size={size:?}"
                );
                assert_eq!(fs::read(&path).unwrap(), bytes);
                stored["format"] = json!("chemifree.draft");
                stored["base_revision"] = json!(0);
                stored["saved_at_ms"] = json!(42);
                let draft_path = root.path().join("drafts/caption-native.json");
                fs::create_dir_all(root.path().join("drafts")).unwrap();
                let bytes = serde_json::to_vec(&stored).unwrap();
                fs::write(&draft_path, &bytes).unwrap();
                assert_eq!(
                    repo.claim_draft(doc.id()).unwrap().unwrap().is_ok(),
                    valid,
                    "draft v{version} history={history} size={size:?}"
                );
                repo.release_draft(doc.id());
                assert_eq!(fs::read(&draft_path).unwrap(), bytes);
            }
        }
    }
}

#[test]
fn v13_mixed_size_runs_save_reopen_and_recover_with_undo_history() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let mut doc = document();
    let Command::AddCaption { mut caption, .. } = caption() else {
        unreachable!()
    };
    caption.runs[1].size_pt = Some(18.5);
    caption.runs[1].font = Some(chemifree_domain::settings::LabelFont::Monospace);
    caption.runs[1].weight = chemifree_domain::scene::CaptionWeight::Bold;
    caption.runs[1].style = chemifree_domain::scene::CaptionStyle::Italic;
    doc.apply(&[Command::AddCaption {
        caption: caption.clone(),
        parent: None,
        index: None,
    }])
    .unwrap();
    repo.save(&doc, None).unwrap();
    assert_eq!(repo.load(doc.id()).unwrap(), Some(doc.clone()));
    let base_revision = doc.revision();
    caption.runs[1].size_pt = Some(72.0);
    doc.apply(&[Command::UpdateCaption {
        object: ObjectId::from_serial(2).unwrap(),
        caption,
    }])
    .unwrap();
    repo.write_draft(&Draft {
        document: doc.clone(),
        base_revision,
        saved_at_ms: 42,
    })
    .unwrap();
    repo.release_draft(doc.id());
    let app = Application::new(LocalCapabilities, FileDocumentRepository::new(root.path()));
    let recovered = app
        .recover(doc.id().as_str(), Some("recover-sized-caption"))
        .unwrap();
    let snapshot = serde_json::to_value(&recovered.document).unwrap();
    assert_eq!(
        snapshot["scene"]["objects"][1]["runs"][0]["size_pt"],
        Value::Null
    );
    assert_eq!(snapshot["scene"]["objects"][1]["runs"][1]["size_pt"], 72.0);
    let undone = app
        .undo(
            doc.id().as_str(),
            Some("undo-sized-caption"),
            recovered.document.revision,
        )
        .unwrap();
    let snapshot = serde_json::to_value(undone.document).unwrap();
    assert_eq!(snapshot["scene"]["objects"][1]["runs"][1]["size_pt"], 18.5);
    assert_eq!(
        snapshot["scene"]["objects"][1]["runs"][1]["font"],
        "monospace"
    );
}

fn strip_caption_colors(value: &mut Value) {
    match value {
        Value::Object(object) => {
            if object.contains_key("script") && object.contains_key("text") {
                object.remove("outline");
                object.remove("shadow");
                object.remove("underline");
                object.remove("color");
            }
            for child in object.values_mut() {
                strip_caption_colors(child);
            }
        }
        Value::Array(values) => {
            for child in values {
                strip_caption_colors(child);
            }
        }
        _ => {}
    }
}

#[test]
fn color_fields_are_strict_in_current_and_history_in_documents_and_drafts() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let mut doc = document();
    doc.apply(&[caption()]).unwrap();
    repo.save(&doc, None).unwrap();
    let path = root.path().join("documents/caption-native.json");
    let mut current: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    current["history"]["undo"] = json!([]);
    current["history"]["redo"] = json!([]);
    doc.undo().unwrap();
    repo.save(&doc, Some(doc.revision() - 1)).unwrap();
    let history_only: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for history in [false, true] {
        for version in 8..=16 {
            for color in [
                None,
                Some(json!(null)),
                Some(json!("#000000")),
                Some(json!("#FFFFFF")),
                Some(json!("#2A7BCD")),
                Some(json!("#abcdef")),
                Some(json!("#12345678")),
                Some(json!("red")),
                Some(json!("var(--foreground)")),
                Some(json!("url(javascript:alert(1))")),
                Some(json!(0)),
                Some(json!(false)),
                Some(json!([])),
                Some(json!({})),
            ] {
                let mut stored = if history {
                    history_only.clone()
                } else {
                    current.clone()
                };
                strip_caption_shadows(&mut stored);
                if version < 16 {
                    strip_caption_outlines(&mut stored);
                }
                if version < 15 {
                    strip_caption_underlines(&mut stored);
                }
                if version < 14 {
                    strip_caption_colors(&mut stored);
                }
                if version < 13 {
                    strip_caption_sizes(&mut stored);
                }
                if version < 12 {
                    strip_caption_fonts(&mut stored);
                }
                if version < 11 {
                    strip_caption_alignment(&mut stored);
                }
                if version < 10 {
                    strip_caption_styles(&mut stored);
                }
                if version == 8 {
                    strip_abbreviations(&mut stored);
                }
                if version < 18 {
                    strip_label_weights(&mut stored);
                }
                stored["format_version"] = json!(version);
                let state = if history {
                    &mut stored["history"]["redo"][0]["objects"][0]["after"]
                } else {
                    &mut stored["document"]["objects"][1]["state"]
                };
                state["runs"][0].as_object_mut().unwrap().remove("color");
                if let Some(color) = &color {
                    state["runs"][0]["color"] = color.clone();
                }
                if version == 14 {
                    state["runs"][1]["color"] = Value::Null;
                }
                let valid = if version < 14 {
                    color.is_none()
                } else {
                    matches!(color, Some(Value::Null))
                        || color
                            .as_ref()
                            .and_then(Value::as_str)
                            .is_some_and(|color| ["#000000", "#FFFFFF", "#2A7BCD"].contains(&color))
                };
                let bytes = serde_json::to_vec(&stored).unwrap();
                fs::write(&path, &bytes).unwrap();
                assert_eq!(
                    repo.load(doc.id()).is_ok(),
                    valid,
                    "v{version} history={history} color={color:?}"
                );
                assert_eq!(fs::read(&path).unwrap(), bytes);
                stored["format"] = json!("chemifree.draft");
                stored["base_revision"] = json!(0);
                stored["saved_at_ms"] = json!(42);
                let draft_path = root.path().join("drafts/caption-native.json");
                fs::create_dir_all(root.path().join("drafts")).unwrap();
                let bytes = serde_json::to_vec(&stored).unwrap();
                fs::write(&draft_path, &bytes).unwrap();
                assert_eq!(
                    repo.claim_draft(doc.id()).unwrap().unwrap().is_ok(),
                    valid,
                    "draft v{version} history={history} color={color:?}"
                );
                repo.release_draft(doc.id());
                assert_eq!(fs::read(&draft_path).unwrap(), bytes);
            }
        }
    }
}

#[test]
fn v17_color_underline_outline_and_shadow_save_reopen_and_draft_keep_complete_undo_redo_states() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let mut doc = document();
    let Command::AddCaption {
        caption: mut ink, ..
    } = caption()
    else {
        panic!("caption")
    };
    ink.runs[0].color = Some("#000000".into());
    doc.apply(&[Command::AddCaption {
        caption: ink.clone(),
        parent: None,
        index: None,
    }])
    .unwrap();
    repo.save(&doc, None).unwrap();
    let saved_revision = doc.revision();
    ink.runs[0].color = Some("#2A7BCD".into());
    ink.runs[0].underline = true;
    ink.runs[0].outline = true;
    ink.runs[0].shadow = true;
    doc.apply(&[Command::UpdateCaption {
        object: ObjectId::from_serial(2).unwrap(),
        caption: ink,
    }])
    .unwrap();
    repo.save(&doc, Some(saved_revision)).unwrap();
    let saved_revision = doc.revision();
    assert_eq!(
        repo.load(doc.id()).unwrap(),
        Some(doc.clone()),
        "positive current outline survives native save"
    );
    doc.undo().unwrap();
    repo.save(&doc, Some(saved_revision)).unwrap();
    assert_eq!(repo.load(doc.id()).unwrap(), Some(doc.clone()));
    repo.write_draft(&Draft {
        document: doc.clone(),
        base_revision: doc.revision(),
        saved_at_ms: 42,
    })
    .unwrap();
    repo.release_draft(doc.id());
    let mut restored = repo
        .claim_draft(doc.id())
        .unwrap()
        .unwrap()
        .unwrap()
        .document;
    assert_eq!(restored, doc);
    restored.redo().unwrap();
    let SceneObject::Caption(ink) = restored.scene_objects()[1].2 else {
        panic!("caption")
    };
    assert_eq!(ink.runs[0].color.as_deref(), Some("#2A7BCD"));
    assert!(ink.runs[0].underline);
    assert!(ink.runs[0].outline);
    assert!(ink.runs[0].shadow);
    assert_eq!(ink.runs[1].color, None);
    restored.undo().unwrap();
    let SceneObject::Caption(ink) = restored.scene_objects()[1].2 else {
        panic!("caption")
    };
    assert_eq!(ink.runs[0].color.as_deref(), Some("#000000"));
    assert!(!ink.runs[0].underline);
    assert!(!ink.runs[0].outline);
    assert!(!ink.runs[0].shadow);
}

#[test]
fn maximum_run_metadata_exceeds_the_old_reservation_and_fits_the_new_one() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let mut doc = document();
    let Command::AddCaption {
        caption: mut ink, ..
    } = caption()
    else {
        panic!("caption")
    };
    ink.runs.truncate(1);
    ink.runs[0].text = "H".into();
    ink.runs[0].color = Some("#ABCDEF".into());
    ink.runs[0].size_pt = Some(4.000_000_000_000_003_6);
    ink.runs[0].font = Some(chemifree_domain::settings::LabelFont::SansSerif);
    ink.runs[0].script = CaptionScript::Superscript;
    ink.runs[0].style = chemifree_domain::scene::CaptionStyle::Italic;
    doc.apply(&[Command::AddCaption {
        caption: ink,
        parent: None,
        index: None,
    }])
    .unwrap();
    repo.save(&doc, None).unwrap();
    let stored: Value = serde_json::from_slice(
        &fs::read(root.path().join("documents/caption-native.json")).unwrap(),
    )
    .unwrap();
    let encoded =
        serde_json::to_vec(&stored["document"]["objects"][1]["state"]["runs"][0]).unwrap();
    assert_eq!(
        encoded.len() - 1,
        185,
        "metadata excludes the single unescaped H byte"
    );
    assert!(
        encoded.len() <= 192 + 2,
        "provider encoding fits the domain reservation"
    );
}

fn strip_caption_underlines(value: &mut Value) {
    match value {
        Value::Object(object) => {
            if object.contains_key("script") && object.contains_key("text") {
                object.remove("outline");
                object.remove("shadow");
                object.remove("underline");
            }
            for child in object.values_mut() {
                strip_caption_underlines(child);
            }
        }
        Value::Array(values) => {
            for child in values {
                strip_caption_underlines(child);
            }
        }
        _ => {}
    }
}

#[test]
fn underline_is_strict_in_current_and_history_for_documents_and_drafts() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let mut doc = document();
    doc.apply(&[caption()]).unwrap();
    repo.save(&doc, None).unwrap();
    let path = root.path().join("documents/caption-native.json");
    let mut current: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    current["history"]["undo"] = json!([]);
    doc.undo().unwrap();
    repo.save(&doc, Some(doc.revision() - 1)).unwrap();
    let history_only: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for history in [false, true] {
        for version in 8..=16 {
            for underline in [
                None,
                Some(json!(false)),
                Some(json!(true)),
                Some(Value::Null),
                Some(json!(0)),
                Some(json!("false")),
                Some(json!([])),
                Some(json!({})),
            ] {
                let mut stored = if history {
                    history_only.clone()
                } else {
                    current.clone()
                };
                strip_caption_shadows(&mut stored);
                if version < 16 {
                    strip_caption_outlines(&mut stored);
                }
                if version < 15 {
                    strip_caption_underlines(&mut stored);
                }
                if version < 14 {
                    strip_caption_colors(&mut stored);
                }
                if version < 13 {
                    strip_caption_sizes(&mut stored);
                }
                if version < 12 {
                    strip_caption_fonts(&mut stored);
                }
                if version < 11 {
                    strip_caption_alignment(&mut stored);
                }
                if version < 10 {
                    strip_caption_styles(&mut stored);
                }
                if version == 8 {
                    strip_abbreviations(&mut stored);
                }
                if version < 18 {
                    strip_label_weights(&mut stored);
                }
                stored["format_version"] = json!(version);
                let state = if history {
                    &mut stored["history"]["redo"][0]["objects"][0]["after"]
                } else {
                    &mut stored["document"]["objects"][1]["state"]
                };
                state["runs"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("underline");
                if let Some(value) = &underline {
                    state["runs"][0]["underline"] = value.clone();
                }
                let valid = if version < 15 {
                    underline.is_none()
                } else {
                    matches!(underline, Some(Value::Bool(_)))
                };
                let bytes = serde_json::to_vec(&stored).unwrap();
                fs::write(&path, &bytes).unwrap();
                assert_eq!(
                    repo.load(doc.id()).is_ok(),
                    valid,
                    "v{version} history={history} underline={underline:?}"
                );
                assert_eq!(fs::read(&path).unwrap(), bytes);
                stored["format"] = json!("chemifree.draft");
                stored["base_revision"] = json!(0);
                stored["saved_at_ms"] = json!(42);
                let draft_path = root.path().join("drafts/caption-native.json");
                fs::create_dir_all(root.path().join("drafts")).unwrap();
                let bytes = serde_json::to_vec(&stored).unwrap();
                fs::write(&draft_path, &bytes).unwrap();
                assert_eq!(
                    repo.claim_draft(doc.id()).unwrap().unwrap().is_ok(),
                    valid,
                    "draft v{version} history={history} underline={underline:?}"
                );
                repo.release_draft(doc.id());
                assert_eq!(fs::read(&draft_path).unwrap(), bytes);
            }
        }
    }
}

#[test]
fn outline_is_strict_in_current_and_history_for_documents_and_drafts() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let mut doc = document();
    doc.apply(&[caption()]).unwrap();
    repo.save(&doc, None).unwrap();
    let path = root.path().join("documents/caption-native.json");
    let mut current: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    current["history"]["undo"] = json!([]);
    doc.undo().unwrap();
    repo.save(&doc, Some(doc.revision() - 1)).unwrap();
    let history_only: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for history in [false, true] {
        for version in 8..=16 {
            for outline in [
                None,
                Some(json!(false)),
                Some(json!(true)),
                Some(Value::Null),
                Some(json!(0)),
                Some(json!("false")),
                Some(json!([])),
                Some(json!({})),
            ] {
                let mut stored = if history {
                    history_only.clone()
                } else {
                    current.clone()
                };
                strip_caption_shadows(&mut stored);
                if version < 16 {
                    strip_caption_outlines(&mut stored);
                }
                if version < 15 {
                    strip_caption_underlines(&mut stored);
                }
                if version < 14 {
                    strip_caption_colors(&mut stored);
                }
                if version < 13 {
                    strip_caption_sizes(&mut stored);
                }
                if version < 12 {
                    strip_caption_fonts(&mut stored);
                }
                if version < 11 {
                    strip_caption_alignment(&mut stored);
                }
                if version < 10 {
                    strip_caption_styles(&mut stored);
                }
                if version == 8 {
                    strip_abbreviations(&mut stored);
                }
                if version < 18 {
                    strip_label_weights(&mut stored);
                }
                stored["format_version"] = json!(version);
                let state = if history {
                    &mut stored["history"]["redo"][0]["objects"][0]["after"]
                } else {
                    &mut stored["document"]["objects"][1]["state"]
                };
                state["runs"][0].as_object_mut().unwrap().remove("outline");
                if let Some(value) = &outline {
                    state["runs"][0]["outline"] = value.clone();
                }
                let valid = if version < 16 {
                    outline.is_none()
                } else {
                    matches!(outline, Some(Value::Bool(_)))
                };
                let bytes = serde_json::to_vec(&stored).unwrap();
                fs::write(&path, &bytes).unwrap();
                assert_eq!(
                    repo.load(doc.id()).is_ok(),
                    valid,
                    "v{version} history={history} outline={outline:?}"
                );
                assert_eq!(fs::read(&path).unwrap(), bytes);
                stored["format"] = json!("chemifree.draft");
                stored["base_revision"] = json!(0);
                stored["saved_at_ms"] = json!(42);
                let draft_path = root.path().join("drafts/caption-native.json");
                fs::create_dir_all(root.path().join("drafts")).unwrap();
                let bytes = serde_json::to_vec(&stored).unwrap();
                fs::write(&draft_path, &bytes).unwrap();
                assert_eq!(
                    repo.claim_draft(doc.id()).unwrap().unwrap().is_ok(),
                    valid,
                    "draft v{version} history={history} outline={outline:?}"
                );
                repo.release_draft(doc.id());
                assert_eq!(fs::read(&draft_path).unwrap(), bytes);
            }
        }
    }
}

fn strip_caption_outlines(value: &mut Value) {
    match value {
        Value::Object(object) => {
            if object.contains_key("script") && object.contains_key("text") {
                object.remove("outline");
                object.remove("shadow");
            }
            for child in object.values_mut() {
                strip_caption_outlines(child);
            }
        }
        Value::Array(values) => {
            for child in values {
                strip_caption_outlines(child);
            }
        }
        _ => {}
    }
}

#[test]
fn v15_maximum_metadata_109_caption_edit_preserves_undo_redo_and_original_bytes() {
    for undone in [false, true] {
        for draft in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let repo = FileDocumentRepository::new(root.path());
            let mut doc =
                Document::new(DocumentId::parse("caption-v15-budget").unwrap(), "Budget").unwrap();
            // All v15 metadata fields use their longest encoding, including the
            // 18-character size float. v16 adds outline=false to these 154-byte runs.
            let caption = |text: char| Caption {
                alignment: chemifree_domain::scene::CaptionAlignment::Center,
                anchor: Point::new(0.0, 0.0).unwrap(),
                runs: vec![
                    CaptionRun {
                        shadow: false,
                        outline: false,
                        underline: false,
                        color: Some("#ABCDEF".into()),
                        size_pt: Some(4.000_000_000_000_003_6),
                        font: Some(chemifree_domain::settings::LabelFont::SansSerif),
                        text: text.to_string().repeat(512),
                        script: CaptionScript::Superscript,
                        weight: Default::default(),
                        style: chemifree_domain::scene::CaptionStyle::Italic,
                    };
                    32
                ],
            };
            let commands: Vec<_> = (0..109)
                .map(|_| Command::AddCaption {
                    caption: caption('"'),
                    parent: None,
                    index: None,
                })
                .collect();
            doc.apply(&commands).unwrap();
            let commands: Vec<_> = (2..111)
                .map(|serial| Command::UpdateCaption {
                    object: ObjectId::from_serial(serial).unwrap(),
                    caption: caption('\\'),
                })
                .collect();
            doc.apply(&commands).unwrap();
            assert_eq!((doc.undo_depth(), doc.redo_depth()), (1, 0));
            if undone {
                doc.undo().unwrap();
            }
            repo.save(&doc, None).unwrap();
            if draft {
                repo.write_draft(&Draft {
                    document: doc.clone(),
                    base_revision: doc.revision(),
                    saved_at_ms: 42,
                })
                .unwrap();
                repo.release_draft(doc.id());
            }
            let path = root.path().join(if draft {
                "drafts/caption-v15-budget.json"
            } else {
                "documents/caption-v15-budget.json"
            });
            let mut stored: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            strip_caption_shadows(&mut stored);
            strip_caption_outlines(&mut stored);
            strip_label_weights(&mut stored);
            stored["format_version"] = json!(15);
            let original = serde_json::to_vec(&stored).unwrap();
            fs::write(&path, &original).unwrap();
            let mut loaded = if draft {
                let claimed = repo.claim_draft(doc.id()).unwrap().unwrap().unwrap();
                repo.prepare_draft_for_recovery(
                    doc.id(),
                    chemifree_application::DraftRecoveryTarget::Original {
                        base_revision: claimed.base_revision,
                    },
                )
                .unwrap();
                claimed.document
            } else {
                repo.load(doc.id()).unwrap().unwrap()
            };
            assert_eq!(
                loaded, doc,
                "v15 undone={undone} draft={draft}: no history may be trimmed"
            );
            assert_eq!(
                fs::read(&path).unwrap(),
                original,
                "migration reads leave exact bytes untouched"
            );
            let expected = if undone { (0, 1) } else { (1, 0) };
            assert_eq!((loaded.undo_depth(), loaded.redo_depth()), expected);
            if undone {
                loaded.redo().unwrap();
            } else {
                loaded.undo().unwrap();
            }
            assert_eq!(
                loaded.scene_objects()[1].2,
                &SceneObject::Caption(caption(if undone { '\\' } else { '"' }))
            );
            if undone {
                loaded.undo().unwrap();
            } else {
                loaded.redo().unwrap();
            }
            assert_eq!(loaded.scene_objects(), doc.scene_objects());
            // Actual v16 reservation is separate from the legacy admission allowance.
            // All fixed envelopes and quote/backslash escaping are included in it.
            let bound =
                CAPTION_CONTENT_WEIGHT_LIMIT + 1_024_000 + CAPTION_HISTORY_WEIGHT_LIMIT * 6 / 5;
            assert!(loaded.persistence_weight() <= bound);
            assert!(bound < MAX_DOCUMENT_BYTES as usize);
            if draft {
                let next = Draft {
                    document: loaded.clone(),
                    base_revision: doc.revision(),
                    saved_at_ms: 43,
                };
                repo.write_draft(&next).unwrap();
                assert_eq!(
                    fs::read(repo.migrated_draft_path(doc.id(), 15)).unwrap(),
                    original
                );
                assert!(fs::metadata(&path).unwrap().len() <= loaded.persistence_weight() as u64);
                repo.release_draft(doc.id());
                assert_eq!(repo.claim_draft(doc.id()).unwrap(), Some(Ok(next)));
                repo.release_draft(doc.id());
            } else {
                repo.save(&loaded, Some(doc.revision())).unwrap();
                assert_eq!(
                    fs::read(repo.migrated_path(doc.id(), 15)).unwrap(),
                    original
                );
                assert!(fs::metadata(&path).unwrap().len() <= loaded.persistence_weight() as u64);
                assert_eq!(repo.load(doc.id()).unwrap(), Some(loaded));
            }
        }
    }
}

fn strip_caption_shadows(value: &mut Value) {
    match value {
        Value::Object(object) => {
            if object.contains_key("script") && object.contains_key("text") {
                object.remove("shadow");
            }
            for child in object.values_mut() {
                strip_caption_shadows(child);
            }
        }
        Value::Array(values) => {
            for child in values {
                strip_caption_shadows(child);
            }
        }
        _ => {}
    }
}

#[test]
fn shadow_is_strict_in_current_and_history_for_documents_and_drafts() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let mut doc = document();
    doc.apply(&[caption()]).unwrap();
    repo.save(&doc, None).unwrap();
    let path = root.path().join("documents/caption-native.json");
    let mut current: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    current["history"]["undo"] = json!([]);
    doc.undo().unwrap();
    repo.save(&doc, Some(doc.revision() - 1)).unwrap();
    let history_only: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for history in [false, true] {
        for version in 8..=17 {
            for shadow in [
                None,
                Some(json!(false)),
                Some(json!(true)),
                Some(Value::Null),
                Some(json!(0)),
                Some(json!("false")),
                Some(json!([])),
                Some(json!({})),
            ] {
                let mut stored = if history {
                    history_only.clone()
                } else {
                    current.clone()
                };
                if version < 17 {
                    strip_caption_shadows(&mut stored);
                }
                if version < 16 {
                    strip_caption_outlines(&mut stored);
                }
                if version < 15 {
                    strip_caption_underlines(&mut stored);
                }
                if version < 14 {
                    strip_caption_colors(&mut stored);
                }
                if version < 13 {
                    strip_caption_sizes(&mut stored);
                }
                if version < 12 {
                    strip_caption_fonts(&mut stored);
                }
                if version < 11 {
                    strip_caption_alignment(&mut stored);
                }
                if version < 10 {
                    strip_caption_styles(&mut stored);
                }
                if version == 8 {
                    strip_abbreviations(&mut stored);
                }
                if version < 18 {
                    strip_label_weights(&mut stored);
                }
                stored["format_version"] = json!(version);
                let state = if history {
                    &mut stored["history"]["redo"][0]["objects"][0]["after"]
                } else {
                    &mut stored["document"]["objects"][1]["state"]
                };
                state["runs"][0].as_object_mut().unwrap().remove("shadow");
                if let Some(value) = &shadow {
                    state["runs"][0]["shadow"] = value.clone();
                }
                let valid = if version < 17 {
                    shadow.is_none()
                } else {
                    matches!(shadow, Some(Value::Bool(_)))
                };
                let bytes = serde_json::to_vec(&stored).unwrap();
                fs::write(&path, &bytes).unwrap();
                assert_eq!(
                    repo.load(doc.id()).is_ok(),
                    valid,
                    "v{version} history={history} shadow={shadow:?}"
                );
                assert_eq!(fs::read(&path).unwrap(), bytes);
                stored["format"] = json!("chemifree.draft");
                stored["base_revision"] = json!(0);
                stored["saved_at_ms"] = json!(42);
                let draft_path = root.path().join("drafts/caption-native.json");
                fs::create_dir_all(root.path().join("drafts")).unwrap();
                let bytes = serde_json::to_vec(&stored).unwrap();
                fs::write(&draft_path, &bytes).unwrap();
                assert_eq!(
                    repo.claim_draft(doc.id()).unwrap().unwrap().is_ok(),
                    valid,
                    "draft v{version} history={history} shadow={shadow:?}"
                );
                repo.release_draft(doc.id());
                assert_eq!(fs::read(&draft_path).unwrap(), bytes);
            }
        }
    }
}

#[test]
fn v16_and_v17_maximum_metadata_109_caption_edit_preserves_undo_redo_and_original_bytes() {
    for version in [16, 17] {
        for undone in [false, true] {
            for draft in [false, true] {
                let root = tempfile::tempdir().unwrap();
                let repo = FileDocumentRepository::new(root.path());
                let mut doc =
                    Document::new(DocumentId::parse("caption-v16-budget").unwrap(), "Budget")
                        .unwrap();
                // All v16 metadata fields use their longest encoding, including the
                // 18-character size float. v16 adds outline=false to these 154-byte runs.
                let caption = |text: char| Caption {
                    alignment: chemifree_domain::scene::CaptionAlignment::Center,
                    anchor: Point::new(0.0, 0.0).unwrap(),
                    runs: vec![
                        CaptionRun {
                            shadow: false,
                            outline: false,
                            underline: false,
                            color: Some("#ABCDEF".into()),
                            size_pt: Some(4.000_000_000_000_003_6),
                            font: Some(chemifree_domain::settings::LabelFont::SansSerif),
                            text: text.to_string().repeat(512),
                            script: CaptionScript::Superscript,
                            weight: Default::default(),
                            style: chemifree_domain::scene::CaptionStyle::Italic,
                        };
                        32
                    ],
                };
                let commands: Vec<_> = (0..109)
                    .map(|_| Command::AddCaption {
                        caption: caption('"'),
                        parent: None,
                        index: None,
                    })
                    .collect();
                doc.apply(&commands).unwrap();
                let commands: Vec<_> = (2..111)
                    .map(|serial| Command::UpdateCaption {
                        object: ObjectId::from_serial(serial).unwrap(),
                        caption: caption('\\'),
                    })
                    .collect();
                doc.apply(&commands).unwrap();
                assert_eq!((doc.undo_depth(), doc.redo_depth()), (1, 0));
                if undone {
                    doc.undo().unwrap();
                }
                repo.save(&doc, None).unwrap();
                if draft {
                    repo.write_draft(&Draft {
                        document: doc.clone(),
                        base_revision: doc.revision(),
                        saved_at_ms: 42,
                    })
                    .unwrap();
                    repo.release_draft(doc.id());
                }
                let path = root.path().join(if draft {
                    "drafts/caption-v16-budget.json"
                } else {
                    "documents/caption-v16-budget.json"
                });
                let mut stored: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                if version == 16 {
                    strip_caption_shadows(&mut stored);
                }
                strip_label_weights(&mut stored);
                stored["format_version"] = json!(version);
                let original = serde_json::to_vec(&stored).unwrap();
                fs::write(&path, &original).unwrap();
                let mut loaded = if draft {
                    let claimed = repo.claim_draft(doc.id()).unwrap().unwrap().unwrap();
                    repo.prepare_draft_for_recovery(
                        doc.id(),
                        chemifree_application::DraftRecoveryTarget::Original {
                            base_revision: claimed.base_revision,
                        },
                    )
                    .unwrap();
                    claimed.document
                } else {
                    repo.load(doc.id()).unwrap().unwrap()
                };
                assert_eq!(
                    loaded, doc,
                    "v16 undone={undone} draft={draft}: no history may be trimmed"
                );
                assert_eq!(
                    fs::read(&path).unwrap(),
                    original,
                    "migration reads leave exact bytes untouched"
                );
                let expected = if undone { (0, 1) } else { (1, 0) };
                assert_eq!((loaded.undo_depth(), loaded.redo_depth()), expected);
                if undone {
                    loaded.redo().unwrap();
                } else {
                    loaded.undo().unwrap();
                }
                assert_eq!(
                    loaded.scene_objects()[1].2,
                    &SceneObject::Caption(caption(if undone { '\\' } else { '"' }))
                );
                if undone {
                    loaded.undo().unwrap();
                } else {
                    loaded.redo().unwrap();
                }
                assert_eq!(loaded.scene_objects(), doc.scene_objects());
                // Actual v16 reservation is separate from the legacy admission allowance.
                // All fixed envelopes and quote/backslash escaping are included in it.
                let bound =
                    CAPTION_CONTENT_WEIGHT_LIMIT + 1_024_000 + CAPTION_HISTORY_WEIGHT_LIMIT * 6 / 5;
                assert!(loaded.persistence_weight() <= bound);
                assert!(bound < MAX_DOCUMENT_BYTES as usize);
                if draft {
                    let next = Draft {
                        document: loaded.clone(),
                        base_revision: doc.revision(),
                        saved_at_ms: 43,
                    };
                    repo.write_draft(&next).unwrap();
                    assert_eq!(
                        fs::read(repo.migrated_draft_path(doc.id(), version)).unwrap(),
                        original
                    );
                    assert!(
                        fs::metadata(&path).unwrap().len() <= loaded.persistence_weight() as u64
                    );
                    repo.release_draft(doc.id());
                    assert_eq!(repo.claim_draft(doc.id()).unwrap(), Some(Ok(next)));
                    repo.release_draft(doc.id());
                } else {
                    repo.save(&loaded, Some(doc.revision())).unwrap();
                    assert_eq!(
                        fs::read(repo.migrated_path(doc.id(), version)).unwrap(),
                        original
                    );
                    assert!(
                        fs::metadata(&path).unwrap().len() <= loaded.persistence_weight() as u64
                    );
                    assert_eq!(repo.load(doc.id()).unwrap(), Some(loaded));
                }
            }
        }
    }
}

fn strip_label_weights(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(object) => {
            object.remove("label_weight");
            for child in object.values_mut() {
                strip_label_weights(child);
            }
        }
        serde_json::Value::Array(values) => {
            for child in values {
                strip_label_weights(child);
            }
        }
        _ => {}
    }
}
