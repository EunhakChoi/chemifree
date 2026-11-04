//! Projection between a CTfile record and the editor's single graph and page.

use std::collections::HashMap;

use chemifree_domain::document::{
    Atom, AtomRef, BondOrder, BondRef, Command, Document, DocumentId, NewAtomPositions, Point,
};
use chemifree_domain::element::Element;
use chemifree_domain::states::{AtomKind, Radical};
use chemifree_domain::stereo::{BondEnd, StereoKey, StereoMark, StereoState};
use chemifree_domain::stereo_drawing::Drawing;
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::contract::{
    AtomKindView, BondEndDto, BondOrderDto, DocumentView, HydrogensDto, MarkDto, RadicalDto,
    SceneObjectView,
};
use crate::ctfile::{CtAtom, CtBond, CtBondStereo, CtError, CtField, CtRadical, CtRecord, CtTab};
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
    Loss {
        consent: LossConsent::NotRequired,
        feature: feature.into(),
        source_path: format!("records[{record}]{path}"),
        disposition: LossDisposition::Dropped,
        reason: reason.into(),
        semantic_impact: impact,
    }
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
    if !record.collections.is_empty() {
        return Err(projection_error(
            index,
            "v3000_collection_not_supported_yet",
            "The record holds V3000 stereo collections, which are not mapped to stereo groups yet; nothing was imported.",
        ));
    }
    if record.chiral {
        losses.push(loss(
            index,
            ".counts.chiral",
            "chiral_flag",
            SemanticImpact::Stereo,
            "The chiral flag is read but not interpreted; the drawing has no chiral flag and states no absolute stereochemistry from it.",
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
            4 => BondOrder::Aromatic,
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
    let first_bond = record.atoms.len();
    let mut marked = false;
    for (j, bond) in record.bonds.iter().enumerate() {
        let mark = match (bond.stereo, bond.order) {
            (CtBondStereo::None, _) => continue,
            (CtBondStereo::Wedge, 1) => StereoMark::Wedge {
                narrow: BondEnd::Begin,
            },
            (CtBondStereo::Hash, 1) => StereoMark::Hash {
                narrow: BondEnd::Begin,
            },
            (CtBondStereo::Either, 1) => StereoMark::Wavy,
            (CtBondStereo::Either, 2) => StereoMark::Crossed,
            _ => {
                return Err(projection_error(
                    index,
                    "unsupported_bond",
                    format!("Bond {} carries a mark its order cannot.", j + 1),
                ));
            }
        };
        marked = true;
        commands.push(Command::SetBondMark {
            bond: BondRef::Created(first_bond + j),
            mark: Some(mark),
        });
    }
    if marked {
        commands.extend(derived_configurations(&commands, index, losses));
    }
    Ok(commands)
}

/// The command that stores the configuration each marked center's drawing shows, and a loss
/// for each center the drawing does not determine. The derivation is the domain's own, run on
/// a scratch document of the record alone (an import adds a separate component, so a center's
/// neighbourhood is the same there); a center that does not derive keeps its mark and stores
/// nothing, so one degenerate center never refuses the whole import.
fn derived_configurations(
    commands: &[Command],
    index: usize,
    losses: &mut Vec<Loss>,
) -> Option<Command> {
    let id = DocumentId::parse("scratch").expect("scratch identifier is valid");
    let mut scratch = Document::new(id, "scratch").ok()?;
    scratch.apply(commands).ok()?;
    let instance = scratch.first_instance();
    let drawing = Drawing::of_document(&scratch, instance);
    let centers: std::collections::HashSet<_> = drawing.marked_centers().into_iter().collect();
    let mut readable = Vec::with_capacity(centers.len());
    for (position, (atom, _)) in scratch.atoms().enumerate() {
        if !centers.contains(&atom) {
            continue;
        }
        match drawing.derive_center(atom) {
            Ok(_) => readable.push(AtomRef::Created(position)),
            Err(cause) => losses.push(loss(
                index,
                &format!(".atoms[{}]", position + 1),
                "stereo_unreadable",
                SemanticImpact::Stereo,
                &format!("The drawn marks at this atom do not determine a configuration ({cause}); the marks are kept and no configuration is stored."),
            )),
        }
    }
    (!readable.is_empty()).then_some(Command::SetConfigurationFromDrawing {
        atoms: Some(readable),
        bonds: None,
        instance: None,
    })
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

/// The file format a drawing is written to, which words the losses of everything the drawing
/// holds beyond its chemistry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Target {
    V2000,
    V3000,
    Smiles,
}

impl Target {
    /// How a drawing-level loss names the format.
    fn format(self) -> &'static str {
        match self {
            Self::V2000 => "this V2000 profile",
            Self::V3000 => "this V3000 profile",
            Self::Smiles => "SMILES",
        }
    }

    /// The CTfile dialect, or `None` for SMILES.
    fn dialect(self) -> Option<&'static str> {
        match self {
            Self::V2000 => Some("V2000"),
            Self::V3000 => Some("V3000"),
            Self::Smiles => None,
        }
    }
}

