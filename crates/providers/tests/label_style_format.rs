//! v19 style is strict in document/history; earlier bytes and complete histories survive.
use std::fs;

use chemifree_application::{DocumentRepository, Draft, DraftRecoveryTarget};
use chemifree_domain::document::{Command, Document, DocumentId};
use chemifree_domain::settings::{LabelStyle, SettingsEdit};
use chemifree_providers::FileDocumentRepository;
use serde_json::{Value, json};

fn document() -> Document {
    let mut doc = Document::new(DocumentId::parse("label-weight").unwrap(), "Labels").unwrap();
    for width in [2.0, 3.0] {
        doc.apply(&[Command::UpdateSettings(SettingsEdit {
            line_width_pt: Some(width),
            ..SettingsEdit::default()
        })])
        .unwrap();
    }
    doc.undo().unwrap();
    doc
}
fn strip_styles(value: &mut Value) {
    match value {
        Value::Object(object) => {
            object.remove("label_style");
            object.remove("label_underline");
            object.remove("label_outline");
            object.remove("label_shadow");
            object.remove("display_labels");
            object.remove("fragment_displays");
            if object.get("kind").and_then(Value::as_str) == Some("graphic") {
                object.remove("line_width_pt");
            }
            for child in object.values_mut() {
                strip_styles(child);
            }
        }
        Value::Array(values) => {
            for child in values {
                strip_styles(child);
            }
        }
        _ => {}
    }
}

#[test]
fn document_and_draft_require_weight_in_current_and_both_history_directions() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let doc = document();
    repo.save(&doc, None).unwrap();
    let path = root.path().join("documents/label-weight.json");
    let template: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(template["format_version"], 27);
    for location in [
        "/document/settings",
        "/history/undo/0/settings/before",
        "/history/undo/0/settings/after",
        "/history/redo/0/settings/before",
        "/history/redo/0/settings/after",
    ] {
        for invalid in [
            None,
            Some(Value::Null),
            Some(json!(700)),
            Some(json!("heavy")),
            Some(json!([])),
            Some(json!({})),
        ] {
            for draft in [false, true] {
                let mut raw = template.clone();
                let target = raw.pointer_mut(location).unwrap().as_object_mut().unwrap();
                target.remove("label_style");
                if let Some(value) = &invalid {
                    target.insert("label_style".into(), value.clone());
                }
                if draft {
                    raw["format"] = json!("chemifree.draft");
                    raw["base_revision"] = json!(doc.revision());
                    raw["saved_at_ms"] = json!(42);
                    let bytes = serde_json::to_vec(&raw).unwrap();
                    let draft_path = root.path().join("drafts/label-weight.json");
                    fs::create_dir_all(draft_path.parent().unwrap()).unwrap();
                    fs::write(&draft_path, &bytes).unwrap();
                    let failure = repo.claim_draft(doc.id()).unwrap().unwrap().unwrap_err();
                    assert!(
                        failure.to_string().contains("label_style"),
                        "{location}: {failure}"
                    );
                    repo.release_draft(doc.id());
                    assert_eq!(fs::read(draft_path).unwrap(), bytes);
                } else {
                    let bytes = serde_json::to_vec(&raw).unwrap();
                    fs::write(&path, &bytes).unwrap();
                    let failure = repo.load(doc.id()).unwrap_err();
                    assert!(
                        failure.to_string().contains("label_style"),
                        "{location}: {failure}"
                    );
                    assert_eq!(fs::read(&path).unwrap(), bytes);
                }
            }
        }
    }
}

#[test]
fn old_headers_forbid_weight_even_in_history_and_future_28_is_untouched() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let doc = document();
    repo.save(&doc, None).unwrap();
    let path = root.path().join("documents/label-weight.json");
    let template: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for version in [16, 17, 18, 28] {
        for location in [
            "/document/settings",
            "/history/undo/0/settings/before",
            "/history/redo/0/settings/after",
        ] {
            let mut raw = template.clone();
            strip_styles(&mut raw);
            raw["format_version"] = json!(version);
            if version < 18 {
                strip_legacy_weights(&mut raw);
            }
            raw.pointer_mut(location)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert("label_style".into(), json!("normal"));
            for draft in [false, true] {
                if draft {
                    raw["format"] = json!("chemifree.draft");
                    raw["base_revision"] = json!(doc.revision());
                    raw["saved_at_ms"] = json!(42);
                }
                let bytes = serde_json::to_vec_pretty(&raw).unwrap();
                let target = if draft {
                    root.path().join("drafts/label-weight.json")
                } else {
                    path.clone()
                };
                fs::create_dir_all(target.parent().unwrap()).unwrap();
                fs::write(&target, &bytes).unwrap();
                let reason = if draft {
                    repo.claim_draft(doc.id())
                        .unwrap()
                        .unwrap()
                        .unwrap_err()
                        .to_string()
                } else {
                    repo.load(doc.id()).unwrap_err().to_string()
                };
                assert!(
                    reason.contains(if version == 28 {
                        "version 28"
                    } else {
                        "label_style"
                    }),
                    "{reason}"
                );
                repo.release_draft(doc.id());
                assert_eq!(fs::read(target).unwrap(), bytes);
            }
        }
    }
}

