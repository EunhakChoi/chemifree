//! Native30 validates original PNG sources on every current/history input before retention.
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chemifree_application::{DocumentRepository, Draft, DraftRecoveryTarget};
use chemifree_domain::document::{Command, Document, DocumentId, ObjectId, Point};
use chemifree_domain::image::{Image, ImageSize, PngCache};
use chemifree_domain::scene::DeleteMode;
use chemifree_providers::png_image::EmbeddedPngDecoder;
use chemifree_providers::{FileDocumentRepository, MAX_DOCUMENT_BYTES};
use serde_json::{Value, json};
use std::fs;
const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DQAAAEgQGALFXOsAAAAABJRU5ErkJggg==";
fn object(n: u32) -> ObjectId {
    ObjectId::from_serial(n).unwrap()
}
fn image_document() -> Document {
    let mut cache = PngCache::default();
    let asset = cache.decode(PNG, &EmbeddedPngDecoder).unwrap();
    let image = Image::new(
        asset,
        Point::new(-10., 20.).unwrap(),
        ImageSize::new(30., 40.).unwrap(),
    )
    .unwrap();
    let mut doc = Document::new(DocumentId::parse("image-native").unwrap(), "Image").unwrap();
    doc.apply(&[Command::AddImage {
        image,
        parent: None,
        index: None,
    }])
    .unwrap();
    doc
}
fn path(root: &tempfile::TempDir) -> std::path::PathBuf {
    root.path().join("documents/image-native.json")
}
fn alter_images(value: &mut Value, edit: &impl Fn(&mut serde_json::Map<String, Value>)) {
    match value {
        Value::Object(map) => {
            if map.get("kind").and_then(Value::as_str) == Some("image") {
                edit(map);
            }
            for child in map.values_mut() {
                alter_images(child, edit);
            }
        }
        Value::Array(values) => {
            for child in values {
                alter_images(child, edit);
            }
        }
        _ => {}
    }
}
#[test]
fn current_history_and_draft_preserve_exact_source_and_only_geometry_changes() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let mut document = image_document();
    document
        .apply(&[Command::UpdateImageGeometry {
            object: object(2),
            origin: Point::new(5., 6.).unwrap(),
            size_pt: ImageSize::new(7., 8.).unwrap(),
        }])
        .unwrap();
    document.undo().unwrap();
    repo.save(&document, None).unwrap();
    let bytes = fs::read(path(&root)).unwrap();
    let raw: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(raw["format_version"], 30);
    assert_eq!(raw["document"]["objects"][1]["state"]["data_base64"], PNG);
    assert_eq!(repo.load(document.id()).unwrap(), Some(document.clone()));
    assert!(bytes.len() <= document.persistence_weight());
    assert!((document.persistence_weight() as u64) < MAX_DOCUMENT_BYTES);
    let draft = Draft {
        document: document.clone(),
        base_revision: document.revision(),
        saved_at_ms: 42,
    };
    repo.write_draft(&draft).unwrap();
    repo.release_draft(document.id());
    assert_eq!(repo.claim_draft(document.id()), Ok(Some(Ok(draft))));
    repo.release_draft(document.id());
}
#[test]
fn old_headers_current_and_history_only_images_and_future_are_refused_without_rewrite() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let mut document = image_document();
    repo.save(&document, None).unwrap();
    let current: Value = serde_json::from_slice(&fs::read(path(&root)).unwrap()).unwrap();
    document
        .apply(&[Command::DeleteObjects {
            objects: vec![object(2)],
            mode: DeleteMode::Subtree,
        }])
        .unwrap();
    repo.save(&document, Some(1)).unwrap();
    let history: Value = serde_json::from_slice(&fs::read(path(&root)).unwrap()).unwrap();
    for source in [&current, &history] {
        for version in 1..=29 {
            let mut value = source.clone();
            value["format_version"] = json!(version);
            let bytes = serde_json::to_vec(&value).unwrap();
            fs::write(path(&root), &bytes).unwrap();
            assert!(
                repo.load(document.id())
                    .unwrap_err()
                    .to_string()
                    .contains("Embedded images require native v30")
            );
            assert_eq!(fs::read(path(&root)).unwrap(), bytes);
        }
    }
    let mut future = current;
    future["format_version"] = json!(31);
    let bytes = serde_json::to_vec(&future).unwrap();
    fs::write(path(&root), &bytes).unwrap();
    assert!(
        repo.load(document.id())
            .unwrap_err()
            .to_string()
            .contains("version 31")
    );
    assert_eq!(fs::read(path(&root)).unwrap(), bytes);
}
#[test]
fn invalid_png_and_geometry_cannot_hide_in_discarded_history() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let mut document = image_document();
    document
        .apply(&[Command::UpdateImageGeometry {
            object: object(2),
            origin: Point::new(1., 2.).unwrap(),
            size_pt: ImageSize::new(3., 4.).unwrap(),
        }])
        .unwrap();
    repo.save(&document, None).unwrap();
    let seed: Value = serde_json::from_slice(&fs::read(path(&root)).unwrap()).unwrap();
    for n in 0..101 {
        document
            .apply(&[Command::Rename {
                title: format!("valid-{n}"),
            }])
            .unwrap();
    }
    repo.save(&document, Some(2)).unwrap();
    let raw: Value = serde_json::from_slice(&fs::read(path(&root)).unwrap()).unwrap();
    assert_eq!(raw["history"]["undo"].as_array().unwrap().len(), 100);
    for mode in ["png", "geometry", "mime", "swap"] {
        let mut value = raw.clone();
        let mut bad = seed["history"]["undo"][1].clone();
        alter_images(&mut bad, &|map| match mode {
            "png" => {
                let mut bytes = STANDARD.decode(PNG).unwrap();
                let last = bytes.len() - 1;
                bytes[last] ^= 1;
                map.insert("data_base64".into(), json!(STANDARD.encode(bytes)));
            }
            "geometry" => {
                map.insert("size_pt".into(), json!({"x":200000,"y":1}));
            }
            _ => {
                map.insert("mime_type".into(), json!("image/jpeg"));
            }
        });
        if mode == "swap" {
            bad = seed["history"]["undo"][1].clone();
            let mut bytes = Vec::new();
            {
                let mut encoder = png::Encoder::new(&mut bytes, 1, 1);
                encoder.set_color(png::ColorType::Rgba);
                encoder.set_depth(png::BitDepth::Eight);
                let mut writer = encoder.write_header().unwrap();
                writer.write_image_data(&[0, 255, 0, 128]).unwrap();
                writer.finish().unwrap();
            }
            bad["objects"][0]["after"]["data_base64"] = json!(STANDARD.encode(bytes));
        }
        let list = value["history"]["undo"].as_array_mut().unwrap();
        list.insert(0, bad); // only this injected entry would be trimmed

        let bytes = serde_json::to_vec(&value).unwrap();
        fs::write(path(&root), &bytes).unwrap();
        let error = repo.load(document.id()).unwrap_err();
        if mode == "swap" {
            assert!(
                error.to_string().contains("immutable image asset"),
                "{error}"
            );
        }
        assert_eq!(fs::read(path(&root)).unwrap(), bytes);
    }
}
#[test]
fn old29_document_and_draft_archive_exact_bytes_when_saved_as30() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let mut document =
        Document::new(DocumentId::parse("image-native").unwrap(), "Old spline").unwrap();
    document
        .apply(&[Command::AddSpline {
            points: [Point::new(0., 0.).unwrap(); 4],
            line_width_pt: None,
            parent: None,
            index: None,
        }])
        .unwrap();
    repo.save(&document, None).unwrap();
    let mut value: Value = serde_json::from_slice(&fs::read(path(&root)).unwrap()).unwrap();
    value["format_version"] = json!(29);
    let original = serde_json::to_vec_pretty(&value).unwrap();
    fs::write(path(&root), &original).unwrap();
    let loaded = repo.load(document.id()).unwrap().unwrap();
    assert_eq!(loaded, document);
    assert_eq!(fs::read(path(&root)).unwrap(), original);
    repo.save(&loaded, Some(loaded.revision())).unwrap();
    assert_eq!(
        fs::read(repo.migrated_path(document.id(), 29)).unwrap(),
        original
    );
    let draft = Draft {
        document: document.clone(),
        base_revision: document.revision(),
        saved_at_ms: 42,
    };
    repo.write_draft(&draft).unwrap();
    repo.release_draft(document.id());
    let file = root.path().join("drafts/image-native.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    value["format_version"] = json!(29);
    let original = serde_json::to_vec_pretty(&value).unwrap();
    fs::write(&file, &original).unwrap();
    assert_eq!(repo.claim_draft(document.id()), Ok(Some(Ok(draft))));
    repo.prepare_draft_for_recovery(
        document.id(),
        DraftRecoveryTarget::Original {
            base_revision: document.revision(),
        },
    )
    .unwrap();
    assert_eq!(
        fs::read(repo.migrated_draft_path(document.id(), 29)).unwrap(),
        original
    );
    repo.release_draft(document.id());
}

