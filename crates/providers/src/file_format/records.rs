//! Stored atom, bond, stereo and history records shared across versions, the version 2 and 3
//! document and draft shells, and the record traits migration reads through.

use chemifree_domain::document::{Atom, Bond};
use serde::{Deserialize, Serialize};

/// A document file whose atom and bond states are `A` and `B`; versions 2 and 3 differ only
/// there.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct File<A, B> {
    format: String,
    format_version: u32,
    pub(super) document: StoredDocument<A, B>,
    pub(super) history: StoredHistory<A, B>,
}

/// A recovery draft: the working copy with the stored revision it started from.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DraftFile<A, B> {
    format: String,
    format_version: u32,
    pub(super) base_revision: u64,
    pub(super) saved_at_ms: u64,
    pub(super) document: StoredDocument<A, B>,
    pub(super) history: StoredHistory<A, B>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StoredDocument<A, B> {
    pub(super) id: String,
    pub(super) revision: u64,
    pub(super) title: String,
    pub(super) molecule: StoredMolecule<A, B>,
    pub(super) pages: Vec<StoredPage>,
    /// In order; the v1 view reports the first one's positions as `atoms[].position`.
    pub(super) instances: Vec<StoredInstance>,
    pub(super) next_atom: u32,
    pub(super) next_bond: u32,
    pub(super) next_instance: u32,
}

/// The chemical graph: atoms and bonds without positions. Atoms and bonds are
/// `{"id": ..., "state": {...}}`, matching history entries.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StoredMolecule<A, B> {
    pub(super) id: String,
    pub(super) atoms: Vec<Stored<A>>,
    pub(super) bonds: Vec<Stored<B>>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Stored<T> {
    pub(super) id: String,
    pub(super) state: T,
}

/// A version 3 atom: exactly one of `element` and `query`.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AtomState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) element: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) query: Option<StoredQuery>,
    pub(super) charge: i8,
    pub(super) isotope: Option<u16>,
    pub(super) hydrogens: StoredHydrogens,
    pub(super) radical: StoredRadical,
    pub(super) attachment_points: Vec<u8>,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum StoredQuery {
    Any {},
    AnyHeavy {},
    Heteroatom {},
    Halogen {},
    List {
        elements: Vec<String>,
        negated: bool,
    },
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum StoredHydrogens {
    Inferred {},
    Unknown {},
    Specified { count: u8 },
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum StoredRadical {
    None,
    Singlet,
    Doublet,
    Triplet,
    Unknown,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StoredPage {
    pub(super) id: String,
}

/// A molecule scene instance with one position per atom of its molecule.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StoredInstance {
    pub(super) id: String,
    pub(super) page: String,
    pub(super) molecule: String,
    pub(super) positions: Vec<StoredPosition>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StoredPosition {
    pub(super) atom: String,
    pub(super) x: f64,
    pub(super) y: f64,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StoredPoint {
    pub(super) x: f64,
    pub(super) y: f64,
}

/// A bond of versions 3 to 5: `query` is present exactly when `order` is `query`. These
/// versions have no stereo mark, so this shape refuses one.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct UnmarkedBondState {
    pub(super) begin: String,
    pub(super) end: String,
    pub(super) order: StoredOrder,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) query: Option<StoredBondQuery>,
}

/// A version 6 bond: as before, and `mark` when the bond is drawn as a stereo depiction.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BondState {
    pub(super) begin: String,
    pub(super) end: String,
    pub(super) order: StoredOrder,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) query: Option<StoredBondQuery>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) mark: Option<StoredMark>,
}

impl From<UnmarkedBondState> for BondState {
    fn from(value: UnmarkedBondState) -> Self {
        Self {
            begin: value.begin,
            end: value.end,
            order: value.order,
            query: value.query,
            mark: None,
        }
    }
}

/// How a bond is drawn. A wedge or hash records which end of the bond is narrow.
#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum StoredMark {
    Wedge { narrow: StoredBondEnd },
    Hash { narrow: StoredBondEnd },
    Wavy {},
    Crossed {},
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum StoredBondEnd {
    Begin,
    End,
}

/// One stated configuration. Its record's id is an atom's for a center and a bond's for a double
/// bond; `Document::restore` checks that the kind matches.
#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum StoredStereoState {
    Center { state: StoredCenter },
    DoubleBond { state: StoredDoubleBond },
}

/// A center's neighbor slots are atom ids, `implicit_hydrogen`, or `lone_pair`, in canonical
/// order (atoms ascending, then hydrogen, then lone pair).
#[derive(Deserialize, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum StoredCenter {
    Tetrahedral {
        neighbors: [String; 4],
        turn: StoredTurn,
    },
    Unknown {
        neighbors: [String; 4],
    },
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum StoredTurn {
    Anticlockwise,
    Clockwise,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum StoredDoubleBond {
    Relative {
        begin_ref: String,
        end_ref: String,
        relation: StoredRelation,
    },
    Unknown {},
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum StoredRelation {
    SameSide,
    OppositeSides,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum StoredOrder {
    Single,
    Double,
    Triple,
    Aromatic,
    Dative,
    Query,
    Unknown,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum StoredBondQuery {
    Any {},
    List { orders: Vec<StoredQueryOrder> },
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum StoredQueryOrder {
    Single,
    Double,
    Triple,
    Aromatic,
}

/// `undo` is oldest first; `redo` is a stack whose last entry is redone next.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StoredHistory<A, B> {
    pub(super) undo: Vec<StoredChangeSet<A, B>>,
    pub(super) redo: Vec<StoredChangeSet<A, B>>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StoredChangeSet<A, B> {
    pub(super) title: Option<StoredChange<String>>,
    pub(super) atoms: Vec<Keyed<A>>,
    pub(super) bonds: Vec<Keyed<B>>,
    pub(super) instances: Vec<Keyed<InstanceState>>,
    pub(super) positions: Vec<PositionChange>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StoredChange<T> {
    pub(super) before: T,
    pub(super) after: T,
}

/// One entity's change; `null` means the entity did not exist on that side.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Keyed<T> {
    pub(super) id: String,
    pub(super) before: Option<T>,
    pub(super) after: Option<T>,
}

/// An instance's parents. Its positions change through `positions` entries.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct InstanceState {
    pub(super) page: String,
    pub(super) molecule: String,
}

/// One atom's position in one instance; `null` where it had none.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PositionChange {
    pub(super) instance: String,
    pub(super) atom: String,
    pub(super) before: Option<StoredPoint>,
    pub(super) after: Option<StoredPoint>,
}

/// A stored atom state of any version, as a domain atom.
pub(super) trait AtomRecord {
    fn into_atom(self) -> Result<Atom, String>;
}

/// A stored bond state of any version, as a domain bond.
pub(super) trait BondRecord {
    fn into_bond(self) -> Result<Bond, String>;
}
