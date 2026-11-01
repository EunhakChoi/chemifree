//! Second pass: interpret the byte-level tree as the common interchange document.

use std::collections::{HashMap, HashSet};

use chemifree_domain::interchange::{
    Content, Document, MAX_DEPTH, MAX_OBJECTS, Object, ObjectKind, Property, PropertyKind,
    PropertyValue,
};

use super::raw::{self, RawItem, RawObject, RawProperty};
use super::text::{self, DecodedString, FontInfo};
use super::{CdxError, REPRESENTED_ATTRIBUTES, describe_validation, tag};

/// A font table of the source together with the platform that wrote it. The common model has
/// no platform field, so the writer consults this to know whether it may keep the platform.
#[derive(Clone, Debug, PartialEq)]
pub struct FontTablePlatform {
    pub table: Object,
    pub platform: u16,
}

/// A decoded CDX file.
#[derive(Clone, Debug, PartialEq)]
pub struct CdxDocument {
    document: Document,
    font_tables: Vec<FontTablePlatform>,
}

impl CdxDocument {
    /// The decoded tree.
    pub fn document(&self) -> &Document {
        &self.document
    }

    /// The font tables in file order with their source platforms.
    pub fn font_tables(&self) -> &[FontTablePlatform] {
        &self.font_tables
    }
}

/// Reads a complete CDX file.
///
/// # Errors
/// Refuses truncated or oversized input, malformed items, values outside the documented
/// ranges, unsupported text encodings and dangling references. Each error names the byte
/// offset of the offending item where one item is at fault.
pub fn read(bytes: &[u8]) -> Result<CdxDocument, CdxError> {
    let file = raw::parse(bytes)?;
    let mut builder = Builder::new(bytes, file.objects);
    builder.collect_tables(&file.root)?;
    let root = builder.object(&file.root, false)?;
    builder.check_references()?;
    let document = Document::new(root).map_err(|error| builder.model_error(&error))?;
    Ok(CdxDocument {
        document,
        font_tables: builder.font_tables,
    })
}

struct Builder<'a> {
    bytes: &'a [u8],
    objects: usize,
    fonts: HashMap<u16, FontInfo>,
    colors: Option<usize>,
    ids: HashMap<u32, u64>,
    references: Vec<(u32, u64)>,
    font_tables: Vec<FontTablePlatform>,
}

fn known_kind(object_tag: u16) -> Option<ObjectKind> {
    Some(match object_tag {
        tag::DOCUMENT => ObjectKind::Document,
        tag::PAGE => ObjectKind::Page,
        tag::GROUP => ObjectKind::Group,
        tag::FRAGMENT => ObjectKind::Fragment,
        tag::NODE => ObjectKind::Node,
        tag::BOND => ObjectKind::Bond,
        tag::TEXT_OBJECT => ObjectKind::Text,
        tag::GRAPHIC => ObjectKind::Graphic,
        _ => return None,
    })
}

fn is_opaque(kind: &ObjectKind) -> bool {
    matches!(
        kind,
        ObjectKind::UnknownBinary(_) | ObjectKind::UnknownXml(_)
    )
}

fn plain(kind: ObjectKind, properties: Vec<Property>, children: Vec<Content>) -> Object {
    Object {
        kind,
        id: None,
        properties,
        children,
    }
}

fn property(kind: PropertyKind, value: PropertyValue) -> Property {
    Property { kind, value }
}

fn bad_value(code: &'static str, raw: &RawProperty, what: &str) -> CdxError {
    CdxError::at(
        code,
        raw.offset,
        format!("Property 0x{:04x}: {what}.", raw.tag),
    )
}

impl<'a> Builder<'a> {
    fn new(bytes: &'a [u8], objects: usize) -> Self {
        Self {
            bytes,
            objects,
            fonts: HashMap::new(),
            colors: None,
            ids: HashMap::new(),
            references: Vec::new(),
            font_tables: Vec::new(),
        }
    }