#[test]
fn maximum_source_files_reach_shared_admission_boundary_and_roundtrip_below64mib() {
    fn chunk(name: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut out = (data.len() as u32).to_be_bytes().to_vec();
        out.extend(name);
        out.extend(data);
        let mut crc = crc32fast::Hasher::new();
        crc.update(name);
        crc.update(data);
        out.extend(crc.finalize().to_be_bytes());
        out
    }
    let mut header = Vec::new();
    header.extend(1u32.to_be_bytes());
    header.extend(1u32.to_be_bytes());
    header.extend([8, 2, 0, 0, 0]);
    let mut compressed = vec![0x78, 0x01];
    for _ in 0..4 {
        compressed.extend([0, 0, 0, 255, 255]);
    }
    compressed.extend([1, 4, 0, 251, 255, 0, 0, 0, 0, 0, 4, 0, 1]);
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.extend(chunk(b"IHDR", &header));
    png.extend(chunk(b"IDAT", &compressed));
    for _ in 0..10915 {
        png.extend(chunk(b"IDAT", &[]));
    }
    png.extend(chunk(b"IEND", &[]));
    assert_eq!(png.len(), 128 * 1024);
    let mut cache = PngCache::default();
    let asset = cache
        .decode(&STANDARD.encode(&png), &EmbeddedPngDecoder)
        .unwrap();
    let image = Image::new(
        asset,
        Point::new(0., 0.).unwrap(),
        ImageSize::new(1., 1.).unwrap(),
    )
    .unwrap();
    let mut document = Document::new(
        DocumentId::parse("image-native").unwrap(),
        "Maximum original PNG",
    )
    .unwrap();
    for count in [23, 23, 23, 23, 23, 23, 5] {
        document
            .apply(&vec![
                Command::AddImage {
                    image: image.clone(),
                    parent: None,
                    index: None
                };
                count
            ])
            .unwrap();
    }
    let before = document.clone();
    assert_eq!(
        document
            .apply(&[Command::AddImage {
                image,
                parent: None,
                index: None
            }])
            .unwrap_err()
            .error,
        chemifree_domain::document::EditError::ImagePersistenceLimit
    );
    assert_eq!(document, before);
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    repo.save(&document, None).unwrap();
    let bytes = fs::read(path(&root)).unwrap();
    assert!(bytes.len() <= document.persistence_weight());
    assert!((document.persistence_weight() as u64) < MAX_DOCUMENT_BYTES);
    assert_eq!(repo.load(document.id()).unwrap(), Some(document));
}

