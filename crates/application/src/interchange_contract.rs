//! Transport DTOs for the ordered, format-independent interchange document.
//!
//! Constructing a domain document from the wire always validates it. Opaque-content
//! baselines are deliberately not accepted from clients; a codec must re-read a supplied
//! original and check it against the submitted document before preserving its bytes.

use chemifree_domain::interchange as core;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InterchangeDocumentDto {
    pub root: InterchangeObjectDto,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct XmlNameDto {
    pub namespace: Option<String>,
    pub local: String,
}

impl From<core::XmlName> for XmlNameDto {
    fn from(value: core::XmlName) -> Self {
        Self {
            namespace: value.namespace,
            local: value.local,
        }
    }
}

impl From<XmlNameDto> for core::XmlName {
    fn from(value: XmlNameDto) -> Self {
        Self {
            namespace: value.namespace,
            local: value.local,
        }
    }
}

// Both kinds have format-specific opaque cases. The match stays exhaustive when the
// core vocabulary changes, without making the domain depend on serialization.
macro_rules! wire_kind {
    ($wire:ident, $core:ident, [$($variant:ident),* $(,)?]) => {
        #[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
        #[serde(tag = "type", content = "value", rename_all = "snake_case", deny_unknown_fields)]
        pub enum $wire {
            $($variant,)*
            UnknownXml(XmlNameDto),
            UnknownBinary(u16),
        }
        impl From<core::$core> for $wire {
            fn from(value: core::$core) -> Self {
                match value {
                    $(core::$core::$variant => Self::$variant,)*
                    core::$core::UnknownXml(name) => Self::UnknownXml(name.into()),
                    core::$core::UnknownBinary(tag) => Self::UnknownBinary(tag),
                }
            }
        }
        impl From<$wire> for core::$core {
            fn from(value: $wire) -> Self {
                match value {
                    $($wire::$variant => Self::$variant,)*
                    $wire::UnknownXml(name) => Self::UnknownXml(name.into()),
                    $wire::UnknownBinary(tag) => Self::UnknownBinary(tag),
                }
            }
        }
    };
}

wire_kind!(
    ObjectKindDto,
    ObjectKind,
    [
        Document, Page, Group, Fragment, Node, Bond, Graphic, Text, StyleRun, FontTable, Font,
        ColorTable, Color, Represents,
    ]
);
wire_kind!(
    PropertyKindDto,
    PropertyKind,
    [
        XmlNamespace,
        Position,
        BoundingBox,
        NodeType,
        AtomicNumber,
        Charge,
        Isotope,
        Radical,
        BondOrder,
        BondBegin,
        BondEnd,
        GraphicType,
        FontIndex,
        FontName,
        FontCharset,
        FontFace,
        FontSize,
        ColorIndex,
        Red,
        Green,
        Blue,
        RepresentObject,
        RepresentAttribute,
    ]
);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum PropertyValueDto {
    Integer(i64),
    Float(f64),
    Point {
        x: f64,
        y: f64,
    },
    Rectangle {
        left: f64,
        top: f64,
        right: f64,
        bottom: f64,
    },
    ObjectReference(u32),
    Text(String),
    Bytes(Vec<u8>),
}

impl From<core::PropertyValue> for PropertyValueDto {
    fn from(value: core::PropertyValue) -> Self {
        match value {
            core::PropertyValue::Integer(value) => Self::Integer(value),
            core::PropertyValue::Float(value) => Self::Float(value),
            core::PropertyValue::Point { x, y } => Self::Point { x, y },
            core::PropertyValue::Rectangle {
                left,
                top,
                right,
                bottom,
            } => Self::Rectangle {
                left,
                top,
                right,
                bottom,
            },
            core::PropertyValue::ObjectReference(value) => Self::ObjectReference(value),
            core::PropertyValue::Text(value) => Self::Text(value),
            core::PropertyValue::Bytes(value) => Self::Bytes(value),
        }
    }
}
impl From<PropertyValueDto> for core::PropertyValue {
    fn from(value: PropertyValueDto) -> Self {
        match value {
            PropertyValueDto::Integer(value) => Self::Integer(value),
            PropertyValueDto::Float(value) => Self::Float(value),
            PropertyValueDto::Point { x, y } => Self::Point { x, y },
            PropertyValueDto::Rectangle {
                left,
                top,
                right,
                bottom,
            } => Self::Rectangle {
                left,
                top,
                right,
                bottom,
            },
            PropertyValueDto::ObjectReference(value) => Self::ObjectReference(value),
            PropertyValueDto::Text(value) => Self::Text(value),
            PropertyValueDto::Bytes(value) => Self::Bytes(value),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PropertyDto {
    pub kind: PropertyKindDto,
    pub value: PropertyValueDto,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ContentDto {
    Object(InterchangeObjectDto),
    Text(String),
    Comment(String),
    ProcessingInstruction {
        target: String,
        value: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InterchangeObjectDto {
    pub kind: ObjectKindDto,
    pub id: Option<u32>,
    pub properties: Vec<PropertyDto>,
    pub children: Vec<ContentDto>,
}

impl From<core::Object> for InterchangeObjectDto {
    fn from(value: core::Object) -> Self {
        Self {
            kind: value.kind.into(),
            id: value.id,
            properties: value
                .properties
                .into_iter()
                .map(|property| PropertyDto {
                    kind: property.kind.into(),
                    value: property.value.into(),
                })
                .collect(),
            children: value
                .children
                .into_iter()
                .map(|child| match child {
                    core::Content::Object(object) => ContentDto::Object(object.into()),
                    core::Content::Text(text) => ContentDto::Text(text),
                    core::Content::Comment(text) => ContentDto::Comment(text),
                    core::Content::ProcessingInstruction { target, value } => {
                        ContentDto::ProcessingInstruction { target, value }
                    }
                })
                .collect(),
        }
    }
}
impl From<InterchangeObjectDto> for core::Object {
    fn from(value: InterchangeObjectDto) -> Self {
        Self {
            kind: value.kind.into(),
            id: value.id,
            properties: value
                .properties
                .into_iter()
                .map(|property| core::Property {
                    kind: property.kind.into(),
                    value: property.value.into(),
                })
                .collect(),
            children: value
                .children
                .into_iter()
                .map(|child| match child {
                    ContentDto::Object(object) => core::Content::Object(object.into()),
                    ContentDto::Text(text) => core::Content::Text(text),
                    ContentDto::Comment(text) => core::Content::Comment(text),
                    ContentDto::ProcessingInstruction { target, value } => {
                        core::Content::ProcessingInstruction { target, value }
                    }
                })
                .collect(),
        }
    }
}
impl From<&core::Document> for InterchangeDocumentDto {
    fn from(value: &core::Document) -> Self {
        Self {
            root: value.root().clone().into(),
        }
    }
}
impl TryFrom<InterchangeDocumentDto> for core::Document {
    type Error = core::ValidationError;

    fn try_from(value: InterchangeDocumentDto) -> Result<Self, Self::Error> {
        Self::new(value.root.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_tree_is_validated_before_it_becomes_a_document() {
        let json = r#"{"root":{"kind":{"type":"document"},"id":null,"properties":[],"children":[{"type":"object","value":{"kind":{"type":"page"},"id":2,"properties":[],"children":[]}}]}}"#;
        let dto: InterchangeDocumentDto = serde_json::from_str(json).unwrap();
        let document = core::Document::try_from(dto).unwrap();
        let encoded = serde_json::to_value(InterchangeDocumentDto::from(&document)).unwrap();
        assert_eq!(
            encoded,
            serde_json::from_str::<serde_json::Value>(json).unwrap()
        );

        let invalid = json.replace("\"id\":2", "\"id\":0");
        let dto: InterchangeDocumentDto = serde_json::from_str(&invalid).unwrap();
        assert_eq!(
            core::Document::try_from(dto),
            Err(core::ValidationError::ZeroId)
        );
    }
}
