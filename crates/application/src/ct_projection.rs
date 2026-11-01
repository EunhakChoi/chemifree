//! Projection between a CTfile record and the editor's single graph and page.

use std::collections::HashMap;

use chemifree_domain::document::{Atom, AtomRef, BondOrder, Command, NewAtomPositions, Point};
use chemifree_domain::element::Element;
use chemifree_domain::states::{AtomKind, Radical};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::contract::{
    AtomKindView, BondOrderDto, DocumentView, HydrogensDto, RadicalDto, SceneObjectView,
};
use crate::ctfile::{CtAtom, CtBond, CtError, CtField, CtRadical, CtRecord};
use crate::{Loss, LossConsent, LossDisposition, SemanticImpact};

/// V2000 coordinates are in angstroms; the editor's positions are points with y increasing down.
const POINTS_PER_ANGSTROM: f64 = 20.0;

fn projection_error(record: usize, code: &'static str, message: impl Into<String>) -> CtError {
    CtError {
        code,
        message: message.into(),
        record,
        line: None,
    }
}

fn loss(record: usize, path: &str, feature: &str, impact: SemanticImpact, reason: &str) -> Loss {
    loss_at(&format!("records[{record}]"), path, feature, impact, reason)
}

/// A dropped feature of whatever `root` names: a record, or a whole drawing.
pub(crate) fn loss_at(
    root: &str,
    path: &str,
    feature: &str,
    impact: SemanticImpact,
    reason: &str,
) -> Loss {
    Loss {
        consent: LossConsent::NotRequired,
        feature: feature.into(),
        source_path: format!("{root}{path}"),
        disposition: LossDisposition::Dropped,
        reason: reason.into(),
        semantic_impact: impact,
    }
}

/// The wording of the drawing-level losses of a writer. The features are the shared
/// vocabulary of every writer; only the reason depends on what the format can hold.
pub(crate) struct DrawingLossWording {
    pub native_ids: &'static str,
    /// The first placement.
    pub placement_id: &'static str,
    /// Each placement after the first.
    pub extra_placement: &'static str,
    pub selection: &'static str,
    pub scene_object: &'static str,
    pub scene_references: &'static str,
    pub settings: &'static str,
}

pub(crate) const MOL_WORDING: DrawingLossWording = DrawingLossWording {
    native_ids: "Native atom and bond IDs are mapped to file indices and are not embedded in V2000.",
    placement_id: "The placement ID is not embedded in V2000.",
    extra_placement: "Only the first placement is written.",
    selection: "The editor selection is not written.",
    scene_object: "Groups and graphics are not represented in this V2000 profile.",
    scene_references: "Scene references are not written.",
    settings: "Drawing styles and settings are not represented in this V2000 profile.",
};

/// Everything a writer drops from a drawing besides atoms and bonds: native IDs, placements,
/// selection, scene objects and references, and settings. Reports only what exists.
pub(crate) fn drawing_export_losses(
    doc: &DocumentView,
    root: &str,
    wording: &DrawingLossWording,
    losses: &mut Vec<Loss>,
) {
    if !doc.atoms.is_empty() || !doc.bonds.is_empty() {
        losses.push(loss_at(
            root,
            ".ids",
            "native_ids",
            SemanticImpact::Document,
            wording.native_ids,
        ));
    }
    if let Some(first) = doc.scene.instances.first() {
        losses.push(loss_at(
            root,
            &format!(".scene.instances[{}]", first.id),
            "placement_id",
            SemanticImpact::Document,
            wording.placement_id,
        ));
    }
    if !doc.selection.atoms.is_empty()
        || !doc.selection.bonds.is_empty()
        || !doc.selection.objects.is_empty()
    {
        losses.push(loss_at(
            root,
            ".selection",
            "selection",
            SemanticImpact::Document,
            wording.selection,
        ));
    }
    for instance in doc.scene.instances.iter().skip(1) {
        losses.push(loss_at(
            root,
            &format!(".scene.instances[{}]", instance.id),
            "extra_placement",
            SemanticImpact::Geometry,
            wording.extra_placement,
        ));
    }
    for object in &doc.scene.objects {
        match object {
            SceneObjectView::Group { id, .. } | SceneObjectView::Graphic { id, .. } => {
                losses.push(loss_at(
                    root,
                    &format!(".scene.objects[{id}]"),
                    "scene_object",
                    SemanticImpact::Rendering,
                    wording.scene_object,
                ))
            }
            SceneObjectView::Instance { .. } => {}
        }
    }
    if !doc.scene.references.is_empty() {
        losses.push(loss_at(
            root,
            ".scene.references",
            "scene_references",
            SemanticImpact::Document,
            wording.scene_references,
        ));
    }
    losses.push(loss_at(
        root,
        ".settings",
        "drawing_settings",
        SemanticImpact::Rendering,
        wording.settings,
    ));
}

