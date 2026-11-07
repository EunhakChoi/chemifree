//! Native v26 graphic width is strict through history/drafts and keeps v25 source bytes.

use std::fs;

use chemifree_application::{DocumentRepository, Draft, DraftRecoveryTarget};
use chemifree_domain::document::{Command, Document, DocumentId, ObjectId, Point, Vector};
use chemifree_domain::scene::{Graphic, GraphicShape, MAX_OBJECTS, SceneObject};
use chemifree_providers::{FileDocumentRepository, MAX_DOCUMENT_BYTES};
use serde_json::{Value, json};

fn object(n: u32) -> ObjectId {
    ObjectId::from_serial(n).unwrap()
}
fn set(width: Option<f64>) -> Command {
    Command::SetGraphicLineWidth {
        object: object(2),
        line_width_pt: width,
    }
}
fn drawing() -> Document {
    let mut doc = Document::new(DocumentId::parse("width-native").unwrap(), "Width").unwrap();
    let commands: Vec<_> = [
        GraphicShape::Line,
        GraphicShape::Arrow,
        GraphicShape::Rectangle,
    ]
    .into_iter()
    .map(|shape| Command::AddGraphic {
        graphic: Graphic {
            line_width_pt: None,
            shape,
            points: vec![
                Point::new(-0.0000000123456789, 2.0).unwrap(),
                Point::new(10.0, 20.0).unwrap(),
            ],
        },
        parent: None,
        index: None,
    })
    .collect();
    doc.apply(&commands).unwrap();
    doc
}
fn v25(mut raw: Value) -> Value {
    fn strip(value: &mut Value) {
        match value {
            Value::Object(object) => {
                if object.get("kind").and_then(Value::as_str) == Some("graphic") {
                    object.remove("line_width_pt");
                }
                for value in object.values_mut() {
                    strip(value);
                }
            }
            Value::Array(values) => {
                for value in values {
                    strip(value);
                }
            }
            _ => {}
        }
    }
    strip(&mut raw);
    raw["format_version"] = json!(25);
    raw
}
fn save_template(root: &tempfile::TempDir, repo: &FileDocumentRepository, doc: &Document) -> Value {
    repo.save(doc, None).unwrap();
    serde_json::from_slice(&fs::read(root.path().join("documents/width-native.json")).unwrap())
        .unwrap()
}

#[test]
fn widths_round_trip_both_history_sides_drafts_and_null_reset() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let mut doc = drawing();
    doc.apply(&[set(Some(0.1))]).unwrap();
    doc.apply(&[set(None)]).unwrap();
    doc.undo().unwrap();
    let raw = save_template(&root, &repo, &doc);
    assert_eq!(raw["format_version"], 30);
    assert_eq!(raw["document"]["objects"][1]["state"]["line_width_pt"], 0.1);
    assert_eq!(repo.load(doc.id()).unwrap(), Some(doc.clone()));
    let mut reopened = repo.load(doc.id()).unwrap().unwrap();
    reopened.redo().unwrap();
    let parts = reopened.to_parts();
    assert!(
        matches!(&parts.objects[1].1,SceneObject::Graphic(graphic) if graphic.line_width_pt.is_none())
    );
    reopened.undo().unwrap();
    assert_eq!(reopened.to_parts().objects, doc.to_parts().objects);
    let draft = Draft {
        document: doc.clone(),
        base_revision: doc.revision(),
        saved_at_ms: 42,
    };
    repo.write_draft(&draft).unwrap();
    repo.release_draft(doc.id());
    assert_eq!(repo.claim_draft(doc.id()), Ok(Some(Ok(draft))));
    repo.release_draft(doc.id());
}