    fn data(&self, raw: &RawProperty) -> &'a [u8] {
        &self.bytes[raw.data.clone()]
    }

    fn data_offset(raw: &RawProperty) -> u64 {
        raw.data.start as u64
    }

    /// Reserves room for objects the model has and the file does not spell out.
    fn reserve(&mut self, count: usize, offset: u64) -> Result<(), CdxError> {
        if self.objects.saturating_add(count) > MAX_OBJECTS {
            return Err(CdxError::at(
                "resource_limit",
                offset,
                format!("The decoded tree would hold more than {MAX_OBJECTS} objects."),
            ));
        }
        self.objects += count;
        Ok(())
    }

    /// Fonts and the color count must be known before any text refers to them.
    fn collect_tables(&mut self, root: &RawObject) -> Result<(), CdxError> {
        for item in &root.items {
            let RawItem::Property(raw) = item else {
                continue;
            };
            match raw.tag {
                tag::FONT_TABLE => {
                    let table = text::parse_font_table(self.data(raw), Self::data_offset(raw))?;
                    for entry in &table.entries {
                        let info = FontInfo {
                            platform: table.platform,
                        };
                        if self.fonts.insert(entry.id, info).is_some() {
                            return Err(CdxError::at(
                                "duplicate_font",
                                entry.offset,
                                format!("Font ID {} is defined more than once.", entry.id),
                            ));
                        }
                    }
                }
                tag::COLOR_TABLE => {
                    let colors = text::parse_color_table(self.data(raw), Self::data_offset(raw))?;
                    if self.colors.replace(colors.len()).is_some() {
                        return Err(CdxError::at(
                            "duplicate_color_table",
                            raw.offset,
                            "The document has more than one color table.",
                        ));
                    }
                }
                _ => (),
            }
        }
        Ok(())
    }

    fn object(&mut self, raw: &RawObject, opaque_parent: bool) -> Result<Object, CdxError> {
        let kind = if opaque_parent {
            ObjectKind::UnknownBinary(raw.tag)
        } else {
            known_kind(raw.tag).unwrap_or(ObjectKind::UnknownBinary(raw.tag))
        };
        let opaque = is_opaque(&kind);
        let id = (raw.id != 0).then_some(raw.id);
        if let Some(id) = id
            && self.ids.insert(id, raw.offset).is_some()
        {
            return Err(CdxError::at(
                "duplicate_id",
                raw.offset,
                format!(
                    "Object ID {id} is used more than once; IDs must be unique across the whole document."
                ),
            ));
        }
        let mut properties = Vec::new();
        let mut children = Vec::new();
        let mut seen = HashSet::new();
        for item in &raw.items {
            match item {
                RawItem::Property(prop) => {
                    let repeatable = matches!(prop.tag, tag::REPRESENTS | tag::FONT_TABLE);
                    if !repeatable && !seen.insert(prop.tag) {
                        return Err(bad_value(
                            "duplicate_property",
                            prop,
                            "the property appears more than once in one object",
                        ));
                    }
                    self.property(&kind, raw, prop, &mut properties, &mut children)?;
                }
                RawItem::Object(child_raw) => {
                    let child = self.object(child_raw, opaque)?;
                    if !kind.may_contain(&child.kind) {
                        return Err(CdxError::at(
                            "invalid_containment",
                            child_raw.offset,
                            format!(
                                "Object 0x{:04x} cannot be inside object 0x{:04x}.",
                                child_raw.tag, raw.tag
                            ),
                        ));
                    }
                    children.push(Content::Object(child));
                }
            }
        }
        self.check_required(&kind, raw, &properties)?;
        Ok(Object {
            kind,
            id,
            properties,
            children,
        })
    }

    fn check_required(
        &self,
        kind: &ObjectKind,
        raw: &RawObject,
        properties: &[Property],
    ) -> Result<(), CdxError> {
        let has = |wanted: PropertyKind| properties.iter().any(|item| item.kind == wanted);
        let reference = |wanted: PropertyKind| {
            properties
                .iter()
                .find_map(|item| match (&item.kind, &item.value) {
                    (found, PropertyValue::ObjectReference(id)) if *found == wanted => Some(*id),
                    _ => None,
                })
        };
        let missing = |what: &str| {
            CdxError::at(
                "missing_property",
                raw.offset,
                format!("Object 0x{:04x} needs {what}.", raw.tag),
            )
        };
        match kind {
            ObjectKind::Bond => {
                let begin = reference(PropertyKind::BondBegin)
                    .ok_or_else(|| missing("a begin node (0x0604)"))?;
                let end = reference(PropertyKind::BondEnd)
                    .ok_or_else(|| missing("an end node (0x0605)"))?;
                if begin == end {
                    return Err(CdxError::at(
                        "invalid_reference",
                        raw.offset,
                        "A bond connects a node to itself.",
                    ));
                }
            }
            ObjectKind::Graphic if !has(PropertyKind::BoundingBox) => {
                return Err(missing("a bounding box (0x0204)"));
            }
            _ => (),
        }
        Ok(())
    }

    fn property(
        &mut self,
        kind: &ObjectKind,
        owner: &RawObject,
        raw: &RawProperty,
        properties: &mut Vec<Property>,
        children: &mut Vec<Content>,
    ) -> Result<(), CdxError> {
        use ObjectKind as O;
        let data = self.data(raw);
        let opaque = |properties: &mut Vec<Property>| {
            properties.push(property(
                PropertyKind::UnknownBinary(raw.tag),
                PropertyValue::Bytes(data.to_vec()),
            ));
        };
        let known = match (kind, raw.tag) {
            (
                O::Document | O::Page | O::Group | O::Fragment | O::Text | O::Graphic,
                tag::BOUNDING_BOX,
            ) => {
                let [top, left, bottom, right] = coordinates::<4>(data, raw)?;
                Some(property(
                    PropertyKind::BoundingBox,
                    PropertyValue::Rectangle {
                        left,
                        top,
                        right,
                        bottom,
                    },
                ))
            }
            (O::Node | O::Text, tag::POSITION) => {
                let [y, x] = coordinates::<2>(data, raw)?;
                Some(property(
                    PropertyKind::Position,
                    PropertyValue::Point { x, y },
                ))
            }
            (O::Node, tag::NODE_TYPE) => Some(enumerated(
                PropertyKind::NodeType,
                integer(data, raw, true)?,
                0..=13,
                raw,
            )?),
            (O::Node, tag::ELEMENT) => Some(enumerated(
                PropertyKind::AtomicNumber,
                integer(data, raw, true)?,
                1..=118,
                raw,
            )?),
            (O::Node, tag::ISOTOPE) => Some(enumerated(
                PropertyKind::Isotope,
                integer(data, raw, true)?,
                0..=i64::from(i16::MAX),
                raw,
            )?),
            (O::Node, tag::CHARGE) => Some(property(
                PropertyKind::Charge,
                PropertyValue::Integer(charge(data, raw)?),
            )),
            (O::Node, tag::RADICAL) => Some(enumerated(
                PropertyKind::Radical,
                integer(data, raw, false)?,
                0..=3,
                raw,
            )?),
            (O::Bond, tag::BOND_ORDER) => Some(enumerated(
                PropertyKind::BondOrder,
                integer(data, raw, false)?,
                1..=i64::from(u16::MAX),
                raw,
            )?),
            (O::Bond, tag::BOND_BEGIN) => {
                Some(self.reference(PropertyKind::BondBegin, data, raw)?)
            }
            (O::Bond, tag::BOND_END) => Some(self.reference(PropertyKind::BondEnd, data, raw)?),
            (O::Graphic, tag::GRAPHIC_TYPE) => Some(enumerated(
                PropertyKind::GraphicType,
                integer(data, raw, true)?,
                0..=7,
                raw,
            )?),
            _ => None,
        };
        if let Some(known) = known {
            properties.push(known);
            return Ok(());
        }
        match (kind, raw.tag) {
            (O::Document, tag::FONT_TABLE) => {
                let table = text::parse_font_table(data, Self::data_offset(raw))?;
                self.reserve(1 + table.entries.len(), raw.offset)?;
                let fonts = table
                    .entries
                    .iter()
                    .map(|entry| {
                        let charset = text::charset_name(entry.charset).unwrap_or_default();
                        Content::Object(plain(
                            ObjectKind::Font,
                            vec![
                                property(
                                    PropertyKind::FontIndex,
                                    PropertyValue::Integer(i64::from(entry.id)),
                                ),
                                property(
                                    PropertyKind::FontCharset,
                                    PropertyValue::Text(charset.to_owned()),
                                ),
                                property(
                                    PropertyKind::FontName,
                                    PropertyValue::Text(entry.name.clone()),
                                ),
                            ],
                            Vec::new(),
                        ))
                    })
                    .collect();
                let object = plain(ObjectKind::FontTable, Vec::new(), fonts);
                self.font_tables.push(FontTablePlatform {
                    table: object.clone(),
                    platform: table.platform,
                });
                children.push(Content::Object(object));
            }
            (O::Document, tag::COLOR_TABLE) => {
                let colors = text::parse_color_table(data, Self::data_offset(raw))?;
                self.reserve(1 + colors.len(), raw.offset)?;
                let entries = colors
                    .iter()
                    .map(|[red, green, blue]| {
                        let level = |value: &u16| PropertyValue::Float(f64::from(*value) / 65535.0);
                        Content::Object(plain(
                            ObjectKind::Color,
                            vec![
                                property(PropertyKind::Red, level(red)),
                                property(PropertyKind::Green, level(green)),
                                property(PropertyKind::Blue, level(blue)),
                            ],
                            Vec::new(),
                        ))
                    })
                    .collect();
                children.push(Content::Object(plain(
                    ObjectKind::ColorTable,
                    Vec::new(),
                    entries,
                )));
            }
            (O::Text, tag::TEXT) => self.text(owner, raw, children)?,
            (O::Graphic, tag::REPRESENTS) => {
                if let Some(object) = self.represents(data, raw)? {
                    self.reserve(1, raw.offset)?;
                    children.push(Content::Object(object));
                } else {
                    if properties
                        .iter()
                        .any(|item| item.kind == PropertyKind::UnknownBinary(raw.tag))
                    {
                        return Err(bad_value(
                            "duplicate_property",
                            raw,
                            "a second Represents property with an unrecognized attribute cannot be kept",
                        ));
                    }
                    opaque(properties);
                }
            }
            _ => opaque(properties),
        }
        Ok(())
    }

    fn reference(
        &mut self,
        kind: PropertyKind,
        data: &[u8],
        raw: &RawProperty,
    ) -> Result<Property, CdxError> {
        let id = object_id(data, raw)?;
        self.references.push((id, raw.offset));
        Ok(property(kind, PropertyValue::ObjectReference(id)))
    }

    /// A Represents property is 6 bytes: the target ID, then the tag of the represented
    /// property. A tag without a verified CDXML name stays an opaque property.
    fn represents(&mut self, data: &[u8], raw: &RawProperty) -> Result<Option<Object>, CdxError> {
        if data.len() != 6 {
            return Err(bad_value(
                "invalid_property",
                raw,
                &format!("a Represents value has 6 bytes, found {}", data.len()),
            ));
        }
        let id = object_id(&data[..4], raw)?;
        let attribute = u16::from_le_bytes([data[4], data[5]]);
        let Some((name, _)) = REPRESENTED_ATTRIBUTES
            .iter()
            .find(|(_, code)| *code == attribute)
        else {
            return Ok(None);
        };
        self.references.push((id, raw.offset));
        Ok(Some(plain(
            ObjectKind::Represents,
            vec![
                property(
                    PropertyKind::RepresentObject,
                    PropertyValue::ObjectReference(id),
                ),
                property(
                    PropertyKind::RepresentAttribute,
                    PropertyValue::Text((*name).to_owned()),
                ),
            ],
            Vec::new(),
        )))
    }

    fn text(
        &mut self,
        owner: &RawObject,
        raw: &RawProperty,
        children: &mut Vec<Content>,
    ) -> Result<(), CdxError> {
        let decoded = text::parse_string(self.data(raw), Self::data_offset(raw), &self.fonts)?;
        let runs = match &decoded {
            DecodedString::Unstyled(_) => 1,
            DecodedString::Styled(runs) => runs.len(),
        };
        if owner.depth + 1 > MAX_DEPTH {
            return Err(CdxError::at(
                "resource_limit",
                raw.offset,
                format!("Style runs would nest deeper than {MAX_DEPTH} levels."),
            ));
        }
        self.reserve(runs, raw.offset)?;
        let content = |text: String| {
            if text.is_empty() {
                Vec::new()
            } else {
                vec![Content::Text(text)]
            }
        };
        match decoded {
            DecodedString::Unstyled(text) => children.push(Content::Object(plain(
                ObjectKind::StyleRun,
                Vec::new(),
                content(text),
            ))),
            DecodedString::Styled(runs) => {
                for run in runs {
                    let available = self.colors.map_or(4, |count| count + 2);
                    if usize::from(run.color) >= available {
                        return Err(CdxError::at(
                            "unknown_color",
                            run.offset + 8,
                            format!("Style run color index {} is not available.", run.color),
                        ));
                    }
                    // Attribute order of the public CDXML example: font, size, face, color.
                    children.push(Content::Object(plain(
                        ObjectKind::StyleRun,
                        vec![
                            property(
                                PropertyKind::FontIndex,
                                PropertyValue::Integer(i64::from(run.font)),
                            ),
                            property(
                                PropertyKind::FontSize,
                                PropertyValue::Float(f64::from(run.size) / 20.0),
                            ),
                            property(
                                PropertyKind::FontFace,
                                PropertyValue::Integer(i64::from(run.face)),
                            ),
                            property(
                                PropertyKind::ColorIndex,
                                PropertyValue::Integer(i64::from(run.color)),
                            ),
                        ],
                        content(run.text),
                    )));
                }
            }
        }
        Ok(())
    }

    fn check_references(&self) -> Result<(), CdxError> {
        for (target, offset) in &self.references {
            if !self.ids.contains_key(target) {
                return Err(CdxError::at(
                    "dangling_reference",
                    *offset,
                    format!("A reference names object {target}, which does not exist."),
                ));
            }
        }
        Ok(())
    }

    fn model_error(&self, error: &chemifree_domain::interchange::ValidationError) -> CdxError {
        use chemifree_domain::interchange::ValidationError as V;
        let (code, message) = describe_validation(error);
        let target = match error {
            V::BondEndpointNotNode(id) | V::CrossFragmentBond(id) | V::DanglingReference(id) => {
                Some(*id)
            }
            _ => None,
        };
        let offset = target.and_then(|id| {
            self.references
                .iter()
                .find(|(target, _)| *target == id)
                .map(|(_, offset)| *offset)
        });
        CdxError {
            code,
            message,
            offset,
        }
    }
}

