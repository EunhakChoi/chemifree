//! Scene/image invariants use an explicit fake at the external PNG decoder port.
//! The real provider's byte/CRC/raster validation is covered by providers/tests/png_image.rs.
use chemifree_domain::document::{
    Command, Document, DocumentId, EditError, ObjectId, Outcome, Point, Vector,
};
use chemifree_domain::image::{
    EmbeddedPng, Image, ImageSize, MAX_BASE64_BYTES, PngCache, PngDecoder, PngMetadata,
};
use chemifree_domain::scene::{
    DeleteMode, ReferenceTarget, SceneObject, SceneReference, Selection, SelectionTransform,
};
use std::sync::atomic::{AtomicUsize, Ordering};
struct Decoder {
    calls: AtomicUsize,
    width: u32,
    height: u32,
    bytes: usize,
}
impl PngDecoder for Decoder {
    fn validate(&self, _: &str) -> Result<PngMetadata, String> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        Ok(PngMetadata {
            width: self.width,
            height: self.height,
            compressed_bytes: self.bytes,
        })
    }
}
fn decoder(edge: u32) -> Decoder {
    Decoder {
        calls: AtomicUsize::new(0),
        width: edge,
        height: edge,
        bytes: 69,
    }
}
fn point(x: f64, y: f64) -> Point {
    Point::new(x, y).unwrap()
}
fn object(n: u32) -> ObjectId {
    ObjectId::from_serial(n).unwrap()
}
fn doc() -> Document {
    Document::new(DocumentId::parse("image").unwrap(), "Image").unwrap()
}
fn add(asset: EmbeddedPng) -> Command {
    Command::AddImage {
        image: Image::new(asset, point(1., 2.), ImageSize::new(10., 20.).unwrap()).unwrap(),
        parent: None,
        index: None,
    }
}
fn image(document: &Document, n: u32) -> &Image {
    let SceneObject::Image(image) = document.object(object(n)).unwrap() else {
        panic!("image")
    };
    image
}
#[test]
fn immutable_source_is_interned_and_geometry_edits_undo_redo_do_not_decode() {
    let decoder = decoder(1);
    let mut cache = PngCache::default();
    let asset = cache.decode("AAAA", &decoder).unwrap();
    assert_eq!(cache.decode("AAAA", &decoder).unwrap(), asset);
    assert_eq!(decoder.calls.load(Ordering::Relaxed), 1);
    let mut document = doc();
    document.apply(&[add(asset.clone())]).unwrap();
    let before = document.clone();
    let size = ImageSize::new(5., 8.).unwrap();
    document
        .apply(&[Command::UpdateImageGeometry {
            object: object(2),
            origin: point(3., 4.),
            size_pt: size,
        }])
        .unwrap();
    assert_eq!(image(&document, 2).asset, asset);
    assert_eq!(image(&document, 2).origin, point(3., 4.));
    assert_eq!(image(&document, 2).size_pt, size);
    let stable = document.clone();
    assert_eq!(
        document
            .apply(&[Command::UpdateImageGeometry {
                object: object(2),
                origin: point(3., 4.),
                size_pt: size
            }])
            .unwrap(),
        Outcome::Unchanged
    );
    assert_eq!(document, stable);
    document.undo().unwrap();
    assert_eq!(document.to_parts().objects, before.to_parts().objects);
    document.redo().unwrap();
    assert_eq!(image(&document, 2).origin, point(3., 4.));
    assert_eq!(decoder.calls.load(Ordering::Relaxed), 1);
}
#[test]
fn group_move_reference_and_arrangement_retain_source_and_transform_refuses_whole_selection() {
    use chemifree_domain::scene::{AlignMode, ObjectArrangement};
    let decoder = decoder(1);
    let mut cache = PngCache::default();
    let asset = cache.decode("AAAA", &decoder).unwrap();
    let mut document = doc();
    document
        .apply(&[
            add(asset.clone()),
            add(asset),
            Command::Group {
                objects: vec![object(2), object(3)],
            },
            Command::AddReference {
                reference: SceneReference {
                    source: object(2),
                    target: ReferenceTarget::Object(object(3)),
                },
            },
        ])
        .unwrap();
    let selection = Selection {
        objects: [object(4), object(2)].into(),
        ..Selection::default()
    };
    document
        .apply(&[Command::MoveSelection {
            selection: selection.clone(),
            instance: None,
            delta: Vector { dx: 5., dy: -1. },
        }])
        .unwrap();
    assert_eq!(image(&document, 2).origin, point(6., 1.));
    assert_eq!(image(&document, 3).origin, point(6., 1.));
    let plan = document
        .object_arrangement(
            &[object(2), object(3)].into(),
            ObjectArrangement::Align(AlignMode::Right),
        )
        .unwrap();
    assert_eq!(
        (plan.units[0].bounds.left, plan.units[0].bounds.bottom),
        (6., 21.)
    );
    let before = document.clone();
    for transform in [
        SelectionTransform::Rotate { degrees: 0. },
        SelectionTransform::Rotate { degrees: 90. },
        SelectionTransform::ReflectHorizontal,
        SelectionTransform::ReflectVertical,
    ] {
        assert_eq!(
            document
                .apply(&[
                    Command::Rename {
                        title: "rollback".into()
                    },
                    Command::TransformSelection {
                        selection: selection.clone(),
                        instance: None,
                        pivot: point(0., 0.),
                        transform
                    }
                ])
                .unwrap_err()
                .error,
            EditError::ImageUnsupported(object(2))
        );
        assert_eq!(document, before);
    }
    assert_eq!(
        chemifree_domain::scene_clipboard::copy_fragment(&document, &selection)
            .unwrap_err()
            .as_str(),
        "image_clipboard_unsupported"
    );
    assert_eq!(document, before);
    assert_eq!(document.references().count(), 1);
}
#[test]
fn positive_size_coupled_end_bounds_and_discarded_immutable_history_are_validated() {
    let decoder = decoder(1);
    let mut cache = PngCache::default();
    let asset = cache.decode("AAAA", &decoder).unwrap();
    assert!(
        Image::new(
            asset.clone(),
            point(-100000., -100000.),
            ImageSize::new(200000., 200000.).unwrap()
        )
        .is_ok()
    );
    for (x, y) in [(0., 1.), (-1., 1.), (f64::INFINITY, 1.), (1., f64::NAN)] {
        assert_eq!(ImageSize::new(x, y), Err(EditError::ImageInvalid));
    }
    let mut document = doc();
    document.apply(&[add(asset)]).unwrap();
    let before = document.clone();
    assert_eq!(
        document
            .apply(&[Command::UpdateImageGeometry {
                object: object(2),
                origin: point(100000., 0.),
                size_pt: ImageSize::new(1., 1.).unwrap()
            }])
            .unwrap_err()
            .error,
        EditError::ImageInvalid
    );
    assert_eq!(document, before);
    document
        .apply(&[Command::UpdateImageGeometry {
            object: object(2),
            origin: point(3., 4.),
            size_pt: ImageSize::new(5., 6.).unwrap(),
        }])
        .unwrap();
    let other = cache.decode("AAAB", &decoder).unwrap();
    let mut bad = document.to_parts().undo[1].clone();
    for (_, change) in &mut bad.objects {
        if let Some(SceneObject::Image(image)) = &mut change.after {
            image.asset = other.clone();
        }
    }
    for n in 0..101 {
        document
            .apply(&[Command::Rename {
                title: format!("valid-{n}"),
            }])
            .unwrap();
    }
    let mut parts = document.to_parts();
    assert_eq!(parts.undo.len(), 100);
    parts.undo.insert(0, bad);
    // Exactly this invalid image entry would be discarded by the depth limit.

    assert!(
        Document::restore(parts)
            .unwrap_err()
            .to_string()
            .contains("immutable image asset")
    );
}
#[test]
fn current_pixel_budget_is_per_instance_and_history_pixel_budget_is_unique_by_source() {
    let decoder = decoder(1024);
    let mut cache = PngCache::default();
    let same = cache.decode("AAAA", &decoder).unwrap();
    let mut document = doc();
    document.apply(&vec![add(same.clone()); 4]).unwrap();
    let before = document.clone();
    assert_eq!(
        document.apply(&[add(same)]).unwrap_err().error,
        EditError::ImagePixelLimit
    );
    assert_eq!(document, before);
    let mut document = doc();
    let mut previous = Vec::new();
    for batch in 0..17 {
        let mut commands = Vec::new();
        if !previous.is_empty() {
            commands.push(Command::DeleteObjects {
                objects: previous,
                mode: DeleteMode::Subtree,
            });
        }
        let first = document.numbering().next_object;
        for n in 0..4 {
            let text = format!("{batch:04}{n:04}");
            commands.push(add(cache.decode(&text, &decoder).unwrap()));
        }
        document.apply(&commands).unwrap();
        previous = (first..first + 4).map(object).collect();
    }
    assert_eq!(
        document.undo_depth(),
        15,
        "68 unique one-Mi-pixel sources force old history out until64 remain"
    );
    assert_eq!(Document::restore(document.to_parts()).unwrap(), document);
}
#[test]
fn image_bytes_use_shared_admission_budget_without_redecoding_repeated_assets() {
    let decoder = Decoder {
        bytes: 128 * 1024,
        ..decoder(1)
    };
    let mut cache = PngCache::default();
    let text = "A".repeat(MAX_BASE64_BYTES - 1) + "=";
    let asset = cache.decode(&text, &decoder).unwrap();
    let mut document = doc();
    let before = document.clone();
    assert_eq!(
        document
            .apply(&(0..24).map(|_| add(asset.clone())).collect::<Vec<_>>())
            .unwrap_err()
            .error,
        EditError::ImagePersistenceLimit
    );
    assert_eq!(document, before);
    document
        .apply(&(0..23).map(|_| add(asset.clone())).collect::<Vec<_>>())
        .unwrap();
    assert_eq!(decoder.calls.load(Ordering::Relaxed), 1);
    document.undo().unwrap();
    document.redo().unwrap();
    assert_eq!(decoder.calls.load(Ordering::Relaxed), 1);
}

#[test]
fn image_asset_port_bounds_are_checked_before_cache_admission() {
    for (width, height, bytes) in [
        (0, 1, 69),
        (2049, 1, 69),
        (2048, 513, 69),
        (1, 1, 128 * 1024 + 1),
    ] {
        let decoder = Decoder {
            calls: AtomicUsize::new(0),
            width,
            height,
            bytes,
        };
        assert!(
            PngCache::default()
                .decode("AAAA", &decoder)
                .unwrap_err()
                .contains("dimensions or compressed bytes")
        );
    }
    let decoder = decoder(1);
    assert!(
        PngCache::default()
            .decode(&"A".repeat(MAX_BASE64_BYTES + 1), &decoder)
            .is_err()
    );
    assert_eq!(
        decoder.calls.load(Ordering::Relaxed),
        0,
        "preflight precedes external decoder work"
    );
}