#[test]
fn native_history_cannot_replay_more_pixels_than_the_current_image_budget() {
    let root = tempfile::tempdir().unwrap();
    let repo = FileDocumentRepository::new(root.path());
    let mut cache = PngCache::default();
    let asset = cache
        .decode(
            include_str!("../../application/tests/fixtures/image-1024-rgba.base64").trim(),
            &EmbeddedPngDecoder,
        )
        .unwrap();
    let image = Image::new(
        asset,
        Point::new(0., 0.).unwrap(),
        ImageSize::new(1., 1.).unwrap(),
    )
    .unwrap();
    let mut document = Document::new(
        DocumentId::parse("image-native").unwrap(),
        "History image pixel limit",
    )
    .unwrap();
    document
        .apply(&vec![
            Command::AddImage {
                image,
                parent: None,
                index: None
            };
            4
        ])
        .unwrap();
    repo.save(&document, None).unwrap();
    let seed: Value = serde_json::from_slice(&fs::read(path(&root)).unwrap()).unwrap();
    for redo in [false, true] {
        let mut raw = seed.clone();
        let mut entry = raw["history"]["undo"][0].clone();
        let image = raw["document"]["objects"][1]["state"].clone();
        let roots = raw["document"]["roots"].clone();
        let mut five = roots.clone();
        five.as_array_mut().unwrap().push(json!("s6"));
        entry["objects"] = if redo {
            json!([{"id":"s6","before":null,"after":image}])
        } else {
            json!([{"id":"s6","before":image,"after":null}])
        };
        entry["roots"] = if redo {
            json!({"before":roots,"after":five})
        } else {
            json!({"before":five,"after":roots})
        };
        raw["document"]["next_object"] = json!(7);
        raw["history"]["undo"] = if redo {
            json!([])
        } else {
            json!([entry.clone()])
        };
        raw["history"]["redo"] = if redo { json!([entry]) } else { json!([]) };
        let bytes = serde_json::to_vec(&raw).unwrap();
        fs::write(path(&root), &bytes).unwrap();
        let error = match repo.load(document.id()) {
            Err(error) => error,
            Ok(_) => panic!("4Mi current pixels must not hide a 5Mi replay state (redo={redo})"),
        };
        assert!(error.to_string().contains("history"), "{error}");
        assert_eq!(fs::read(path(&root)).unwrap(), bytes);
    }
}
