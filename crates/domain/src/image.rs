//! Embedded PNG values and geometry. Decoding is an inner-owned port implemented by providers.
use crate::document::{EditError, Point, Vector};
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Weak};

pub const MAX_PNG_BYTES: usize = 128 * 1024;
pub const MAX_IMAGE_EDGE: u32 = 2048;
pub const MAX_IMAGE_PIXELS: u64 = 1_048_576;
pub const MAX_CURRENT_IMAGE_PIXELS: u64 = 4_194_304;
pub const MAX_HISTORY_IMAGE_PIXELS: u64 = 67_108_864;
pub const MAX_BASE64_BYTES: usize = MAX_PNG_BYTES.div_ceil(3) * 4;

/// A decoder must verify canonical base64, every chunk/CRC, the complete supported PNG
/// stream and IEND/EOF before returning. It must not retain decoded raster pixels.
pub trait PngDecoder: Send + Sync {
    fn validate(&self, data_base64: &str) -> Result<PngMetadata, String>;
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PngMetadata {
    pub width: u32,
    pub height: u32,
    pub compressed_bytes: usize,
}
#[derive(Debug, Eq, PartialEq)]
struct PngData {
    base64: Arc<str>,
    metadata: PngMetadata,
}
/// Immutable verified source data. Clones, geometry changes and undo never re-decode it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmbeddedPng(Arc<PngData>);
impl EmbeddedPng {
    pub fn data_base64(&self) -> &str {
        &self.0.base64
    }
    pub fn width(&self) -> u32 {
        self.0.metadata.width
    }
    pub fn height(&self) -> u32 {
        self.0.metadata.height
    }
    pub fn pixels(&self) -> u64 {
        u64::from(self.width()) * u64::from(self.height())
    }
}
/// Weak interning avoids repeated decode while any current/history state holds the asset.
/// The cache does not keep expired images alive; stale keys are removed before insertion.
#[derive(Default)]
pub struct PngCache {
    assets: HashMap<Arc<str>, Weak<PngData>>,
}
impl PngCache {
    pub fn decode(&mut self, text: &str, decoder: &dyn PngDecoder) -> Result<EmbeddedPng, String> {
        if let Some(asset) = self.assets.get(text).and_then(Weak::upgrade) {
            return Ok(EmbeddedPng(asset));
        }
        if text.is_empty() || text.len() > MAX_BASE64_BYTES {
            return Err("PNG source exceeds the 128 KiB compressed-byte policy".into());
        }
        let metadata = decoder.validate(text)?;
        if metadata.width == 0
            || metadata.height == 0
            || metadata.width > MAX_IMAGE_EDGE
            || metadata.height > MAX_IMAGE_EDGE
            || u64::from(metadata.width) * u64::from(metadata.height) > MAX_IMAGE_PIXELS
            || metadata.compressed_bytes > MAX_PNG_BYTES
        {
            return Err(
                "PNG dimensions or compressed bytes exceed the embedded-image policy".into(),
            );
        }
        self.assets.retain(|_, value| value.strong_count() > 0);
        let base64: Arc<str> = text.into();
        let asset = Arc::new(PngData {
            base64: base64.clone(),
            metadata,
        });
        self.assets.insert(base64, Arc::downgrade(&asset));
        Ok(EmbeddedPng(asset))
    }
    pub fn live_pixels(&self) -> u64 {
        self.assets
            .values()
            .filter_map(Weak::upgrade)
            .map(|data| u64::from(data.metadata.width) * u64::from(data.metadata.height))
            .sum()
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImageSize {
    x: f64,
    y: f64,
}
impl ImageSize {
    pub fn new(x: f64, y: f64) -> Result<Self, EditError> {
        if !x.is_finite() || !y.is_finite() || x <= 0. || y <= 0. || x > 200_000. || y > 200_000. {
            return Err(EditError::ImageInvalid);
        }
        Ok(Self { x, y })
    }
    pub fn x(self) -> f64 {
        self.x
    }
    pub fn y(self) -> f64 {
        self.y
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    pub asset: EmbeddedPng,
    pub origin: Point,
    pub size_pt: ImageSize,
}
impl Image {
    pub fn new(asset: EmbeddedPng, origin: Point, size_pt: ImageSize) -> Result<Self, EditError> {
        let image = Self {
            asset,
            origin,
            size_pt,
        };
        image.validate()?;
        Ok(image)
    }
    pub(crate) fn validate(&self) -> Result<(), EditError> {
        self.end()?;
        Ok(())
    }
    pub fn end(&self) -> Result<Point, EditError> {
        Point::new(
            self.origin.x() + self.size_pt.x,
            self.origin.y() + self.size_pt.y,
        )
        .map_err(|_| EditError::ImageInvalid)
    }
    pub(crate) fn translated(&self, delta: Vector) -> Result<Self, EditError> {
        Self::new(
            self.asset.clone(),
            self.origin.translated(delta)?,
            self.size_pt,
        )
    }
}
impl fmt::Display for ImageSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}×{} pt", self.x, self.y)
    }
}
