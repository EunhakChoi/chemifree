//! Projection between SMILES and the editor's single graph and page: placement of a read
//! structure, and the losses of reading and writing it.

use chemifree_domain::document::{Atom, AtomRef, BondOrder, Command, NewAtomPositions, Point};
use chemifree_domain::smiles::{RadicalNote, Read, Written};
use serde_json::Map;

use crate::contract::DocumentView;
use crate::ct_projection::{self, DrawingLossWording, loss_at};
use crate::documents::DEFAULT_TITLE;
use crate::{Loss, LossConsent, LossDisposition, SemanticImpact};

/// Points between neighbouring grid cells of a placed structure.
const GRID_SPACING: f64 = 30.0;
/// Points between the drawing's existing atoms and a placed structure.
const GAP: f64 = 60.0;

/// What a refused projection reports.
pub(crate) struct Refusal {
    pub code: &'static str,
    pub message: String,
    pub details: Map<String, serde_json::Value>,
}

fn refusal(code: &'static str, message: impl Into<String>) -> Refusal {
    Refusal {
        code,
        message: message.into(),
        details: Map::new(),
    }
}

const SOURCE: &str = "source";

fn source_path(start: usize, end: usize) -> String {
    format!("{SOURCE}[{start}..{end}]")
}

/// Everything reading a string loses or infers. It depends on the text alone, never on the
/// drawing the structure is added to, so a replayed import reports the same.
pub(crate) fn read_losses(read: &Read) -> Vec<Loss> {
    let mut losses = Vec::new();
    losses.push(Loss {
        consent: LossConsent::NotRequired,
        feature: "generated_layout".into(),
        source_path: SOURCE.into(),
        disposition: LossDisposition::Normalized,
        reason: "SMILES has no coordinates. Atoms are placed on a plain grid in input order, to the right of the existing atoms; this is not a structure diagram, and no layout algorithm is implemented.".into(),
        semantic_impact: SemanticImpact::Geometry,
    });
    if let Some(span) = read.ignored_text {
        losses.push(Loss {
            consent: LossConsent::NotRequired,
            feature: "ignored_text".into(),
            source_path: source_path(span.start, span.end),
            disposition: LossDisposition::Dropped,
            reason: "Text after the whitespace that ends the SMILES (usually a name) is not read and not stored.".into(),
            semantic_impact: SemanticImpact::Document,
        });
    }
    for atom in &read.atoms {
        let (disposition, reason) = match atom.radical_note {
            RadicalNote::Stated => continue,
            RadicalNote::Inferred => (
                LossDisposition::Normalized,
                "SMILES has no radical field. The valence model reads the open valence of this bracket atom as a doublet radical.",
            ),
            RadicalNote::Undetermined => (
                LossDisposition::Unknown,
                "The valence model cannot decide this bracket atom's radical state (the atom or charge is outside its table, or two open valences could be a singlet or a triplet); it is recorded as unknown.",
            ),
        };
        losses.push(Loss {
            consent: LossConsent::NotRequired,
            feature: "radical_state".into(),
            source_path: source_path(atom.span.start, atom.span.end),
            disposition,
            reason: reason.into(),
            semantic_impact: SemanticImpact::Chemistry,
        });
    }
    losses
}

/// The edits that add a read structure to the drawing: every atom on a grid beside the existing
/// atoms, then every bond in input order. Document validation runs in the candidate transaction.
pub(crate) fn import_commands(read: &Read, doc: &DocumentView) -> Result<Vec<Command>, Refusal> {
    if doc.scene.instances.len() != 1 {
        return Err(refusal(
            "unsupported_scene",
            "Import requires exactly one molecule placement.",
        ));
    }
    let (left, top) = if doc.atoms.is_empty() {
        (0.0, 0.0)
    } else {
        let right = doc
            .atoms
            .iter()
            .map(|atom| atom.position.x)
            .fold(f64::MIN, f64::max);
        let top = doc
            .atoms
            .iter()
            .map(|atom| atom.position.y)
            .fold(f64::MAX, f64::min);
        (right + GAP, top)
    };
    let count = read.atoms.len();
    let columns = (1..=count).find(|c| c * c >= count).unwrap_or(1);
    let mut commands = Vec::with_capacity(count + read.bonds.len());
    for (index, source) in read.atoms.iter().enumerate() {
        let point = Point::new(
            left + (index % columns) as f64 * GRID_SPACING,
            top + (index / columns) as f64 * GRID_SPACING,
        )
        .map_err(|_| {
            refusal(
                "coordinate_out_of_range",
                format!(
                    "Atom {} cannot be placed beside the existing atoms.",
                    index + 1
                ),
            )
        })?;
        let atom: Atom = source.atom.clone();
        commands.push(Command::AddAtom {
            atom,
            positions: NewAtomPositions::Single(point),
            bond_to: None,
        });
    }
    for bond in &read.bonds {
        let order: BondOrder = bond.order.clone();
        commands.push(Command::AddBond {
            begin: AtomRef::Created(bond.begin),
            end: AtomRef::Created(bond.end),
            order,
        });
    }
    Ok(commands)
}