#[test]
fn current_and_history_fields_are_required_and_invalid_values_refuse_without_rewrites() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let mut doc = drawing();
    doc.apply(&[set(Some(2.0))]).unwrap();
    doc.apply(&[set(Some(4.0))]).unwrap();
    doc.undo().unwrap();
    let template = save_template(&root, &repo, &doc);
    for location in [
        "/document/objects/1/state",
        "/history/undo/1/objects/0/after",
        "/history/redo/0/objects/0/before",
        "/history/redo/0/objects/0/after",
    ] {
        for bad in [
            None,
            Some(json!("inherit")),
            Some(json!(false)),
            Some(json!(0.0)),
            Some(json!(0.099)),
            Some(json!(10.001)),
            Some(json!([])),
        ] {
            for draft in [false, true] {
                let mut raw = template.clone();
                let value = raw.pointer_mut(location).unwrap().as_object_mut().unwrap();
                value.remove("line_width_pt");
                if let Some(bad) = &bad {
                    value.insert("line_width_pt".into(), bad.clone());
                }
                let path = if draft {
                    raw["format"] = json!("chemifree.draft");
                    raw["base_revision"] = json!(doc.revision());
                    raw["saved_at_ms"] = json!(42);
                    root.path().join("drafts/width-native.json")
                } else {
                    root.path().join("documents/width-native.json")
                };
                let bytes = serde_json::to_vec(&raw).unwrap();
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(&path, &bytes).unwrap();
                if draft {
                    assert!(
                        repo.claim_draft(doc.id()).unwrap().unwrap().is_err(),
                        "{location} {bad:?}"
                    );
                    repo.release_draft(doc.id());
                } else {
                    assert!(repo.load(doc.id()).is_err(), "{location} {bad:?}");
                }
                assert_eq!(fs::read(&path).unwrap(), bytes);
            }
        }
    }
    let mut future = template;
    future["format_version"] = json!(31);
    let path = root.path().join("documents/width-native.json");
    let bytes = serde_json::to_vec(&future).unwrap();
    fs::write(&path, &bytes).unwrap();
    assert!(repo.load(doc.id()).is_err());
    assert_eq!(fs::read(&path).unwrap(), bytes);
}

#[test]
fn v25_inherits_through_full_history_and_archives_exact_document_and_draft_before_replacement() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let mut doc = drawing();
    doc.apply(&[Command::MoveObjects {
        objects: vec![object(2)],
        delta: Vector { dx: 5.0, dy: 6.0 },
    }])
    .unwrap();
    doc.apply(&[Command::UpdateGraphicPoints {
        object: object(2),
        points: [
            Point::new(8.0, 9.0).unwrap(),
            Point::new(10.0, 11.0).unwrap(),
        ],
    }])
    .unwrap();
    doc.undo().unwrap();
    let current = save_template(&root, &repo, &doc);
    let old = v25(current);
    let bytes = serde_json::to_vec_pretty(&old).unwrap();
    let path = root.path().join("documents/width-native.json");
    fs::write(&path, &bytes).unwrap();
    let loaded = repo.load(doc.id()).unwrap().unwrap();
    assert_eq!(loaded, doc);
    assert_eq!(fs::read(&path).unwrap(), bytes);
    repo.save(&loaded, Some(loaded.revision())).unwrap();
    assert_eq!(fs::read(repo.migrated_path(doc.id(), 25)).unwrap(), bytes);
    let draft = Draft {
        document: doc.clone(),
        base_revision: doc.revision(),
        saved_at_ms: 42,
    };
    repo.write_draft(&draft).unwrap();
    repo.release_draft(doc.id());
    let path = root.path().join("drafts/width-native.json");
    let raw: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let bytes = serde_json::to_vec_pretty(&v25(raw)).unwrap();
    fs::write(&path, &bytes).unwrap();
    assert_eq!(repo.claim_draft(doc.id()), Ok(Some(Ok(draft))));
    assert_eq!(fs::read(&path).unwrap(), bytes);
    repo.prepare_draft_for_recovery(
        doc.id(),
        DraftRecoveryTarget::Original {
            base_revision: doc.revision(),
        },
    )
    .unwrap();
    assert_eq!(
        fs::read(repo.migrated_draft_path(doc.id(), 25)).unwrap(),
        bytes
    );
    repo.release_draft(doc.id());
}

