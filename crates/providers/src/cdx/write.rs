//! Serializes the common interchange document as CDX.
//!
//! Everything written is re-readable by [`super::read`]. Content with no CDX encoding, and
//! numbers that are not exact multiples of the binary resolution, are reported as losses
//! rather than rewritten silently; values that cannot be stored at all are refused.

use std::collections::HashSet;

use chemifree_application::{Loss, LossConsent, LossDisposition, SemanticImpact};
use chemifree_domain::interchange::{
    Content, Document, Object, ObjectKind, Property, PropertyKind, PropertyValue,
};

use super::read::CdxDocument;
use super::text::{self, PLATFORM_WINDOWS};
use super::{CdxError, HEADER_LEN, MAGIC, MAX_BYTES, REPRESENTED_ATTRIBUTES, RESERVED_PREFIX, tag};

/// Bytes and the losses incurred writing them.
#[derive(Debug)]
pub struct CdxWrite {
    pub bytes: Vec<u8>,
    pub losses: Vec<Loss>,
}

/// Resolution of coordinates: 1/65536 point, so a rounded value is off by at most this.
const COORDINATE_BOUND: f64 = 0.5 / 65536.0;
/// Resolution of color components: 1/65535, so a rounded value is off by at most this.
const COLOR_BOUND: f64 = 0.5 / 65535.0;
/// Resolution of font sizes: 1/20 point.
const SIZE_BOUND: f64 = 0.5 / 20.0;

/// Writes `document`. `original` is the re-parsed source when one exists; it tells the
/// writer which font-table platform it may treat as known.
///
/// # Errors
/// Refuses opaque content (see the shared opaque policy), values outside the binary format's
/// range, text it cannot encode exactly, and trees the format cannot express.
pub fn write(document: &Document, original: Option<&CdxDocument>) -> Result<CdxWrite, CdxError> {
    let mut writer = Writer {
        out: Vec::new(),
        original,
        stack: Vec::new(),
        precision: [
            Precision::default(),
            Precision::default(),
            Precision::default(),
        ],
        dropped_namespace: false,
        xml_content: Counted::default(),
        represents: Counted::default(),
        style_defaults: Counted::default(),
        child_order: Counted::default(),
        platform_unknown: Counted::default(),
        dropped_ids: Counted::default(),
    };
    writer.out.extend_from_slice(MAGIC);
    writer.out.extend_from_slice(&RESERVED_PREFIX);
    writer.out.extend_from_slice(&[0; HEADER_LEN - 12]);
    writer.check_dropped_ids(document.root())?;
    writer.object(document.root())?;
    writer.out.extend_from_slice(&[0, 0]);
    if writer.out.len() > MAX_BYTES {
        return Err(too_big());
    }
    let losses = writer.losses();
    Ok(CdxWrite {
        bytes: writer.out,
        losses,
    })
}

fn too_big() -> CdxError {
    CdxError::at(
        "resource_limit",
        MAX_BYTES as u64,
        format!("CDX output exceeds {MAX_BYTES} bytes."),
    )
}

#[derive(Default)]
struct Counted {
    count: usize,
    first: Option<String>,
}

impl Counted {
    fn note(&mut self, path: impl FnOnce() -> String) {
        if self.first.is_none() {
            self.first = Some(path());
        }
        self.count += 1;
    }
}

#[derive(Default)]
struct Precision {
    seen: Counted,
    worst: f64,
}

#[derive(Clone, Copy)]
enum Quantity {
    Coordinate = 0,
    Color = 1,
    FontSize = 2,
}

struct Writer<'a> {
    out: Vec<u8>,
    original: Option<&'a CdxDocument>,
    stack: Vec<String>,
    precision: [Precision; 3],
    dropped_namespace: bool,
    xml_content: Counted,
    represents: Counted,
    style_defaults: Counted,
    child_order: Counted,
    platform_unknown: Counted,
    dropped_ids: Counted,
}

