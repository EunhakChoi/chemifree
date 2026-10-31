//! Independent CDXML reader and writer for the common interchange document.
//!
//! External entities are never resolved. Uninterpreted extensions are retained in an unchanged
//! source document; an edit that would require rewriting them is refused rather than guessed.

use std::fmt;
use std::fmt::Write;

use chemifree_domain::interchange::{
    Content, Document, Object, ObjectKind, Property, PropertyKind, PropertyValue, XmlName,
};
use roxmltree::{Node, NodeType, ParsingOptions};

const MAX_XML_BYTES: usize = 1024 * 1024;
const MAX_XML_NODES: u32 = 100_000;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CdxmlError {
    pub code: &'static str,
    pub message: String,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

impl fmt::Display for CdxmlError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}
impl std::error::Error for CdxmlError {}

/// An interchange document with its original XML for safe same-format preservation.
#[derive(Clone, Debug, PartialEq)]
pub struct CdxmlDocument {
    document: Document,
    original: Option<(Object, String)>,
    declaration: Option<String>,
    doctype: Option<String>,
    unsafe_outside_root: bool,
}

impl CdxmlDocument {
    /// Wraps an independently constructed interchange document for CDXML serialization.
    pub fn from_document(document: Document) -> Self {
        Self {
            document,
            original: None,
            declaration: None,
            doctype: None,
            unsafe_outside_root: false,
        }
    }
    pub fn document(&self) -> &Document {
        &self.document
    }
    pub fn document_mut(&mut self) -> &mut Document {
        &mut self.document
    }

    /// True when outside-root material could be retained only by writing the original.
    pub fn has_unrewritable_prolog(&self) -> bool {
        self.unsafe_outside_root
    }

    /// Explicitly drops opaque subtree/property content and outside-root material.
    pub fn discard_uninterpreted(
        &mut self,
    ) -> Result<usize, chemifree_domain::interchange::ValidationError> {
        let count = self.document.discard_opaque()?;
        if count > 0 || self.unsafe_outside_root {
            self.original = None;
            self.unsafe_outside_root = false;
        }
        Ok(count)
    }
}

fn error(code: &'static str, message: impl Into<String>, node: Option<Node<'_, '_>>) -> CdxmlError {
    let position = node.map(|node| node.document().text_pos_at(node.range().start));
    CdxmlError {
        code,
        message: message.into(),
        line: position.map(|pos| pos.row),
        column: position.map(|pos| pos.col),
    }
}

/// Parses CDXML without loading a DTD, network resource, or external entity.
///
/// A standard external doctype is accepted without fetching it. Internal subsets are refused,
/// so entity declarations cannot trigger expansion or external-resource access.
pub fn read(text: &str) -> Result<CdxmlDocument, CdxmlError> {
    if text.len() > MAX_XML_BYTES {
        return Err(error("resource_limit", "CDXML exceeds 1 MiB.", None));
    }
    if has_entity_declaration(text) {
        return Err(error(
            "unsupported_entity",
            "DTD internal subsets and entity declarations are not accepted.",
            None,
        ));
    }
    let options = ParsingOptions {
        allow_dtd: true,
        nodes_limit: MAX_XML_NODES,
        entity_resolver: None,
    };
    let xml = roxmltree::Document::parse_with_options(text, options).map_err(|cause| {
        let pos = cause.pos();
        CdxmlError {
            code: "invalid_xml",
            message: cause.to_string(),
            line: Some(pos.row),
            column: Some(pos.col),
        }
    })?;
    if !declares_utf8(text) {
        return Err(error(
            "unsupported_encoding",
            "Only UTF-8 XML declarations are accepted.",
            None,
        ));
    }
    let root = xml.root_element();
    if root.tag_name().name() != "CDXML" {
        return Err(error(
            "invalid_root",
            "Expected a CDXML document element.",
            Some(root),
        ));
    }
    let namespace = root.tag_name().namespace();
    let range = root.range();
    let (declaration, doctype, unsafe_prolog) = prolog(&text[..range.start]);
    let unsafe_outside_root = unsafe_prolog || !text[range.end..].trim().is_empty();
    let parsed = parse_object(root, namespace, 0, false)?;
    let document = Document::new(parsed.clone())
        .map_err(|cause| error("invalid_document", cause.to_string(), Some(root)))?;
    Ok(CdxmlDocument {
        document,
        original: Some((parsed, text.to_owned())),
        declaration,
        doctype,
        unsafe_outside_root,
    })
}