fn width_error(raw: &RawProperty, found: usize, expected: &str) -> CdxError {
    bad_value(
        "invalid_property",
        raw,
        &format!("the value has {found} bytes; expected {expected}"),
    )
}

/// Integers are read by the width the file gives them, as the public documentation asks.
fn integer(data: &[u8], raw: &RawProperty, signed: bool) -> Result<i64, CdxError> {
    Ok(match (data.len(), signed) {
        (1, true) => i64::from(i8::from_le_bytes([data[0]])),
        (1, false) => i64::from(data[0]),
        (2, true) => i64::from(i16::from_le_bytes([data[0], data[1]])),
        (2, false) => i64::from(u16::from_le_bytes([data[0], data[1]])),
        (4, true) => i64::from(i32::from_le_bytes([data[0], data[1], data[2], data[3]])),
        (4, false) => i64::from(u32::from_le_bytes([data[0], data[1], data[2], data[3]])),
        (found, _) => return Err(width_error(raw, found, "1, 2 or 4")),
    })
}

fn enumerated(
    kind: PropertyKind,
    value: i64,
    range: std::ops::RangeInclusive<i64>,
    raw: &RawProperty,
) -> Result<Property, CdxError> {
    if !range.contains(&value) {
        return Err(bad_value(
            "out_of_range",
            raw,
            &format!(
                "value {value} is outside the documented range {} to {}",
                range.start(),
                range.end()
            ),
        ));
    }
    Ok(property(kind, PropertyValue::Integer(value)))
}