#[test]
fn old_headers_cannot_smuggle_even_null_width_fields_in_current_or_history() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let mut doc = drawing();
    doc.apply(&[Command::MoveObjects {
        objects: vec![object(2)],
        delta: Vector { dx: 1.0, dy: 2.0 },
    }])
    .unwrap();
    doc.undo().unwrap();
    let old = v25(save_template(&root, &repo, &doc));
    for version in 1..=25 {
        for location in [
            "/document/objects/1/state",
            "/history/undo/0/objects/0/after",
            "/history/redo/0/objects/0/before",
        ] {
            for draft in [false, true] {
                let mut raw = old.clone();
                raw["format_version"] = json!(version);
                raw.pointer_mut(location)
                    .unwrap()
                    .as_object_mut()
                    .unwrap()
                    .insert("line_width_pt".into(), Value::Null);
                let path = if draft {
                    raw["format"] = json!("chemifree.draft");
                    raw["base_revision"] = json!(doc.revision());
                    raw["saved_at_ms"] = json!(42);
                    root.path().join("drafts/width-native.json")
                } else {
                    root.path().join("documents/width-native.json")
                };
                let bytes = serde_json::to_vec(&raw).unwrap();
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(&path, &bytes).unwrap();
                let error = if draft {
                    let error = repo.claim_draft(doc.id()).unwrap().unwrap().unwrap_err();
                    repo.release_draft(doc.id());
                    error
                } else {
                    repo.load(doc.id()).unwrap_err().to_string()
                };
                assert!(
                    error.contains("Graphic line_width_pt"),
                    "{version}: {error}"
                );
                assert_eq!(fs::read(&path).unwrap(), bytes);
            }
        }
    }
}

#[test]
fn invalid_width_history_is_checked_before_overlong_history_retention() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let mut doc = drawing();
    doc.apply(&[set(Some(2.0))]).unwrap();
    let mut raw = save_template(&root, &repo, &doc);
    let mut bad = raw["history"]["undo"][0].clone();
    bad["objects"][0]["after"]["line_width_pt"] = json!(11.0);
    let undo = raw["history"]["undo"].as_array_mut().unwrap();
    undo.splice(0..0, std::iter::repeat_n(bad, 101));
    let path = root.path().join("documents/width-native.json");
    let bytes = serde_json::to_vec(&raw).unwrap();
    fs::write(&path, &bytes).unwrap();
    assert!(
        repo.load(doc.id())
            .unwrap_err()
            .to_string()
            .contains("invalid graphic history")
    );
    assert_eq!(fs::read(&path).unwrap(), bytes);
}

#[test]
fn maximum_graphic_history_with_long_float_lexemes_stays_within_existing_storage_reservations() {
    let mut parts = Document::new(DocumentId::parse("width-budget").unwrap(), "Width budget")
        .unwrap()
        .to_parts();
    let tiny = -2.2250738585072014e-308;
    let ids: Vec<_> = (0..MAX_OBJECTS - 1)
        .map(|index| object(2_000_000_000 + index as u32))
        .collect();
    for id in &ids {
        parts.objects.push((
            *id,
            SceneObject::Graphic(Graphic {
                line_width_pt: Some(0.10000000000000002),
                shape: GraphicShape::Rectangle,
                points: vec![
                    Point::new(tiny, tiny).unwrap(),
                    Point::new(-tiny, -tiny).unwrap(),
                ],
            }),
        ));
        parts.roots.push(*id);
    }
    parts.next_object = 2_000_000_000 + MAX_OBJECTS as u32;
    let mut doc = Document::restore(parts).unwrap();
    for index in 0..100 {
        let step = if index % 2 == 0 { 1e-309 } else { -1e-309 };
        doc.apply(&[Command::MoveObjects {
            objects: ids.clone(),
            delta: Vector { dx: step, dy: step },
        }])
        .unwrap();
    }
    assert_eq!(doc.undo_depth(), 100);
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    repo.save(&doc, None).unwrap();
    let bytes = fs::read(root.path().join("documents/width-budget.json")).unwrap();
    assert!(bytes.len() <= doc.persistence_weight());
    assert!((bytes.len() as u64) < MAX_DOCUMENT_BYTES);
    assert_eq!(repo.load(doc.id()).unwrap(), Some(doc.clone()));
    let draft = Draft {
        document: doc.clone(),
        base_revision: doc.revision(),
        saved_at_ms: 42,
    };
    repo.write_draft(&draft).unwrap();
    repo.release_draft(doc.id());
    let bytes = fs::read(root.path().join("drafts/width-budget.json")).unwrap();
    assert!(bytes.len() <= doc.persistence_weight());
    assert!((bytes.len() as u64) < MAX_DOCUMENT_BYTES);
    assert_eq!(repo.claim_draft(doc.id()), Ok(Some(Ok(draft))));
    repo.release_draft(doc.id());
}
