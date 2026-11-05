//! Independent CDX binary reader and writer for the common interchange document.
//!
//! Written from the public "CDX File Format" documentation: a 28-byte header, then tagged
//! objects and properties. Everything is little-endian. A property tag has bit 15 clear, an
//! object tag has it set, and each object ends with a zero tag. Font and color tables are
//! properties in CDX but objects in the common model; styled text is one string property
//! that becomes style-run objects.
//!
//! Uninterpreted objects and properties are kept as opaque binary. The reader never guesses
//! a text encoding and refuses anything it cannot place exactly.

mod raw;
mod read;
mod text;
mod write;

use std::fmt;

pub use read::{CdxDocument, read};
pub use write::{CdxWrite, write};

/// Largest accepted or produced file.
pub const MAX_BYTES: usize = 1024 * 1024;

pub(crate) const MAGIC: &[u8; 8] = b"VjCD0100";
pub(crate) const HEADER_LEN: usize = 28;
/// Bytes 8..12 of the header as the public specification gives them.
pub(crate) const RESERVED_PREFIX: [u8; 4] = [0x04, 0x03, 0x02, 0x01];

/// Object and property tags of the supported subset.
pub(crate) mod tag {
    pub const REPRESENTS: u16 = 0x000e;
    pub const FONT_TABLE: u16 = 0x0100;
    pub const POSITION: u16 = 0x0200;
    pub const BOUNDING_BOX: u16 = 0x0204;
    pub const COLOR_TABLE: u16 = 0x0300;
    pub const NODE_TYPE: u16 = 0x0400;
    pub const ELEMENT: u16 = 0x0402;
    pub const ISOTOPE: u16 = 0x0420;
    pub const CHARGE: u16 = 0x0421;
    pub const RADICAL: u16 = 0x0422;
    pub const SHOW_ATOM_ENHANCED_STEREO: u16 = 0x0445;
    pub const ENHANCED_STEREO_TYPE: u16 = 0x0446;
    pub const ENHANCED_STEREO_GROUP_NUM: u16 = 0x0447;
    pub const BOND_DISPLAY: u16 = 0x0601;
    pub const BOND_ORDER: u16 = 0x0600;
    pub const BOND_BEGIN: u16 = 0x0604;
    pub const BOND_END: u16 = 0x0605;
    pub const TEXT: u16 = 0x0700;
    pub const GRAPHIC_TYPE: u16 = 0x0a00;

    pub const DOCUMENT: u16 = 0x8000;
    pub const PAGE: u16 = 0x8001;
    pub const GROUP: u16 = 0x8002;
    pub const FRAGMENT: u16 = 0x8003;
    pub const NODE: u16 = 0x8004;
    pub const BOND: u16 = 0x8005;
    pub const TEXT_OBJECT: u16 = 0x8006;
    pub const GRAPHIC: u16 = 0x8007;
}

/// Property attributes a `Represents` object may name, with their CDX tags. Other tags
/// have no verified CDXML spelling here and stay opaque.
pub(crate) const REPRESENTED_ATTRIBUTES: [(&str, u16); 7] = [
    ("p", tag::POSITION),
    ("NodeType", tag::NODE_TYPE),
    ("Element", tag::ELEMENT),
    ("Isotope", tag::ISOTOPE),
    ("Charge", tag::CHARGE),
    ("Radical", tag::RADICAL),
    ("Order", tag::BOND_ORDER),
];

/// A refused CDX file or tree. The offset is the byte position of the offending item
/// (zero is a real position); it is `None` when no single item is at fault.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CdxError {
    pub code: &'static str,
    pub message: String,
    pub offset: Option<u64>,
}

impl CdxError {
    pub(crate) fn at(code: &'static str, offset: u64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            offset: Some(offset),
        }
    }

    pub(crate) fn general(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            offset: None,
        }
    }
}

impl fmt::Display for CdxError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}
impl std::error::Error for CdxError {}