// roxmltree correctly requires an &str but intentionally does not check the declaration's
// encoding. Inspect it only after XML parsing has validated the declaration's syntax.
fn declares_utf8(text: &str) -> bool {
    let prefix = text.trim_start_matches('\u{feff}');
    let Some(declaration) = prefix.strip_prefix("<?xml") else {
        return true;
    };
    if !declaration.starts_with(char::is_whitespace) {
        return true;
    }
    let Some(end) = declaration.find("?>") else {
        return true;
    };
    let declaration = &declaration[..end];
    let Some(index) = declaration.find("encoding") else {
        return true;
    };
    let value = declaration[index + "encoding".len()..].trim_start();
    let Some(value) = value.strip_prefix('=').map(str::trim_start) else {
        return false;
    };
    let Some(quote) = value.chars().next() else {
        return false;
    };
    if quote != '\'' && quote != '"' {
        return false;
    }
    let Some(end) = value[1..].find(quote) else {
        return false;
    };
    value[1..1 + end].eq_ignore_ascii_case("utf-8")
}

fn doctype_end(text: &str) -> Option<usize> {
    let mut quote = None;
    let mut depth = 0_u32;
    for (index, character) in text.char_indices() {
        match (quote, character) {
            (Some(open), close) if open == close => quote = None,
            (Some(_), _) => (),
            (None, '\'' | '"') => quote = Some(character),
            (None, '[') => depth += 1,
            (None, ']') => depth = depth.saturating_sub(1),
            (None, '>') if depth == 0 => return Some(index + 1),
            _ => (),
        }
    }
    None
}

fn has_entity_declaration(mut text: &str) -> bool {
    loop {
        text = text.trim_start_matches('\u{feff}').trim_start();
        if text.starts_with("<!--") {
            let Some(end) = text.find("-->") else {
                return false;
            };
            text = &text[end + 3..];
        } else if text.starts_with("<?") {
            let Some(end) = text.find("?>") else {
                return false;
            };
            text = &text[end + 2..];
        } else if text.starts_with("<!DOCTYPE") {
            let mut quote = None;
            for character in text.chars() {
                match (quote, character) {
                    (Some(open), close) if open == close => quote = None,
                    (Some(_), _) => (),
                    (None, '\'' | '"') => quote = Some(character),
                    (None, '[') => return true,
                    (None, '>') => return false,
                    _ => (),
                }
            }
            return false;
        } else {
            return false;
        }
    }
}

/// Retains a simple doctype verbatim; other prolog material is not safe to rewrite implicitly.
fn prolog(mut prefix: &str) -> (Option<String>, Option<String>, bool) {
    let mut declaration = None;
    let mut doctype = None;
    loop {
        prefix = prefix.trim_start();
        if prefix.is_empty() {
            return (declaration, doctype, false);
        }
        if prefix
            .strip_prefix("<?xml")
            .is_some_and(|rest| rest.starts_with(char::is_whitespace))
            && declaration.is_none()
        {
            let Some(end) = prefix.find("?>") else {
                return (declaration, doctype, true);
            };
            declaration = Some(prefix[..end + 2].to_owned());
            prefix = &prefix[end + 2..];
        } else if prefix.starts_with("<!DOCTYPE") && doctype.is_none() {
            let Some(end) = doctype_end(prefix) else {
                return (declaration, doctype, true);
            };
            doctype = Some(prefix[..end].to_owned());
            prefix = &prefix[end..];
        } else {
            return (declaration, doctype, true);
        }
    }
}

