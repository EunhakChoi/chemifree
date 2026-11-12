//! Stereo marks, configurations, groups and the stereo snapshot view.

use chemifree_domain::document::Document;
use chemifree_domain::stereo::{
    BondEnd, CenterState, DoubleBondState, Relation, Slot, StereoError, StereoMark, StereoRecord,
    Turn,
};
use chemifree_domain::stereo_group::GroupKind;
use serde::{Deserialize, Serialize};

/// What an enhanced-stereo group states. `absolute` has no number.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StereoGroupKindDto {
    Absolute,
    And,
    Or,
}

impl From<StereoGroupKindDto> for GroupKind {
    fn from(kind: StereoGroupKindDto) -> Self {
        match kind {
            StereoGroupKindDto::Absolute => Self::Absolute,
            StereoGroupKindDto::And => Self::And,
            StereoGroupKindDto::Or => Self::Or,
        }
    }
}

impl From<GroupKind> for StereoGroupKindDto {
    fn from(kind: GroupKind) -> Self {
        match kind {
            GroupKind::Absolute => Self::Absolute,
            GroupKind::And => Self::And,
            GroupKind::Or => Self::Or,
        }
    }
}

/// A bond mark, as snapshots show it and `set_bond_mark` states it. `begin` and `end` in a mark
/// are the bond's own `begin` and `end`; the bond is never rewritten to move the narrow end.
/// The public functional model's `wedge_begin` and `hash_end` display values name the same
/// facts as `wedge` and `hash` with `narrow`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MarkDto {
    /// Solid wedge on a single bond.
    Wedge { narrow: BondEndDto },
    /// Hashed wedge on a single bond.
    Hash { narrow: BondEndDto },
    /// Wavy single bond: unknown or either. No narrow end.
    Wavy {},
    /// Crossed double bond: unknown or either. No narrow end.
    Crossed {},
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BondEndDto {
    Begin,
    End,
}

/// The stated stereo configurations of a document: centers by atom id, then double bonds by
/// bond id, each with its slots in canonical order.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct StereoView {
    pub atoms: Vec<AtomStereoView>,
    pub bonds: Vec<BondStereoView>,
    /// Enhanced-stereo groups in identifier order. Added within v1; always present, empty when
    /// the document has none.
    pub groups: Vec<StereoGroupView>,
}