const WORDING: DrawingLossWording = DrawingLossWording {
    native_ids: "Native atom and bond IDs are not written; SMILES names atoms by position.",
    placement_id: "SMILES records no placement; this placement, and its ID, are not written.",
    extra_placement: "SMILES records no placement; this placement is not written.",
    selection: "The editor selection is not written.",
    scene_object: "Groups and graphics are not represented in SMILES.",
    scene_references: "Scene references are not written.",
    settings: "Drawing styles and settings are not represented in SMILES.",
};

/// Everything writing this drawing as SMILES drops or rewrites. Each entry is reported only
/// where the content exists.
pub(crate) fn export_losses(doc: &DocumentView, written: &Written, canonical: bool) -> Vec<Loss> {
    let mut losses = Vec::new();
    let root = "document";
    if !doc.atoms.is_empty() {
        losses.push(loss_at(
            root,
            ".atoms",
            "coordinates",
            SemanticImpact::Geometry,
            "SMILES records no coordinates.",
        ));
    }
    if !doc.title.is_empty() && doc.title != DEFAULT_TITLE {
        losses.push(loss_at(
            root,
            ".title",
            "title",
            SemanticImpact::Document,
            "The drawing title is not written.",
        ));
    }
    for atom in &doc.atoms {
        if !atom.attachment_points.is_empty() {
            let mut dropped = loss_at(
                root,
                &format!(".atoms[{}].attachment_points", atom.id),
                "attachment_points",
                SemanticImpact::Chemistry,
                "Attachment points are not written; SMILES has none.",
            );
            dropped.consent = LossConsent::Required;
            losses.push(dropped);
        }
    }
    ct_projection::drawing_export_losses(doc, root, &WORDING, &mut losses);
    for id in &written.hydrogen_state_changes {
        losses.push(Loss {
            consent: LossConsent::NotRequired,
            feature: "hydrogen_state".into(),
            source_path: format!("{root}.atoms[{id}].hydrogens"),
            disposition: LossDisposition::Normalized,
            reason: "SMILES states a bracket atom's hydrogen count and leaves a bare atom's to the valence rule. The count is the same; whether the drawing records it as stated or inferred is not kept.".into(),
            semantic_impact: SemanticImpact::None,
        });
    }
    if canonical {
        if !written.folded_hydrogens.is_empty() {
            losses.push(Loss {
                consent: LossConsent::NotRequired,
                feature: "explicit_hydrogen_atoms".into(),
                source_path: format!("{root}.atoms"),
                disposition: LossDisposition::Normalized,
                reason: format!(
                    "{} explicit hydrogen atoms are folded into their heavy atoms' hydrogen counts. Their IDs are in folded_hydrogens and not in the atom order.",
                    written.folded_hydrogens.len()
                ),
                semantic_impact: SemanticImpact::Chemistry,
            });
        }
        if !written.kekule_bonds_aromatized.is_empty() {
            losses.push(Loss {
                consent: LossConsent::NotRequired,
                feature: "aromatic_bonds".into(),
                source_path: format!("{root}.bonds"),
                disposition: LossDisposition::Normalized,
                reason: format!(
                    "{} single and double bonds of aromatic systems are written as aromatic, as the canonical form requires.",
                    written.kekule_bonds_aromatized.len()
                ),
                semantic_impact: SemanticImpact::Chemistry,
            });
        }
    }
    losses
}