fn object_kind(node: Node<'_, '_>, root_namespace: Option<&str>) -> ObjectKind {
    let name = node.tag_name();
    if name.namespace() == root_namespace {
        match name.name() {
            "CDXML" => return ObjectKind::Document,
            "page" => return ObjectKind::Page,
            "group" => return ObjectKind::Group,
            "fragment" => return ObjectKind::Fragment,
            "n" => return ObjectKind::Node,
            "b" => return ObjectKind::Bond,
            "graphic" => return ObjectKind::Graphic,
            "t" => return ObjectKind::Text,
            "s" => return ObjectKind::StyleRun,
            "fonttable" => return ObjectKind::FontTable,
            "font" => return ObjectKind::Font,
            "colortable" => return ObjectKind::ColorTable,
            "color" => return ObjectKind::Color,
            "represent" => return ObjectKind::Represents,
            _ => (),
        }
    }
    ObjectKind::UnknownXml(XmlName {
        namespace: name.namespace().map(str::to_owned),
        local: name.name().to_owned(),
    })
}

fn parse_object(
    node: Node<'_, '_>,
    root_namespace: Option<&str>,
    depth: usize,
    opaque_ancestor: bool,
) -> Result<Object, CdxmlError> {
    if depth > 128 {
        return Err(error(
            "resource_limit",
            "CDXML nesting exceeds 128 levels.",
            Some(node),
        ));
    }
    let kind = if opaque_ancestor {
        let name = node.tag_name();
        ObjectKind::UnknownXml(XmlName {
            namespace: name.namespace().map(str::to_owned),
            local: name.name().to_owned(),
        })
    } else {
        object_kind(node, root_namespace)
    };
    let opaque = matches!(kind, ObjectKind::UnknownXml(_));
    let mut object = Object {
        kind,
        id: None,
        properties: Vec::new(),
        children: Vec::new(),
    };
    if object.kind == ObjectKind::Document
        && let Some(namespace) = root_namespace
    {
        object.properties.push(Property {
            kind: PropertyKind::XmlNamespace,
            value: PropertyValue::Text(namespace.to_owned()),
        });
    }
    for attribute in node.attributes() {
        let name = attribute.name();
        let namespace = attribute.namespace();
        let value = attribute.value();
        if namespace.is_none() && name == "id" && object.kind != ObjectKind::Font && !opaque {
            let parsed: u32 = value.parse().map_err(|_| {
                error(
                    "invalid_id",
                    "An object ID must be an unsigned integer.",
                    Some(node),
                )
            })?;
            object.id = (parsed != 0).then_some(parsed);
        } else {
            object
                .properties
                .push(parse_property(&object.kind, namespace, name, value, node)?);
        }
    }
    for child in node.children() {
        match child.node_type() {
            NodeType::Element => object.children.push(Content::Object(parse_object(
                child,
                root_namespace,
                depth + 1,
                opaque,
            )?)),
            NodeType::Text => {
                let text = child.text().unwrap_or_default();
                if opaque
                    || !text.trim().is_empty()
                    || matches!(object.kind, ObjectKind::Text | ObjectKind::StyleRun)
                {
                    object.children.push(Content::Text(text.to_owned()));
                }
            }
            NodeType::Comment => object.children.push(Content::Comment(
                child.text().unwrap_or_default().to_owned(),
            )),
            NodeType::PI => {
                if let Some(pi) = child.pi() {
                    object.children.push(Content::ProcessingInstruction {
                        target: pi.target.to_owned(),
                        value: pi.value.map(str::to_owned),
                    });
                }
            }
            NodeType::Root => (),
        }
    }
    Ok(object)
}

fn integer(value: &str, node: Node<'_, '_>, name: &str) -> Result<i64, CdxmlError> {
    value.parse().map_err(|_| {
        error(
            "invalid_property",
            format!("{name} must be an integer."),
            Some(node),
        )
    })
}

fn number(value: &str, node: Node<'_, '_>, name: &str) -> Result<f64, CdxmlError> {
    let number: f64 = value.parse().map_err(|_| {
        error(
            "invalid_property",
            format!("{name} must be a number."),
            Some(node),
        )
    })?;
    if !number.is_finite() {
        return Err(error(
            "invalid_property",
            format!("{name} must be finite."),
            Some(node),
        ));
    }
    Ok(number)
}

fn numbers<const N: usize>(
    value: &str,
    node: Node<'_, '_>,
    name: &str,
) -> Result<[f64; N], CdxmlError> {
    let values: Vec<f64> = value
        .split_whitespace()
        .map(|part| number(part, node, name))
        .collect::<Result<_, _>>()?;
    values.try_into().map_err(|_| {
        error(
            "invalid_property",
            format!("{name} needs {N} numbers."),
            Some(node),
        )
    })
}