fn kind_name(kind: &ObjectKind) -> &'static str {
    match kind {
        ObjectKind::Document => "document",
        ObjectKind::Page => "page",
        ObjectKind::Group => "group",
        ObjectKind::Fragment => "fragment",
        ObjectKind::Node => "node",
        ObjectKind::Bond => "bond",
        ObjectKind::Graphic => "graphic",
        ObjectKind::Text => "text",
        ObjectKind::StyleRun => "style run",
        ObjectKind::FontTable => "font table",
        ObjectKind::Font => "font",
        ObjectKind::ColorTable => "color table",
        ObjectKind::Color => "color",
        ObjectKind::Represents => "represents",
        ObjectKind::UnknownXml(_) | ObjectKind::UnknownBinary(_) => "opaque object",
    }
}

fn object_tag(kind: &ObjectKind) -> Option<u16> {
    Some(match kind {
        ObjectKind::Document => tag::DOCUMENT,
        ObjectKind::Page => tag::PAGE,
        ObjectKind::Group => tag::GROUP,
        ObjectKind::Fragment => tag::FRAGMENT,
        ObjectKind::Node => tag::NODE,
        ObjectKind::Bond => tag::BOND,
        ObjectKind::Graphic => tag::GRAPHIC,
        ObjectKind::Text => tag::TEXT_OBJECT,
        _ => return None,
    })
}

fn is_xml_space(text: &str) -> bool {
    text.chars().all(|c| matches!(c, ' ' | '\t' | '\n' | '\r'))
}

