//! Readers from wire fields to domain values, and the refusal they report.

use chemifree_domain::document::{
    AtomId, AtomRef, BondId, BondOrder, BondRef, GroupId, GroupRef, ObjectId, Point, Vector,
};
use chemifree_domain::element::Element;
use chemifree_domain::scene::{
    Caption, CaptionRun, CaptionScript, CaptionStyle, CaptionWeight, ObjectRef, ReferenceTarget,
    SceneError, SceneReference, Selection, SelectionRequest, TransactionSelection,
};
use chemifree_domain::settings::LabelFont;
use chemifree_domain::states::{
    AtomKind, AttachmentPoints, BondQuery, Hydrogens, QueryAtom, StateError,
};
use chemifree_domain::stereo::{AtomConfiguration, BondConfiguration, SlotRef};

use crate::error::{ApplicationError, ErrorCode};

use super::chemistry::{BondOrderDto, BondQueryDto, HydrogensDto, QueryDto};
use super::request::{PointDto, SelectionDto, TransactionSelections};
use super::scene::{
    CaptionAlignmentDto, CaptionRunDto, CaptionScriptDto, CaptionStyleDto, CaptionWeightDto,
    ReferenceTargetDto,
};
use super::settings::LabelFontDto;
use super::stereo::{AtomConfigurationDto, BondConfigurationDto, IMPLICIT_HYDROGEN, LONE_PAIR};

/// Why a command could not be converted: a message, and the refused combination when there
/// is one.
pub(super) struct Refusal {
    pub(super) message: String,
    pub(super) state: Option<StateError>,
    pub(super) scene: Option<SceneError>,
    /// The code of a stereo group value the wire refuses (`stereo_group_number_range`).
    pub(super) group: Option<&'static str>,
}

impl From<String> for Refusal {
    fn from(message: String) -> Self {
        Self {
            message,
            state: None,
            scene: None,
            group: None,
        }
    }
}

impl From<StateError> for Refusal {
    fn from(state: StateError) -> Self {
        Self {
            message: state.to_string(),
            state: Some(state),
            scene: None,
            group: None,
        }
    }
}

/// The stored number of a group: any integer of the 32-bit signed range, `0` and negatives
/// included, or none.
pub(super) fn group_number(number: Option<i64>) -> Result<Option<i32>, Refusal> {
    number
        .map(|value| {
            i32::try_from(value).map_err(|_| Refusal {
                message: format!(
                    "{value} is outside the stereo group number range {} to {}",
                    i32::MIN,
                    i32::MAX
                ),
                state: None,
                scene: None,
                group: Some(GROUP_NUMBER_RANGE),
            })
        })
        .transpose()
}

/// The `reason` of a stereo group number outside the 32-bit signed range. The domain never
/// sees such a number, so this is the wire's own code, listed beside the domain's.
const GROUP_NUMBER_RANGE: &str = "stereo_group_number_range";

pub(super) fn atom_refs(values: &[String]) -> Result<Vec<AtomRef>, String> {
    values.iter().map(|id| atom_ref(id)).collect()
}

/// A group ID (`e1`), or `$N` for the group created by command N.
pub(super) fn group_ref(value: &str) -> Result<GroupRef, String> {
    match value.strip_prefix('$') {
        Some(index) if !index.is_empty() && index.bytes().all(|b| b.is_ascii_digit()) => index
            .parse()
            .map(GroupRef::Created)
            .map_err(|_| format!("{value:?} is not a command reference")),
        Some(_) => Err(format!("{value:?} is not a command reference")),
        None => GroupId::parse(value)
            .map(GroupRef::Id)
            .ok_or_else(|| format!("{value:?} is not a stereo group identifier")),
    }
}

pub(super) fn atom_configuration(dto: &AtomConfigurationDto) -> Result<AtomConfiguration, String> {
    Ok(match dto {
        AtomConfigurationDto::Tetrahedral { neighbors, turn } => AtomConfiguration::Tetrahedral {
            neighbors: slot_refs(neighbors)?,
            turn: (*turn).into(),
        },
        AtomConfigurationDto::Unknown { neighbors } => AtomConfiguration::Unknown {
            neighbors: slot_refs(neighbors)?,
        },
    })
}

pub(super) fn bond_configuration(dto: &BondConfigurationDto) -> Result<BondConfiguration, String> {
    Ok(match dto {
        BondConfigurationDto::Relative {
            begin_neighbor,
            end_neighbor,
            relation,
        } => BondConfiguration::Relative {
            begin_neighbor: slot_ref(begin_neighbor)?,
            end_neighbor: slot_ref(end_neighbor)?,
            relation: (*relation).into(),
        },
        BondConfigurationDto::Unknown {} => BondConfiguration::Unknown,
    })
}

/// A neighbor position as a command names it: `implicit_hydrogen`, `lone_pair`, or an atom
/// (`a3`, or `$N`). An atom identifier can never equal either word.
fn slot_ref(value: &str) -> Result<SlotRef, String> {
    match value {
        IMPLICIT_HYDROGEN => Ok(SlotRef::ImplicitHydrogen),
        LONE_PAIR => Ok(SlotRef::LonePair),
        _ => atom_ref(value).map(SlotRef::Atom),
    }
}

