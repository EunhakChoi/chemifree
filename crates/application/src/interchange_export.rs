//! Projection of the editable drawing into a bounded, loss-accounted CDXML/CDX tree.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use chemifree_domain::document::{Atom, AtomId, Bond, BondId, BondOrder, Document, Point};
use chemifree_domain::interchange::{
    Content, Document as InterchangeDocument, Object, ObjectKind, Property, PropertyKind,
    PropertyValue,
};
use chemifree_domain::states::{AtomKind, Hydrogens};
use chemifree_domain::stereo::{
    BondEnd, CenterState, StereoKey, StereoMark, StereoState, StereoTable,
};
use chemifree_domain::stereo_drawing::Drawing;
use chemifree_domain::stereo_group::GroupKind;

use crate::contract::{DocumentResponse, DocumentView, HydrogensDto, RadicalDto, SceneObjectView};
use crate::interchange_codec::{InterchangeError, InterchangeFormat};
use crate::stereo_projection;
use crate::{Loss, LossConsent, LossDisposition, SemanticImpact};

/// Only the current graph, configurations and scene are captured; undo history is not copied.
pub(crate) struct ExportSnapshot {
    view: DocumentView,
    atoms: BTreeMap<AtomId, Atom>,
    bonds: BTreeMap<BondId, Bond>,
    stereo: StereoTable,
}

impl ExportSnapshot {
    pub(crate) fn capture(document: &Document) -> Self {
        Self {
            view: DocumentResponse::new(document, 0, None).document,
            atoms: document
                .atoms()
                .map(|(id, atom)| (id, atom.clone()))
                .collect(),
            bonds: document
                .bonds()
                .map(|(id, bond)| (id, bond.clone()))
                .collect(),
            stereo: document.stereo().clone(),
        }
    }
}

pub(crate) struct Projection {
    pub(crate) document: InterchangeDocument,
    pub(crate) losses: Vec<Loss>,
}

fn fail(code: &'static str, message: impl Into<String>) -> InterchangeError {
    InterchangeError {
        code,
        message: message.into(),
        line: None,
        column: None,
        offset: None,
    }
}

fn loss(path: impl Into<String>, feature: &str, impact: SemanticImpact, reason: &str) -> Loss {
    Loss {
        consent: if feature == "caption" {
            LossConsent::Required
        } else {
            LossConsent::NotRequired
        },
        feature: feature.into(),
        source_path: path.into(),
        disposition: LossDisposition::Dropped,
        reason: reason.into(),
        semantic_impact: impact,
    }
}

fn stereo_loss(path: impl Into<String>, feature: &str, reason: &str) -> Loss {
    Loss {
        consent: LossConsent::Required,
        ..loss(path, feature, SemanticImpact::Stereo, reason)
    }
}

fn object(kind: ObjectKind, id: u32) -> Object {
    Object {
        kind,
        id: Some(id),
        properties: Vec::new(),
        children: Vec::new(),
    }
}

fn integer(kind: PropertyKind, value: i64) -> Property {
    Property {
        kind,
        value: PropertyValue::Integer(value),
    }
}

fn written_point(point: Point, format: InterchangeFormat) -> Result<Point, InterchangeError> {
    if format == InterchangeFormat::Cdxml {
        return Ok(point);
    }
    let quantize = |value: f64| {
        let scaled = (value * 65536.0).round();
        if scaled < f64::from(i32::MIN) || scaled > f64::from(i32::MAX) {
            Err(fail(
                "out_of_range",
                "A position is outside the CDX fixed-point coordinate range.",
            ))
        } else {
            Ok(scaled / 65536.0)
        }
    };
    Point::new(quantize(point.x())?, quantize(point.y())?)
        .map_err(|cause| fail("coordinate_out_of_range", cause.to_string()))
}

