//! First pass: the byte-level tree of tagged objects and properties.
//!
//! Every length is checked before use, the depth and object count are bounded before any
//! allocation they would cause, and nothing here interprets a property.

use std::ops::Range;

use chemifree_domain::interchange::{MAX_DEPTH, MAX_OBJECTS};

use super::{CdxError, HEADER_LEN, MAGIC, MAX_BYTES, tag};

pub(super) struct RawProperty {
    pub tag: u16,
    /// Position of the property's tag.
    pub offset: u64,
    /// Where the value lies in the file.
    pub data: Range<usize>,
}

pub(super) enum RawItem {
    Property(RawProperty),
    Object(RawObject),
}

pub(super) struct RawObject {
    pub tag: u16,
    pub id: u32,
    /// Position of the object's tag.
    pub offset: u64,
    pub depth: usize,
    pub items: Vec<RawItem>,
}

pub(super) struct RawFile {
    pub root: RawObject,
    /// Objects found in the file; the model's own limit also counts objects made later.
    pub objects: usize,
}

struct Cursor<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl Cursor<'_> {
    fn truncated(&self) -> CdxError {
        CdxError::at(
            "truncated",
            self.position as u64,
            "The file ends in the middle of an item.",
        )
    }

    fn take(&mut self, length: usize) -> Result<&[u8], CdxError> {
        let end = self
            .position
            .checked_add(length)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| self.truncated())?;
        let slice = &self.bytes[self.position..end];
        self.position = end;
        Ok(slice)
    }

    fn u16(&mut self) -> Result<u16, CdxError> {
        let bytes = self.take(2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    fn u32(&mut self) -> Result<u32, CdxError> {
        let bytes = self.take(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }
}

pub(super) fn parse(bytes: &[u8]) -> Result<RawFile, CdxError> {
    if bytes.len() > MAX_BYTES {
        return Err(CdxError::at(
            "resource_limit",
            MAX_BYTES as u64,
            format!("CDX input exceeds {MAX_BYTES} bytes."),
        ));
    }
    let compared = bytes.len().min(MAGIC.len());
    if bytes[..compared] != MAGIC[..compared] {
        return Err(CdxError::at(
            "invalid_signature",
            0,
            "The file does not start with the CDX signature VjCD0100.",
        ));
    }
    if bytes.len() < HEADER_LEN {
        return Err(CdxError::at(
            "truncated",
            bytes.len() as u64,
            format!("The {HEADER_LEN}-byte header is incomplete."),
        ));
    }
    // Header bytes 8..28 are reserved; the specification gives their usual values but does
    // not make other values an error, so they are not interpreted.
    let mut cursor = Cursor {
        bytes,
        position: HEADER_LEN,
    };
    let root_offset = cursor.position as u64;
    let root_tag = cursor.u16()?;
    if root_tag != tag::DOCUMENT {
        return Err(CdxError::at(
            "invalid_root",
            root_offset,
            format!("The first object has tag 0x{root_tag:04x}; a Document (0x8000) is required."),
        ));
    }
    let mut objects = 0;
    let root = parse_object(&mut cursor, root_tag, root_offset, 0, &mut objects)?;
    let marker_offset = cursor.position as u64;
    if cursor.u16()? != 0 {
        return Err(CdxError::at(
            "invalid_end_marker",
            marker_offset,
            "The Document is not followed by the two zero bytes that end the file.",
        ));
    }
    if cursor.position != bytes.len() {
        return Err(CdxError::at(
            "trailing_data",
            cursor.position as u64,
            "Bytes follow the end-of-file marker.",
        ));
    }
    Ok(RawFile { root, objects })
}

fn parse_object(
    cursor: &mut Cursor<'_>,
    object_tag: u16,
    offset: u64,
    depth: usize,
    objects: &mut usize,
) -> Result<RawObject, CdxError> {
    if depth > MAX_DEPTH {
        return Err(CdxError::at(
            "resource_limit",
            offset,
            format!("Object nesting exceeds {MAX_DEPTH} levels."),
        ));
    }
    if *objects >= MAX_OBJECTS {
        return Err(CdxError::at(
            "resource_limit",
            offset,
            format!("The file holds more than {MAX_OBJECTS} objects."),
        ));
    }
    *objects += 1;
    let id = cursor.u32()?;
    let mut items = Vec::new();
    loop {
        let item_offset = cursor.position as u64;
        let item_tag = cursor.u16()?;
        if item_tag == 0 {
            break;
        }
        if item_tag & 0x8000 != 0 {
            items.push(RawItem::Object(parse_object(
                cursor,
                item_tag,
                item_offset,
                depth + 1,
                objects,
            )?));
            continue;
        }
        let mut length = usize::from(cursor.u16()?);
        if length == 0xffff {
            // The extended form: a four-byte length follows the marker. A shorter value
            // spelled this way is longer than necessary but still unambiguous.
            length = usize::try_from(cursor.u32()?).map_err(|_| {
                CdxError::at(
                    "resource_limit",
                    item_offset,
                    "A property length does not fit memory.",
                )
            })?;
        }
        let start = cursor.position;
        let end = start
            .checked_add(length)
            .filter(|end| *end <= cursor.bytes.len())
            .ok_or_else(|| {
                CdxError::at(
                    "truncated",
                    item_offset,
                    format!(
                        "Property 0x{item_tag:04x} claims {length} bytes but only {} remain.",
                        cursor.bytes.len() - start
                    ),
                )
            })?;
        cursor.position = end;
        items.push(RawItem::Property(RawProperty {
            tag: item_tag,
            offset: item_offset,
            data: start..end,
        }));
    }
    Ok(RawObject {
        tag: object_tag,
        id,
        offset,
        depth,
        items,
    })
}
