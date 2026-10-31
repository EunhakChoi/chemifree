//! Loss-accounted projection from a rich interchange tree into one editable graph.
//!
//! CDXML/CDX trees remain available separately. This projection names one page and one
//! top-level fragment; it never reuses their IDs as native IDs or treats unknown node types
//! as ordinary atoms. Source paths remain stable within the validated source tree.

use std::collections::HashMap;

use chemifree_domain::document::{Atom, AtomRef, BondOrder, Command, NewAtomPositions, Point};
use chemifree_domain::element::Element;
use chemifree_domain::interchange::{
    Content, Document, Object, ObjectKind, PropertyKind, PropertyValue,
};
use chemifree_domain::states::{AtomKind, Radical};
use serde::Serialize;

use crate::interchange_codec::InterchangeError;
use crate::{Loss, LossDisposition, SemanticImpact};

#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct SourceObject {
    pub path: String,
    pub id: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct ProjectedAtom {
    pub source: SourceObject,
    pub atomic_number: u8,
    pub position: [f64; 2],
    pub charge: i8,
    pub isotope: Option<u16>,
    pub radical: u8,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct ProjectedBond {
    pub source: SourceObject,
    /// Indices into `atoms`, not source IDs.
    pub begin: usize,
    pub end: usize,
    pub order: u16,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct Projection {
    pub document: SourceObject,
    pub page: SourceObject,
    pub fragment: SourceObject,
    pub atoms: Vec<ProjectedAtom>,
    pub bonds: Vec<ProjectedBond>,
    pub losses: Vec<Loss>,
}

/// A selectable top-level fragment. Paths distinguish the source locations in a loss report.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct Choice {
    pub page_id: u32,
    pub fragment_id: u32,
    pub page_path: String,
    pub fragment_path: String,
}

pub(crate) fn choices(document: &Document) -> Vec<Choice> {
    fn fragments(
        page_id: u32,
        page_path: &[usize],
        object: &Object,
        indices: &mut Vec<usize>,
        choices: &mut Vec<Choice>,
    ) {
        for (index, child) in object.children.iter().enumerate() {
            let Content::Object(child) = child else {
                continue;
            };
            indices.push(index);
            if child.kind == ObjectKind::Fragment {
                if let Some(fragment_id) = child.id {
                    choices.push(Choice {
                        page_id,
                        fragment_id,
                        page_path: path(page_path),
                        fragment_path: path(indices),
                    });
                }
            } else if child.kind == ObjectKind::Group {
                fragments(page_id, page_path, child, indices, choices);
            }
            indices.pop();
        }
    }

    let mut result = Vec::new();
    for (index, child) in document.root().children.iter().enumerate() {
        let Content::Object(page) = child else {
            continue;
        };
        if page.kind == ObjectKind::Page
            && let Some(page_id) = page.id
        {
            let page_path = vec![index];
            fragments(
                page_id,
                &page_path,
                page,
                &mut page_path.clone(),
                &mut result,
            );
        }
    }
    result
}

fn fail(code: &'static str, message: impl Into<String>) -> InterchangeError {
    InterchangeError {
        code,
        message: message.into(),
        line: None,
        column: None,
    }
}

fn path(indices: &[usize]) -> String {
    let mut result = "root".to_owned();
    for index in indices {
        result.push_str(&format!(".children[{index}]"));
    }
    result
}

fn source(object: &Object, indices: &[usize]) -> SourceObject {
    SourceObject {
        path: path(indices),
        id: object.id,
    }
}

fn dropped(path: String, feature: &str, impact: SemanticImpact, reason: &str) -> Loss {
    Loss {
        feature: feature.into(),
        source_path: path,
        disposition: LossDisposition::Dropped,
        reason: reason.into(),
        semantic_impact: impact,
    }
}

fn property<'a>(object: &'a Object, kind: &PropertyKind) -> Option<&'a PropertyValue> {
    object
        .properties
        .iter()
        .find(|entry| &entry.kind == kind)
        .map(|entry| &entry.value)
}