fn drawing_losses(view: &DocumentView) -> Vec<Loss> {
    let mut losses = Vec::new();
    let mut push =
        |path: String, feature, impact, reason| losses.push(loss(path, feature, impact, reason));
    if !view.title.is_empty() {
        push(
            "document.title".into(),
            "title",
            SemanticImpact::Document,
            "The drawing title is not written by this bounded export profile.",
        );
    }
    if !view.atoms.is_empty() || !view.bonds.is_empty() {
        push(
            "document.ids".into(),
            "native_ids",
            SemanticImpact::Document,
            "Native graph IDs are mapped to file object IDs and are not embedded.",
        );
    }
    for (index, instance) in view.scene.instances.iter().enumerate() {
        if index == 0 {
            push(
                format!("document.scene.instances[{}]", instance.id),
                "placement_id",
                SemanticImpact::Document,
                "The native placement ID is not embedded.",
            );
        } else {
            push(
                format!("document.scene.instances[{}]", instance.id),
                "extra_placement",
                SemanticImpact::Geometry,
                "Only the first molecule placement is written.",
            );
        }
    }
    for item in &view.scene.objects {
        match item {
            SceneObjectView::Caption { id, .. } => push(
                format!("document.scene.objects[{id}]"),
                "caption",
                SemanticImpact::Rendering,
                "Independent caption text and script styles are not written by this export profile.",
            ),
            SceneObjectView::Group { id, .. } | SceneObjectView::Graphic { id, .. } => push(
                format!("document.scene.objects[{id}]"),
                "scene_object",
                SemanticImpact::Rendering,
                "Scene groups and graphics are not written by this bounded export profile.",
            ),
            SceneObjectView::Instance { .. } => {}
        }
    }
    if !view.scene.references.is_empty() {
        push(
            "document.scene.references".into(),
            "scene_references",
            SemanticImpact::Document,
            "Scene references are not written.",
        );
    }
    if !view.selection.atoms.is_empty()
        || !view.selection.bonds.is_empty()
        || !view.selection.objects.is_empty()
    {
        push(
            "document.selection".into(),
            "selection",
            SemanticImpact::Document,
            "The editor selection is not written.",
        );
    }
    push(
        "document.settings".into(),
        "drawing_settings",
        SemanticImpact::Rendering,
        "Drawing styles and settings are not written by this bounded export profile.",
    );
    losses
}

