//! Native v27 admits ellipse states without weakening old-profile or history checks.

use std::fs;

use chemifree_application::{DocumentRepository, Draft, DraftRecoveryTarget};
use chemifree_domain::document::{Command, Document, DocumentId, ObjectId, Point, Vector};
use chemifree_domain::scene::{DeleteMode, Graphic, GraphicShape};
use chemifree_providers::{FileDocumentRepository, MAX_DOCUMENT_BYTES};
use serde_json::{Value, json};

fn object(id: u32) -> ObjectId {
    ObjectId::from_serial(id).unwrap()
}
fn drawing(shape: GraphicShape) -> Document {
    let mut doc = Document::new(DocumentId::parse("shape-native").unwrap(), "Shape").unwrap();
    doc.apply(&[Command::AddGraphic {
        graphic: Graphic {
            shape,
            line_width_pt: Some(2.75),
            points: vec![
                Point::new(-4.0, -2.0).unwrap(),
                Point::new(6.0, 8.0).unwrap(),
            ],
        },
        parent: None,
        index: None,
    }])
    .unwrap();
    doc
}
fn raw(root: &tempfile::TempDir) -> Value {
    serde_json::from_slice(&fs::read(root.path().join("documents/shape-native.json")).unwrap())
        .unwrap()
}
fn remove_width(value: &mut Value) {
    match value {
        Value::Object(object) => {
            if object.get("kind").and_then(Value::as_str) == Some("graphic") {
                object.remove("line_width_pt");
            }
            for child in object.values_mut() {
                remove_width(child);
            }
        }
        Value::Array(values) => {
            for child in values {
                remove_width(child);
            }
        }
        _ => {}
    }
}

#[test]
fn ellipse_current_undo_redo_and_draft_states_round_trip_with_unmodified_storage_reservations() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let mut doc = drawing(GraphicShape::Ellipse);
    doc.apply(&[Command::MoveObjects {
        objects: vec![object(2)],
        delta: Vector { dx: 5.0, dy: 6.0 },
    }])
    .unwrap();
    doc.apply(&[Command::UpdateGraphicPoints {
        object: object(2),
        points: [Point::new(0.0, 0.0).unwrap(); 2],
    }])
    .unwrap();
    doc.undo().unwrap();
    repo.save(&doc, None).unwrap();
    let value = raw(&root);
    assert_eq!(value["format_version"], 28);
    assert_eq!(value["document"]["objects"][1]["state"]["shape"], "ellipse");
    assert_eq!(repo.load(doc.id()).unwrap(), Some(doc.clone()));
    let mut loaded = repo.load(doc.id()).unwrap().unwrap();
    loaded.redo().unwrap();
    loaded.undo().unwrap();
    assert_eq!(loaded.to_parts().objects, doc.to_parts().objects);
    let draft = Draft {
        document: doc.clone(),
        base_revision: doc.revision(),
        saved_at_ms: 42,
    };
    repo.write_draft(&draft).unwrap();
    repo.release_draft(doc.id());
    assert_eq!(repo.claim_draft(doc.id()), Ok(Some(Ok(draft))));
    repo.release_draft(doc.id());
    let bytes = fs::read(root.path().join("documents/shape-native.json")).unwrap();
    assert!(bytes.len() <= doc.persistence_weight());
    assert!((bytes.len() as u64) < MAX_DOCUMENT_BYTES);
    let other = tempfile::tempdir().unwrap();
    let other_repo = FileDocumentRepository::new(other.path());
    let mut rectangle = drawing(GraphicShape::Rectangle);
    rectangle
        .apply(&[Command::MoveObjects {
            objects: vec![object(2)],
            delta: Vector { dx: 5.0, dy: 6.0 },
        }])
        .unwrap();
    rectangle
        .apply(&[Command::UpdateGraphicPoints {
            object: object(2),
            points: [Point::new(0.0, 0.0).unwrap(); 2],
        }])
        .unwrap();
    rectangle.undo().unwrap();
    other_repo.save(&rectangle, None).unwrap();
    assert_eq!(doc.persistence_weight(), rectangle.persistence_weight());
    assert!(
        bytes.len()
            < fs::read(other.path().join("documents/shape-native.json"))
                .unwrap()
                .len()
    );
}

