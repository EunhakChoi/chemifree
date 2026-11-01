//! Bounded, independent reader and writer for the documented CDX binary framing.
//! Unknown tagged items remain opaque; no claim is made about references they may contain.
use std::collections::BTreeMap;

use chemifree_domain::interchange::{
    Content, Document, Object, ObjectKind as O, Property, PropertyKind as K, PropertyValue as V,
};

const LIMIT: usize = 1024 * 1024;
const HEADER: &[u8; 28] = b"VjCD0100\x04\x03\x02\x01\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0";
const TEXT: u16 = 0x0700;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CdxError {
    pub code: &'static str,
    pub message: String,
    /// Zero-based byte offset in the supplied input, when the error has one.
    pub offset: Option<usize>,
}

/// Decoded common tree and information that could not be represented in it.
#[derive(Clone, Debug, PartialEq)]
pub struct CdxRead {
    pub document: Document,
    /// The font-table platform belongs to CDX framing and is not held in the common tree;
    /// this is the value read, when the file has a font table.
    pub font_platform: Option<u16>,
}

/// Encoded CDX bytes and the precision the encoding could not keep.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CdxWrite {
    pub bytes: Vec<u8>,
    /// Color channels that were not an exact multiple of 1/65535 and were rounded.
    pub rounded_color_channels: usize,
}
impl std::fmt::Display for CdxError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.offset {
            Some(offset) => write!(formatter, "{} at byte {offset}", self.message),
            None => formatter.write_str(&self.message),
        }
    }
}
impl std::error::Error for CdxError {}
fn fail(code: &'static str, message: impl Into<String>) -> CdxError {
    CdxError {
        code,
        message: message.into(),
        offset: None,
    }
}
fn fail_at(code: &'static str, message: impl Into<String>, offset: usize) -> CdxError {
    CdxError {
        offset: Some(offset),
        ..fail(code, message)
    }
}
fn object(kind: O, id: u32) -> Object {
    Object {
        kind,
        id: (id != 0).then_some(id),
        properties: Vec::new(),
        children: Vec::new(),
    }
}
fn property(kind: K, value: V) -> Property {
    Property { kind, value }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
    origin: usize,
    objects: usize,
    properties: usize,
    font_platform: Option<u16>,
}
impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8], at: usize, origin: usize) -> Self {
        Self {
            bytes,
            at,
            origin,
            objects: 0,
            properties: 0,
            font_platform: None,
        }
    }
    fn take(&mut self, count: usize) -> Result<&'a [u8], CdxError> {
        let end = self
            .at
            .checked_add(count)
            .filter(|&end| end <= self.bytes.len())
            .ok_or_else(|| {
                fail_at(
                    "truncated_cdx",
                    "A CDX item extends beyond its input.",
                    self.origin + self.at,
                )
            })?;
        let result = &self.bytes[self.at..end];
        self.at = end;
        Ok(result)
    }
    fn u16(&mut self) -> Result<u16, CdxError> {
        Ok(u16::from_le_bytes(
            self.take(2)?.try_into().expect("two bytes"),
        ))
    }
    fn u32(&mut self) -> Result<u32, CdxError> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("four bytes"),
        ))
    }
    fn body(&mut self, current: &mut Object, unknown: bool, depth: usize) -> Result<(), CdxError> {
        if depth > 128 {
            return Err(fail_at(
                "resource_limit",
                "CDX nesting exceeds 128 levels.",
                self.at,
            ));
        }
        let mut local_properties = 0usize;
        loop {
            let tag = self.u16()?;
            if tag == 0 {
                return Ok(());
            }
            if tag & 0x8000 != 0 {
                self.objects += 1;
                if self.objects > 100_000 {
                    return Err(fail_at(
                        "resource_limit",
                        "CDX contains too many objects.",
                        self.at,
                    ));
                }
                let id = self.u32()?;
                let kind = if unknown {
                    O::UnknownBinary(tag)
                } else {
                    match tag {
                        0x8001 => O::Page,
                        0x8002 => O::Group,
                        0x8003 => O::Fragment,
                        0x8004 => O::Node,
                        0x8005 => O::Bond,
                        0x8006 => O::Text,
                        0x8007 => O::Graphic,
                        _ => O::UnknownBinary(tag),
                    }
                };
                let opaque = matches!(kind, O::UnknownBinary(_));
                let mut child = object(kind, id);
                self.body(&mut child, opaque, depth + 1)?;
                current.children.push(Content::Object(child));
            } else {
                local_properties += 1;
                self.properties += 1;
                if local_properties > 512 || self.properties > 100_000 {
                    return Err(fail_at(
                        "resource_limit",
                        "CDX has too many properties.",
                        self.at,
                    ));
                }
                let short = self.u16()?;
                let length = if short == u16::MAX {
                    usize::try_from(self.u32()?).map_err(|_| {
                        fail_at("resource_limit", "CDX property is too large.", self.at)
                    })?
                } else {
                    usize::from(short)
                };
                let value = self.take(length)?;
                if !unknown && current.kind != O::Document && matches!(tag, 0x0100 | 0x0300) {
                    return Err(fail_at(
                        "unsupported_table_scope",
                        "Scoped font and color tables outside the document are not interpreted.",
                        self.at - length,
                    ));
                }
                if !unknown && current.kind == O::Document && tag == 0x0100 {
                    let (table, platform) = font_table(value, self.at - length)?;
                    // Any table off platform 1 must stay visible, whatever order the tables come in.
                    if self.font_platform.is_none_or(|known| known == 1) {
                        self.font_platform = Some(platform);
                    }
                    current.children.push(Content::Object(table));
                } else if !unknown && current.kind == O::Document && tag == 0x0300 {
                    current
                        .children
                        .push(Content::Object(color_table(value, self.at - length)?));
                } else {
                    current.properties.push(decode_property(
                        &current.kind,
                        tag,
                        value,
                        self.at - length,
                        unknown,
                    )?);
                }
            }
        }
    }
}
fn exact(bytes: &[u8], len: usize, at: usize) -> Result<&[u8], CdxError> {
    if bytes.len() != len {
        Err(fail_at(
            "invalid_property_length",
            format!("Expected {len} bytes, found {}.", bytes.len()),
            at,
        ))
    } else {
        Ok(bytes)
    }
}
fn unsigned16(bytes: &[u8], at: usize) -> Result<u16, CdxError> {
    Ok(u16::from_le_bytes(
        exact(bytes, 2, at)?.try_into().expect("two bytes"),
    ))
}
fn signed16(bytes: &[u8], at: usize) -> Result<i16, CdxError> {
    Ok(i16::from_le_bytes(
        exact(bytes, 2, at)?.try_into().expect("two bytes"),
    ))
}
fn unsigned32(bytes: &[u8], at: usize) -> Result<u32, CdxError> {
    Ok(u32::from_le_bytes(
        exact(bytes, 4, at)?.try_into().expect("four bytes"),
    ))
}
fn coord(bytes: &[u8]) -> f64 {
    f64::from(i32::from_le_bytes(bytes.try_into().expect("four bytes"))) / 65536.0
}
/// An enumerated or bounded integer; the ranges are those of the common model, so a value
/// outside them is refused here with its location rather than as a generic document error.
fn ranged(
    kind: K,
    value: i64,
    allowed: std::ops::RangeInclusive<i64>,
    at: usize,
) -> Result<Property, CdxError> {
    if allowed.contains(&value) {
        Ok(property(kind, V::Integer(value)))
    } else {
        Err(fail_at(
            "invalid_property_value",
            format!(
                "CDX value {value} is outside the supported {} to {}.",
                allowed.start(),
                allowed.end()
            ),
            at,
        ))
    }
}
fn decode_property(
    kind: &O,
    tag: u16,
    bytes: &[u8],
    at: usize,
    unknown: bool,
) -> Result<Property, CdxError> {
    if unknown {
        return Ok(property(K::UnknownBinary(tag), V::Bytes(bytes.to_vec())));
    }
    let value = match (kind, tag) {
        (O::Node | O::Text, 0x0200) => {
            let b = exact(bytes, 8, at)?;
            property(
                K::Position,
                V::Point {
                    x: coord(&b[4..8]),
                    y: coord(&b[..4]),
                },
            )
        }
        (O::Document | O::Page | O::Group | O::Fragment | O::Text | O::Graphic, 0x0204) => {
            let b = exact(bytes, 16, at)?;
            property(
                K::BoundingBox,
                V::Rectangle {
                    left: coord(&b[4..8]),
                    top: coord(&b[..4]),
                    right: coord(&b[12..16]),
                    bottom: coord(&b[8..12]),
                },
            )
        }
        (O::Node, 0x0400) => ranged(K::NodeType, signed16(bytes, at)?.into(), 0..=13, at)?,
        (O::Node, 0x0402) => ranged(K::AtomicNumber, signed16(bytes, at)?.into(), 1..=118, at)?,
        (O::Node, 0x0420) => ranged(K::Isotope, signed16(bytes, at)?.into(), 0..=32767, at)?,
        (O::Node, 0x0421) => {
            let charge = match bytes.len() {
                1 => bytes[0] as i8,
                4 => {
                    // The public Charge page defines the 4-byte form (an early ChemDraw bug) as
                    // `(INT32)saved >> 24`, which discards the low 24 bits. A fractional charge
                    // has no meaning in this profile, so those bits must be zero.
                    let saved = i32::from_le_bytes(bytes.try_into().expect("four bytes"));
                    if saved & 0x00ff_ffff != 0 {
                        return Err(fail_at(
                            "unsupported_fractional_charge",
                            "A 4-byte CDX charge with non-zero low 24 bits is not an integer charge.",
                            at,
                        ));
                    }
                    (saved >> 24) as i8
                }
                _ => {
                    return Err(fail_at(
                        "invalid_property_length",
                        "A CDX charge requires one or four bytes.",
                        at,
                    ));
                }
            };
            property(K::Charge, V::Integer(i64::from(charge)))
        }
        (O::Node, 0x0422) => {
            // The public Radical page enumerates None, Singlet, Doublet and Triplet (0 to 3).
            let radical = exact(bytes, 1, at)?[0];
            if radical > 3 {
                return Err(fail_at(
                    "invalid_property_value",
                    format!("CDX radical value {radical} is not one of the documented 0 to 3."),
                    at,
                ));
            }
            property(K::Radical, V::Integer(i64::from(radical)))
        }
        (O::Bond, 0x0600) => ranged(K::BondOrder, unsigned16(bytes, at)?.into(), 1..=65535, at)?,
        (O::Bond, 0x0604) => property(K::BondBegin, V::ObjectReference(unsigned32(bytes, at)?)),
        (O::Bond, 0x0605) => property(K::BondEnd, V::ObjectReference(unsigned32(bytes, at)?)),
        (O::Graphic, 0x0a00) => ranged(K::GraphicType, signed16(bytes, at)?.into(), 0..=7, at)?,
        _ => property(K::UnknownBinary(tag), V::Bytes(bytes.to_vec())),
    };
    Ok(value)
}
/// XML 1.0 cannot carry C0 controls other than tab, line feed and carriage return, so such
/// text could not be transcoded to CDXML.
fn xml_carriable(bytes: &[u8]) -> bool {
    bytes
        .iter()
        .all(|&byte| byte >= 0x20 || matches!(byte, b'\t' | b'\n' | b'\r'))
}
fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&byte| char::from(byte)).collect()
}
fn font_table(bytes: &[u8], at: usize) -> Result<(Object, u16), CdxError> {
    let mut c = Cursor::new(bytes, 0, at);
    let platform = c.u16()?;
    if platform > 1 {
        return Err(fail_at(
            "unsupported_font_platform",
            "Unknown CDX font platform.",
            at,
        ));
    }
    let count = c.u16()?;
    let mut table = object(O::FontTable, 0);
    for _ in 0..count {
        let id = c.u16()?;
        let charset = c.u16()?;
        let length = usize::from(c.u16()?);
        let name_bytes = c.take(length)?;
        if !name_bytes.is_ascii() {
            return Err(fail_at(
                "unsupported_encoding",
                "Non-ASCII font names require an encoding this profile does not infer.",
                at,
            ));
        }
        if !xml_carriable(name_bytes) {
            return Err(fail_at(
                "unsupported_text_character",
                "A CDX font name contains a control character that XML 1.0 cannot carry.",
                at,
            ));
        }
        let name = latin1(name_bytes);
        let mut font = object(O::Font, 0);
        font.properties
            .push(property(K::FontIndex, V::Integer(i64::from(id))));
        font.properties.push(property(K::FontName, V::Text(name)));
        if charset != 1252 {
            return Err(fail_at(
                "unsupported_encoding",
                format!("Font charset {charset} has no supported CDXML mapping."),
                at,
            ));
        }
        font.properties
            .push(property(K::FontCharset, V::Text("iso-8859-1".to_owned())));
        table.children.push(Content::Object(font));
    }
    if c.at != bytes.len() {
        return Err(fail_at(
            "invalid_font_table",
            "Trailing bytes in the CDX font table.",
            at + c.at,
        ));
    }
    if count == 0 {
        return Err(fail_at(
            "unsupported_empty_table",
            "This interchange profile does not interpret a CDX font table with zero entries.",
            at + 2,
        ));
    }
    Ok((table, platform))
}
fn color_table(bytes: &[u8], at: usize) -> Result<Object, CdxError> {
    let mut c = Cursor::new(bytes, 0, at);
    let count = c.u16()?;
    let mut table = object(O::ColorTable, 0);
    for _ in 0..count {
        let mut color = object(O::Color, 0);
        for kind in [K::Red, K::Green, K::Blue] {
            color
                .properties
                .push(property(kind, V::Float(f64::from(c.u16()?) / 65535.0)));
        }
        table.children.push(Content::Object(color));
    }
    if c.at != bytes.len() {
        return Err(fail_at(
            "invalid_color_table",
            "Trailing bytes in the CDX color table.",
            at + c.at,
        ));
    }
    if count == 0 {
        return Err(fail_at(
            "unsupported_empty_table",
            "This interchange profile does not interpret a CDX color table with zero entries.",
            at,
        ));
    }
    Ok(table)
}
fn fonts(root: &Object) -> BTreeMap<u16, String> {
    let mut result = BTreeMap::new();
    for child in &root.children {
        let Content::Object(table) = child else {
            continue;
        };
        if table.kind != O::FontTable {
            continue;
        }
        for child in &table.children {
            let Content::Object(font) = child else {
                continue;
            };
            let id = font
                .properties
                .iter()
                .find_map(|p| match (&p.kind, &p.value) {
                    (K::FontIndex, V::Integer(n)) => u16::try_from(*n).ok(),
                    _ => None,
                });
            let charset = font
                .properties
                .iter()
                .find_map(|p| match (&p.kind, &p.value) {
                    (K::FontCharset, V::Text(s)) => Some(s.clone()),
                    _ => None,
                });
            if let (Some(id), Some(charset)) = (id, charset) {
                result.insert(id, charset);
            }
        }
    }
    result
}
fn text_runs(root: &mut Object, fonts: &BTreeMap<u16, String>) -> Result<(), CdxError> {
    if root.kind == O::Text {
        let mut raw = root
            .properties
            .iter()
            .enumerate()
            .filter(|(_, p)| p.kind == K::UnknownBinary(TEXT));
        let (Some((index, _)), None) = (raw.next(), raw.next()) else {
            return Err(fail(
                "invalid_text",
                "A CDX text object must have exactly one text property.",
            ));
        };
        let V::Bytes(bytes) = root.properties.remove(index).value else {
            unreachable!("binary property")
        };
        let mut c = Cursor::new(&bytes, 0, 0);
        // The raw text property was detached from the stream; do not report its local
        // offset as a file offset when its style table is truncated.
        let unlocated = |error: CdxError| CdxError {
            offset: None,
            ..error
        };
        let count = usize::from(c.u16().map_err(unlocated)?);
        let mut runs = Vec::with_capacity(count);
        for _ in 0..count {
            let mut run = [0u16; 5];
            for value in &mut run {
                *value = c.u16().map_err(unlocated)?;
            }
            runs.push(run);
        }
        let text = &bytes[c.at..];
        if !xml_carriable(text) {
            return Err(fail(
                "unsupported_text_character",
                "CDX text contains a control character that XML 1.0 cannot carry.",
            ));
        }
        if text.iter().any(|byte| (0x80..=0x9f).contains(byte)) {
            return Err(fail(
                "unsupported_encoding",
                "Bytes with ambiguous Latin-1/Windows-1252 meanings are not decoded.",
            ));
        }
        // An empty segment is not materialized, matching how CDXML reads an empty element.
        let segment = |bytes: &[u8]| (!bytes.is_empty()).then(|| Content::Text(latin1(bytes)));
        if count == 0 {
            let mut style = object(O::StyleRun, 0);
            style.children.extend(segment(text));
            root.children.push(Content::Object(style));
        } else {
            if runs[0][0] != 0
                || runs.windows(2).any(|pair| pair[0][0] >= pair[1][0])
                || usize::from(runs.last().expect("nonempty")[0]) > text.len()
            {
                return Err(fail("invalid_text", "CDX text style offsets are invalid."));
            }
            for (index, [offset, font, face, size, color]) in runs.iter().enumerate() {
                if fonts.get(font).map(String::as_str) != Some("iso-8859-1") {
                    return Err(fail(
                        "unsupported_encoding",
                        format!(
                            "Text uses unsupported or missing CDX font charset for font {font}."
                        ),
                    ));
                }
                let end = runs
                    .get(index + 1)
                    .map_or(text.len(), |run| usize::from(run[0]));
                let mut style = object(O::StyleRun, 0);
                style.properties.extend([
                    property(K::FontIndex, V::Integer(i64::from(*font))),
                    property(K::FontFace, V::Integer(i64::from(*face))),
                    property(K::FontSize, V::Float(f64::from(*size) / 20.0)),
                    property(K::ColorIndex, V::Integer(i64::from(*color))),
                ]);
                style
                    .children
                    .extend(segment(&text[usize::from(*offset)..end]));
                root.children.push(Content::Object(style));
            }
        }
    }
    for child in &mut root.children {
        if let Content::Object(child) = child {
            text_runs(child, fonts)?;
        }
    }
    Ok(())
}