fn integer(object: &Object, kind: &PropertyKind, default: i64) -> Result<i64, InterchangeError> {
    match property(object, kind) {
        None => Ok(default),
        Some(PropertyValue::Integer(value)) => Ok(*value),
        _ => Err(fail(
            "invalid_document",
            format!("Property {kind:?} has the wrong type."),
        )),
    }
}

fn reference(object: &Object, kind: &PropertyKind) -> Result<u32, InterchangeError> {
    match property(object, kind) {
        Some(PropertyValue::ObjectReference(value)) => Ok(*value),
        _ => Err(fail(
            "invalid_document",
            format!("Bond lacks a valid {kind:?} reference."),
        )),
    }
}

fn has_opaque_property(object: &Object) -> bool {
    object.properties.iter().any(|property| {
        matches!(
            property.kind,
            PropertyKind::UnknownXml(_) | PropertyKind::UnknownBinary(_)
        )
    })
}

/// Finds a selected page and a top-level fragment below that page or one of its groups.
fn selection(
    document: &Document,
    page_id: u32,
    fragment_id: u32,
) -> Result<(Vec<usize>, Vec<usize>), InterchangeError> {
    let root = document.root();
    let page_index = root.children.iter().position(|child| matches!(child, Content::Object(object) if object.kind == ObjectKind::Page && object.id == Some(page_id)))
        .ok_or_else(|| fail("page_not_found", "No page has the requested ID."))?;
    let Content::Object(page) = &root.children[page_index] else {
        unreachable!("matched object")
    };
    fn visit(object: &Object, indices: &mut Vec<usize>, id: u32) -> Option<Vec<usize>> {
        for (index, child) in object.children.iter().enumerate() {
            let Content::Object(child) = child else {
                continue;
            };
            indices.push(index);
            if child.kind == ObjectKind::Fragment && child.id == Some(id) {
                return Some(indices.clone());
            }
            if child.kind == ObjectKind::Group
                && let Some(found) = visit(child, indices, id)
            {
                return Some(found);
            }
            indices.pop();
        }
        None
    }
    let page_path = vec![page_index];
    let fragment_path = visit(page, &mut page_path.clone(), fragment_id).ok_or_else(|| {
        fail(
            "fragment_not_found",
            "No top-level fragment on this page has the requested ID.",
        )
    })?;
    Ok((page_path, fragment_path))
}

fn at<'a>(root: &'a Object, indices: &[usize]) -> &'a Object {
    indices.iter().fold(root, |object, index| {
        let Content::Object(child) = &object.children[*index] else {
            unreachable!("selection paths name objects")
        };
        child
    })
}