fn slot_refs(values: &[String; 4]) -> Result<[SlotRef; 4], String> {
    let mut slots = [SlotRef::ImplicitHydrogen; 4];
    for (slot, value) in slots.iter_mut().zip(values) {
        *slot = slot_ref(value)?;
    }
    Ok(slots)
}

/// The kind named by `element` or `query`, at most one of them; `None` when neither is given.
pub(super) fn atom_kind(
    symbol: Option<&str>,
    query: Option<&QueryDto>,
) -> Result<Option<AtomKind>, Refusal> {
    Ok(match (symbol, query) {
        (Some(_), Some(_)) => return Err(StateError::AtomKindConflict.into()),
        (Some(symbol), None) => Some(AtomKind::Element(element(symbol)?)),
        (None, Some(query)) => Some(AtomKind::Query(query_atom(query)?)),
        (None, None) => None,
    })
}

/// The domain checks the list's size and repeats.
pub(crate) fn query_atom(query: &QueryDto) -> Result<QueryAtom, String> {
    Ok(match query {
        QueryDto::Any {} => QueryAtom::Any,
        QueryDto::AnyHeavy {} => QueryAtom::AnyHeavy,
        QueryDto::Heteroatom {} => QueryAtom::Heteroatom,
        QueryDto::Halogen {} => QueryAtom::Halogen,
        QueryDto::List { elements, negated } => QueryAtom::List {
            elements: elements
                .iter()
                .map(|symbol| element(symbol))
                .collect::<Result<_, _>>()?,
            negated: *negated,
        },
    })
}

/// A bond order with its query, which `query` and only `query` carries. The domain checks the
/// query's list.
pub(crate) fn bond_order(
    order: BondOrderDto,
    query: Option<&BondQueryDto>,
) -> Result<BondOrder, StateError> {
    Ok(match (order, query) {
        (BondOrderDto::Query, Some(BondQueryDto::Any {})) => BondOrder::Query(BondQuery::Any),
        (BondOrderDto::Query, Some(BondQueryDto::List { orders })) => BondOrder::Query(
            BondQuery::OneOf(orders.iter().map(|order| (*order).into()).collect()),
        ),
        (BondOrderDto::Query, None) => return Err(StateError::BondQueryMissing),
        (_, Some(_)) => return Err(StateError::BondQueryUnexpected),
        (BondOrderDto::Single, None) => BondOrder::Single,
        (BondOrderDto::Double, None) => BondOrder::Double,
        (BondOrderDto::Triple, None) => BondOrder::Triple,
        (BondOrderDto::Aromatic, None) => BondOrder::Aromatic,
        (BondOrderDto::Dative, None) => BondOrder::Dative,
        (BondOrderDto::Unknown, None) => BondOrder::Unknown,
    })
}

/// The domain checks the stated count's range; this only narrows the wire integer.
pub(crate) fn to_hydrogens(hydrogens: HydrogensDto) -> Result<Hydrogens, StateError> {
    Ok(match hydrogens {
        HydrogensDto::Inferred {} => Hydrogens::Inferred,
        HydrogensDto::Unknown {} => Hydrogens::Unknown,
        HydrogensDto::Specified { count } => Hydrogens::Specified(
            u8::try_from(count).map_err(|_| StateError::HydrogenCountOutOfRange)?,
        ),
    })
}

pub(crate) fn attachment_points(values: &[u64]) -> Result<AttachmentPoints, StateError> {
    let narrowed = values
        .iter()
        .map(|value| u8::try_from(*value))
        .collect::<Result<Vec<u8>, _>>()
        .map_err(|_| StateError::AttachmentPointsInvalid)?;
    AttachmentPoints::new(&narrowed)
}

/// The domain checks the translated positions.
pub(super) fn vector(delta: PointDto) -> Vector {
    Vector {
        dx: delta.x,
        dy: delta.y,
    }
}

pub(crate) fn instance_id(value: &str) -> Result<ObjectId, String> {
    ObjectId::parse(value).ok_or_else(|| format!("{value:?} is not a scene instance identifier"))
}

pub(super) fn reference(
    source: &str,
    target: &ReferenceTargetDto,
) -> Result<SceneReference, String> {
    let target = match target {
        ReferenceTargetDto::Object { id } => ReferenceTarget::Object(instance_id(id)?),
        ReferenceTargetDto::Atom { id } => ReferenceTarget::Atom(atom_id(id)?),
        ReferenceTargetDto::Bond { id } => ReferenceTarget::Bond(bond_id(id)?),
    };
    Ok(SceneReference {
        source: instance_id(source)?,
        target,
    })
}

pub(crate) fn exact_selection(value: &SelectionDto) -> Result<Selection, String> {
    Ok(Selection {
        objects: value
            .objects
            .iter()
            .map(|id| instance_id(id))
            .collect::<Result<_, _>>()?,
        atoms: value
            .atoms
            .iter()
            .map(|id| atom_id(id))
            .collect::<Result<_, _>>()?,
        bonds: value
            .bonds
            .iter()
            .map(|id| bond_id(id))
            .collect::<Result<_, _>>()?,
    })
}