/// Parses the documented implicit-document form and the explicitly tagged document form.
pub fn read(bytes: &[u8]) -> Result<CdxRead, CdxError> {
    if bytes.len() > LIMIT {
        return Err(fail("resource_limit", "CDX exceeds 1 MiB."));
    }
    if !bytes.starts_with(HEADER) {
        return Err(fail_at(
            "invalid_cdx_header",
            "Expected the 28-byte CDX header.",
            0,
        ));
    }
    let mut cursor = Cursor::new(bytes, HEADER.len(), 0);
    let mut root = object(O::Document, 0);
    let explicit_document = cursor.bytes[cursor.at..].starts_with(&[0x00, 0x80]);
    if explicit_document {
        cursor.u16()?;
        root.id = match cursor.u32()? {
            0 => None,
            id => Some(id),
        };
    }
    cursor.body(&mut root, false, 0)?;
    if explicit_document && cursor.u16()? != 0 {
        return Err(fail_at(
            "invalid_cdx_terminator",
            "An explicit document object must be followed by the file terminator.",
            cursor.at - 2,
        ));
    }
    if cursor.at != bytes.len() {
        return Err(fail_at(
            "trailing_cdx",
            "Unexpected bytes after the CDX document terminator.",
            cursor.at,
        ));
    }
    let table = fonts(&root);
    text_runs(&mut root, &table)?;
    // Whole-tree validation may discover a duplicate or dangling reference anywhere in the
    // document; the end-of-file cursor is not a reliable location for that error.
    let document = Document::new(root).map_err(|e| fail("invalid_document", e.to_string()))?;
    Ok(CdxRead {
        document,
        font_platform: cursor.font_platform,
    })
}

