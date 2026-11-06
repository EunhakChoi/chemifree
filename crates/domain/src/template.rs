//! A single explicit anchor joins an unchanged native fragment to an existing atom.

use std::collections::BTreeSet;
use std::fmt;

use crate::clipboard::{Fragment, FragmentError, PastePlan};
use crate::document::{
    AtomId, AtomRef, BondId, BondOrder, Command, Document, DocumentId, MAX_COMMANDS, ObjectId,
    Vector,
};
use crate::stereo::StereoKey;

/// A local source atom and whether joining it would preserve all copied stereo statements.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Anchor {
    pub atom: AtomId,
    pub supported: bool,
}

/// Paste commands plus exactly one final exocyclic single bond, never selected automatically.
#[derive(Clone, Debug, PartialEq)]
pub struct AttachmentPlan {
    pub paste: PastePlan,
    pub connection: BondId,
}

/// A terminal atom is retained as the source anchor; its outside bond is unchanged.
#[derive(Clone, Debug, PartialEq)]
pub struct ReplacementPlan {
    pub paste: PastePlan,
    pub boundary: BondId,
}

/// Every refusal preserves both the fragment and the destination.
#[derive(Clone, Debug, PartialEq)]
pub enum TemplateError {
    Fragment(FragmentError),
    Disconnected,
    AnchorUnknown,
    TargetUnknown(AtomId),
    AnchorStereo,
    Instance,
    Direction,
    Terminal,
}
impl TemplateError {
    /// Stable diagnostic reason; fragment failures retain their existing reasons.
    pub fn reason(&self) -> &'static str {
        match self {
            Self::Fragment(error) => error.as_str(),
            Self::Disconnected => "template_disconnected",
            Self::AnchorUnknown => "template_anchor_unknown",
            Self::TargetUnknown(_) => "template_target_invalid",
            Self::AnchorStereo => "template_anchor_stereo_unsupported",
            Self::Instance => "template_instance_invalid",
            Self::Direction => "template_direction_invalid",
            Self::Terminal => "template_target_not_terminal_single",
        }
    }
}
impl fmt::Display for TemplateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fragment(error) => write!(formatter,"{error}"),
            Self::Disconnected => formatter.write_str("Attachment requires one connected template component"),
            Self::AnchorUnknown => formatter.write_str("Choose an existing explicit local template atom"),
            Self::TargetUnknown(atom) => write!(formatter,"Destination atom {atom} does not exist"),
            Self::AnchorStereo => formatter.write_str("This source anchor would lose a stored center or incident double-bond configuration"),
            Self::Instance => formatter.write_str("Attachment requires the explicit only destination placement"),
            Self::Direction => formatter.write_str("Attachment direction must be finite and nonzero"),
            Self::Terminal => formatter.write_str("Replacement requires exactly one non-query single outside bond"),
        }
    }
}

// Templates cannot implicitly edit hidden members; expand the display first.
fn refuse_display_target(
    destination: &Document,
    instance: ObjectId,
    target: AtomId,
) -> Result<(), TemplateError> {
    if destination
        .fragment_displays()
        .any(|(key, display)| key.instance == instance && display.members.contains(&target))
        || destination.abbreviations().any(|(key, terminal)| {
            key.instance == instance && (key.atom == target || terminal == target)
        })
    {
        return Err(FragmentError::AbbreviationDisplay.into());
    }
    Ok(())
}

/// Replace a terminal atom's state and bind the template there without deleting its identity.
///
/// # Errors
/// Refuses non-terminal/non-single/query boundaries, unsupported source anchors, style/placement
/// mismatches and the ordinary native graph/coordinate/identifier/transaction limits.
pub fn replace(
    fragment: &Fragment,
    destination: &Document,
    instance: ObjectId,
    source_anchor: AtomId,
    target: AtomId,
) -> Result<ReplacementPlan, TemplateError> {
    let anchors = inspect(fragment)?;
    let anchor = anchors
        .iter()
        .find(|anchor| anchor.atom == source_anchor)
        .ok_or(TemplateError::AnchorUnknown)?;
    if !anchor.supported {
        return Err(TemplateError::AnchorStereo);
    }
    if destination.instances().count() != 1 || destination.first_instance() != instance {
        return Err(TemplateError::Instance);
    }
    refuse_display_target(destination, instance, target)?;
    let origin = destination
        .position(instance, target)
        .ok_or(TemplateError::TargetUnknown(target))?;
    let mut incident = destination
        .bonds()
        .filter(|(_, bond)| bond.begin == target || bond.end == target);
    let (boundary, outside) = incident.next().ok_or(TemplateError::Terminal)?;
    if incident.next().is_some() || outside.order != BondOrder::Single {
        return Err(TemplateError::Terminal);
    }
    let source = fragment
        .atoms
        .iter()
        .find(|atom| atom.id == source_anchor)
        .ok_or(TemplateError::AnchorUnknown)?;
    let paste = fragment.replace_anchor_commands(
        destination,
        Vector {
            dx: origin.x() - source.position.x(),
            dy: origin.y() - source.position.y(),
        },
        source_anchor,
        target,
    )?;
    Ok(ReplacementPlan { paste, boundary })
}
impl std::error::Error for TemplateError {}
impl From<FragmentError> for TemplateError {
    fn from(error: FragmentError) -> Self {
        Self::Fragment(error)
    }
}