pub(crate) fn to_selection(
    selection: &TransactionSelections,
) -> Result<TransactionSelection, ApplicationError> {
    fn after(value: &SelectionDto) -> Result<SelectionRequest, String> {
        Ok(SelectionRequest {
            objects: value
                .objects
                .iter()
                .map(|id| object_ref(id))
                .collect::<Result<_, _>>()?,
            atoms: value
                .atoms
                .iter()
                .map(|id| atom_id(id))
                .collect::<Result<_, _>>()?,
            bonds: value
                .bonds
                .iter()
                .map(|id| bond_id(id))
                .collect::<Result<_, _>>()?,
        })
    }
    let result = || -> Result<TransactionSelection, String> {
        Ok(TransactionSelection {
            before: selection.before.as_ref().map(exact_selection).transpose()?,
            after: selection.after.as_ref().map(after).transpose()?,
        })
    };
    result().map_err(|message| ApplicationError {
        invalid_scene: Some(SceneError::SelectionUnknown),
        ..ApplicationError::new(
            ErrorCode::InvalidCommand,
            format!("invalid selection: {message}"),
        )
    })
}

fn object_ref(value: &str) -> Result<ObjectRef, String> {
    match value.strip_prefix('$') {
        Some(index) if !index.is_empty() && index.bytes().all(|b| b.is_ascii_digit()) => index
            .parse()
            .map(ObjectRef::Created)
            .map_err(|_| format!("{value:?} is not a command reference")),
        Some(_) => Err(format!("{value:?} is not a command reference")),
        None => instance_id(value).map(ObjectRef::Id),
    }
}

fn element(symbol: &str) -> Result<Element, String> {
    Element::from_symbol(symbol).ok_or_else(|| format!("{symbol:?} is not an element symbol"))
}

pub(crate) fn point(position: PointDto) -> Result<Point, String> {
    Point::new(position.x, position.y).map_err(|error| error.to_string())
}

pub(super) fn atom_id(value: &str) -> Result<AtomId, String> {
    AtomId::parse(value).ok_or_else(|| format!("{value:?} is not an atom identifier"))
}

pub(super) fn atom_ref(value: &str) -> Result<AtomRef, String> {
    match value.strip_prefix('$') {
        Some(index) if !index.is_empty() && index.bytes().all(|b| b.is_ascii_digit()) => index
            .parse()
            .map(AtomRef::Created)
            .map_err(|_| format!("{value:?} is not a command reference")),
        Some(_) => Err(format!("{value:?} is not a command reference")),
        None => atom_id(value).map(AtomRef::Id),
    }
}

pub(super) fn bond_id(value: &str) -> Result<BondId, String> {
    BondId::parse(value).ok_or_else(|| format!("{value:?} is not a bond identifier"))
}

/// A bond ID, or `$N` for the bond created by command N.
pub(super) fn bond_ref(value: &str) -> Result<BondRef, String> {
    match value.strip_prefix('$') {
        Some(index) if !index.is_empty() && index.bytes().all(|b| b.is_ascii_digit()) => index
            .parse()
            .map(BondRef::Created)
            .map_err(|_| format!("{value:?} is not a command reference")),
        Some(_) => Err(format!("{value:?} is not a command reference")),
        None => bond_id(value).map(BondRef::Id),
    }
}

// Coordinate conversion follows the other scene commands: its String becomes a Refusal
// at to_command, and to_commands supplies the failing command index. Caption content is
// validated by the Domain so its typed caption_invalid reason is preserved there.
pub(super) fn caption(
    anchor: PointDto,
    alignment: CaptionAlignmentDto,
    runs: &[CaptionRunDto],
) -> Result<Caption, String> {
    Ok(Caption {
        alignment: alignment.into(),
        anchor: point(anchor)?,
        runs: runs
            .iter()
            .map(|run| CaptionRun {
                shadow: run.shadow,
                outline: run.outline,
                underline: run.underline,
                color: run.color.clone(),
                size_pt: run.size_pt,
                font: run.font.map(|font| match font {
                    LabelFontDto::SansSerif => LabelFont::SansSerif,
                    LabelFontDto::Serif => LabelFont::Serif,
                    LabelFontDto::Monospace => LabelFont::Monospace,
                }),
                text: run.text.clone(),
                style: match run.style {
                    CaptionStyleDto::Normal => CaptionStyle::Normal,
                    CaptionStyleDto::Italic => CaptionStyle::Italic,
                },
                weight: match run.weight {
                    CaptionWeightDto::Normal => CaptionWeight::Normal,
                    CaptionWeightDto::Bold => CaptionWeight::Bold,
                },
                script: match run.script {
                    CaptionScriptDto::Baseline => CaptionScript::Baseline,
                    CaptionScriptDto::Subscript => CaptionScript::Subscript,
                    CaptionScriptDto::Superscript => CaptionScript::Superscript,
                },
            })
            .collect(),
    })
}