fn encode_property(out: &mut Vec<u8>, tag: u16, bytes: &[u8]) -> Result<(), CdxError> {
    let len = u32::try_from(bytes.len())
        .map_err(|_| fail("resource_limit", "CDX property is too long."))?;
    out.extend_from_slice(&tag.to_le_bytes());
    if len >= u32::from(u16::MAX) {
        out.extend_from_slice(&u16::MAX.to_le_bytes());
        out.extend_from_slice(&len.to_le_bytes());
    } else {
        out.extend_from_slice(&(len as u16).to_le_bytes());
    }
    out.extend_from_slice(bytes);
    if out.len() > LIMIT {
        return Err(fail("resource_limit", "CDX output exceeds 1 MiB."));
    }
    Ok(())
}
fn integer(object: &Object, kind: K) -> Option<i64> {
    object.properties.iter().find_map(|p| {
        if p.kind == kind {
            if let V::Integer(n) = p.value {
                Some(n)
            } else {
                None
            }
        } else {
            None
        }
    })
}
fn text_property(object: &Object, kind: K) -> Option<&str> {
    object.properties.iter().find_map(|p| {
        if p.kind == kind {
            if let V::Text(s) = &p.value {
                Some(s.as_str())
            } else {
                None
            }
        } else {
            None
        }
    })
}
fn float(object: &Object, kind: K) -> Option<f64> {
    object
        .properties
        .iter()
        .find_map(|p| match (&p.kind, &p.value) {
            (k, V::Float(n)) if *k == kind => Some(*n),
            _ => None,
        })
}
fn latin1_bytes(text: &str) -> Result<Vec<u8>, CdxError> {
    if text.len() > LIMIT {
        return Err(fail("resource_limit", "CDX text exceeds 1 MiB."));
    }
    let bytes: Vec<u8> = text
        .chars()
        .map(|c| {
            u8::try_from(u32::from(c)).map_err(|_| {
                fail(
                    "unsupported_encoding",
                    "This CDX writer supports ISO Latin-1 text only.",
                )
            })
        })
        .collect::<Result<_, _>>()?;
    // The reader refuses these, so the writer must not produce them.
    if !xml_carriable(&bytes) {
        return Err(fail(
            "unsupported_text_character",
            "CDX text contains a control character that XML 1.0 cannot carry.",
        ));
    }
    Ok(bytes)
}
fn encode_fonts(table: &Object) -> Result<Vec<u8>, CdxError> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(
        &u16::try_from(table.children.len())
            .map_err(|_| fail("resource_limit", "Too many fonts."))?
            .to_le_bytes(),
    );
    for child in &table.children {
        let Content::Object(font) = child else {
            return Err(fail(
                "unrepresentable_content",
                "A font table has unsupported content.",
            ));
        };
        if font.kind != O::Font
            || font.id.is_some()
            || font
                .properties
                .iter()
                .any(|p| !matches!(p.kind, K::FontIndex | K::FontName | K::FontCharset))
            || !font.children.is_empty()
        {
            return Err(fail(
                "unrepresentable_content",
                "A font has unsupported attributes or children.",
            ));
        }
        let id = integer(font, K::FontIndex)
            .and_then(|n| u16::try_from(n).ok())
            .ok_or_else(|| fail("invalid_document", "A font index is required."))?;
        if text_property(font, K::FontCharset) != Some("iso-8859-1") {
            return Err(fail(
                "unsupported_encoding",
                "Only the documented iso-8859-1 font charset is written.",
            ));
        }
        let name = latin1_bytes(
            text_property(font, K::FontName)
                .ok_or_else(|| fail("invalid_document", "A font name is required."))?,
        )?;
        if !name.is_ascii() {
            return Err(fail(
                "unsupported_encoding",
                "Non-ASCII font names require an encoding this profile does not infer.",
            ));
        }
        if bytes.len().saturating_add(6).saturating_add(name.len()) > LIMIT {
            return Err(fail("resource_limit", "CDX font table exceeds 1 MiB."));
        }
        bytes.extend_from_slice(&id.to_le_bytes());
        bytes.extend_from_slice(&1252u16.to_le_bytes());
        bytes.extend_from_slice(
            &u16::try_from(name.len())
                .map_err(|_| fail("resource_limit", "Font name is too long."))?
                .to_le_bytes(),
        );
        bytes.extend_from_slice(&name);
    }
    Ok(bytes)
}
fn quantized(value: f64, scale: f64) -> Result<i32, CdxError> {
    let scaled = value * scale;
    if !scaled.is_finite()
        || !(f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&scaled)
        || scaled != scaled.round()
    {
        return Err(fail(
            "unrepresentable_number",
            "The value is outside or between the exact CDX numeric steps.",
        ));
    }
    Ok(scaled.round() as i32)
}
/// Colors are stored as 16-bit channels; a CDXML value between two steps is rounded and counted.
fn encode_colors(table: &Object, rounded: &mut usize) -> Result<Vec<u8>, CdxError> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(
        &u16::try_from(table.children.len())
            .map_err(|_| fail("resource_limit", "Too many colors."))?
            .to_le_bytes(),
    );
    for child in &table.children {
        let Content::Object(color) = child else {
            return Err(fail(
                "unrepresentable_content",
                "A color table has unsupported content.",
            ));
        };
        if color.kind != O::Color
            || color.id.is_some()
            || !color.children.is_empty()
            || color.properties.len() != 3
        {
            return Err(fail(
                "unrepresentable_content",
                "A color has unsupported attributes or children.",
            ));
        }
        for kind in [K::Red, K::Green, K::Blue] {
            let value = float(color, kind)
                .ok_or_else(|| fail("invalid_document", "Each color needs red, green and blue."))?;
            let scaled = value * 65535.0;
            if !(0.0..=65535.0).contains(&scaled) {
                return Err(fail(
                    "unrepresentable_number",
                    "Color is outside the CDX range.",
                ));
            }
            // Floating-point error in k/65535 is not a precision loss.
            if (scaled - scaled.round()).abs() > 1e-6 {
                *rounded += 1;
            }
            bytes.extend_from_slice(&(scaled.round() as u16).to_le_bytes());
        }
    }
    Ok(bytes)
}
fn encode_text(object: &Object, font_map: &BTreeMap<u16, String>) -> Result<Vec<u8>, CdxError> {
    let mut styles = Vec::new();
    let mut text = Vec::new();
    for child in &object.children {
        let style = match child {
            Content::Object(style) => style,
            // Indentation between runs carries no meaning in the XML form.
            Content::Text(value) if value.trim().is_empty() => continue,
            _ => {
                return Err(fail(
                    "unrepresentable_content",
                    "A text object has unsupported content.",
                ));
            }
        };
        if style.kind != O::StyleRun || style.id.is_some() {
            return Err(fail(
                "unrepresentable_content",
                "A text style has an unsupported ID or kind.",
            ));
        }
        let offset = text.len();
        // An empty run (no characters) reads back from XML as a run with no children.
        match style.children.as_slice() {
            [] => {}
            [Content::Text(value)] => {
                let encoded = latin1_bytes(value)?;
                if text.len().saturating_add(encoded.len()) > LIMIT {
                    return Err(fail("resource_limit", "CDX text exceeds 1 MiB."));
                }
                text.extend_from_slice(&encoded);
            }
            _ => {
                return Err(fail(
                    "unrepresentable_content",
                    "A CDX style run must contain at most one text segment.",
                ));
            }
        }
        if style.properties.is_empty() {
            styles.push((offset, None));
        } else {
            if style.properties.iter().any(|p| {
                !matches!(
                    p.kind,
                    K::FontIndex | K::FontFace | K::FontSize | K::ColorIndex
                )
            }) {
                return Err(fail(
                    "unrepresentable_content",
                    "A text style has unsupported properties.",
                ));
            }
            let required = |kind: K, what: &str| {
                integer(style, kind)
                    .and_then(|v| u16::try_from(v).ok())
                    .ok_or_else(|| {
                        fail(
                            "unrepresentable_style",
                            format!("A styled CDX run needs an explicit {what}."),
                        )
                    })
            };
            let font = required(K::FontIndex, "font")?;
            if font_map.get(&font).map(String::as_str) != Some("iso-8859-1") {
                return Err(fail(
                    "unsupported_encoding",
                    "The style font must use ISO Latin-1.",
                ));
            }
            let face = required(K::FontFace, "face")?;
            let size = float(style, K::FontSize).ok_or_else(|| {
                fail(
                    "unrepresentable_style",
                    "A styled CDX run needs an explicit size.",
                )
            })?;
            let size = u16::try_from(quantized(size, 20.0)?)
                .map_err(|_| fail("unrepresentable_style", "Font size is out of range."))?;
            let color = required(K::ColorIndex, "color")?;
            styles.push((offset, Some((font, face, size, color))));
        }
    }
    if text.iter().any(|byte| (0x80..=0x9f).contains(byte)) {
        return Err(fail(
            "unsupported_encoding",
            "Bytes with ambiguous Latin-1/Windows-1252 meanings are not written.",
        ));
    }
    let unstyled = styles.iter().filter(|(_, style)| style.is_none()).count();
    if unstyled > 1 && unstyled == styles.len() {
        return Err(fail(
            "unrepresentable_style",
            "Several unstyled runs cannot be written without merging them into one.",
        ));
    }
    if styles.len() <= 1 && unstyled == styles.len() {
        // No run, or one unstyled run: the implicit string.
        let mut result = vec![0, 0];
        result.extend_from_slice(&text);
        return Ok(result);
    }
    if unstyled > 0 {
        return Err(fail(
            "unrepresentable_style",
            "Mixed implicit and explicit CDX text styles are not supported.",
        ));
    }
    if styles.windows(2).any(|pair| pair[0].0 >= pair[1].0) {
        return Err(fail(
            "unrepresentable_style",
            "CDX style runs must have distinct increasing offsets.",
        ));
    }
    let mut result = Vec::new();
    result.extend_from_slice(
        &u16::try_from(styles.len())
            .map_err(|_| fail("resource_limit", "Too many style runs."))?
            .to_le_bytes(),
    );
    for (offset, style) in styles {
        result.extend_from_slice(
            &u16::try_from(offset)
                .map_err(|_| {
                    fail(
                        "unrepresentable_style",
                        "A style begins after the CDX offset range.",
                    )
                })?
                .to_le_bytes(),
        );
        let (font, face, size, color) = style.expect("checked");
        for value in [font, face, size, color] {
            result.extend_from_slice(&value.to_le_bytes());
        }
    }
    result.extend_from_slice(&text);
    Ok(result)
}
fn encode_item(
    out: &mut Vec<u8>,
    object: &Object,
    font_map: &BTreeMap<u16, String>,
    root: bool,
    rounded_colors: &mut usize,
) -> Result<(), CdxError> {
    let tag = match object.kind {
        O::Document => 0x8000,
        O::Page => 0x8001,
        O::Group => 0x8002,
        O::Fragment => 0x8003,
        O::Node => 0x8004,
        O::Bond => 0x8005,
        O::Text => 0x8006,
        O::Graphic => 0x8007,
        _ => {
            return Err(fail(
                "unrepresentable_content",
                "This object is not supported by the CDX writer.",
            ));
        }
    };
    if !root || object.id.is_some() {
        out.extend_from_slice(&u16::to_le_bytes(tag));
        out.extend_from_slice(&object.id.unwrap_or(0).to_le_bytes());
    }
    for p in &object.properties {
        let mut bytes = Vec::new();
        let tag = match (&p.kind, &p.value) {
            (K::Position, V::Point { x, y }) => {
                for v in [*y, *x] {
                    bytes.extend_from_slice(&quantized(v, 65536.0)?.to_le_bytes());
                }
                0x0200
            }
            (
                K::BoundingBox,
                V::Rectangle {
                    left,
                    top,
                    right,
                    bottom,
                },
            ) => {
                for v in [*top, *left, *bottom, *right] {
                    bytes.extend_from_slice(&quantized(v, 65536.0)?.to_le_bytes());
                }
                0x0204
            }
            (K::NodeType | K::AtomicNumber | K::Isotope | K::GraphicType, V::Integer(v)) => {
                let n = i16::try_from(*v).map_err(|_| {
                    fail(
                        "unrepresentable_number",
                        "Integer is outside the CDX signed-16 range.",
                    )
                })?;
                bytes.extend_from_slice(&n.to_le_bytes());
                match p.kind {
                    K::NodeType => 0x0400,
                    K::AtomicNumber => 0x0402,
                    K::Isotope => 0x0420,
                    _ => 0x0a00,
                }
            }
            (K::Charge, V::Integer(v)) => {
                bytes.push(i8::try_from(*v).map_err(|_| {
                    fail(
                        "unrepresentable_number",
                        "Charge is outside signed-8 range.",
                    )
                })? as u8);
                0x0421
            }
            (K::Radical, V::Integer(v)) => {
                // The reader accepts only the documented 0 to 3, so the writer must too.
                bytes.push(u8::try_from(*v).ok().filter(|v| *v <= 3).ok_or_else(|| {
                    fail(
                        "invalid_property_value",
                        "Radical is not one of the documented values 0 to 3.",
                    )
                })?);
                0x0422
            }
            (K::BondOrder, V::Integer(v)) => {
                bytes.extend_from_slice(
                    &u16::try_from(*v)
                        .map_err(|_| {
                            fail(
                                "unrepresentable_number",
                                "Bond order is outside unsigned-16 range.",
                            )
                        })?
                        .to_le_bytes(),
                );
                0x0600
            }
            (K::BondBegin | K::BondEnd, V::ObjectReference(id)) => {
                bytes.extend_from_slice(&id.to_le_bytes());
                if p.kind == K::BondBegin {
                    0x0604
                } else {
                    0x0605
                }
            }
            _ => {
                return Err(fail(
                    "unrepresentable_property",
                    format!("Property {:?} has no supported CDX encoding.", p.kind),
                ));
            }
        };
        encode_property(out, tag, &bytes)?;
    }
    if object.kind == O::Text {
        encode_property(out, TEXT, &encode_text(object, font_map)?)?;
    } else {
        for child in &object.children {
            let child = match child {
                Content::Object(child) => child,
                Content::Text(value) if value.trim().is_empty() => continue,
                _ => {
                    return Err(fail(
                        "unrepresentable_content",
                        "CDX cannot encode this XML content.",
                    ));
                }
            };
            match child.kind {
                O::FontTable if root && child.id.is_none() && child.properties.is_empty() => {
                    encode_property(out, 0x0100, &encode_fonts(child)?)?
                }
                O::ColorTable if root && child.id.is_none() && child.properties.is_empty() => {
                    encode_property(out, 0x0300, &encode_colors(child, rounded_colors)?)?
                }
                _ => encode_item(out, child, font_map, false, rounded_colors)?,
            }
        }
    }
    out.extend_from_slice(&[0, 0]);
    if out.len() > LIMIT {
        return Err(fail("resource_limit", "CDX output exceeds 1 MiB."));
    }
    Ok(())
}