fn drawing_loss(
    prefix: &str,
    path: &str,
    feature: &str,
    impact: SemanticImpact,
    reason: &str,
) -> Loss {
    Loss {
        consent: LossConsent::NotRequired,
        feature: feature.into(),
        source_path: format!("{prefix}{path}"),
        disposition: LossDisposition::Dropped,
        reason: reason.into(),
        semantic_impact: impact,
    }
}

/// What a written file does not keep of the drawing around the molecule: coordinates (when
/// the format has none), title, native IDs, placements, scene objects and references,
/// selection and drawing settings. Each is reported only when the drawing really holds it.
pub(crate) fn drawing_losses(
    doc: &DocumentView,
    prefix: &str,
    target: Target,
    losses: &mut Vec<Loss>,
) {
    let smiles = target == Target::Smiles;
    let format = target.format();
    let dialect = target.dialect().unwrap_or("");
    let mut push = |path: &str, feature: &str, impact, reason: &str| {
        losses.push(drawing_loss(prefix, path, feature, impact, reason));
    };
    if smiles && !doc.atoms.is_empty() {
        push(
            ".atoms",
            "coordinates",
            SemanticImpact::Geometry,
            "Atom positions are not written; SMILES has no coordinates.",
        );
    }
    if smiles && !doc.title.is_empty() && doc.title != crate::DEFAULT_TITLE {
        push(
            ".title",
            "title",
            SemanticImpact::Document,
            "The drawing's title is not written; a SMILES string has no title.",
        );
    }
    if !doc.atoms.is_empty() || !doc.bonds.is_empty() {
        push(
            ".ids",
            "native_ids",
            SemanticImpact::Document,
            &if smiles {
                "Native atom and bond IDs are not written; the written atom order maps text positions to them.".to_owned()
            } else {
                format!(
                    "Native atom and bond IDs are mapped to file indices and are not embedded in {dialect}."
                )
            },
        );
    }
    if let Some(first) = doc.scene.instances.first() {
        push(
            &format!(".scene.instances[{}]", first.id),
            "placement_id",
            SemanticImpact::Document,
            &if smiles {
                "The placement and its ID are not written; SMILES has no placement.".to_owned()
            } else {
                format!("The placement ID is not embedded in {dialect}.")
            },
        );
    }
    if !doc.selection.atoms.is_empty()
        || !doc.selection.bonds.is_empty()
        || !doc.selection.objects.is_empty()
    {
        push(
            ".selection",
            "selection",
            SemanticImpact::Document,
            "The editor selection is not written.",
        );
    }
    for instance in doc.scene.instances.iter().skip(1) {
        push(
            &format!(".scene.instances[{}]", instance.id),
            "extra_placement",
            SemanticImpact::Geometry,
            if smiles {
                "This additional placement is not written; SMILES has no placement."
            } else {
                "Only the first placement is written."
            },
        );
    }
    for object in &doc.scene.objects {
        match object {
            SceneObjectView::Group { id, .. } | SceneObjectView::Graphic { id, .. } => push(
                &format!(".scene.objects[{id}]"),
                "scene_object",
                SemanticImpact::Rendering,
                &format!("Groups and graphics are not represented in {format}."),
            ),
            SceneObjectView::Instance { .. } => {}
        }
    }
    if !doc.scene.references.is_empty() {
        push(
            ".scene.references",
            "scene_references",
            SemanticImpact::Document,
            "Scene references are not written.",
        );
    }
    push(
        ".settings",
        "drawing_settings",
        SemanticImpact::Rendering,
        &format!("Drawing styles and settings are not represented in {format}."),
    );
}

