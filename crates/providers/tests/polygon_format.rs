//! Native v28 cardinality, legacy rejection and conservative variable-size reservation.
use chemifree_application::{DocumentRepository, Draft};
use chemifree_domain::document::{Command, Document, DocumentId, ObjectId, Point};
use chemifree_domain::scene::DeleteMode;
use chemifree_providers::{FileDocumentRepository, MAX_DOCUMENT_BYTES};
use serde_json::{Value, json};
use std::fs;
fn object(n: u32) -> ObjectId {
    ObjectId::from_serial(n).unwrap()
}
fn document() -> Document {
    let mut doc = Document::new(DocumentId::parse("polygon-native").unwrap(), "Polygon").unwrap();
    doc.apply(&[Command::AddPolygon {
        vertices: vec![Point::new(-100000., 100000.).unwrap(); 64],
        line_width_pt: Some(10.),
        parent: None,
        index: None,
    }])
    .unwrap();
    doc
}
fn mutate_graphics(value: &mut Value, edit: &impl Fn(&mut serde_json::Map<String, Value>)) {
    match value {
        Value::Object(map) => {
            if map.get("kind").and_then(Value::as_str) == Some("graphic") {
                edit(map);
            }
            for child in map.values_mut() {
                mutate_graphics(child, edit);
            }
        }
        Value::Array(values) => {
            for child in values {
                mutate_graphics(child, edit);
            }
        }
        _ => {}
    }
}
#[test]
fn native28_current_history_and_draft_preserve_every_vertex_and_fit_physical_reservation() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let mut doc = document();
    doc.apply(&[Command::UpdatePolygonVertices {
        object: object(2),
        vertices: vec![Point::new(1., 2.).unwrap(); 3],
    }])
    .unwrap();
    doc.undo().unwrap();
    repo.save(&doc, None).unwrap();
    let path = root.path().join("documents/polygon-native.json");
    let bytes = fs::read(path).unwrap();
    let raw: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(raw["format_version"], 28);
    assert_eq!(repo.load(doc.id()).unwrap(), Some(doc.clone()));
    assert!(bytes.len() <= doc.persistence_weight());
    assert!((bytes.len() as u64) < MAX_DOCUMENT_BYTES);
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
fn old_headers_and_malformed_history_only_polygon_refuse_without_rewriting() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let mut doc = document();
    repo.save(&doc, None).unwrap();
    let path = root.path().join("documents/polygon-native.json");
    let current: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    doc.apply(&[Command::DeleteObjects {
        objects: vec![object(2)],
        mode: DeleteMode::Subtree,
    }])
    .unwrap();
    repo.save(&doc, Some(1)).unwrap();
    let history: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for template in [&current, &history] {
        for version in 1..=27 {
            let mut old = template.clone();
            old["format_version"] = json!(version);
            let bytes = serde_json::to_vec(&old).unwrap();
            fs::write(&path, &bytes).unwrap();
            let error = repo.load(doc.id()).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("Polygon graphics require native v28"),
                "v{version}: {error}"
            );
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
    }
    for count in [0, 2, 65] {
        let mut invalid = history.clone();
        mutate_graphics(&mut invalid, &|map| {
            map.insert("points".into(), json!(vec![json!({"x":0,"y":0}); count]));
        });
        let bytes = serde_json::to_vec(&invalid).unwrap();
        fs::write(&path, &bytes).unwrap();
        assert!(
            repo.load(doc.id())
                .unwrap_err()
                .to_string()
                .contains("point count")
        );
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
}

#[test]
fn version27_document_and_draft_archive_exact_bytes_before_rewrite_to28() {
    use chemifree_application::DraftRecoveryTarget;
    use chemifree_domain::scene::{Graphic, GraphicShape};
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let mut doc = Document::new(
        DocumentId::parse("polygon-native").unwrap(),
        "Legacy ellipse",
    )
    .unwrap();
    doc.apply(&[Command::AddGraphic {
        graphic: Graphic {
            shape: GraphicShape::Ellipse,
            line_width_pt: None,
            points: vec![Point::new(0., 0.).unwrap(); 2],
        },
        parent: None,
        index: None,
    }])
    .unwrap();
    repo.save(&doc, None).unwrap();
    let path = root.path().join("documents/polygon-native.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["format_version"] = json!(27);
    let original = serde_json::to_vec_pretty(&value).unwrap();
    fs::write(&path, &original).unwrap();
    let loaded = repo.load(doc.id()).unwrap().unwrap();
    assert_eq!(loaded, doc);
    assert_eq!(fs::read(&path).unwrap(), original);
    repo.save(&loaded, Some(loaded.revision())).unwrap();
    assert_eq!(
        fs::read(repo.migrated_path(doc.id(), 27)).unwrap(),
        original
    );
    let draft = Draft {
        document: doc.clone(),
        base_revision: doc.revision(),
        saved_at_ms: 42,
    };
    repo.write_draft(&draft).unwrap();
    repo.release_draft(doc.id());
    let path = root.path().join("drafts/polygon-native.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["format_version"] = json!(27);
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
        fs::read(repo.migrated_draft_path(doc.id(), 27)).unwrap(),
        original
    );
    repo.release_draft(doc.id());
}

#[test]
fn maximum_polygon_forest_history_stays_below_reservation_and64mib() {
    use chemifree_domain::document::Vector;
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let mut doc = Document::new(
        DocumentId::parse("polygon-native").unwrap(),
        "Maximum vertices",
    )
    .unwrap();
    let vertices = (0..64)
        .map(|n| Point::new(n as f64 * 100., -(n as f64) * 100.).unwrap())
        .collect::<Vec<_>>();
    for count in [500, 499] {
        doc.apply(
            &(0..count)
                .map(|_| Command::AddPolygon {
                    vertices: vertices.clone(),
                    line_width_pt: None,
                    parent: None,
                    index: None,
                })
                .collect::<Vec<_>>(),
        )
        .unwrap();
    }
    for _ in 0..3 {
        doc.apply(&[Command::MoveObjects {
            objects: (2..502).map(object).collect(),
            delta: Vector { dx: 1., dy: 1. },
        }])
        .unwrap();
    }
    assert_eq!(
        doc.undo_depth(),
        1,
        "two 500-polygon before/after entries exceed shared history reservation"
    );
    repo.save(&doc, None).unwrap();
    let bytes = fs::read(root.path().join("documents/polygon-native.json")).unwrap();
    assert!(
        bytes.len() <= doc.persistence_weight(),
        "{} > {}",
        bytes.len(),
        doc.persistence_weight()
    );
    assert!((doc.persistence_weight() as u64) < MAX_DOCUMENT_BYTES);
    assert_eq!(repo.load(doc.id()).unwrap(), Some(doc));
}