/// Writes the bounded common-model subset without guessing unsupported encodings or numbers.
pub fn write(document: &Document) -> Result<CdxWrite, CdxError> {
    document
        .validate()
        .map_err(|e| fail("invalid_document", e.to_string()))?;
    fn check_limits(object: &Object, root: bool, total: &mut usize) -> Result<(), CdxError> {
        let synthetic = if root {
            object.children.iter().filter(|child| matches!(child, Content::Object(object) if matches!(object.kind, O::FontTable | O::ColorTable))).count()
        } else if object.kind == O::Text {
            1
        } else {
            0
        };
        let local = object.properties.len().saturating_add(synthetic);
        *total = (*total).saturating_add(local);
        if local > 512 || *total > 100_000 {
            return Err(fail("resource_limit", "CDX has too many properties."));
        }
        if object.kind != O::Text {
            for child in &object.children {
                if let Content::Object(child) = child
                    && !matches!(child.kind, O::FontTable | O::ColorTable)
                {
                    check_limits(child, false, total)?;
                }
            }
        }
        Ok(())
    }
    let mut total_properties = 0;
    check_limits(document.root(), true, &mut total_properties)?;
    let mut out = HEADER.to_vec();
    let mut rounded_color_channels = 0;
    encode_item(
        &mut out,
        document.root(),
        &fonts(document.root()),
        true,
        &mut rounded_color_channels,
    )?;
    if document.root().id.is_some() {
        // Unlike the implicit-document form, an explicit document has its own object
        // terminator before the file's separate document terminator.
        out.extend_from_slice(&[0, 0]);
        if out.len() > LIMIT {
            return Err(fail("resource_limit", "CDX output exceeds 1 MiB."));
        }
    }
    Ok(CdxWrite {
        bytes: out,
        rounded_color_channels,
    })
}