fn reference(value: &str, node: Node<'_, '_>, name: &str) -> Result<u32, CdxmlError> {
    let id: u32 = value.parse().map_err(|_| {
        error(
            "invalid_reference",
            format!("{name} must be an object ID."),
            Some(node),
        )
    })?;
    if id == 0 {
        return Err(error(
            "invalid_reference",
            format!("{name} cannot be zero."),
            Some(node),
        ));
    }
    Ok(id)
}

fn order_bits(value: &str) -> Option<i64> {
    let mut bits = 0;
    for token in value.split_whitespace() {
        bits |= match token {
            "1" => 0x0001,
            "2" => 0x0002,
            "3" => 0x0004,
            "4" => 0x0008,
            "5" => 0x0010,
            "6" => 0x0020,
            "0.5" => 0x0040,
            "1.5" => 0x0080,
            "2.5" => 0x0100,
            "3.5" => 0x0200,
            "4.5" => 0x0400,
            "5.5" => 0x0800,
            "dative" => 0x1000,
            "ionic" => 0x2000,
            "hydrogen" => 0x4000,
            "threecenter" => 0x8000,
            _ => return None,
        };
    }
    (bits != 0).then_some(bits)
}

// Shared numeric values and XML names from the published CDX Node_Type table. A node
// need not denote an atom; consumers must check its type before interpreting chemistry.
const GRAPHIC_TYPES: [&str; 8] = [
    "Undefined",
    "Line",
    "Arc",
    "Rectangle",
    "Oval",
    "Orbital",
    "Bracket",
    "Symbol",
];
const NODE_TYPES: [&str; 14] = [
    "Unspecified",
    "Element",
    "ElementList",
    "ElementListNickname",
    "Nickname",
    "Fragment",
    "Formula",
    "GenericNickname",
    "AnonymousAlternativeGroup",
    "NamedAlternativeGroup",
    "MultiAttachment",
    "VariableAttachment",
    "ExternalConnectionPoint",
    "LinkNode",
];

fn parse_property(
    kind: &ObjectKind,
    namespace: Option<&str>,
    name: &str,
    value: &str,
    node: Node<'_, '_>,
) -> Result<Property, CdxmlError> {
    use ObjectKind as O;
    use PropertyKind as K;
    use PropertyValue as V;
    let unknown = || Property {
        kind: K::UnknownXml(XmlName {
            namespace: namespace.map(str::to_owned),
            local: name.to_owned(),
        }),
        value: V::Text(value.to_owned()),
    };
    if namespace.is_some() {
        return Ok(unknown());
    }
    let property = match (kind, name) {
        (O::Document | O::Page | O::Group | O::Fragment | O::Text | O::Graphic, "BoundingBox") => {
            let [left, top, right, bottom] = numbers(value, node, name)?;
            Property {
                kind: K::BoundingBox,
                value: V::Rectangle {
                    left,
                    top,
                    right,
                    bottom,
                },
            }
        }
        (O::Node | O::Text, "p") => {
            let [x, y] = numbers(value, node, name)?;
            Property {
                kind: K::Position,
                value: V::Point { x, y },
            }
        }
        (O::Node, "NodeType") => match NODE_TYPES.iter().position(|name| *name == value) {
            Some(index) => Property {
                kind: K::NodeType,
                value: V::Integer(index as i64),
            },
            None => return Ok(unknown()),
        },
        (O::Graphic, "GraphicType") => match GRAPHIC_TYPES.iter().position(|name| *name == value) {
            Some(index) => Property {
                kind: K::GraphicType,
                value: V::Integer(index as i64),
            },
            None => return Ok(unknown()),
        },
        (O::Node, "Element") => Property {
            kind: K::AtomicNumber,
            value: V::Integer(integer(value, node, name)?),
        },
        (O::Node, "Charge") => Property {
            kind: K::Charge,
            value: V::Integer(integer(value, node, name)?),
        },
        (O::Node, "Isotope") => Property {
            kind: K::Isotope,
            value: V::Integer(integer(value, node, name)?),
        },
        (O::Node, "Radical") => {
            let radical = match value {
                "None" => 0,
                "Singlet" => 1,
                "Doublet" => 2,
                "Triplet" => 3,
                _ => return Ok(unknown()),
            };
            Property {
                kind: K::Radical,
                value: V::Integer(radical),
            }
        }
        (O::Bond, "B") => Property {
            kind: K::BondBegin,
            value: V::ObjectReference(reference(value, node, name)?),
        },
        (O::Bond, "E") => Property {
            kind: K::BondEnd,
            value: V::ObjectReference(reference(value, node, name)?),
        },
        (O::Bond, "Order") => match order_bits(value) {
            Some(bits) => Property {
                kind: K::BondOrder,
                value: V::Integer(bits),
            },
            None => return Ok(unknown()),
        },
        (O::Font, "id") | (O::StyleRun, "font") => Property {
            kind: K::FontIndex,
            value: V::Integer(integer(value, node, name)?),
        },
        (O::Font, "name") => Property {
            kind: K::FontName,
            value: V::Text(value.into()),
        },
        (O::Font, "charset") => Property {
            kind: K::FontCharset,
            value: V::Text(value.into()),
        },
        (O::StyleRun, "face") => Property {
            kind: K::FontFace,
            value: V::Integer(integer(value, node, name)?),
        },
        (O::StyleRun, "size") => Property {
            kind: K::FontSize,
            value: V::Float(number(value, node, name)?),
        },
        (O::StyleRun, "color") => Property {
            kind: K::ColorIndex,
            value: V::Integer(integer(value, node, name)?),
        },
        (O::Color, "r" | "g" | "b") => Property {
            kind: match name {
                "r" => K::Red,
                "g" => K::Green,
                _ => K::Blue,
            },
            value: V::Float(number(value, node, name)?),
        },
        (O::Represents, "object") => Property {
            kind: K::RepresentObject,
            value: V::ObjectReference(reference(value, node, name)?),
        },
        (O::Represents, "attribute") => Property {
            kind: K::RepresentAttribute,
            value: V::Text(value.into()),
        },
        _ => return Ok(unknown()),
    };
    Ok(property)
}