impl Writer<'_> {
    fn path(&self) -> String {
        self.stack.join("/")
    }

    fn refuse(&self, code: &'static str, message: impl Into<String>) -> CdxError {
        CdxError::general(code, format!("{} (at {}).", message.into(), self.path()))
    }

    /// Style runs, font and color tables and their entries are properties or parts of
    /// properties in CDX and cannot carry an object ID. Dropping an ID is reported; dropping
    /// one that a reference names would leave the file pointing at nothing, so it is refused.
    fn check_dropped_ids(&mut self, root: &Object) -> Result<(), CdxError> {
        fn walk<'a>(
            object: &'a Object,
            stack: &mut Vec<&'a Object>,
            dropped: &mut Vec<u32>,
            first_path: &mut Option<String>,
            references: &mut HashSet<u32>,
        ) {
            stack.push(object);
            if let Some(id) = object.id
                && matches!(
                    object.kind,
                    ObjectKind::StyleRun
                        | ObjectKind::FontTable
                        | ObjectKind::Font
                        | ObjectKind::ColorTable
                        | ObjectKind::Color
                )
            {
                dropped.push(id);
                if first_path.is_none() {
                    *first_path = Some(
                        stack
                            .iter()
                            .map(|item| kind_name(&item.kind))
                            .collect::<Vec<_>>()
                            .join("/"),
                    );
                }
            }
            for item in &object.properties {
                if let PropertyValue::ObjectReference(id) = item.value {
                    references.insert(id);
                }
            }
            for child in &object.children {
                if let Content::Object(child) = child {
                    walk(child, stack, dropped, first_path, references);
                }
            }
            stack.pop();
        }
        let mut dropped = Vec::new();
        let mut first_path = None;
        let mut references = HashSet::new();
        walk(
            root,
            &mut Vec::new(),
            &mut dropped,
            &mut first_path,
            &mut references,
        );
        if let Some(id) = dropped.iter().find(|id| references.contains(id)) {
            return Err(CdxError::general(
                "unsupported_object_id",
                format!("Object {id} cannot carry an ID in CDX, but a reference names it."),
            ));
        }
        for _ in &dropped {
            self.dropped_ids
                .note(|| first_path.clone().unwrap_or_default());
        }
        Ok(())
    }

    /// Comments, instructions and visible text directly inside an object that CDX stores as a
    /// property have no CDX encoding; whitespace between children is insignificant.
    fn note_markup(&mut self, object: &Object) {
        for child in &object.children {
            match child {
                Content::Object(_) => (),
                Content::Text(text) if is_xml_space(text) => (),
                Content::Text(_) | Content::Comment(_) | Content::ProcessingInstruction { .. } => {
                    self.xml_content.note(|| self.stack.join("/"));
                }
            }
        }
    }

    fn property_bytes(&mut self, property_tag: u16, data: &[u8]) -> Result<(), CdxError> {
        if self.out.len() > MAX_BYTES {
            return Err(too_big());
        }
        self.out.extend_from_slice(&property_tag.to_le_bytes());
        match u16::try_from(data.len()) {
            Ok(length) if length != 0xffff => self.out.extend_from_slice(&length.to_le_bytes()),
            _ => {
                let length = u32::try_from(data.len()).map_err(|_| too_big())?;
                self.out.extend_from_slice(&0xffff_u16.to_le_bytes());
                self.out.extend_from_slice(&length.to_le_bytes());
            }
        }
        self.out.extend_from_slice(data);
        Ok(())
    }

    fn object(&mut self, object: &Object) -> Result<(), CdxError> {
        let Some(object_tag) = object_tag(&object.kind) else {
            return Err(CdxError::general(
                "unsafe_unknown_rewrite",
                format!(
                    "A {} cannot be written as a CDX object.",
                    kind_name(&object.kind)
                ),
            ));
        };
        let label = match object.id {
            Some(id) => format!("{}#{id}", kind_name(&object.kind)),
            None => kind_name(&object.kind).to_owned(),
        };
        self.stack.push(label);
        self.out.extend_from_slice(&object_tag.to_le_bytes());
        // The specification's ID zero means "no ID assigned".
        self.out
            .extend_from_slice(&object.id.unwrap_or(0).to_le_bytes());
        for item in &object.properties {
            self.simple_property(&object.kind, item)?;
        }
        match object.kind {
            ObjectKind::Document => self.tables(object)?,
            ObjectKind::Text => self.text(object)?,
            ObjectKind::Graphic => self.represents(object)?,
            _ => (),
        }
        for child in &object.children {
            match child {
                Content::Object(child) if object_tag_is_object(&child.kind) => {
                    self.object(child)?;
                }
                Content::Object(_) => (),
                Content::Text(text) if is_xml_space(text) => (),
                Content::Text(_) | Content::Comment(_) | Content::ProcessingInstruction { .. } => {
                    self.xml_content.note(|| self.stack.join("/"));
                }
            }
        }
        self.out.extend_from_slice(&[0, 0]);
        self.stack.pop();
        Ok(())
    }

    fn simple_property(&mut self, owner: &ObjectKind, item: &Property) -> Result<(), CdxError> {
        use PropertyKind as K;
        use PropertyValue as V;
        match (&item.kind, &item.value) {
            (K::XmlNamespace, _) => self.dropped_namespace = true,
            (
                K::BoundingBox,
                V::Rectangle {
                    left,
                    top,
                    right,
                    bottom,
                },
            ) => {
                let mut data = Vec::with_capacity(16);
                for value in [top, left, bottom, right] {
                    data.extend_from_slice(&self.coordinate(*value)?.to_le_bytes());
                }
                self.property_bytes(tag::BOUNDING_BOX, &data)?;
            }
            (K::Position, V::Point { x, y }) => {
                let mut data = Vec::with_capacity(8);
                for value in [y, x] {
                    data.extend_from_slice(&self.coordinate(*value)?.to_le_bytes());
                }
                self.property_bytes(tag::POSITION, &data)?;
            }
            (K::NodeType, V::Integer(value)) => {
                self.int16(tag::NODE_TYPE, *value, 0..=13, "node type")?;
            }
            (K::AtomicNumber, V::Integer(value)) => {
                self.int16(tag::ELEMENT, *value, 1..=118, "element")?;
            }
            (K::Isotope, V::Integer(value)) => {
                self.int16(tag::ISOTOPE, *value, 0..=i64::from(i16::MAX), "isotope")?;
            }
            (K::Charge, V::Integer(value)) => {
                let charge = i8::try_from(*value).map_err(|_| {
                    self.refuse(
                        "out_of_range",
                        format!("Charge {value} does not fit an INT8"),
                    )
                })?;
                self.property_bytes(tag::CHARGE, &charge.to_le_bytes())?;
            }
            (K::Radical, V::Integer(value)) => {
                let radical = u8::try_from(*value)
                    .ok()
                    .filter(|value| *value <= 3)
                    .ok_or_else(|| {
                        self.refuse(
                            "unsupported_property",
                            format!(
                                "Radical value {value} is not one of the documented values 0 to 3"
                            ),
                        )
                    })?;
                self.property_bytes(tag::RADICAL, &[radical])?;
            }
            (K::BondOrder, V::Integer(value)) => {
                let order = u16::try_from(*value)
                    .ok()
                    .filter(|value| *value != 0)
                    .ok_or_else(|| {
                        self.refuse(
                            "out_of_range",
                            format!("Bond order {value} is outside 1 to 65535"),
                        )
                    })?;
                self.property_bytes(tag::BOND_ORDER, &order.to_le_bytes())?;
            }
            (K::BondBegin, V::ObjectReference(id)) => {
                self.property_bytes(tag::BOND_BEGIN, &id.to_le_bytes())?;
            }
            (K::BondEnd, V::ObjectReference(id)) => {
                self.property_bytes(tag::BOND_END, &id.to_le_bytes())?;
            }
            (K::GraphicType, V::Integer(value)) => {
                self.int16(tag::GRAPHIC_TYPE, *value, 0..=7, "graphic type")?;
            }
            _ => {
                return Err(self.refuse(
                    "unsafe_unknown_rewrite",
                    format!("A property of a {} has no CDX encoding", kind_name(owner)),
                ));
            }
        }
        Ok(())
    }

    fn int16(
        &mut self,
        property_tag: u16,
        value: i64,
        range: std::ops::RangeInclusive<i64>,
        what: &str,
    ) -> Result<(), CdxError> {
        if !range.contains(&value) {
            return Err(self.refuse(
                "out_of_range",
                format!(
                    "The {what} {value} is outside {} to {}",
                    range.start(),
                    range.end()
                ),
            ));
        }
        let value = i16::try_from(value).map_err(|_| {
            self.refuse(
                "out_of_range",
                format!("The {what} {value} does not fit 16 bits"),
            )
        })?;
        self.property_bytes(property_tag, &value.to_le_bytes())
    }

    /// Rounds to the nearest representable value. `unit` is the number of steps per 1.0.
    fn quantize(
        &mut self,
        value: f64,
        unit: f64,
        range: (f64, f64),
        quantity: Quantity,
        what: &str,
    ) -> Result<i64, CdxError> {
        let scaled = (value * unit).round();
        if !scaled.is_finite() || scaled < range.0 || scaled > range.1 {
            return Err(self.refuse(
                "out_of_range",
                format!("The {what} {value} is outside the range the format can store"),
            ));
        }
        let steps = scaled as i64;
        let error = (steps as f64 / unit - value).abs();
        if error > 0.0 {
            let slot = &mut self.precision[quantity as usize];
            slot.worst = slot.worst.max(error);
            self.precision[quantity as usize]
                .seen
                .note(|| self.stack.join("/"));
        }
        Ok(steps)
    }

    fn coordinate(&mut self, value: f64) -> Result<i32, CdxError> {
        let steps = self.quantize(
            value,
            65536.0,
            (f64::from(i32::MIN), f64::from(i32::MAX)),
            Quantity::Coordinate,
            "coordinate",
        )?;
        i32::try_from(steps)
            .map_err(|_| self.refuse("out_of_range", "A coordinate does not fit 32 bits"))
    }

    fn color_component(&mut self, value: f64) -> Result<u16, CdxError> {
        let steps = self.quantize(
            value,
            65535.0,
            (0.0, 65535.0),
            Quantity::Color,
            "color component",
        )?;
        u16::try_from(steps)
            .map_err(|_| self.refuse("out_of_range", "A color component does not fit 16 bits"))
    }

    fn font_size(&mut self, value: f64) -> Result<u16, CdxError> {
        let steps = self.quantize(value, 20.0, (0.0, 65535.0), Quantity::FontSize, "font size")?;
        u16::try_from(steps)
            .map_err(|_| self.refuse("out_of_range", "A font size does not fit 16 bits"))
    }

    fn text_property<'a>(object: &'a Object, wanted: &PropertyKind) -> Option<&'a PropertyValue> {
        object
            .properties
            .iter()
            .find(|item| &item.kind == wanted)
            .map(|item| &item.value)
    }

    /// Font and color tables become properties of the document, ahead of its objects.
    fn tables(&mut self, document: &Object) -> Result<(), CdxError> {
        let mut seen_object = false;
        let mut table_index = 0;
        for child in &document.children {
            let Content::Object(child) = child else {
                continue;
            };
            match child.kind {
                ObjectKind::FontTable | ObjectKind::ColorTable if seen_object => {
                    self.child_order.note(|| "document".into());
                }
                ObjectKind::FontTable | ObjectKind::ColorTable => (),
                _ => seen_object = true,
            }
        }
        for child in &document.children {
            let Content::Object(child) = child else {
                continue;
            };
            match child.kind {
                ObjectKind::FontTable => {
                    self.stack.push(format!("font table[{table_index}]"));
                    let data = self.font_table(child, table_index)?;
                    self.property_bytes(tag::FONT_TABLE, &data)?;
                    self.stack.pop();
                    table_index += 1;
                }
                ObjectKind::ColorTable => {
                    self.stack.push("color table".into());
                    let data = self.color_table(child)?;
                    self.property_bytes(tag::COLOR_TABLE, &data)?;
                    self.stack.pop();
                }
                _ => (),
            }
        }
        Ok(())
    }

    fn font_table(&mut self, table: &Object, index: usize) -> Result<Vec<u8>, CdxError> {
        self.note_markup(table);
        let known_platform = self.original.is_some_and(|original| {
            original
                .font_table(index)
                .is_some_and(|(source, platform)| platform == PLATFORM_WINDOWS && source == table)
        });
        if !known_platform {
            self.platform_unknown.note(|| self.stack.join("/"));
        }
        let fonts: Vec<&Object> = table
            .children
            .iter()
            .filter_map(|child| match child {
                Content::Object(font) if font.kind == ObjectKind::Font => Some(font),
                _ => None,
            })
            .collect();
        let count = u16::try_from(fonts.len())
            .map_err(|_| self.refuse("out_of_range", "A font table holds at most 65535 fonts"))?;
        let mut data = PLATFORM_WINDOWS.to_le_bytes().to_vec();
        data.extend_from_slice(&count.to_le_bytes());
        for font in fonts {
            self.note_markup(font);
            let Some(PropertyValue::Integer(id)) =
                Self::text_property(font, &PropertyKind::FontIndex)
            else {
                return Err(self.refuse("missing_property", "A font has no ID"));
            };
            let Some(PropertyValue::Text(name)) =
                Self::text_property(font, &PropertyKind::FontName)
            else {
                return Err(self.refuse("missing_property", "A font has no name"));
            };
            let charset = match Self::text_property(font, &PropertyKind::FontCharset) {
                Some(PropertyValue::Text(name)) => text::charset_code(name).ok_or_else(|| {
                    self.refuse("unsupported_charset", format!("Font character set \"{name}\" has no verified CDX code; only \"{}\" is supported", text::CHARSET_NAME))
                })?,
                _ => {
                    return Err(self.refuse(
                        "missing_property",
                        "A font has no character set, and one is not assumed",
                    ));
                }
            };
            let mut encoded = Vec::new();
            text::encode_text(name, &mut encoded).map_err(|reason| {
                self.refuse("unsupported_text", format!("Font name: {reason}"))
            })?;
            let length = u16::try_from(encoded.len()).map_err(|_| {
                self.refuse("out_of_range", "A font name is longer than 65535 bytes")
            })?;
            let id = u16::try_from(*id)
                .map_err(|_| self.refuse("out_of_range", "A font ID does not fit 16 bits"))?;
            data.extend_from_slice(&id.to_le_bytes());
            data.extend_from_slice(&charset.to_le_bytes());
            data.extend_from_slice(&length.to_le_bytes());
            data.extend_from_slice(&encoded);
        }
        Ok(data)
    }

    fn color_table(&mut self, table: &Object) -> Result<Vec<u8>, CdxError> {
        self.note_markup(table);
        let colors: Vec<&Object> = table
            .children
            .iter()
            .filter_map(|child| match child {
                Content::Object(color) if color.kind == ObjectKind::Color => Some(color),
                _ => None,
            })
            .collect();
        let count = u16::try_from(colors.len())
            .map_err(|_| self.refuse("out_of_range", "A color table holds at most 65535 colors"))?;
        let mut data = count.to_le_bytes().to_vec();
        for color in colors {
            self.note_markup(color);
            for kind in [PropertyKind::Red, PropertyKind::Green, PropertyKind::Blue] {
                let Some(PropertyValue::Float(level)) = Self::text_property(color, &kind) else {
                    return Err(self.refuse(
                        "missing_property",
                        "A color lacks a red, green or blue component",
                    ));
                };
                data.extend_from_slice(&self.color_component(*level)?.to_le_bytes());
            }
        }
        Ok(data)
    }

    fn represents(&mut self, graphic: &Object) -> Result<(), CdxError> {
        for child in &graphic.children {
            let Content::Object(child) = child else {
                continue;
            };
            if child.kind != ObjectKind::Represents {
                continue;
            }
            self.note_markup(child);
            if child.id.is_some() {
                return Err(self.refuse(
                    "unsupported_represents_id",
                    "A Represents object with its own ID cannot be written: CDX stores it as an unnumbered property",
                ));
            }
            let (Some(PropertyValue::ObjectReference(target)), Some(PropertyValue::Text(name))) = (
                Self::text_property(child, &PropertyKind::RepresentObject),
                Self::text_property(child, &PropertyKind::RepresentAttribute),
            ) else {
                return Err(self.refuse(
                    "missing_property",
                    "A Represents object needs an object and an attribute",
                ));
            };
            match REPRESENTED_ATTRIBUTES
                .iter()
                .find(|(known, _)| known == name)
            {
                Some((_, attribute)) => {
                    let mut data = target.to_le_bytes().to_vec();
                    data.extend_from_slice(&attribute.to_le_bytes());
                    self.property_bytes(tag::REPRESENTS, &data)?;
                }
                None => self
                    .represents
                    .note(|| format!("{}: attribute \"{name}\"", self.stack.join("/"))),
            }
        }
        Ok(())
    }

    fn text(&mut self, text_object: &Object) -> Result<(), CdxError> {
        let runs: Vec<&Object> = text_object
            .children
            .iter()
            .filter_map(|child| match child {
                Content::Object(run) if run.kind == ObjectKind::StyleRun => Some(run),
                _ => None,
            })
            .collect();
        // The model requires a style run in every text object; a string written without one
        // would read back with an invented empty run.
        if runs.is_empty() {
            return Err(self.refuse("missing_property", "A text object has no style run"));
        }
        let mut data = Vec::new();
        let unstyled = runs.len() == 1 && runs[0].properties.is_empty();
        let count = u16::try_from(if unstyled { 0 } else { runs.len() }).map_err(|_| {
            self.refuse(
                "out_of_range",
                "A text object holds at most 65535 style runs",
            )
        })?;
        data.extend_from_slice(&count.to_le_bytes());
        let mut body = Vec::new();
        let mut header = Vec::new();
        for (index, run) in runs.iter().enumerate() {
            let mut content = String::new();
            for part in &run.children {
                match part {
                    Content::Text(piece) => content.push_str(piece),
                    Content::Comment(_) | Content::ProcessingInstruction { .. } => {
                        self.xml_content
                            .note(|| format!("{}/style run[{index}]", self.stack.join("/")));
                    }
                    Content::Object(_) => (),
                }
            }
            if !unstyled {
                let start = u16::try_from(body.len()).map_err(|_| {
                    self.refuse("out_of_range", format!("Style run {index} starts after character 65535, which a run start cannot address"))
                })?;
                let font = self
                    .run_integer(run, &PropertyKind::FontIndex, "font")?
                    .ok_or_else(|| {
                        self.refuse(
                            "unsupported_style_run",
                            format!("Style run {index} has no font, and CDX requires one"),
                        )
                    })?;
                let size = match Self::text_property(run, &PropertyKind::FontSize) {
                    Some(PropertyValue::Float(size)) => self.font_size(*size)?,
                    _ => {
                        return Err(self.refuse(
                            "unsupported_style_run",
                            format!("Style run {index} has no size, and CDX requires one"),
                        ));
                    }
                };
                let color = self
                    .run_integer(run, &PropertyKind::ColorIndex, "color")?
                    .ok_or_else(|| {
                        self.refuse(
                            "unsupported_style_run",
                            format!("Style run {index} has no color, and CDX requires one"),
                        )
                    })?;
                let face = match self.run_integer(run, &PropertyKind::FontFace, "face")? {
                    Some(face) => face,
                    None => {
                        self.style_defaults.note(|| self.stack.join("/"));
                        0
                    }
                };
                header.extend_from_slice(&start.to_le_bytes());
                for value in [font, face, size, color] {
                    header.extend_from_slice(&value.to_le_bytes());
                }
            }
            text::encode_text(&content, &mut body)
                .map_err(|reason| self.refuse("unsupported_text", reason))?;
        }
        data.extend_from_slice(&header);
        data.extend_from_slice(&body);
        self.property_bytes(tag::TEXT, &data)
    }

    fn run_integer(
        &self,
        run: &Object,
        kind: &PropertyKind,
        what: &str,
    ) -> Result<Option<u16>, CdxError> {
        match Self::text_property(run, kind) {
            Some(PropertyValue::Integer(value)) => u16::try_from(*value).map(Some).map_err(|_| {
                self.refuse(
                    "out_of_range",
                    format!("The style {what} {value} does not fit 16 bits"),
                )
            }),
            _ => Ok(None),
        }
    }

    fn losses(&self) -> Vec<Loss> {
        let mut losses = Vec::new();
        let names = ["coordinate", "color component", "font size"];
        let units = [
            ("1/65536 point", COORDINATE_BOUND, SemanticImpact::Geometry),
            ("1/65535", COLOR_BOUND, SemanticImpact::Rendering),
            ("1/20 point", SIZE_BOUND, SemanticImpact::Rendering),
        ];
        for (index, slot) in self.precision.iter().enumerate() {
            if slot.seen.count > 0 {
                losses.push(Loss {
                    feature: "precision".into(),
                    source_path: slot.seen.first.clone().unwrap_or_default(),
                    disposition: LossDisposition::Normalized,
                    reason: format!(
                        "{} {} value(s) were rounded to the nearest multiple of {}; the largest change is {} and the bound is {}.",
                        slot.seen.count, names[index], units[index].0, slot.worst, units[index].1
                    ),
                    semantic_impact: units[index].2,
                    consent: LossConsent::Required,
                });
            }
        }
        if self.platform_unknown.count > 0 {
            losses.push(Loss {
                feature: "font_platform".into(),
                source_path: self.platform_unknown.first.clone().unwrap_or_default(),
                disposition: LossDisposition::Normalized,
                reason: format!(
                    "{} font table(s) were written for the Windows platform (1); their source platform is not known to be Windows.",
                    self.platform_unknown.count
                ),
                semantic_impact: SemanticImpact::Unknown,
                    consent: LossConsent::NotRequired,
            });
        }
        if self.dropped_ids.count > 0 {
            losses.push(Loss {
                feature: "object_ids".into(),
                source_path: self.dropped_ids.first.clone().unwrap_or_default(),
                disposition: LossDisposition::Dropped,
                reason: format!(
                    "{} object ID(s) on style runs, font tables, color tables and their entries have no CDX encoding and were not written; no reference names them.",
                    self.dropped_ids.count
                ),
                semantic_impact: SemanticImpact::Document,
                    consent: LossConsent::Required,
            });
        }
        if self.dropped_namespace {
            losses.push(Loss {
                feature: "xml_namespace".into(),
                source_path: "document".into(),
                disposition: LossDisposition::Dropped,
                reason: "The XML root namespace has no CDX encoding.".into(),
                semantic_impact: SemanticImpact::None,
                consent: LossConsent::NotRequired,
            });
        }
        if self.xml_content.count > 0 {
            losses.push(Loss {
                feature: "xml_content".into(),
                source_path: self.xml_content.first.clone().unwrap_or_default(),
                disposition: LossDisposition::Dropped,
                reason: format!(
                    "{} XML comment, instruction or stray text item(s) outside style runs have no CDX encoding; text on both sides of a dropped item inside a run was joined.",
                    self.xml_content.count
                ),
                semantic_impact: SemanticImpact::Unknown,
                    consent: LossConsent::Required,
            });
        }
        if self.represents.count > 0 {
            losses.push(Loss {
                feature: "represents".into(),
                source_path: self.represents.first.clone().unwrap_or_default(),
                disposition: LossDisposition::Dropped,
                reason: format!(
                    "{} Represents object(s) name an attribute with no verified CDX tag.",
                    self.represents.count
                ),
                semantic_impact: SemanticImpact::Chemistry,
                consent: LossConsent::Required,
            });
        }
        if self.style_defaults.count > 0 {
            losses.push(Loss {
                feature: "style_defaults".into(),
                source_path: self.style_defaults.first.clone().unwrap_or_default(),
                disposition: LossDisposition::Normalized,
                reason: format!(
                    "{} style run(s) had no face; the plain face (0) was written.",
                    self.style_defaults.count
                ),
                semantic_impact: SemanticImpact::Rendering,
                consent: LossConsent::NotRequired,
            });
        }
        if self.child_order.count > 0 {
            losses.push(Loss {
                feature: "child_order".into(),
                source_path: "document".into(),
                disposition: LossDisposition::Normalized,
                reason: "Font and color tables are properties in CDX and were written before the document's objects.".into(),
                semantic_impact: SemanticImpact::None,
                    consent: LossConsent::NotRequired,
            });
        }
        losses
    }
}

fn object_tag_is_object(kind: &ObjectKind) -> bool {
    !matches!(
        kind,
        ObjectKind::FontTable
            | ObjectKind::Font
            | ObjectKind::ColorTable
            | ObjectKind::Color
            | ObjectKind::Represents
            | ObjectKind::StyleRun
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extended_length_is_used_exactly_from_0xffff() {
        let mut writer = Writer {
            out: Vec::new(),
            original: None,
            stack: Vec::new(),
            precision: Default::default(),
            dropped_namespace: false,
            xml_content: Counted::default(),
            represents: Counted::default(),
            style_defaults: Counted::default(),
            child_order: Counted::default(),
            platform_unknown: Counted::default(),
            dropped_ids: Counted::default(),
        };
        writer.property_bytes(0x1234, &vec![7; 0xfffe]).unwrap();
        assert_eq!(&writer.out[2..4], &0xfffe_u16.to_le_bytes());
        let first = writer.out.len();
        writer.property_bytes(0x1234, &vec![7; 0xffff]).unwrap();
        assert_eq!(&writer.out[first + 2..first + 4], &[0xff, 0xff]);
        assert_eq!(&writer.out[first + 4..first + 8], &0xffff_u32.to_le_bytes());
    }
}