/// Whether the document has a font table, which the writer records as platform 1.
pub fn has_font_table(document: &Document) -> bool {
    document
        .root()
        .children
        .iter()
        .any(|child| matches!(child, Content::Object(table) if table.kind == O::FontTable))
}

/// Removes XML comments and processing instructions, which CDX has no way to hold, and joins
/// the text segments they separated. Returns `None` when there are none, without copying.
pub fn without_xml_notes(document: &Document) -> Result<Option<(Document, usize)>, CdxError> {
    fn is_note(content: &Content) -> bool {
        matches!(
            content,
            Content::Comment(_) | Content::ProcessingInstruction { .. }
        )
    }
    fn known(object: &Object) -> impl Iterator<Item = &Object> {
        object.children.iter().filter_map(|child| match child {
            Content::Object(child)
                if !matches!(child.kind, O::UnknownXml(_) | O::UnknownBinary(_)) =>
            {
                Some(child)
            }
            _ => None,
        })
    }
    fn count(object: &Object) -> usize {
        object.children.iter().filter(|c| is_note(c)).count()
            + known(object).map(count).sum::<usize>()
    }
    fn strip(object: &mut Object) -> usize {
        let before = object.children.len();
        object.children.retain(|child| !is_note(child));
        let removed = before - object.children.len();
        if removed > 0 {
            // Text on either side of a note was one string.
            let mut joined: Vec<Content> = Vec::with_capacity(object.children.len());
            for child in object.children.drain(..) {
                match (joined.last_mut(), child) {
                    (Some(Content::Text(left)), Content::Text(right)) => left.push_str(&right),
                    (_, child) => joined.push(child),
                }
            }
            object.children = joined;
        }
        removed
            + object
                .children
                .iter_mut()
                .filter_map(|child| match child {
                    Content::Object(child)
                        if !matches!(child.kind, O::UnknownXml(_) | O::UnknownBinary(_)) =>
                    {
                        Some(strip(child))
                    }
                    _ => None,
                })
                .sum::<usize>()
    }
    if count(document.root()) == 0 {
        return Ok(None);
    }
    let mut root = document.root().clone();
    let removed = strip(&mut root);
    let stripped = Document::new(root).map_err(|e| fail("invalid_document", e.to_string()))?;
    Ok(Some((stripped, removed)))
}