fn atom(object: &Object, indices: &[usize]) -> Result<ProjectedAtom, InterchangeError> {
    if has_opaque_property(object) {
        return Err(fail(
            "unsupported_node",
            format!(
                "{} has an uninterpreted attribute that may affect chemistry.",
                path(indices)
            ),
        ));
    }
    if object
        .children
        .iter()
        .any(|child| matches!(child, Content::Object(_) | Content::Text(_)))
    {
        return Err(fail(
            "unsupported_node",
            format!(
                "{} has a label or child object whose chemical interpretation is not established.",
                path(indices)
            ),
        ));
    }
    let node_type = integer(object, &PropertyKind::NodeType, 1)?;
    if node_type != 1 {
        return Err(fail(
            "unsupported_node",
            format!("{} is not an element node.", path(indices)),
        ));
    }
    let atomic_number = u8::try_from(integer(object, &PropertyKind::AtomicNumber, 6)?)
        .ok()
        .and_then(Element::from_atomic_number)
        .ok_or_else(|| {
            fail(
                "unsupported_element",
                format!("{} has an invalid element.", path(indices)),
            )
        })?
        .atomic_number();
    let charge = i8::try_from(integer(object, &PropertyKind::Charge, 0)?).map_err(|_| {
        fail(
            "unsupported_charge",
            format!("{} has an invalid charge.", path(indices)),
        )
    })?;
    let mass = integer(object, &PropertyKind::Isotope, 0)?;
    let isotope = if mass == 0 {
        None
    } else {
        Some(u16::try_from(mass).map_err(|_| {
            fail(
                "unsupported_isotope",
                "Isotope is outside the editor range.",
            )
        })?)
    };
    let radical = u8::try_from(integer(object, &PropertyKind::Radical, 0)?)
        .ok()
        .filter(|value| *value <= 3)
        .ok_or_else(|| {
            fail(
                "unsupported_radical",
                format!("{} has an unknown radical.", path(indices)),
            )
        })?;
    let position = match property(object, &PropertyKind::Position) {
        Some(PropertyValue::Point { x, y }) => [*x, *y],
        None => {
            return Err(fail(
                "missing_position",
                format!(
                    "{} has no position for the editor projection.",
                    path(indices)
                ),
            ));
        }
        _ => return Err(fail("invalid_document", "Node position is not a point.")),
    };
    Point::new(position[0], position[1]).map_err(|_| {
        fail(
            "coordinate_out_of_range",
            format!("{} cannot be placed in the editor.", path(indices)),
        )
    })?;
    Ok(ProjectedAtom {
        source: source(object, indices),
        atomic_number,
        position,
        charge,
        isotope,
        radical,
    })
}

fn bond(
    object: &Object,
    indices: &[usize],
    ids: &HashMap<u32, usize>,
) -> Result<ProjectedBond, InterchangeError> {
    if has_opaque_property(object) {
        return Err(fail(
            "unsupported_bond",
            format!(
                "{} has an uninterpreted attribute that may affect chemistry.",
                path(indices)
            ),
        ));
    }
    if object
        .children
        .iter()
        .any(|child| matches!(child, Content::Object(_) | Content::Text(_)))
    {
        return Err(fail(
            "unsupported_bond",
            format!(
                "{} has child content whose chemical interpretation is not established.",
                path(indices)
            ),
        ));
    }
    let begin = reference(object, &PropertyKind::BondBegin)?;
    let end = reference(object, &PropertyKind::BondEnd)?;
    let begin = *ids.get(&begin).ok_or_else(|| {
        fail(
            "invalid_reference",
            "Bond begins outside the selected fragment.",
        )
    })?;
    let end = *ids.get(&end).ok_or_else(|| {
        fail(
            "invalid_reference",
            "Bond ends outside the selected fragment.",
        )
    })?;
    let order = integer(object, &PropertyKind::BondOrder, 1)?;
    if order == 4096 {
        return Err(fail(
            "unsupported_bond",
            format!(
                "{} has a dative bond whose positive endpoint cannot be mapped confidently to the editor's donor endpoint.",
                path(indices)
            ),
        ));
    }
    if ![1, 2, 4, 128].contains(&order) {
        return Err(fail(
            "unsupported_bond",
            format!(
                "{} has a bond order or query outside the editor projection.",
                path(indices)
            ),
        ));
    }
    Ok(ProjectedBond {
        source: source(object, indices),
        begin,
        end,
        order: u16::try_from(order).expect("checked order"),
    })
}