/// Writes one page and fragment, preserving the drawn marks. Stored configurations are
/// carried only through those marks and the positions that the output actually stores.
pub(crate) fn project(
    snapshot: &ExportSnapshot,
    format: InterchangeFormat,
) -> Result<Projection, InterchangeError> {
    let view = &snapshot.view;
    let mut losses = drawing_losses(view);
    let mut root = object(ObjectKind::Document, 1);
    let mut page = object(ObjectKind::Page, 2);
    let mut fragment = object(ObjectKind::Fragment, 3);
    let mut ids = HashMap::new();
    let mut positions = Vec::with_capacity(view.atoms.len());
    let mut written_atoms = snapshot.atoms.clone();
    for atom in written_atoms.values_mut() {
        atom.hydrogens = Hydrogens::Inferred;
        atom.attachment_points = Default::default();
    }
    for (index, atom) in view.atoms.iter().enumerate() {
        let id = u32::try_from(index + 4).map_err(|_| fail("resource_limit", "Too many atoms."))?;
        let native = AtomId::parse(&atom.id)
            .ok_or_else(|| fail("invalid_atom", "The snapshot has an invalid atom ID."))?;
        let source = snapshot
            .atoms
            .get(&native)
            .ok_or_else(|| fail("invalid_atom", "The snapshot is missing an atom."))?;
        let AtomKind::Element(element) = source.kind else {
            return Err(fail(
                "unsupported_atom",
                "Query atoms are outside this export profile.",
            ));
        };
        let point = Point::new(atom.position.x, atom.position.y)
            .map_err(|cause| fail("coordinate_out_of_range", cause.to_string()))?;
        let written = written_point(point, format)?;
        positions.push((native, written));
        ids.insert(atom.id.as_str(), (id, index));
        let mut node = object(ObjectKind::Node, id);
        node.properties = vec![
            Property {
                kind: PropertyKind::Position,
                value: PropertyValue::Point {
                    x: point.x(),
                    y: point.y(),
                },
            },
            integer(
                PropertyKind::AtomicNumber,
                i64::from(element.atomic_number()),
            ),
            integer(PropertyKind::Charge, i64::from(atom.charge)),
            integer(
                PropertyKind::Radical,
                match atom.radical {
                    RadicalDto::None => 0,
                    RadicalDto::Singlet => 1,
                    RadicalDto::Doublet => 2,
                    RadicalDto::Triplet => 3,
                    RadicalDto::Unknown => {
                        return Err(fail(
                            "unsupported_radical",
                            "An unknown radical is outside this export profile.",
                        ));
                    }
                },
            ),
        ];
        if let Some(isotope) = atom.isotope {
            node.properties
                .push(integer(PropertyKind::Isotope, i64::from(isotope)));
        }
        if !matches!(atom.hydrogens, HydrogensDto::Inferred {}) {
            let mut item = loss(
                format!("document.atoms[{}].hydrogens", atom.id),
                "hydrogen_state",
                SemanticImpact::Chemistry,
                "The stored hydrogen state is not written; a destination reader may infer another count.",
            );
            item.consent = LossConsent::Required;
            losses.push(item);
        }
        if !atom.attachment_points.is_empty() {
            let mut item = loss(
                format!("document.atoms[{}].attachment_points", atom.id),
                "attachment_points",
                SemanticImpact::Chemistry,
                "Atom attachment points are not written by this bounded export profile.",
            );
            item.consent = LossConsent::Required;
            losses.push(item);
        }
        fragment.children.push(Content::Object(node));
    }
    let drawing = Drawing::new(&written_atoms, &snapshot.bonds, positions);
    let mut mismatched = HashSet::new();
    let grouped: HashSet<&str> = view
        .stereo
        .groups
        .iter()
        .flat_map(|group| group.members.iter().map(String::as_str))
        .collect();
    let mut ungrouped = Vec::new();
    for (key, state) in snapshot.stereo.iter() {
        if let (StereoKey::Center(atom), StereoState::Center(CenterState::Tetrahedral { .. })) =
            (key, state)
            && !grouped.contains(atom.to_string().as_str())
        {
            ungrouped.push(atom.to_string());
        }
        match (key, state) {
            (StereoKey::Center(atom), StereoState::Center(stored)) if drawing.derive_center(atom).ok().as_ref() != Some(stored) => {
                mismatched.insert(atom);
                losses.push(stereo_loss(format!("document.atoms[{atom}]"), "stereo_configuration", "The stored configuration differs from what the written marks and positions determine; only the drawing is written."));
            }
            (StereoKey::DoubleBond(bond), StereoState::DoubleBond(_)) => losses.push(stereo_loss(format!("document.bonds[{bond}]"), "double_bond_configuration", "Stored double-bond configurations are not written or derived by this interchange import profile; the bond and its drawn mark are retained.")),
            _ => {}
        }
    }
    if !ungrouped.is_empty() {
        let mut item = stereo_loss(
            format!("document.atoms[{}]", ungrouped.join(", ")),
            "stereo_ungrouped_centers",
            "These stored tetrahedral centers belong to no enhanced stereo group. The file states no enhanced stereochemistry for them; how another reader treats them is not established by this profile.",
        );
        item.disposition = LossDisposition::Unknown;
        losses.push(item);
    }
    write_groups(
        view,
        &drawing,
        &mismatched,
        &ids,
        &mut fragment,
        &mut losses,
    )?;
    for (index, (native, bond)) in snapshot.bonds.iter().enumerate() {
        let id = u32::try_from(view.atoms.len() + index + 4)
            .map_err(|_| fail("resource_limit", "Too many bonds."))?;
        let order = match bond.order {
            BondOrder::Single => 1,
            BondOrder::Double => 2,
            BondOrder::Triple => 4,
            BondOrder::Aromatic => 128,
            _ => {
                return Err(fail(
                    "unsupported_bond",
                    "Dative, query and unknown bonds are outside this export profile.",
                ));
            }
        };
        let begin = bond.begin.to_string();
        let end = bond.end.to_string();
        let begin = ids
            .get(begin.as_str())
            .ok_or_else(|| fail("invalid_reference", "A bond begins outside the graph."))?
            .0;
        let end = ids
            .get(end.as_str())
            .ok_or_else(|| fail("invalid_reference", "A bond ends outside the graph."))?
            .0;
        let mut item = object(ObjectKind::Bond, id);
        item.properties = vec![
            Property {
                kind: PropertyKind::BondBegin,
                value: PropertyValue::ObjectReference(begin),
            },
            Property {
                kind: PropertyKind::BondEnd,
                value: PropertyValue::ObjectReference(end),
            },
            integer(PropertyKind::BondOrder, order),
        ];
        if let Some(mark) = bond.mark {
            if !mark.fits(&bond.order) {
                return Err(fail(
                    "unsupported_bond",
                    format!("Bond {native} has a mark that does not fit its order."),
                ));
            }
            let number: i64 = match mark {
                StereoMark::Wedge {
                    narrow: BondEnd::Begin,
                } => 6,
                StereoMark::Wedge {
                    narrow: BondEnd::End,
                } => 7,
                StereoMark::Hash {
                    narrow: BondEnd::Begin,
                } => 3,
                StereoMark::Hash {
                    narrow: BondEnd::End,
                } => 4,
                StereoMark::Wavy | StereoMark::Crossed => 8,
            };
            item.properties
                .push(integer(PropertyKind::BondDisplay, number));
        }
        fragment.children.push(Content::Object(item));
    }
    page.children.push(Content::Object(fragment));
    root.children.push(Content::Object(page));
    let document =
        InterchangeDocument::new(root).map_err(|cause| fail(cause.code(), cause.message()))?;
    Ok(Projection { document, losses })
}