fn implicit_hydrogen_uncertainty(record: usize) -> Loss {
    Loss {
        consent: LossConsent::NotRequired,
        feature: "default_hydrogen_interpretation".into(),
        source_path: format!("records[{record}].atoms"),
        disposition: LossDisposition::Unknown,
        reason: "Default V2000 hydrogen and valence fields do not establish the same implicit hydrogen count in every reader. Chemifree records an inferred state, and its separate bounded valence inspection may differ from the source or destination reader.".into(),
        semantic_impact: SemanticImpact::Chemistry,
    }
}

/// Build domain edits for one record; document validation is performed by a candidate transaction.
pub(crate) fn import_commands(
    record: &CtRecord,
    index: usize,
    doc: &DocumentView,
    losses: &mut Vec<Loss>,
) -> Result<Vec<Command>, CtError> {
    if doc.scene.instances.len() != 1 {
        return Err(projection_error(
            index,
            "unsupported_scene",
            "Import requires exactly one molecule placement.",
        ));
    }
    if !record.title.is_empty() && record.title != doc.title {
        losses.push(loss(
            index,
            ".title",
            "source_title",
            SemanticImpact::Document,
            "Import appends to the current drawing and does not replace its title.",
        ));
    }
    if !record.program.is_empty() || !record.comment.is_empty() {
        losses.push(loss(
            index,
            ".header",
            "source_header",
            SemanticImpact::Document,
            "Program and comment lines are not stored by the editor.",
        ));
    }
    for (i, _) in record.fields.iter().enumerate() {
        losses.push(loss(
            index,
            &format!(".fields[{}]", i + 1),
            "sd_field",
            SemanticImpact::Document,
            "SD field values are returned in the preview but are not stored in the drawing.",
        ));
    }
    if !record.atoms.is_empty() {
        losses.push(implicit_hydrogen_uncertainty(index));
    }
    let mut commands = Vec::with_capacity(record.atoms.len() + record.bonds.len());
    for (i, source) in record.atoms.iter().enumerate() {
        if source.z != 0.0 {
            losses.push(loss(
                index,
                &format!(".atoms[{}].z", i + 1),
                "z_coordinate",
                SemanticImpact::Geometry,
                "The editor stores two-dimensional positions only.",
            ));
        }
        let element = Element::from_symbol(&source.element).ok_or_else(|| {
            projection_error(index, "unsupported_atom", "Invalid element from codec.")
        })?;
        let point = Point::new(
            source.x * POINTS_PER_ANGSTROM,
            -source.y * POINTS_PER_ANGSTROM,
        )
        .map_err(|_| {
            projection_error(
                index,
                "coordinate_out_of_range",
                format!("Atom {} cannot be placed in the editor.", i + 1),
            )
        })?;
        let mut atom = Atom::new(AtomKind::Element(element));
        atom.charge = source.charge;
        atom.isotope = source.isotope;
        atom.radical = match source.radical {
            CtRadical::None => Radical::None,
            CtRadical::Singlet => Radical::Singlet,
            CtRadical::Doublet => Radical::Doublet,
            CtRadical::Triplet => Radical::Triplet,
        };
        commands.push(Command::AddAtom {
            atom,
            positions: NewAtomPositions::Single(point),
            bond_to: None,
        });
    }
    for bond in &record.bonds {
        if bond.begin >= record.atoms.len() || bond.end >= record.atoms.len() {
            return Err(projection_error(
                index,
                "invalid_bond",
                "Codec returned an invalid endpoint.",
            ));
        }
        let order = match bond.order {
            1 => BondOrder::Single,
            2 => BondOrder::Double,
            3 => BondOrder::Triple,
            _ => {
                return Err(projection_error(
                    index,
                    "unsupported_bond",
                    "Unsupported codec bond order.",
                ));
            }
        };
        commands.push(Command::AddBond {
            begin: AtomRef::Created(bond.begin),
            end: AtomRef::Created(bond.end),
            order,
        });
    }
    Ok(commands)
}