/// Reports every unprojected subtree once, and every unprojected property on selected objects.
fn losses_for_tree(
    object: &Object,
    indices: &mut Vec<usize>,
    page: &[usize],
    fragment: &[usize],
    losses: &mut Vec<Loss>,
) {
    let selected_container =
        indices.is_empty() || indices.as_slice() == page || indices.as_slice() == fragment;
    let selected_graph_item = indices.len() == fragment.len() + 1
        && indices.starts_with(fragment)
        && matches!(object.kind, ObjectKind::Node | ObjectKind::Bond);
    let ancestor = fragment.starts_with(indices) || page.starts_with(indices);
    if !selected_container && !selected_graph_item && !ancestor {
        let (feature, impact) = match &object.kind {
            ObjectKind::Page => ("unselected_page", SemanticImpact::Document),
            ObjectKind::Fragment => ("unselected_fragment", SemanticImpact::Chemistry),
            ObjectKind::Text | ObjectKind::StyleRun => ("styled_text", SemanticImpact::Rendering),
            ObjectKind::FontTable
            | ObjectKind::ColorTable
            | ObjectKind::Font
            | ObjectKind::Color => ("style_table", SemanticImpact::Rendering),
            ObjectKind::Graphic => ("graphic", SemanticImpact::Rendering),
            ObjectKind::Represents => ("reference", SemanticImpact::Document),
            ObjectKind::UnknownXml(_) | ObjectKind::UnknownBinary(_) => {
                ("opaque_object", SemanticImpact::Unknown)
            }
            _ => ("unprojected_object", SemanticImpact::Document),
        };
        losses.push(dropped(
            path(indices),
            feature,
            impact,
            "This object and its contents are not represented in the editor projection.",
        ));
        return;
    }
    if object.kind == ObjectKind::Group {
        losses.push(dropped(
            path(indices),
            "group",
            SemanticImpact::Document,
            "The selected graph is projected without its source group.",
        ));
    }
    for (index, item) in object.properties.iter().enumerate() {
        let consumed = match &object.kind {
            ObjectKind::Node if selected_graph_item => matches!(
                item.kind,
                PropertyKind::Position
                    | PropertyKind::NodeType
                    | PropertyKind::AtomicNumber
                    | PropertyKind::Charge
                    | PropertyKind::Isotope
                    | PropertyKind::Radical
            ),
            ObjectKind::Bond if selected_graph_item => matches!(
                item.kind,
                PropertyKind::BondBegin | PropertyKind::BondEnd | PropertyKind::BondOrder
            ),
            _ => false,
        };
        if !consumed {
            let impact = if matches!(
                item.kind,
                PropertyKind::UnknownXml(_) | PropertyKind::UnknownBinary(_)
            ) {
                SemanticImpact::Unknown
            } else {
                SemanticImpact::Document
            };
            losses.push(dropped(
                format!("{}.properties[{index}]", path(indices)),
                "unprojected_property",
                impact,
                "This source property is not stored in the editor.",
            ));
        }
    }
    for (index, child) in object.children.iter().enumerate() {
        indices.push(index);
        match child {
            Content::Object(child) => losses_for_tree(child, indices, page, fragment, losses),
            Content::Text(_) | Content::Comment(_) | Content::ProcessingInstruction { .. } => {
                losses.push(dropped(
                    path(indices),
                    "source_text",
                    SemanticImpact::Document,
                    "This source text or markup is not stored in the editor.",
                ))
            }
        }
        indices.pop();
    }
}