/// Validate the native fragment and list explicit anchors; no IDs are reserved or content edited.
pub fn inspect(fragment: &Fragment) -> Result<Vec<Anchor>, TemplateError> {
    let scratch = Document::new_with_settings(
        DocumentId::parse("template-validation").expect("constant identifier"),
        "Template validation",
        fragment.settings.clone(),
    )
    .map_err(|error| TemplateError::Fragment(FragmentError::Invalid(error)))?;
    fragment.paste_commands(&scratch, Vector { dx: 0.0, dy: 0.0 })?;
    let first = fragment
        .atoms
        .first()
        .ok_or(TemplateError::Disconnected)?
        .id;
    let mut seen = BTreeSet::from([first]);
    let mut pending = vec![first];
    while let Some(atom) = pending.pop() {
        for bond in &fragment.bonds {
            let next = if bond.bond.begin == atom {
                Some(bond.bond.end)
            } else if bond.bond.end == atom {
                Some(bond.bond.begin)
            } else {
                None
            };
            if let Some(next) = next
                && seen.insert(next)
            {
                pending.push(next);
            }
        }
    }
    if seen.len() != fragment.atoms.len() {
        return Err(TemplateError::Disconnected);
    }
    Ok(fragment
        .atoms
        .iter()
        .map(|atom| Anchor {
            atom: atom.id,
            supported: !fragment.stereo.iter().any(|record| match record.key {
                StereoKey::Center(center) => center == atom.id,
                StereoKey::DoubleBond(id) => fragment.bonds.iter().any(|bond| {
                    bond.id == id && (bond.bond.begin == atom.id || bond.bond.end == atom.id)
                }),
            }),
        })
        .collect())
}

/// Plan translation-only insertion and one ordinary AddBond. Source-anchor stereo loss is refused.
pub fn attach(
    fragment: &Fragment,
    destination: &Document,
    instance: ObjectId,
    source_anchor: AtomId,
    target: AtomId,
    direction: Vector,
) -> Result<AttachmentPlan, TemplateError> {
    let anchors = inspect(fragment)?;
    let anchor = anchors
        .iter()
        .find(|anchor| anchor.atom == source_anchor)
        .ok_or(TemplateError::AnchorUnknown)?;
    if !anchor.supported {
        return Err(TemplateError::AnchorStereo);
    }
    if destination.instances().count() != 1 || destination.first_instance() != instance {
        return Err(TemplateError::Instance);
    }
    refuse_display_target(destination, instance, target)?;
    let origin = destination
        .position(instance, target)
        .ok_or(TemplateError::TargetUnknown(target))?;
    let scale = direction.dx.abs().max(direction.dy.abs());
    if !direction.dx.is_finite() || !direction.dy.is_finite() || scale == 0.0 {
        return Err(TemplateError::Direction);
    }
    let dx = direction.dx / scale;
    let dy = direction.dy / scale;
    let norm = dx.hypot(dy);
    let source = fragment
        .atoms
        .iter()
        .find(|atom| atom.id == source_anchor)
        .ok_or(TemplateError::AnchorUnknown)?;
    let length = destination.settings().bond_length_pt;
    let offset = Vector {
        dx: origin.x() + length * (dx / norm) - source.position.x(),
        dy: origin.y() + length * (dy / norm) - source.position.y(),
    };
    let mut paste = fragment.paste_commands(destination, offset)?;
    if paste.commands.len() >= MAX_COMMANDS {
        return Err(FragmentError::CommandLimit.into());
    }
    let mapped = paste
        .atoms
        .iter()
        .find(|atom| atom.source == source_anchor)
        .ok_or(TemplateError::AnchorUnknown)?
        .target;
    let serial = destination
        .numbering()
        .next_bond
        .checked_add(paste.bonds.len() as u32)
        .and_then(BondId::from_serial)
        .ok_or(TemplateError::Fragment(FragmentError::IdentifierLimit))?;
    paste.commands.push(Command::AddBond {
        begin: AtomRef::Id(target),
        end: AtomRef::Id(mapped),
        order: BondOrder::Single,
    });
    let mut candidate = destination.clone();
    candidate
        .apply(&paste.commands)
        .map_err(|error| TemplateError::Fragment(FragmentError::Invalid(error.error)))?;
    Ok(AttachmentPlan {
        paste,
        connection: serial,
    })
}