/// Writes the current document. An unchanged source is returned byte for byte, including
/// uninterpreted extensions. A modified document with opaque content is refused because its
/// extension values may contain references or namespace-sensitive text that cannot be rewritten.
pub fn write(file: &CdxmlDocument) -> Result<String, CdxmlError> {
    file.document
        .validate()
        .map_err(|cause| error("invalid_document", cause.to_string(), None))?;
    if let Some((original_root, original)) = &file.original
        && file.document.root() == original_root
    {
        return Ok(original.clone());
    }
    if contains_opaque(file.document.root()) {
        return Err(error(
            "unsafe_unknown_rewrite",
            "Modified CDXML contains opaque content whose references or namespaces cannot be safely rewritten.",
            None,
        ));
    }
    if file.unsafe_outside_root {
        return Err(error(
            "unsafe_prolog_rewrite",
            "Comments or instructions outside the root cannot be safely rewritten.",
            None,
        ));
    }
    let mut result = file
        .declaration
        .clone()
        .unwrap_or_else(|| "<?xml version=\"1.0\" encoding=\"UTF-8\"?>".into());
    result.push('\n');
    if let Some(doctype) = &file.doctype {
        result.push_str(doctype);
        result.push('\n');
    }
    let namespace = file.document.root().properties.iter().find_map(|property| {
        if let (PropertyKind::XmlNamespace, PropertyValue::Text(uri)) =
            (&property.kind, &property.value)
        {
            Some(uri.as_str())
        } else {
            None
        }
    });
    write_object(file.document.root(), namespace, true, &mut result)?;
    result.push('\n');
    Ok(result)
}

fn contains_opaque(object: &Object) -> bool {
    matches!(
        object.kind,
        ObjectKind::UnknownXml(_) | ObjectKind::UnknownBinary(_)
    ) || object.properties.iter().any(|p| {
        matches!(
            p.kind,
            PropertyKind::UnknownXml(_) | PropertyKind::UnknownBinary(_)
        )
    }) || object
        .children
        .iter()
        .any(|child| matches!(child, Content::Object(child) if contains_opaque(child)))
}