pub(crate) fn project(
    document: &Document,
    page_id: u32,
    fragment_id: u32,
) -> Result<Projection, InterchangeError> {
    document
        .validate()
        .map_err(|error| fail("invalid_document", error.to_string()))?;
    let (page_path, fragment_path) = selection(document, page_id, fragment_id)?;
    let root = document.root();
    let selected = at(root, &fragment_path);
    if has_opaque_property(selected) || selected.children.iter().any(|child| matches!(child,
        Content::Object(child) if matches!(child.kind, ObjectKind::UnknownXml(_) | ObjectKind::UnknownBinary(_)))) {
        return Err(fail(
            "unsupported_fragment",
            format!("{} has unrecognized molecular attributes or child objects.", path(&fragment_path)),
        ));
    }
    let mut losses = Vec::new();
    let mut atoms = Vec::new();
    let mut ids = HashMap::new();
    for (index, child) in selected.children.iter().enumerate() {
        let Content::Object(node) = child else {
            continue;
        };
        if node.kind != ObjectKind::Node {
            continue;
        }
        let mut indices = fragment_path.clone();
        indices.push(index);
        let item = atom(node, &indices)?;
        if let Some(id) = node.id {
            ids.insert(id, atoms.len());
        }
        atoms.push(item);
    }
    let mut bonds = Vec::new();
    for (index, child) in selected.children.iter().enumerate() {
        let Content::Object(item) = child else {
            continue;
        };
        if item.kind != ObjectKind::Bond {
            continue;
        }
        let mut indices = fragment_path.clone();
        indices.push(index);
        let projected = bond(item, &indices, &ids)?;
        if projected.order == 2 {
            losses.push(Loss {
                feature: "double_bond_stereo".into(),
                source_path: path(&indices),
                disposition: LossDisposition::Unknown,
                reason: "The editor does not interpret double-bond stereochemistry that another reader may derive from the drawing.".into(),
                semantic_impact: SemanticImpact::Stereo,
            });
        }
        bonds.push(projected);
    }
    losses_for_tree(
        root,
        &mut Vec::new(),
        &page_path,
        &fragment_path,
        &mut losses,
    );
    if !atoms.is_empty() {
        losses.push(Loss {
            feature: "implicit_hydrogen_interpretation".into(),
            source_path: path(&fragment_path),
            disposition: LossDisposition::Unknown,
            reason: "The editor records inferred hydrogen states; its bounded valence inspection may differ from the source reader's implicit-hydrogen policy.".into(),
            semantic_impact: SemanticImpact::Chemistry,
        });
    }
    Ok(Projection {
        document: source(root, &[]),
        page: source(at(root, &page_path), &page_path),
        fragment: source(selected, &fragment_path),
        atoms,
        bonds,
        losses,
    })
}