/// The stereo a SMILES string does not carry: marks and stated configurations are not written,
/// so the caller must accept that they are dropped. Each loss is reported only when the drawing
/// holds that kind, with its marks and records in ID order. Wavy and crossed marks and
/// `unknown` records count too: "unknown" is a statement a format that cannot carry it drops.
/// Every enhanced stereo group is reported too, singletons included: a target without group
/// support carries none of them.
pub(crate) fn stereo_losses(doc: &DocumentView) -> Vec<Loss> {
    let consent_loss = |feature: &str, source_path: String, reason: &str| Loss {
        consent: LossConsent::Required,
        feature: feature.into(),
        source_path,
        disposition: LossDisposition::Dropped,
        reason: reason.into(),
        semantic_impact: SemanticImpact::Stereo,
    };
    let mut losses = Vec::new();
    let marked: Vec<&str> = doc
        .bonds
        .iter()
        .filter(|bond| bond.mark.is_some())
        .map(|bond| bond.id.as_str())
        .collect();
    if !marked.is_empty() {
        losses.push(consent_loss(
            "stereo_marks",
            format!("bonds[{}]", marked.join(", ")),
            "SMILES stereochemistry is not implemented; the drawn wedge, hash, wavy and crossed marks are not written.",
        ));
    }
    if !doc.stereo.atoms.is_empty() || !doc.stereo.bonds.is_empty() {
        let mut paths = Vec::new();
        if !doc.stereo.atoms.is_empty() {
            let atoms: Vec<&str> = doc.stereo.atoms.iter().map(|r| r.atom.as_str()).collect();
            paths.push(format!("atoms[{}]", atoms.join(", ")));
        }
        if !doc.stereo.bonds.is_empty() {
            let bonds: Vec<&str> = doc.stereo.bonds.iter().map(|r| r.bond.as_str()).collect();
            paths.push(format!("bonds[{}]", bonds.join(", ")));
        }
        losses.push(consent_loss(
            "stereo_configuration",
            paths.join(", "),
            "SMILES stereochemistry is not implemented; the stated configurations are not written, so mirror images and cis/trans forms give the same text.",
        ));
    }
    if !doc.stereo.groups.is_empty() {
        let groups: Vec<&str> = doc.stereo.groups.iter().map(|g| g.id.as_str()).collect();
        losses.push(consent_loss(
            "stereo_group",
            format!("groups[{}]", groups.join(", ")),
            "This SMILES writer has no enhanced stereo groups; the absolute, and and or groups are not written, so a document that states which centers belong together reads as a single compound.",
        ));
    }
    losses
}

/// Whether the drawing needs V3000 to be written at all: more than 999 atoms or bonds, or an
/// aromatic bond, which V2000 has no type for. What `ctab: "auto"` decides on.
pub(crate) fn needs_v3000(doc: &DocumentView) -> bool {
    doc.atoms.len() > MAX_V2000_ITEMS
        || doc.bonds.len() > MAX_V2000_ITEMS
        || doc
            .bonds
            .iter()
            .any(|bond| bond.order == BondOrderDto::Aromatic)
}

const MAX_V2000_ITEMS: usize = 999;

/// The stereo a MOL or SDF file does not carry of the drawing, each reported only when the
/// drawing holds it, in ID order. Wedge, hash, wavy and crossed marks are written. A stored
/// configuration is carried only through the marks and the coordinates, so one the drawing does
/// not show (a derivation of the drawn marks that differs from it or that fails) needs consent,
/// as does every enhanced stereo group, singletons included: neither dialect carries them yet.
pub(crate) fn molfile_stereo_losses(
    document: &Document,
    view: &DocumentView,
    index: usize,
) -> Vec<Loss> {
    let consent_loss = |feature: &str, source_path: String, reason: &str| Loss {
        consent: LossConsent::Required,
        feature: feature.into(),
        source_path: format!("records[{index}].{source_path}"),
        disposition: LossDisposition::Dropped,
        reason: reason.into(),
        semantic_impact: SemanticImpact::Stereo,
    };
    let drawing = Drawing::of_document(document, document.first_instance());
    let mut centers = Vec::new();
    let mut double_bonds = Vec::new();
    for (key, state) in document.stereo().iter() {
        match (key, state) {
            (StereoKey::Center(atom), StereoState::Center(stored))
                if drawing.derive_center(atom).ok().as_ref() != Some(stored) =>
            {
                centers.push(atom.to_string());
            }
            (StereoKey::DoubleBond(bond), StereoState::DoubleBond(stored))
                if drawing.derive_double_bond(bond).ok().as_ref() != Some(stored) =>
            {
                double_bonds.push(bond.to_string());
            }
            _ => {}
        }
    }
    let mut losses = Vec::new();
    if !centers.is_empty() {
        losses.push(consent_loss(
            "stereo_configuration_not_drawn",
            format!("atoms[{}]", centers.join(", ")),
            "The stated configuration of this center is not what its drawn marks and positions show, and a MOL file states stereo only through the marks, so a reader gets another configuration or none.",
        ));
    }
    if !double_bonds.is_empty() {
        losses.push(consent_loss(
            "stereo_bond_configuration",
            format!("bonds[{}]", double_bonds.join(", ")),
            "The stated double-bond configuration is not shown by a crossed mark or by the coordinates, and a MOL file has no field for it, so a reader gets another arrangement or none.",
        ));
    }
    if !view.stereo.groups.is_empty() {
        let groups: Vec<&str> = view.stereo.groups.iter().map(|g| g.id.as_str()).collect();
        losses.push(consent_loss(
            "stereo_group",
            format!("groups[{}]", groups.join(", ")),
            "This MOL writer does not write V3000 stereo collections yet; the absolute, and and or groups are not written, so a document that states which centers belong together reads as a single compound.",
        ));
    }
    if view.holds_stereo() {
        losses.push(Loss {
            consent: LossConsent::NotRequired,
            feature: "chiral_flag".into(),
            source_path: format!("records[{index}].counts.chiral"),
            disposition: LossDisposition::Unknown,
            reason: "The chiral flag is written as 0 and no absolute stereochemistry is stated; how a reader treats the written marks with flag 0 is not established by the sources this profile follows.".into(),
            semantic_impact: SemanticImpact::Stereo,
        });
    }
    losses
}