#[test]
fn old_bytes_are_archived_and_normal_migration_retains_undo_redo_and_draft_recovery() {
    for version in [16, 17, 18] {
        let root = tempfile::tempdir().unwrap();
        let repo = FileDocumentRepository::new(root.path());
        let doc = document();
        repo.save(&doc, None).unwrap();
        let path = root.path().join("documents/label-weight.json");
        let mut raw: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        strip_styles(&mut raw);
        raw["format_version"] = json!(version);
        if version < 18 {
            strip_legacy_weights(&mut raw);
        }
        let original = serde_json::to_vec_pretty(&raw).unwrap();
        fs::write(&path, &original).unwrap();
        let mut loaded = repo.load(doc.id()).unwrap().unwrap();
        assert_eq!(loaded, doc);
        assert_eq!(fs::read(&path).unwrap(), original);
        loaded.redo().unwrap();
        assert_eq!(loaded.settings().line_width_pt, 3.0);
        loaded.undo().unwrap();
        loaded
            .apply(&[Command::UpdateSettings(SettingsEdit {
                label_style: Some(LabelStyle::Italic),
                ..SettingsEdit::default()
            })])
            .unwrap();
        repo.save(&loaded, Some(doc.revision())).unwrap();
        assert_eq!(
            fs::read(repo.migrated_path(doc.id(), version)).unwrap(),
            original
        );
        assert_eq!(repo.load(doc.id()).unwrap(), Some(loaded));

        // Start the independent draft recovery case from its unchanged saved base.
        fs::write(root.path().join("documents/label-weight.json"), &original).unwrap();
        raw["format"] = json!("chemifree.draft");
        raw["base_revision"] = json!(doc.revision());
        raw["saved_at_ms"] = json!(42);
        let original = serde_json::to_vec_pretty(&raw).unwrap();
        let path = root.path().join("drafts/label-weight.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, &original).unwrap();
        let claimed = repo.claim_draft(doc.id()).unwrap().unwrap().unwrap();
        assert_eq!(claimed.document, doc);
        assert_eq!(fs::read(&path).unwrap(), original);
        repo.prepare_draft_for_recovery(
            doc.id(),
            DraftRecoveryTarget::Original {
                base_revision: claimed.base_revision,
            },
        )
        .unwrap();
        assert_eq!(
            fs::read(repo.migrated_draft_path(doc.id(), version)).unwrap(),
            original
        );
        let mut changed = claimed.document;
        changed
            .apply(&[Command::UpdateSettings(SettingsEdit {
                label_style: Some(LabelStyle::Italic),
                ..SettingsEdit::default()
            })])
            .unwrap();
        let next = Draft {
            document: changed.clone(),
            base_revision: doc.revision(),
            saved_at_ms: 43,
        };
        repo.write_draft(&next).unwrap();
        repo.release_draft(doc.id());
        let mut recovered = repo
            .claim_draft(doc.id())
            .unwrap()
            .unwrap()
            .unwrap()
            .document;
        assert_eq!(recovered, changed);
        recovered.undo().unwrap();
        assert_eq!(recovered.settings().label_style, LabelStyle::Normal);
        recovered.redo().unwrap();
        assert_eq!(recovered.settings().label_style, LabelStyle::Italic);
        assert_eq!(
            fs::read(repo.migrated_draft_path(doc.id(), version)).unwrap(),
            original
        );
    }
}

fn strip_legacy_weights(value: &mut Value) {
    match value {
        Value::Object(object) => {
            object.remove("label_weight");
            for child in object.values_mut() {
                strip_legacy_weights(child);
            }
        }
        Value::Array(values) => {
            for child in values {
                strip_legacy_weights(child);
            }
        }
        _ => {}
    }
}