fn tag_name(kind: &ObjectKind) -> Option<&'static str> {
    Some(match kind {
        ObjectKind::Document => "CDXML",
        ObjectKind::Page => "page",
        ObjectKind::Group => "group",
        ObjectKind::Fragment => "fragment",
        ObjectKind::Node => "n",
        ObjectKind::Bond => "b",
        ObjectKind::Graphic => "graphic",
        ObjectKind::Text => "t",
        ObjectKind::StyleRun => "s",
        ObjectKind::FontTable => "fonttable",
        ObjectKind::Font => "font",
        ObjectKind::ColorTable => "colortable",
        ObjectKind::Color => "color",
        ObjectKind::Represents => "represent",
        ObjectKind::UnknownXml(_) | ObjectKind::UnknownBinary(_) => return None,
    })
}

fn property_name(object: &ObjectKind, kind: &PropertyKind) -> Option<&'static str> {
    Some(match kind {
        PropertyKind::XmlNamespace => return None,
        PropertyKind::Position => "p",
        PropertyKind::BoundingBox => "BoundingBox",
        PropertyKind::NodeType => "NodeType",
        PropertyKind::AtomicNumber => "Element",
        PropertyKind::Charge => "Charge",
        PropertyKind::Isotope => "Isotope",
        PropertyKind::Radical => "Radical",
        PropertyKind::BondOrder => "Order",
        PropertyKind::BondBegin => "B",
        PropertyKind::BondEnd => "E",
        PropertyKind::GraphicType => "GraphicType",
        PropertyKind::FontIndex if *object == ObjectKind::Font => "id",
        PropertyKind::FontIndex => "font",
        PropertyKind::FontName => "name",
        PropertyKind::FontCharset => "charset",
        PropertyKind::FontFace => "face",
        PropertyKind::FontSize => "size",
        PropertyKind::ColorIndex => "color",
        PropertyKind::Red => "r",
        PropertyKind::Green => "g",
        PropertyKind::Blue => "b",
        PropertyKind::RepresentObject => "object",
        PropertyKind::RepresentAttribute => "attribute",
        PropertyKind::UnknownXml(_) | PropertyKind::UnknownBinary(_) => return None,
    })
}

fn order_text(bits: i64) -> Result<String, CdxmlError> {
    let orders = [
        (0x0001, "1"),
        (0x0002, "2"),
        (0x0004, "3"),
        (0x0008, "4"),
        (0x0010, "5"),
        (0x0020, "6"),
        (0x0040, "0.5"),
        (0x0080, "1.5"),
        (0x0100, "2.5"),
        (0x0200, "3.5"),
        (0x0400, "4.5"),
        (0x0800, "5.5"),
        (0x1000, "dative"),
        (0x2000, "ionic"),
        (0x4000, "hydrogen"),
        (0x8000, "threecenter"),
    ];
    // 0xffff has a special "unspecified" meaning in CDX. Omitting Order in CDXML
    // means a single bond, so writing either omission or a list would change meaning.
    if bits == 0xffff {
        return Err(error(
            "unsupported_property",
            "The CDX unspecified bond order has no unambiguous CDXML representation.",
            None,
        ));
    }
    if !(1..=0xffff).contains(&bits) {
        return Err(error(
            "invalid_property",
            "Bond order bitset is out of range.",
            None,
        ));
    }
    Ok(orders
        .iter()
        .filter(|(mask, _)| bits & mask != 0)
        .map(|(_, name)| *name)
        .collect::<Vec<_>>()
        .join(" "))
}

fn property_text(property: &Property) -> Result<String, CdxmlError> {
    use PropertyKind as K;
    use PropertyValue as V;
    Ok(match (&property.kind, &property.value) {
        (K::NodeType, V::Integer(value)) => NODE_TYPES
            .get(*value as usize)
            .ok_or_else(|| error("unsupported_property", "Unknown node type.", None))?
            .to_string(),
        (K::GraphicType, V::Integer(value)) => GRAPHIC_TYPES
            .get(*value as usize)
            .ok_or_else(|| error("unsupported_property", "Unknown graphic type.", None))?
            .to_string(),
        (K::Radical, V::Integer(value)) => match value {
            0 => "None",
            1 => "Singlet",
            2 => "Doublet",
            3 => "Triplet",
            _ => {
                return Err(error(
                    "unsupported_property",
                    "Unknown radical value.",
                    None,
                ));
            }
        }
        .into(),
        (K::BondOrder, V::Integer(value)) => order_text(*value)?,
        (_, V::Integer(value)) => value.to_string(),
        (_, V::Float(value)) => value.to_string(),
        (_, V::Point { x, y }) => format!("{x} {y}"),
        (
            _,
            V::Rectangle {
                left,
                top,
                right,
                bottom,
            },
        ) => format!("{left} {top} {right} {bottom}"),
        (_, V::ObjectReference(value)) => value.to_string(),
        (_, V::Text(value)) => value.clone(),
        (_, V::Bytes(_)) => {
            return Err(error(
                "unsupported_property",
                "A binary extension cannot be emitted as CDXML.",
                None,
            ));
        }
    })
}