/// A charge is an INT8. A 4-byte value was written by one ChemDraw release as a fixed-point
/// number; the public page says the charge is then `saved >> 24`. A value with a fractional
/// part has no integer charge, so it is refused instead of truncated.
fn charge(data: &[u8], raw: &RawProperty) -> Result<i64, CdxError> {
    match data.len() {
        1 => Ok(i64::from(i8::from_le_bytes([data[0]]))),
        4 => {
            let saved = i32::from_le_bytes([data[0], data[1], data[2], data[3]]);
            if saved & 0x00ff_ffff != 0 {
                return Err(bad_value(
                    "out_of_range",
                    raw,
                    "a 4-byte charge has a non-zero fractional part, so it is not an integer charge",
                ));
            }
            Ok(i64::from(saved >> 24))
        }
        found => Err(width_error(raw, found, "1 (or the legacy 4)")),
    }
}

fn object_id(data: &[u8], raw: &RawProperty) -> Result<u32, CdxError> {
    let [a, b, c, d] = *data else {
        return Err(width_error(raw, data.len(), "4"));
    };
    match u32::from_le_bytes([a, b, c, d]) {
        0 => Err(bad_value(
            "invalid_reference",
            raw,
            "a reference cannot be object 0",
        )),
        id => Ok(id),
    }
}

/// `N` coordinates in 1/65536 point units, in file order.
fn coordinates<const N: usize>(data: &[u8], raw: &RawProperty) -> Result<[f64; N], CdxError> {
    if data.len() != 4 * N {
        return Err(width_error(raw, data.len(), &(4 * N).to_string()));
    }
    let mut result = [0.0; N];
    for (index, value) in result.iter_mut().enumerate() {
        let at = 4 * index;
        let raw_value = i32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]]);
        *value = f64::from(raw_value) / 65536.0;
    }
    Ok(result)
}