/// One enhanced-stereo group. `stored_number` is the number as it was stated (`null` when none;
/// `0` and negatives are kept), `number` the interpreted one (`null` for `absolute`, otherwise
/// the stored number or `0`), `label` the display text built from kind and interpreted number
/// (`abs`, `and2`, `or0`). `members` are atom IDs, ascending, each a stored tetrahedral center.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct StereoGroupView {
    pub id: String,
    pub kind: StereoGroupKindDto,
    pub stored_number: Option<i32>,
    pub number: Option<i32>,
    pub label: String,
    pub members: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AtomStereoView {
    pub atom: String,
    pub configuration: AtomConfigurationDto,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct BondStereoView {
    pub bond: String,
    pub configuration: BondConfigurationDto,
}

/// What is stated about a tetrahedral stereocenter. `neighbors` are four distinct slots in
/// canonical order: atom ids ascending, then `implicit_hydrogen`, then `lone_pair`. `turn` is
/// seen looking from the first slot toward the center: `anticlockwise` is OpenSMILES `@` (the
/// public model's positive `parity`), `clockwise` is `@@` (negative).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AtomConfigurationDto {
    Tetrahedral {
        neighbors: [String; 4],
        turn: TurnDto,
    },
    /// Stated unknown over this neighbor set.
    Unknown { neighbors: [String; 4] },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnDto {
    Anticlockwise,
    Clockwise,
}

/// What is stated about a double bond. The neighbors are in canonical form: the
/// lowest-identifier explicit neighbor of the end, or `implicit_hydrogen` where the end has no
/// other explicit neighbor. `opposite_sides` is the public model's `opposite_side`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BondConfigurationDto {
    Relative {
        begin_neighbor: String,
        end_neighbor: String,
        relation: RelationDto,
    },
    /// Stated unknown or either.
    Unknown {},
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationDto {
    SameSide,
    OppositeSides,
}

impl From<&MarkDto> for StereoMark {
    fn from(mark: &MarkDto) -> Self {
        let end = |narrow| match narrow {
            BondEndDto::Begin => BondEnd::Begin,
            BondEndDto::End => BondEnd::End,
        };
        match mark {
            MarkDto::Wedge { narrow } => Self::Wedge {
                narrow: end(*narrow),
            },
            MarkDto::Hash { narrow } => Self::Hash {
                narrow: end(*narrow),
            },
            MarkDto::Wavy {} => Self::Wavy,
            MarkDto::Crossed {} => Self::Crossed,
        }
    }
}

impl From<TurnDto> for Turn {
    fn from(turn: TurnDto) -> Self {
        match turn {
            TurnDto::Anticlockwise => Self::Anticlockwise,
            TurnDto::Clockwise => Self::Clockwise,
        }
    }
}

impl From<Turn> for TurnDto {
    fn from(turn: Turn) -> Self {
        match turn {
            Turn::Anticlockwise => Self::Anticlockwise,
            Turn::Clockwise => Self::Clockwise,
        }
    }
}

impl From<RelationDto> for Relation {
    fn from(relation: RelationDto) -> Self {
        match relation {
            RelationDto::SameSide => Self::SameSide,
            RelationDto::OppositeSides => Self::OppositeSides,
        }
    }
}

impl From<Relation> for RelationDto {
    fn from(relation: Relation) -> Self {
        match relation {
            Relation::SameSide => Self::SameSide,
            Relation::OppositeSides => Self::OppositeSides,
        }
    }
}

impl From<StereoMark> for MarkDto {
    fn from(mark: StereoMark) -> Self {
        let end = |narrow| match narrow {
            BondEnd::Begin => BondEndDto::Begin,
            BondEnd::End => BondEndDto::End,
        };
        match mark {
            StereoMark::Wedge { narrow } => Self::Wedge {
                narrow: end(narrow),
            },
            StereoMark::Hash { narrow } => Self::Hash {
                narrow: end(narrow),
            },
            StereoMark::Wavy => Self::Wavy {},
            StereoMark::Crossed => Self::Crossed {},
        }
    }
}

/// The wire spellings of the two slots that are not atoms. An atom id can never equal either.
pub(super) const IMPLICIT_HYDROGEN: &str = "implicit_hydrogen";
pub(super) const LONE_PAIR: &str = "lone_pair";

/// A slot as the wire spells it: an atom id, `implicit_hydrogen`, or `lone_pair`.
pub(crate) fn slot_name(slot: Slot) -> String {
    match slot {
        Slot::Atom(atom) => atom.to_string(),
        Slot::ImplicitHydrogen => IMPLICIT_HYDROGEN.to_owned(),
        Slot::LonePair => LONE_PAIR.to_owned(),
    }
}

/// A center state as snapshots and commands spell it.
pub fn center_dto(state: &CenterState) -> AtomConfigurationDto {
    match state {
        CenterState::Tetrahedral { slots, turn } => AtomConfigurationDto::Tetrahedral {
            neighbors: slots.map(slot_name),
            turn: (*turn).into(),
        },
        CenterState::Unknown { slots } => AtomConfigurationDto::Unknown {
            neighbors: slots.map(slot_name),
        },
    }
}

/// A double-bond state as snapshots and commands spell it.
pub fn double_bond_dto(state: &DoubleBondState) -> BondConfigurationDto {
    match state {
        DoubleBondState::Relative {
            begin_ref,
            end_ref,
            relation,
        } => BondConfigurationDto::Relative {
            begin_neighbor: slot_name(*begin_ref),
            end_neighbor: slot_name(*end_ref),
            relation: (*relation).into(),
        },
        DoubleBondState::Unknown => BondConfigurationDto::Unknown {},
    }
}

impl StereoView {
    /// The document's records as snapshots show them.
    ///
    /// # Errors
    ///
    /// [`StereoError::RecordInvalid`] when a record's kind does not match its key. A document
    /// never holds one (restore and every edit keep them equal), so the view refuses to be
    /// built rather than leave a stored record out of a snapshot.
    pub fn try_new(document: &Document) -> Result<Self, StereoError> {
        let mut view = Self::default();
        for record in document.stereo().typed()? {
            match record {
                StereoRecord::Center { atom, state } => view.atoms.push(AtomStereoView {
                    atom: atom.to_string(),
                    configuration: center_dto(state),
                }),
                StereoRecord::DoubleBond { bond, state } => view.bonds.push(BondStereoView {
                    bond: bond.to_string(),
                    configuration: double_bond_dto(state),
                }),
            }
        }
        view.groups = document
            .stereo_groups()
            .iter()
            .map(|(id, group)| StereoGroupView {
                id: id.to_string(),
                kind: group.kind().into(),
                stored_number: group.stored_number(),
                number: group.interpreted_number(),
                label: group.label(),
                members: group.members().iter().map(ToString::to_string).collect(),
            })
            .collect();
        Ok(view)
    }
}
