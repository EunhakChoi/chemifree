//! A format-independent, ordered interchange document for CDXML and CDX.
//!
//! Unlike the editor's single-graph document, this representation can carry multiple pages,
//! fragments, styled text and format extensions. A property or object that is not interpreted
//! remains explicitly opaque; it is never assumed to be free of references.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// XML expanded name; prefixes are serialization details, while the namespace URI is semantic.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XmlName {
    pub namespace: Option<String>,
    pub local: String,
}

/// The common object kinds that both interchange codecs can consume.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ObjectKind {
    Document,
    Page,
    Group,
    Fragment,
    Node,
    Bond,
    Graphic,
    Text,
    StyleRun,
    FontTable,
    Font,
    ColorTable,
    Color,
    Represents,
    /// An XML extension that has no known binary equivalent.
    UnknownXml(XmlName),
    /// A binary object whose semantics are not known.
    UnknownBinary(u16),
}

/// A supported property or an uninterpreted format-specific property.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PropertyKind {
    /// The expanded namespace of a CDXML document, absent for unnamespaced XML.
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
    UnknownXml(XmlName),
    UnknownBinary(u16),
}

/// A value decoded by a codec; opaque binary data has no presumed reference semantics.
#[derive(Clone, Debug, PartialEq)]
pub enum PropertyValue {
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

#[derive(Clone, Debug, PartialEq)]
pub struct Property {
    pub kind: PropertyKind,
    pub value: PropertyValue,
}

/// Ordered child content: styled text and object ordering are independent of graph identity.
#[derive(Clone, Debug, PartialEq)]
pub enum Content {
    Object(Object),
    Text(String),
    Comment(String),
    ProcessingInstruction {
        target: String,
        value: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Object {
    pub kind: ObjectKind,
    /// A missing ID is distinct from zero: zero in CDX denotes an unreferenced object.
    pub id: Option<u32>,
    pub properties: Vec<Property>,
    pub children: Vec<Content>,
}

/// An interchange tree, with a baseline used to detect unsafe ID rewrites near opaque data.
#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    root: Object,
    opaque_baseline: Option<OpaqueBaseline>,
}

#[derive(Clone, Debug, PartialEq)]
struct OpaqueBaseline {
    ids_by_path: Vec<(Vec<usize>, u32)>,
    parts: Vec<OpaquePart>,
}

#[derive(Clone, Debug, PartialEq)]
enum OpaquePart {
    Object {
        path: Vec<usize>,
        value: Object,
    },
    Property {
        path: Vec<usize>,
        index: usize,
        value: Property,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ValidationError {
    RootMustBeDocument,
    PageRequired,
    ZeroId,
    DuplicateId(u32),
    DanglingReference(u32),
    BondEndpointNotNode(u32),
    BondOutsideFragment,
    NodeOutsideFragment,
    MissingBondEndpoint,
    MissingGraphicBounds,
    MissingRepresentProperty,
    MissingTextStyle,
    InvalidContainment,
    SelfBond,
    CrossFragmentBond(u32),
    DuplicateProperty,
    MissingFontIndex,
    MissingFontName,
    DuplicateFont(u16),
    UnknownFont(u16),
    DuplicateColorTable,
    UnknownColor(u16),
    InvalidProperty,
    DepthLimit,
    ObjectLimit,
    UnsafeIdRewrite,
    OpaqueContentChanged,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}
impl std::error::Error for ValidationError {}

const MAX_DEPTH: usize = 128;
const MAX_OBJECTS: usize = 100_000;

impl Document {
    /// Creates a document only after checking global IDs and known references.
    pub fn new(root: Object) -> Result<Self, ValidationError> {
        let mut document = Self {
            root,
            opaque_baseline: None,
        };
        let facts = document.inspect()?;
        if !facts.opaque_parts.is_empty() {
            document.opaque_baseline = Some(OpaqueBaseline {
                ids_by_path: facts.ids_by_path,
                parts: facts.opaque_parts,
            });
        }
        Ok(document)
    }

    pub fn root(&self) -> &Object {
        &self.root
    }

    /// Applies an edit atomically after checking IDs, references and opaque-content safety.
    pub fn edit(&mut self, change: impl FnOnce(&mut Object)) -> Result<(), ValidationError> {
        let mut candidate = self.clone();
        change(&mut candidate.root);
        candidate.validate()?;
        // A clean document can acquire an extension through an edit. From this point
        // on, its IDs and opaque parts need the same protection as parsed extensions.
        if candidate.opaque_baseline.is_none() {
            let facts = candidate.inspect()?;
            if !facts.opaque_parts.is_empty() {
                candidate.opaque_baseline = Some(OpaqueBaseline {
                    ids_by_path: facts.ids_by_path,
                    parts: facts.opaque_parts,
                });
            }
        }
        *self = candidate;
        Ok(())
    }

    /// Rejects dangling or duplicate IDs and any ID remap while opaque content remains.
    pub fn validate(&self) -> Result<(), ValidationError> {
        let facts = self.inspect()?;
        if let Some(original) = &self.opaque_baseline {
            if original.ids_by_path != facts.ids_by_path {
                return Err(ValidationError::UnsafeIdRewrite);
            }
            if original.parts != facts.opaque_parts {
                return Err(ValidationError::OpaqueContentChanged);
            }
        }
        Ok(())
    }

    /// Explicitly discards opaque objects and properties after checking the remaining tree.
    /// Returns the number removed, so a caller can report the loss. A refusal changes nothing.
    pub fn discard_opaque(&mut self) -> Result<usize, ValidationError> {
        let mut root = self.root.clone();
        let removed = discard_opaque_from(&mut root);
        let replacement = Self::new(root)?;
        *self = replacement;
        Ok(removed)
    }

    fn inspect(&self) -> Result<Facts, ValidationError> {
        if self.root.kind != ObjectKind::Document {
            return Err(ValidationError::RootMustBeDocument);
        }
        if !self.root.children.iter().any(|child| {
            matches!(
                child,
                Content::Object(Object {
                    kind: ObjectKind::Page,
                    ..
                })
            )
        }) {
            return Err(ValidationError::PageRequired);
        }
        let mut facts = Facts::default();
        inspect_object(&self.root, &mut Vec::new(), None, &mut facts)?;
        for reference in &facts.references {
            let (kind, fragment) = facts
                .kinds
                .get(&reference.target)
                .ok_or(ValidationError::DanglingReference(reference.target))?;
            if let Some(expected) = &reference.atom_fragment {
                if *kind != ObjectKind::Node {
                    return Err(ValidationError::BondEndpointNotNode(reference.target));
                }
                if fragment.as_ref() != Some(expected) {
                    return Err(ValidationError::CrossFragmentBond(reference.target));
                }
            }
        }
        for font in &facts.font_references {
            if !facts.fonts.contains(font) {
                return Err(ValidationError::UnknownFont(*font));
            }
        }
        for color in &facts.color_references {
            let available = match facts.color_count {
                Some(count) => usize::from(*color) < 2 + count,
                None => *color <= 3,
            };
            if !available {
                return Err(ValidationError::UnknownColor(*color));
            }
        }
        Ok(facts)
    }
}

#[derive(Default)]
struct Facts {
    count: usize,
    opaque_parts: Vec<OpaquePart>,
    kinds: BTreeMap<u32, (ObjectKind, Option<Vec<usize>>)>,
    ids_by_path: Vec<(Vec<usize>, u32)>,
    references: Vec<Reference>,
    fonts: BTreeSet<u16>,
    font_references: Vec<u16>,
    color_count: Option<usize>,
    color_references: Vec<u16>,
}

struct Reference {
    target: u32,
    /// Some only for a bond endpoint, which must name a node in the same fragment.
    atom_fragment: Option<Vec<usize>>,
}

fn inspect_object(
    object: &Object,
    path: &mut Vec<usize>,
    fragment: Option<&[usize]>,
    facts: &mut Facts,
) -> Result<(), ValidationError> {
    if path.len() > MAX_DEPTH {
        return Err(ValidationError::DepthLimit);
    }
    facts.count += 1;
    if facts.count > MAX_OBJECTS {
        return Err(ValidationError::ObjectLimit);
    }
    if matches!(
        object.kind,
        ObjectKind::UnknownXml(_) | ObjectKind::UnknownBinary(_)
    ) {
        facts.opaque_parts.push(OpaquePart::Object {
            path: path.clone(),
            value: object.clone(),
        });
    }
    if object.kind == ObjectKind::ColorTable {
        if facts.color_count.is_some() {
            return Err(ValidationError::DuplicateColorTable);
        }
        facts.color_count = Some(
            object
                .children
                .iter()
                .filter(|child| {
                    matches!(
                        child,
                        Content::Object(Object {
                            kind: ObjectKind::Color,
                            ..
                        })
                    )
                })
                .count(),
        );
    }
    let fragment_path = if object.kind == ObjectKind::Fragment {
        Some(path.clone())
    } else {
        fragment.map(<[usize]>::to_vec)
    };
    if object.kind == ObjectKind::Bond && fragment_path.is_none() {
        return Err(ValidationError::BondOutsideFragment);
    }
    if object.kind == ObjectKind::Node && fragment_path.is_none() {
        return Err(ValidationError::NodeOutsideFragment);
    }
    if let Some(id) = object.id {
        if id == 0 {
            return Err(ValidationError::ZeroId);
        }
        if facts
            .kinds
            .insert(id, (object.kind.clone(), fragment_path.clone()))
            .is_some()
        {
            return Err(ValidationError::DuplicateId(id));
        }
        facts.ids_by_path.push((path.clone(), id));
    }
    for (index, property) in object.properties.iter().enumerate() {
        if object.properties[..index]
            .iter()
            .any(|previous| previous.kind == property.kind)
        {
            return Err(ValidationError::DuplicateProperty);
        }
        if matches!(
            property.kind,
            PropertyKind::UnknownXml(_) | PropertyKind::UnknownBinary(_)
        ) {
            facts.opaque_parts.push(OpaquePart::Property {
                path: path.clone(),
                index,
                value: property.clone(),
            });
        }
        if !valid_property(property) || !property_allowed(&object.kind, &property.kind) {
            return Err(ValidationError::InvalidProperty);
        }
        if let (PropertyKind::FontIndex, PropertyValue::Integer(index)) =
            (&property.kind, &property.value)
        {
            let font = *index as u16;
            if object.kind == ObjectKind::Font {
                if !facts.fonts.insert(font) {
                    return Err(ValidationError::DuplicateFont(font));
                }
            } else if object.kind == ObjectKind::StyleRun {
                facts.font_references.push(font);
            }
        }
        if let (PropertyKind::ColorIndex, PropertyValue::Integer(index)) =
            (&property.kind, &property.value)
        {
            facts.color_references.push(*index as u16);
        }
        match &property.value {
            PropertyValue::ObjectReference(0) => return Err(ValidationError::ZeroId),
            PropertyValue::ObjectReference(id) => {
                let atom_fragment = if matches!(
                    property.kind,
                    PropertyKind::BondBegin | PropertyKind::BondEnd
                ) {
                    Some(
                        fragment_path
                            .clone()
                            .ok_or(ValidationError::BondOutsideFragment)?,
                    )
                } else {
                    None
                };
                facts.references.push(Reference {
                    target: *id,
                    atom_fragment,
                });
            }
            PropertyValue::Float(value) if !value.is_finite() => {
                return Err(ValidationError::InvalidProperty);
            }
            PropertyValue::Point { x, y } if !x.is_finite() || !y.is_finite() => {
                return Err(ValidationError::InvalidProperty);
            }
            PropertyValue::Rectangle {
                left,
                top,
                right,
                bottom,
            } if ![left, top, right, bottom].iter().all(|v| v.is_finite()) => {
                return Err(ValidationError::InvalidProperty);
            }
            _ => (),
        }
    }
    if object.kind == ObjectKind::Font
        && !object
            .properties
            .iter()
            .any(|property| property.kind == PropertyKind::FontIndex)
    {
        return Err(ValidationError::MissingFontIndex);
    }
    if object.kind == ObjectKind::Font
        && !object
            .properties
            .iter()
            .any(|property| property.kind == PropertyKind::FontName)
    {
        return Err(ValidationError::MissingFontName);
    }
    if object.kind == ObjectKind::Bond {
        let endpoint = |kind| {
            object
                .properties
                .iter()
                .find(|property| property.kind == kind)
                .and_then(|property| {
                    if let PropertyValue::ObjectReference(id) = &property.value {
                        Some(*id)
                    } else {
                        None
                    }
                })
        };
        let begin =
            endpoint(PropertyKind::BondBegin).ok_or(ValidationError::MissingBondEndpoint)?;
        let end = endpoint(PropertyKind::BondEnd).ok_or(ValidationError::MissingBondEndpoint)?;
        if begin == end {
            return Err(ValidationError::SelfBond);
        }
    }
    if object.kind == ObjectKind::Graphic
        && !object
            .properties
            .iter()
            .any(|property| property.kind == PropertyKind::BoundingBox)
    {
        return Err(ValidationError::MissingGraphicBounds);
    }
    if object.kind == ObjectKind::Represents
        && [
            PropertyKind::RepresentObject,
            PropertyKind::RepresentAttribute,
        ]
        .iter()
        .any(|kind| {
            !object
                .properties
                .iter()
                .any(|property| &property.kind == kind)
        })
    {
        return Err(ValidationError::MissingRepresentProperty);
    }
    if object.kind == ObjectKind::Text
        && !object.children.iter().any(|child| {
            matches!(
                child,
                Content::Object(Object {
                    kind: ObjectKind::StyleRun,
                    ..
                })
            )
        })
    {
        return Err(ValidationError::MissingTextStyle);
    }
    for (index, child) in object.children.iter().enumerate() {
        if let Content::Object(child) = child {
            if !allowed_child(&object.kind, &child.kind) {
                return Err(ValidationError::InvalidContainment);
            }
            path.push(index);
            inspect_object(child, path, fragment_path.as_deref(), facts)?;
            path.pop();
        }
    }
    Ok(())
}

fn allowed_child(parent: &ObjectKind, child: &ObjectKind) -> bool {
    use ObjectKind as O;
    if matches!(parent, O::UnknownXml(_) | O::UnknownBinary(_)) {
        return matches!(child, O::UnknownXml(_) | O::UnknownBinary(_));
    }
    if matches!(child, O::UnknownXml(_) | O::UnknownBinary(_)) {
        return true;
    }
    match parent {
        O::Document => matches!(child, O::Page | O::FontTable | O::ColorTable),
        O::Page | O::Group => matches!(child, O::Group | O::Fragment | O::Text | O::Graphic),
        O::Fragment => matches!(child, O::Node | O::Bond | O::Graphic),
        O::Node => matches!(child, O::Fragment | O::Text),
        O::Graphic => *child == O::Represents,
        O::Text => *child == O::StyleRun,
        O::FontTable => *child == O::Font,
        O::ColorTable => *child == O::Color,
        _ => false,
    }
}

fn discard_opaque_from(object: &mut Object) -> usize {
    let before = object.properties.len();
    object.properties.retain(|property| {
        !matches!(
            property.kind,
            PropertyKind::UnknownXml(_) | PropertyKind::UnknownBinary(_)
        )
    });
    let mut removed = before - object.properties.len();
    object.children.retain_mut(|child| match child {
        Content::Object(child)
            if matches!(
                child.kind,
                ObjectKind::UnknownXml(_) | ObjectKind::UnknownBinary(_)
            ) =>
        {
            removed += 1;
            false
        }
        Content::Object(child) => {
            removed += discard_opaque_from(child);
            true
        }
        _ => true,
    });
    removed
}

fn valid_property(property: &Property) -> bool {
    use PropertyKind as K;
    use PropertyValue as V;
    match (&property.kind, &property.value) {
        (K::Position, V::Point { .. }) | (K::BoundingBox, V::Rectangle { .. }) => true,
        // The public CDX enumeration uses the same values for its XML names.
        (K::NodeType, V::Integer(value)) => (0..=13).contains(value),
        (K::GraphicType, V::Integer(value)) => (0..=7).contains(value),
        (K::AtomicNumber, V::Integer(value)) => (1..=118).contains(value),
        (K::Charge, V::Integer(value)) => (-128..=127).contains(value),
        (K::Isotope, V::Integer(value)) => (0..=i64::from(i16::MAX)).contains(value),
        (K::Radical, V::Integer(value)) => (0..=255).contains(value),
        (K::BondOrder, V::Integer(value)) => (1..=i64::from(u16::MAX)).contains(value),
        (K::BondBegin | K::BondEnd | K::RepresentObject, V::ObjectReference(_)) => true,
        (K::FontIndex | K::FontFace | K::ColorIndex, V::Integer(value)) => {
            (0..=i64::from(u16::MAX)).contains(value)
        }
        (K::FontSize, V::Float(value)) => *value >= 0.0,
        (K::Red | K::Green | K::Blue, V::Float(value)) => (0.0..=1.0).contains(value),
        (K::XmlNamespace, V::Text(value)) => !value.is_empty(),
        (K::FontName | K::FontCharset | K::RepresentAttribute | K::UnknownXml(_), V::Text(_)) => {
            true
        }
        (K::UnknownBinary(_), V::Bytes(_)) => true,
        _ => false,
    }
}

fn property_allowed(object: &ObjectKind, property: &PropertyKind) -> bool {
    use ObjectKind as O;
    use PropertyKind as P;
    match property {
        P::UnknownXml(_) | P::UnknownBinary(_) => true,
        P::XmlNamespace => *object == O::Document,
        P::BoundingBox => matches!(
            object,
            O::Document | O::Page | O::Group | O::Fragment | O::Text | O::Graphic
        ),
        P::Position => matches!(object, O::Node | O::Text),
        P::NodeType | P::AtomicNumber | P::Charge | P::Isotope | P::Radical => *object == O::Node,
        P::BondOrder | P::BondBegin | P::BondEnd => *object == O::Bond,
        P::GraphicType => *object == O::Graphic,
        P::FontIndex => matches!(object, O::Font | O::StyleRun),
        P::FontName | P::FontCharset => *object == O::Font,
        P::FontFace | P::FontSize | P::ColorIndex => *object == O::StyleRun,
        P::Red | P::Green | P::Blue => *object == O::Color,
        P::RepresentObject | P::RepresentAttribute => *object == O::Represents,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(kind: ObjectKind, id: Option<u32>, children: Vec<Content>) -> Object {
        Object {
            kind,
            id,
            properties: Vec::new(),
            children,
        }
    }

    fn tree() -> Object {
        let atom = |id| Content::Object(object(ObjectKind::Node, Some(id), Vec::new()));
        let mut bond = object(ObjectKind::Bond, Some(5), Vec::new());
        bond.properties = vec![
            Property {
                kind: PropertyKind::BondBegin,
                value: PropertyValue::ObjectReference(3),
            },
            Property {
                kind: PropertyKind::BondEnd,
                value: PropertyValue::ObjectReference(4),
            },
        ];
        object(
            ObjectKind::Document,
            None,
            vec![Content::Object(object(
                ObjectKind::Page,
                Some(1),
                vec![
                    Content::Object(object(
                        ObjectKind::Fragment,
                        Some(2),
                        vec![atom(3), atom(4), Content::Object(bond)],
                    )),
                    Content::Object(object(
                        ObjectKind::Text,
                        Some(6),
                        vec![Content::Object(object(
                            ObjectKind::StyleRun,
                            None,
                            vec![Content::Text("α β".into())],
                        ))],
                    )),
                ],
            ))],
        )
    }

    #[test]
    fn keeps_ordered_pages_graph_and_unicode_styled_text() {
        let document = Document::new(tree()).unwrap();
        assert_eq!(document.root().children.len(), 1);
        let Content::Object(page) = &document.root().children[0] else {
            panic!("page expected")
        };
        assert_eq!(page.kind, ObjectKind::Page);
        assert_eq!(page.children.len(), 2);
        let Content::Object(text) = &page.children[1] else {
            panic!("text expected")
        };
        let Content::Object(run) = &text.children[0] else {
            panic!("style expected")
        };
        assert_eq!(run.children, vec![Content::Text("α β".into())]);
    }

    #[test]
    fn rejects_duplicate_and_dangling_references() {
        let mut duplicate = tree();
        let Content::Object(page) = &mut duplicate.children[0] else {
            panic!()
        };
        page.id = Some(3);
        assert_eq!(
            Document::new(duplicate).unwrap_err(),
            ValidationError::DuplicateId(3)
        );
        let mut dangling = tree();
        let Content::Object(page) = &mut dangling.children[0] else {
            panic!()
        };
        let Content::Object(fragment) = &mut page.children[0] else {
            panic!()
        };
        let Content::Object(bond) = &mut fragment.children[2] else {
            panic!()
        };
        bond.properties[1].value = PropertyValue::ObjectReference(999);
        assert_eq!(
            Document::new(dangling).unwrap_err(),
            ValidationError::DanglingReference(999)
        );
    }

    #[test]
    fn opaque_content_refuses_id_rewrites_and_unacknowledged_removal() {
        let mut root = tree();
        root.properties.push(Property {
            kind: PropertyKind::UnknownXml(XmlName {
                namespace: None,
                local: "future".into(),
            }),
            value: PropertyValue::Text("possibly refers to 1".into()),
        });
        let mut doc = Document::new(root).unwrap();
        let before = doc.clone();
        assert_eq!(
            doc.edit(|root| root.properties.clear()),
            Err(ValidationError::OpaqueContentChanged)
        );
        assert_eq!(doc, before);
        let mut doc = Document::new({
            let mut root = tree();
            root.properties.push(Property {
                kind: PropertyKind::UnknownBinary(42),
                value: PropertyValue::Bytes(vec![1, 2]),
            });
            root
        })
        .unwrap();
        let before = doc.clone();
        assert_eq!(
            doc.edit(|root| {
                let Content::Object(page) = &mut root.children[0] else {
                    panic!()
                };
                page.id = Some(7);
            }),
            Err(ValidationError::UnsafeIdRewrite)
        );
        assert_eq!(doc, before);
    }

    #[test]
    fn newly_added_opaque_content_establishes_its_own_baseline() {
        let mut document = Document::new(tree()).unwrap();
        document
            .edit(|root| {
                root.properties.push(Property {
                    kind: PropertyKind::UnknownXml(XmlName {
                        namespace: None,
                        local: "future".into(),
                    }),
                    value: PropertyValue::Text("may refer to page 1".into()),
                })
            })
            .unwrap();
        let before = document.clone();
        assert_eq!(
            document.edit(|root| {
                let Content::Object(page) = &mut root.children[0] else {
                    panic!()
                };
                page.id = Some(7);
            }),
            Err(ValidationError::UnsafeIdRewrite)
        );
        assert_eq!(document, before);
        assert_eq!(
            document.edit(|root| root.properties.clear()),
            Err(ValidationError::OpaqueContentChanged)
        );
        assert_eq!(document, before);
    }

    #[test]
    fn explicit_opaque_discard_reports_count_and_allows_id_changes() {
        let mut root = tree();
        root.properties.push(Property {
            kind: PropertyKind::UnknownBinary(42),
            value: PropertyValue::Bytes(vec![1, 2]),
        });
        let mut doc = Document::new(root).unwrap();
        assert_eq!(doc.discard_opaque(), Ok(1));
        doc.edit(|root| {
            let Content::Object(page) = &mut root.children[0] else {
                panic!()
            };
            page.id = Some(7);
        })
        .unwrap();
        assert_eq!(doc.validate(), Ok(()));
    }
}