fn write_groups(
    view: &DocumentView,
    drawing: &Drawing<'_>,
    mismatched: &HashSet<AtomId>,
    ids: &HashMap<&str, (u32, usize)>,
    fragment: &mut Object,
    losses: &mut Vec<Loss>,
) -> Result<(), InterchangeError> {
    let mut output = Vec::new();
    for group in &view.stereo.groups {
        let readable = group.members.iter().all(|member| {
            AtomId::parse(member).is_some_and(|atom| {
                !mismatched.contains(&atom)
                    && drawing
                        .derive_center(atom)
                        .is_ok_and(|state| stereo_projection::is_tetrahedral(&state))
            }) && ids.contains_key(member.as_str())
        });
        if readable {
            output.push(group);
        } else {
            losses.push(stereo_loss(format!("document.stereo.groups[{}]", group.id), "stereo_group", "The entire group is omitted because at least one member's written drawing does not determine its stored tetrahedral configuration."));
        }
    }
    let mut reserved: BTreeMap<GroupKind, BTreeSet<i32>> = BTreeMap::new();
    for kind in [GroupKind::And, GroupKind::Or] {
        let of_kind: Vec<_> = output
            .iter()
            .filter(|group| GroupKind::from(group.kind) == kind)
            .collect();
        if of_kind.len() > 127 {
            return Err(fail(
                "stereo_group_limit",
                format!(
                    "More than 127 outputtable {} groups cannot be represented by this export profile.",
                    kind.as_str()
                ),
            ));
        }
        reserved.insert(
            kind,
            of_kind
                .iter()
                .filter_map(|group| group.number.filter(|number| (1..=127).contains(number)))
                .collect(),
        );
    }
    if !output.is_empty() {
        let mut display = loss(
            "document.stereo.groups",
            "enhanced_stereo_display",
            SemanticImpact::Rendering,
            "The drawing stores no enhanced-stereo indicator visibility setting. This export explicitly shows indicators for written stereo groups.",
        );
        display.disposition = LossDisposition::Normalized;
        losses.push(display);
    }
    for group in output {
        let kind = GroupKind::from(group.kind);
        let number = match kind {
            GroupKind::Absolute => 2,
            GroupKind::Or => 3,
            GroupKind::And => 4,
        };
        let written_number = if kind == GroupKind::Absolute {
            None
        } else {
            let used = reserved.entry(kind).or_default();
            let stated = group.number.unwrap_or(0);
            if (1..=127).contains(&stated) {
                Some(stated)
            } else {
                let free = (1..=127)
                    .find(|number| !used.contains(number))
                    .ok_or_else(|| {
                        fail(
                            "stereo_group_limit",
                            "No free enhanced stereo group number remains.",
                        )
                    })?;
                used.insert(free);
                let mut item = loss(
                    format!("document.stereo.groups[{}].number", group.id),
                    "stereo_group_number",
                    SemanticImpact::Stereo,
                    &format!(
                        "The {} group number {stated} is written as {free}; its members remain one distinct group.",
                        kind.as_str()
                    ),
                );
                item.disposition = LossDisposition::Normalized;
                losses.push(item);
                Some(free)
            }
        };
        for member in &group.members {
            let (_, index) = ids
                .get(member.as_str())
                .ok_or_else(|| fail("invalid_reference", "A group member is outside the graph."))?;
            let Some(Content::Object(atom)) = fragment.children.get_mut(*index) else {
                return Err(fail("invalid_atom", "A group member has no output atom."));
            };
            atom.properties
                .push(integer(PropertyKind::ShowAtomEnhancedStereo, 1));
            atom.properties
                .push(integer(PropertyKind::EnhancedStereoType, number));
            if let Some(value) = written_number {
                atom.properties.push(integer(
                    PropertyKind::EnhancedStereoGroupNum,
                    i64::from(value),
                ));
            }
        }
        losses.push(loss(format!("document.stereo.groups[{}]", group.id), "stereo_group_identity", SemanticImpact::Document, "Native stereo group IDs are not embedded; kind, members and the output group number are written."));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chemifree_domain::document::{
        AtomRef, BondRef, Command, DocumentId, NewAtomPositions, Vector,
    };
    use chemifree_domain::element::Element;

    fn two_centers() -> Document {
        let carbon = Element::from_symbol("C").unwrap();
        let mut commands = Vec::new();
        for x in [0.0, 100.0] {
            for (dx, y) in [(0.0, 0.0), (-20.0, -10.0), (20.0, -10.0), (0.0, 20.0)] {
                commands.push(Command::AddAtom {
                    atom: Atom::from(carbon),
                    positions: NewAtomPositions::Single(Point::new(x + dx, y).unwrap()),
                    bond_to: None,
                });
            }
        }
        for center in [0, 4] {
            for offset in 1..=3 {
                commands.push(Command::AddBond {
                    begin: AtomRef::Created(center),
                    end: AtomRef::Created(center + offset),
                    order: BondOrder::Single,
                });
            }
        }
        for bond in [8, 11] {
            commands.push(Command::SetBondMark {
                bond: BondRef::Created(bond),
                mark: Some(StereoMark::Wedge {
                    narrow: BondEnd::Begin,
                }),
            });
        }
        commands.push(Command::SetConfigurationFromDrawing {
            atoms: Some(vec![AtomRef::Created(0), AtomRef::Created(4)]),
            bonds: None,
            instance: None,
        });
        let mut document =
            Document::new(DocumentId::parse("export-test").unwrap(), "test").unwrap();
        document.apply(&commands).unwrap();
        document
    }

    fn output_nodes(projection: &Projection) -> Vec<&Object> {
        let Content::Object(page) = &projection.document.root().children[0] else {
            panic!("expected page")
        };
        let Content::Object(fragment) = &page.children[0] else {
            panic!("expected fragment")
        };
        fragment
            .children
            .iter()
            .filter_map(|child| match child {
                Content::Object(node) if node.kind == ObjectKind::Node => Some(node),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn written_groups_show_indicators_with_one_explicit_rendering_normalization() {
        let mut document = two_centers();
        for format in [InterchangeFormat::Cdxml, InterchangeFormat::Cdx] {
            let projection = project(&ExportSnapshot::capture(&document), format).unwrap();
            assert!(
                !projection
                    .losses
                    .iter()
                    .any(|loss| loss.feature == "enhanced_stereo_display"),
                "No group is written, so no indicator visibility is chosen."
            );
            assert!(output_nodes(&projection).iter().all(|node| {
                !node
                    .properties
                    .iter()
                    .any(|property| property.kind == PropertyKind::ShowAtomEnhancedStereo)
            }));
        }
        document
            .apply(&[
                Command::CreateStereoGroup {
                    kind: GroupKind::And,
                    number: Some(1),
                    atoms: vec![AtomRef::Id(AtomId::parse("a1").unwrap())],
                },
                Command::CreateStereoGroup {
                    kind: GroupKind::Or,
                    number: Some(1),
                    atoms: vec![AtomRef::Id(AtomId::parse("a5").unwrap())],
                },
            ])
            .unwrap();
        for format in [InterchangeFormat::Cdxml, InterchangeFormat::Cdx] {
            let projection = project(&ExportSnapshot::capture(&document), format).unwrap();
            let display: Vec<_> = projection
                .losses
                .iter()
                .filter(|loss| loss.feature == "enhanced_stereo_display")
                .collect();
            assert_eq!(display, vec![&Loss {
                feature: "enhanced_stereo_display".into(),
                source_path: "document.stereo.groups".into(),
                disposition: LossDisposition::Normalized,
                reason: "The drawing stores no enhanced-stereo indicator visibility setting. This export explicitly shows indicators for written stereo groups.".into(),
                semantic_impact: SemanticImpact::Rendering,
                consent: LossConsent::NotRequired,
            }], "The display choice is reported once, even for multiple groups.");
            let flags: Vec<_> = output_nodes(&projection)
                .iter()
                .flat_map(|node| {
                    node.properties
                        .iter()
                        .filter(|property| property.kind == PropertyKind::ShowAtomEnhancedStereo)
                        .map(move |property| (node.id, property.value.clone()))
                })
                .collect();
            assert_eq!(
                flags,
                vec![
                    (Some(4), PropertyValue::Integer(1)),
                    (Some(8), PropertyValue::Integer(1)),
                ],
                "Only the two written group members receive the true indicator flag."
            );
        }
    }

    #[test]
    fn renumbering_reserves_later_valid_numbers_before_allocating() {
        let mut document = two_centers();
        document
            .apply(&[
                Command::CreateStereoGroup {
                    kind: GroupKind::And,
                    number: Some(128),
                    atoms: vec![AtomRef::Id(AtomId::parse("a1").unwrap())],
                },
                Command::CreateStereoGroup {
                    kind: GroupKind::And,
                    number: Some(1),
                    atoms: vec![AtomRef::Id(AtomId::parse("a5").unwrap())],
                },
            ])
            .unwrap();
        let projection =
            project(&ExportSnapshot::capture(&document), InterchangeFormat::Cdx).unwrap();
        let nodes = output_nodes(&projection);
        let group_number = |node: &Object| {
            node.properties
                .iter()
                .find(|property| property.kind == PropertyKind::EnhancedStereoGroupNum)
                .map(|property| property.value.clone())
        };
        assert_eq!(group_number(nodes[0]), Some(PropertyValue::Integer(2)));
        assert_eq!(group_number(nodes[4]), Some(PropertyValue::Integer(1)));
        let renumbered: Vec<_> = projection
            .losses
            .iter()
            .filter(|loss| loss.feature == "stereo_group_number")
            .collect();
        assert_eq!(renumbered.len(), 1);
        assert_eq!(renumbered[0].disposition, LossDisposition::Normalized);
        assert_eq!(renumbered[0].consent, LossConsent::NotRequired);
    }

    #[test]
    fn one_unreadable_member_omits_the_whole_group_without_ungrouped_duplicate() {
        let mut document = two_centers();
        document
            .apply(&[Command::CreateStereoGroup {
                kind: GroupKind::Absolute,
                number: None,
                atoms: vec![
                    AtomRef::Id(AtomId::parse("a1").unwrap()),
                    AtomRef::Id(AtomId::parse("a5").unwrap()),
                ],
            }])
            .unwrap();
        let mut snapshot = ExportSnapshot::capture(&document);
        // This is the first center's wedge endpoint: collapse it onto the center.
        snapshot.view.atoms[1].position.x = 0.0;
        snapshot.view.atoms[1].position.y = 0.0;
        let projection = project(&snapshot, InterchangeFormat::Cdxml).unwrap();
        let group_losses: Vec<_> = projection
            .losses
            .iter()
            .filter(|loss| loss.feature == "stereo_group")
            .collect();
        assert_eq!(group_losses.len(), 1);
        assert_eq!(group_losses[0].consent, LossConsent::Required);
        assert_eq!(group_losses[0].disposition, LossDisposition::Dropped);
        assert!(
            !projection
                .losses
                .iter()
                .any(|loss| loss.feature == "enhanced_stereo_display"),
            "An entirely omitted group makes no indicator visibility choice."
        );
        assert!(
            !projection
                .losses
                .iter()
                .any(|loss| loss.feature == "stereo_ungrouped_centers"),
            "The original group already accounts for this cause."
        );
        for node in output_nodes(&projection) {
            assert!(
                !node
                    .properties
                    .iter()
                    .any(|property| property.kind == PropertyKind::EnhancedStereoType),
                "No subset of an omitted group may survive."
            );
        }
    }

    #[test]
    fn cdx_rounding_below_minimum_bond_length_omits_the_entire_group() {
        let mut document = two_centers();
        let center = AtomId::parse("a1").unwrap();
        let other_center = AtomId::parse("a5").unwrap();
        // The diagonal bond is just over 0.001 point long. Rounding each component
        // from 46.375 to 46 CDX units makes its length just under that threshold.
        // Binary fractions make both the movement and the CDX rounding unambiguous.
        let diagonal_component = 46.375 / 65536.0;
        let minimum = chemifree_domain::stereo_drawing::MIN_BOND_LENGTH;
        assert!(2.0 * diagonal_component * diagonal_component > minimum * minimum);
        assert!(2.0 * (46.0 / 65536.0_f64).powi(2) < minimum * minimum);
        document
            .apply(&[
                Command::MoveAtoms {
                    atoms: vec![AtomId::parse("a2").unwrap()],
                    delta: Vector {
                        dx: 20.0 - diagonal_component,
                        dy: 10.0 - diagonal_component,
                    },
                    instance: None,
                },
                Command::SetConfigurationFromDrawing {
                    atoms: Some(vec![AtomRef::Id(center)]),
                    bonds: None,
                    instance: None,
                },
                Command::CreateStereoGroup {
                    kind: GroupKind::Absolute,
                    number: None,
                    atoms: vec![AtomRef::Id(center), AtomRef::Id(other_center)],
                },
            ])
            .unwrap();
        let snapshot = ExportSnapshot::capture(&document);
        let cdxml = project(&snapshot, InterchangeFormat::Cdxml).unwrap();
        let cdx = project(&snapshot, InterchangeFormat::Cdx).unwrap();
        let typed_members = |projection: &Projection| {
            output_nodes(projection)
                .iter()
                .filter(|node| {
                    node.properties
                        .iter()
                        .any(|property| property.kind == PropertyKind::EnhancedStereoType)
                })
                .count()
        };
        assert_eq!(
            typed_members(&cdxml),
            2,
            "Full-precision coordinates retain both members."
        );
        assert_eq!(
            typed_members(&cdx),
            0,
            "The readable member must not survive alone."
        );
        assert!(
            !cdxml
                .losses
                .iter()
                .any(|loss| loss.feature == "stereo_group"),
            "The original drawing determines its stored configurations."
        );
        let group_losses: Vec<_> = cdx
            .losses
            .iter()
            .filter(|loss| loss.feature == "stereo_group")
            .collect();
        assert_eq!(group_losses.len(), 1);
        assert_eq!(group_losses[0].consent, LossConsent::Required);
        assert_eq!(group_losses[0].disposition, LossDisposition::Dropped);
    }
}