/// Convert a drawing to the part representable by the chosen dialect; no document is changed.
///
/// Wedge, hash, wavy and crossed marks are written (a wedge or hash whose narrow end is the
/// bond's `end` is written with the endpoints swapped, the bond staying the same bond). What the
/// file cannot carry of the stereo is reported by [`molfile_stereo_losses`], which the caller
/// adds. V2000 has no type for an aromatic bond and a query is in neither dialect yet; each is
/// refused with the reason.
pub(crate) fn export_record(
    doc: &DocumentView,
    index: usize,
    fields: Vec<CtField>,
    target: Target,
    losses: &mut Vec<Loss>,
) -> Result<CtRecord, CtError> {
    let tab = if target == Target::V3000 {
        CtTab::V3000
    } else {
        CtTab::V2000
    };
    let dialect = target.dialect().unwrap_or("V2000");
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
                &format!("The stored hydrogen state is not represented in this {dialect} profile."),
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
            BondOrderDto::Aromatic if tab == CtTab::V3000 => 4,
            BondOrderDto::Aromatic => {
                return Err(projection_error(
                    index,
                    "unsupported_bond",
                    format!(
                        "Bond {} is aromatic, which V2000 cannot carry; write V3000 (ctab v3000 or auto).",
                        bond.id
                    ),
                ));
            }
            _ => {
                return Err(projection_error(
                    index,
                    "unsupported_bond",
                    format!(
                        "Bond {} is not single, double, triple or aromatic.",
                        bond.id
                    ),
                ));
            }
        };
        let (begin, end) = (bond.begin.as_str(), bond.end.as_str());
        let (stereo, swap) = match &bond.mark {
            None => (CtBondStereo::None, false),
            Some(MarkDto::Wedge { narrow }) => (CtBondStereo::Wedge, *narrow == BondEndDto::End),
            Some(MarkDto::Hash { narrow }) => (CtBondStereo::Hash, *narrow == BondEndDto::End),
            Some(MarkDto::Wavy {} | MarkDto::Crossed {}) => (CtBondStereo::Either, false),
        };
        let (begin, end) = if swap { (end, begin) } else { (begin, end) };
        let begin = *ids.get(begin).ok_or_else(|| {
            projection_error(index, "invalid_bond", "Bond references a missing atom.")
        })?;
        let end = *ids.get(end).ok_or_else(|| {
            projection_error(index, "invalid_bond", "Bond references a missing atom.")
        })?;
        bonds.push(CtBond {
            begin,
            end,
            order,
            stereo,
        });
    }
    drawing_losses(doc, &format!("records[{index}]"), target, losses);
    Ok(CtRecord {
        title: doc.title.clone(),
        program: String::new(),
        comment: String::new(),
        atoms,
        bonds,
        fields,
        ctab: tab,
        chiral: false,
        collections: Vec::new(),
    })
}

/// A hasher that serde writes into directly, so a large request is hashed without being
/// copied into an intermediate buffer.
struct HashWriter(Sha256);

impl std::io::Write for HashWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Stable binding of the complete prospective editor document, source record and losses.
pub(crate) fn digest<T: Serialize>(value: &T) -> String {
    let mut writer = HashWriter(Sha256::new());
    serde_json::to_writer(&mut writer, value)
        .expect("validated projection contains serializable finite values");
    format!("{:x}", writer.0.finalize())
}