impl Projection {
    /// Construct the exact transaction that a preview or creation validates.
    pub(crate) fn commands(&self) -> Result<Vec<Command>, InterchangeError> {
        let mut commands = Vec::with_capacity(self.atoms.len() + self.bonds.len());
        for item in &self.atoms {
            let element = Element::from_atomic_number(item.atomic_number)
                .ok_or_else(|| fail("unsupported_element", "Element is invalid."))?;
            let mut atom = Atom::new(AtomKind::Element(element));
            atom.charge = item.charge;
            atom.isotope = item.isotope;
            atom.radical = match item.radical {
                0 => Radical::None,
                1 => Radical::Singlet,
                2 => Radical::Doublet,
                3 => Radical::Triplet,
                _ => return Err(fail("unsupported_radical", "Radical is invalid.")),
            };
            let point = Point::new(item.position[0], item.position[1]).map_err(|_| {
                fail(
                    "coordinate_out_of_range",
                    "Position is outside the editor range.",
                )
            })?;
            commands.push(Command::AddAtom {
                atom,
                positions: NewAtomPositions::Single(point),
                bond_to: None,
            });
        }
        for item in &self.bonds {
            let order = match item.order {
                1 => BondOrder::Single,
                2 => BondOrder::Double,
                4 => BondOrder::Triple,
                128 => BondOrder::Aromatic,
                _ => return Err(fail("unsupported_bond", "Bond order is invalid.")),
            };
            commands.push(Command::AddBond {
                begin: AtomRef::Created(item.begin),
                end: AtomRef::Created(item.end),
                order,
            });
        }
        Ok(commands)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chemifree_domain::interchange::{Property, XmlName};

    fn object(
        kind: ObjectKind,
        id: Option<u32>,
        properties: Vec<Property>,
        children: Vec<Content>,
    ) -> Content {
        Content::Object(Object {
            kind,
            id,
            properties,
            children,
        })
    }
    fn item(kind: PropertyKind, value: PropertyValue) -> Property {
        Property { kind, value }
    }
    fn fixture(missing_position: bool, unknown_type: bool) -> Document {
        let mut carbon = vec![];
        if !missing_position {
            carbon.push(item(
                PropertyKind::Position,
                PropertyValue::Point { x: 1., y: 2. },
            ));
        }
        if unknown_type {
            carbon.push(item(
                PropertyKind::UnknownXml(XmlName {
                    namespace: None,
                    local: "NodeType".into(),
                }),
                PropertyValue::Text("FutureType".into()),
            ));
        }
        let oxygen = vec![
            item(
                PropertyKind::Position,
                PropertyValue::Point { x: 3., y: 4. },
            ),
            item(PropertyKind::AtomicNumber, PropertyValue::Integer(8)),
            item(PropertyKind::Charge, PropertyValue::Integer(-1)),
            item(PropertyKind::Isotope, PropertyValue::Integer(18)),
        ];
        let bond = vec![
            item(PropertyKind::BondBegin, PropertyValue::ObjectReference(3)),
            item(PropertyKind::BondEnd, PropertyValue::ObjectReference(4)),
            item(PropertyKind::BondOrder, PropertyValue::Integer(2)),
        ];
        let root = Object {
            kind: ObjectKind::Document,
            id: None,
            properties: vec![],
            children: vec![
                object(
                    ObjectKind::Page,
                    Some(1),
                    vec![],
                    vec![
                        object(
                            ObjectKind::Fragment,
                            Some(2),
                            vec![],
                            vec![
                                object(ObjectKind::Node, Some(3), carbon, vec![]),
                                object(ObjectKind::Node, Some(4), oxygen, vec![]),
                                object(ObjectKind::Bond, Some(5), bond, vec![]),
                            ],
                        ),
                        object(
                            ObjectKind::Text,
                            Some(6),
                            vec![],
                            vec![object(
                                ObjectKind::StyleRun,
                                None,
                                vec![],
                                vec![Content::Text("α & β".into())],
                            )],
                        ),
                    ],
                ),
                object(
                    ObjectKind::Page,
                    Some(7),
                    vec![],
                    vec![object(
                        ObjectKind::Fragment,
                        Some(8),
                        vec![],
                        vec![object(ObjectKind::Node, Some(9), vec![], vec![])],
                    )],
                ),
            ],
        };
        Document::new(root).unwrap()
    }

    #[test]
    fn selected_graph_is_mapped_and_only_unrepresented_features_are_lost() {
        let projection = project(&fixture(false, false), 1, 2).unwrap();
        assert_eq!(
            (
                projection.document.id,
                projection.page.id,
                projection.fragment.id
            ),
            (None, Some(1), Some(2))
        );
        assert_eq!(
            projection
                .atoms
                .iter()
                .map(|atom| (
                    atom.source.id,
                    atom.atomic_number,
                    atom.charge,
                    atom.isotope,
                    atom.position
                ))
                .collect::<Vec<_>>(),
            [
                (Some(3), 6, 0, None, [1., 2.]),
                (Some(4), 8, -1, Some(18), [3., 4.])
            ]
        );
        assert_eq!(
            (
                projection.bonds[0].source.id,
                projection.bonds[0].begin,
                projection.bonds[0].end,
                projection.bonds[0].order
            ),
            (Some(5), 0, 1, 2)
        );
        assert_eq!(
            projection
                .losses
                .iter()
                .map(|loss| loss.feature.as_str())
                .collect::<Vec<_>>(),
            [
                "double_bond_stereo",
                "styled_text",
                "unselected_page",
                "implicit_hydrogen_interpretation"
            ]
        );
        assert_eq!(projection.commands().unwrap().len(), 3);
    }

    #[test]
    fn missing_positions_and_unknown_node_types_are_refused() {
        assert_eq!(
            project(&fixture(true, false), 1, 2).unwrap_err().code,
            "missing_position"
        );
        assert_eq!(
            project(&fixture(false, true), 1, 2).unwrap_err().code,
            "unsupported_node"
        );
    }
}