fn xml_characters(text: &str) -> bool {
    text.chars().all(|character| matches!(character, '\t' | '\n' | '\r' | '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}'))
}

fn escape(text: &str, attribute: bool) -> Result<String, CdxmlError> {
    if !xml_characters(text) {
        return Err(error(
            "invalid_xml_text",
            "Text contains a character forbidden in XML 1.0.",
            None,
        ));
    }
    let mut result = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => result.push_str("&amp;"),
            '<' => result.push_str("&lt;"),
            '>' => result.push_str("&gt;"),
            '"' if attribute => result.push_str("&quot;"),
            '\n' if attribute => result.push_str("&#10;"),
            '\r' => result.push_str("&#13;"),
            '\t' if attribute => result.push_str("&#9;"),
            _ => result.push(character),
        }
    }
    Ok(result)
}

fn write_object(
    object: &Object,
    namespace: Option<&str>,
    root: bool,
    output: &mut String,
) -> Result<(), CdxmlError> {
    let tag = tag_name(&object.kind).ok_or_else(|| {
        error(
            "unsafe_unknown_rewrite",
            "Unknown objects cannot be rewritten.",
            None,
        )
    })?;
    let _ = write!(output, "<{tag}");
    if root && let Some(uri) = namespace {
        if uri == "http://www.w3.org/XML/1998/namespace" || uri == "http://www.w3.org/2000/xmlns/" {
            return Err(error(
                "invalid_xml_namespace",
                "Reserved namespaces cannot be the default document namespace.",
                None,
            ));
        }
        let _ = write!(output, " xmlns=\"{}\"", escape(uri, true)?);
    }
    if let Some(id) = object.id {
        let _ = write!(output, " id=\"{id}\"");
    }
    for property in &object.properties {
        if root && property.kind == PropertyKind::XmlNamespace {
            continue;
        }
        let name = property_name(&object.kind, &property.kind).ok_or_else(|| {
            error(
                "unsafe_unknown_rewrite",
                "Unknown properties cannot be rewritten.",
                None,
            )
        })?;
        let value = property_text(property)?;
        let _ = write!(output, " {name}=\"{}\"", escape(&value, true)?);
    }
    if object.children.is_empty() {
        output.push_str("/>");
        return Ok(());
    }
    output.push('>');
    for child in &object.children {
        match child {
            Content::Object(child) => write_object(child, namespace, false, output)?,
            Content::Text(text) => output.push_str(&escape(text, false)?),
            Content::Comment(text) => {
                if text.contains("--")
                    || text.ends_with('-')
                    || text.contains('\r')
                    || !xml_characters(text)
                {
                    return Err(error("invalid_comment", "Invalid XML comment.", None));
                }
                let _ = write!(output, "<!--{text}-->");
            }
            Content::ProcessingInstruction { target, value } => {
                let mut characters = target.chars();
                let valid_target = characters
                    .next()
                    .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == ':')
                    && characters
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | ':' | '-' | '.'));
                if !valid_target
                    || target.eq_ignore_ascii_case("xml")
                    || value
                        .as_ref()
                        .is_some_and(|v| v.contains("?>") || v.contains('\r') || !xml_characters(v))
                {
                    return Err(error(
                        "invalid_instruction",
                        "Invalid processing instruction.",
                        None,
                    ));
                }
                let _ = write!(output, "<?{target}");
                if let Some(value) = value {
                    let _ = write!(output, " {value}");
                }
                output.push_str("?>");
            }
        }
    }
    let _ = write!(output, "</{tag}>");
    Ok(())
}