pub(crate) fn unselected_losses(records: &[CtRecord], selected: usize, losses: &mut Vec<Loss>) {
    for index in 1..=records.len() {
        if index != selected {
            losses.push(loss(
                index,
                "",
                "unselected_record",
                SemanticImpact::Document,
                "This record is returned by preview but not added to the drawing.",
            ));
        }
    }
}

/// Convert a drawing to the part representable by this codec; no document is changed.
pub(crate) fn export_record(
    doc: &DocumentView,
    index: usize,
    fields: Vec<CtField>,
    losses: &mut Vec<Loss>,
) -> Result<CtRecord, CtError> {
    if !doc.atoms.is_empty() {
        losses.push(implicit_hydrogen_uncertainty(index));
    }
    let mut atoms = Vec::with_capacity(doc.atoms.len());
    let mut ids = HashMap::new();
    for (i, atom) in doc.atoms.iter().enumerate() {
        if atom.kind != AtomKindView::Element {
            return Err(projection_error(
                index,
                "unsupported_atom",
                format!("Atom {} is a query, not an element.", atom.id),
            ));
        }
        if !matches!(atom.hydrogens, HydrogensDto::Inferred {}) {
            losses.push(loss(
                index,
                &format!(".atoms[{}].hydrogens", atom.id),
                "hydrogen_state",
                SemanticImpact::Chemistry,
                "The stored hydrogen state is not represented in this V2000 profile.",
            ));
        }
        if !atom.attachment_points.is_empty() {
            losses.push(loss(
                index,
                &format!(".atoms[{}].attachment_points", atom.id),
                "attachment_points",
                SemanticImpact::Chemistry,
                "Attachment points are outside this profile.",
            ));
        }
        let radical = match atom.radical {
            RadicalDto::None => CtRadical::None,
            RadicalDto::Singlet => CtRadical::Singlet,
            RadicalDto::Doublet => CtRadical::Doublet,
            RadicalDto::Triplet => CtRadical::Triplet,
            RadicalDto::Unknown => {
                return Err(projection_error(
                    index,
                    "unsupported_radical",
                    format!("Atom {} has an unknown radical state.", atom.id),
                ));
            }
        };
        ids.insert(atom.id.as_str(), i);
        atoms.push(CtAtom {
            element: atom.element.clone(),
            x: atom.position.x / POINTS_PER_ANGSTROM,
            y: -atom.position.y / POINTS_PER_ANGSTROM,
            z: 0.0,
            charge: atom.charge,
            isotope: atom.isotope,
            radical,
        });
    }
    let mut bonds = Vec::with_capacity(doc.bonds.len());
    for bond in &doc.bonds {
        let order = match bond.order {
            BondOrderDto::Single => 1,
            BondOrderDto::Double => 2,
            BondOrderDto::Triple => 3,
            _ => {
                return Err(projection_error(
                    index,
                    "unsupported_bond",
                    format!("Bond {} is not single, double or triple.", bond.id),
                ));
            }
        };
        let begin = *ids.get(bond.begin.as_str()).ok_or_else(|| {
            projection_error(index, "invalid_bond", "Bond references a missing atom.")
        })?;
        let end = *ids.get(bond.end.as_str()).ok_or_else(|| {
            projection_error(index, "invalid_bond", "Bond references a missing atom.")
        })?;
        bonds.push(CtBond { begin, end, order });
    }
    drawing_export_losses(doc, &format!("records[{index}]"), &MOL_WORDING, losses);
    Ok(CtRecord {
        title: doc.title.clone(),
        program: String::new(),
        comment: String::new(),
        atoms,
        bonds,
        fields,
    })
}

/// Stable binding of the complete prospective editor document, source record and losses.
///
/// The value is serialized straight into the hash, so a large source or document is never
/// copied into a second buffer or a JSON `Value`.
pub(crate) fn digest<T: Serialize>(value: &T) -> String {
    let mut hasher = Sha256::new();
    serde_json::to_writer(&mut hasher, value)
        .expect("validated projection contains serializable finite values");
    format!("{:x}", hasher.finalize())
}