#[test]
fn old_headers_refuse_current_and_history_only_ellipses_even_before_retention_without_rewriting() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let mut doc = drawing(GraphicShape::Ellipse);
    repo.save(&doc, None).unwrap();
    let current = raw(&root);
    doc.apply(&[Command::DeleteObjects {
        objects: vec![object(2)],
        mode: DeleteMode::Subtree,
    }])
    .unwrap();
    let path = root.path().join("documents/shape-native.json");
    repo.save(&doc, Some(1)).unwrap();
    let history_only = raw(&root);
    assert_eq!(repo.load(doc.id()).unwrap(), Some(doc.clone()));
    for template in [current, history_only] {
        for version in 1..=26 {
            for draft in [false, true] {
                let mut value = template.clone();
                value["format_version"] = json!(version);
                if version < 26 {
                    remove_width(&mut value);
                }
                if version == 26 {
                    let first = value["history"]["undo"][0].clone();
                    value["history"]["undo"]
                        .as_array_mut()
                        .unwrap()
                        .splice(0..0, std::iter::repeat_n(first, 101));
                }
                let target = if draft {
                    value["format"] = json!("chemifree.draft");
                    value["base_revision"] = json!(doc.revision());
                    value["saved_at_ms"] = json!(42);
                    root.path().join("drafts/shape-native.json")
                } else {
                    path.clone()
                };
                let bytes = serde_json::to_vec(&value).unwrap();
                fs::create_dir_all(target.parent().unwrap()).unwrap();
                fs::write(&target, &bytes).unwrap();
                let error = if draft {
                    let error = repo.claim_draft(doc.id()).unwrap().unwrap().unwrap_err();
                    repo.release_draft(doc.id());
                    error
                } else {
                    repo.load(doc.id()).unwrap_err().to_string()
                };
                assert!(
                    error.contains("Ellipse graphics require native v27"),
                    "{version}: {error}"
                );
                assert_eq!(fs::read(&target).unwrap(), bytes);
            }
        }
    }
}

#[test]
fn v26_legacy_shapes_migrate_without_graphic_changes_and_archive_exact_document_and_draft_bytes() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let mut doc = drawing(GraphicShape::Rectangle);
    doc.apply(&[Command::MoveObjects {
        objects: vec![object(2)],
        delta: Vector { dx: 1.0, dy: 2.0 },
    }])
    .unwrap();
    doc.undo().unwrap();
    repo.save(&doc, None).unwrap();
    let mut value = raw(&root);
    value["format_version"] = json!(26);
    let original = serde_json::to_vec_pretty(&value).unwrap();
    let path = root.path().join("documents/shape-native.json");
    fs::write(&path, &original).unwrap();
    let loaded = repo.load(doc.id()).unwrap().unwrap();
    assert_eq!(loaded, doc);
    assert_eq!(fs::read(&path).unwrap(), original);
    repo.save(&loaded, Some(loaded.revision())).unwrap();
    assert_eq!(
        fs::read(repo.migrated_path(doc.id(), 26)).unwrap(),
        original
    );
    assert_eq!(raw(&root)["format_version"], 28);
    let draft = Draft {
        document: doc.clone(),
        base_revision: doc.revision(),
        saved_at_ms: 42,
    };
    repo.write_draft(&draft).unwrap();
    repo.release_draft(doc.id());
    let path = root.path().join("drafts/shape-native.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["format_version"] = json!(26);
    let original = serde_json::to_vec_pretty(&value).unwrap();
    fs::write(&path, &original).unwrap();
    assert_eq!(repo.claim_draft(doc.id()), Ok(Some(Ok(draft))));
    repo.prepare_draft_for_recovery(
        doc.id(),
        DraftRecoveryTarget::Original {
            base_revision: doc.revision(),
        },
    )
    .unwrap();
    assert_eq!(
        fs::read(repo.migrated_draft_path(doc.id(), 26)).unwrap(),
        original
    );
    repo.release_draft(doc.id());
}

#[test]
fn future_native_and_shape_changing_history_are_refused_without_rewriting() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let mut doc = drawing(GraphicShape::Ellipse);
    doc.apply(&[Command::MoveObjects {
        objects: vec![object(2)],
        delta: Vector { dx: 1.0, dy: 2.0 },
    }])
    .unwrap();
    repo.save(&doc, None).unwrap();
    let template = raw(&root);
    let mut future = template.clone();
    future["format_version"] = json!(29);
    let mut changed = template;
    changed["history"]["undo"][1]["objects"][0]["before"]["shape"] = json!("rectangle");
    for value in [future, changed] {
        let path = root.path().join("documents/shape-native.json");
        let bytes = serde_json::to_vec(&value).unwrap();
        fs::write(&path, &bytes).unwrap();
        assert!(repo.load(doc.id()).is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
}
