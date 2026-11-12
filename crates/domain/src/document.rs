//! A chemical drawing document: a titled chemical graph of atoms and bonds kept apart from
//! its scene, stable identifiers, atomic transactions, and change-set history.
//!
//! The graph (molecule [`MOLECULE_ID`]) holds atoms and bonds without positions. The scene
//! holds one page ([`PAGE_ID`]) with an ordered containment tree of scene objects (see
//! [`crate::scene`]): molecule scene instances, each drawing the graph on that page with exactly
//! one position per atom, groups, and simple graphics. Documents, molecules, atoms, bonds, scene
//! objects, and pages are separate identifier namespaces. A document has one graph and one page.
//! The selection is document state, recorded by history with every change.
//!
//! This is a drawing model. It records what the user drew and stated, including hydrogen,
//! radical, query, attachment-point, and bond-kind states (see [`crate::states`]), and enforces
//! structural invariants (no dangling or duplicate bonds, a position per atom in every
//! instance, bounded values, valid state combinations). The drawing model does not alter
//! recorded states to perform chemistry; interpretation belongs to separate domain rules.

mod abbreviation;
mod accessors;
mod apply;
mod change_set;
mod command;
mod content;
mod draft;
mod entities;
mod error;
mod fragment_display;
mod history;
mod lifecycle;
mod persistence;
pub use accessors::Numbering;
pub use change_set::{Change, ChangeSet};
pub use command::Command;
pub use content::validated_title;
pub use entities::{
    Atom, AtomEdit, AtomRef, Bond, BondOrder, BondRef, BondTo, FragmentDisplay, FragmentDisplayRun,
    GroupRef, NewAtomPositions, Point, PositionKey, RingPattern, RingSize, Vector,
};
pub use error::{Applied, EditError, HistoryError, Outcome, RevisionsExhausted, TransactionError};

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;

use crate::scene::{SceneObject, SceneReference, SceneTree, Selection};
use crate::settings::Settings;
use crate::stereo::{StereoKey, StereoState, StereoTable};
use crate::stereo_group::{StereoGroup, StereoGroupTable};

pub const MAX_ATOMS: usize = 2_000;
pub const MAX_BONDS: usize = 4_000;
/// Molecule scene instances per document, among its scene objects. With [`MAX_ATOMS`] this
/// bounds the positions a document holds.
pub const MAX_INSTANCES: usize = 20;
/// The identifier of a document's one chemical graph.
pub const MOLECULE_ID: &str = "m1";
/// The identifier of a document's one page.
pub const PAGE_ID: &str = "p1";
pub const MAX_COMMANDS: usize = 500;
pub const MAX_TITLE_CHARS: usize = 200;
/// Scene coordinates are points (1/72 inch); x grows right and y grows down.
pub const MAX_COORDINATE: f64 = 100_000.0;
pub const MIN_CHARGE: i8 = -8;
pub const MAX_CHARGE: i8 = 8;
pub const MAX_ISOTOPE_MASS_NUMBER: u16 = 999;
/// Undo entries beyond this depth are discarded, oldest first.
pub const HISTORY_LIMIT: usize = 100;
/// The largest revision. Clients exchange revisions as JSON numbers, which are exact only up
/// to 2^53 - 1.
pub const MAX_REVISION: u64 = (1 << 53) - 1;
/// The largest revision an edit, undo, or redo may reach. The one above it is kept for a copy
/// reopened after those changes are discarded, which must not name the stored commit with a
/// revision the discarded changes already used.
pub const MAX_EDIT_REVISION: u64 = MAX_REVISION - 1;
/// Entity changes kept across all history entries. With the entity limits this bounds the
/// stored history, so a saved document stays within what storage providers read back.
pub const HISTORY_CHANGE_LIMIT: usize = 100_000;
/// Shared current-state admission budget when variable-sized text, polygons or splines exist in content or history.
/// The caption-era public constant name is retained for compatibility.
/// Physical encoding also reserves per-run metadata; see persistence_weight.
pub const CAPTION_CONTENT_WEIGHT_LIMIT: usize = 48 * 1024 * 1024;
/// Shared whole-history admission budget for variable-sized text, polygons or splines. The latest edit must
/// fit by itself; older undo entries make room. Runs without shadow retain v16 admission
/// weights; physical encoding reserves at most 6/5 of history admission.
pub const CAPTION_HISTORY_WEIGHT_LIMIT: usize = 8 * 1024 * 1024;
const MAX_DOCUMENT_ID_CHARS: usize = 64;

/// Storage-safe document identifier: 1-64 characters of `[a-z0-9-]`, starting alphanumeric.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DocumentId(String);

impl DocumentId {
    pub fn parse(value: &str) -> Option<Self> {
        let mut chars = value.chars();
        let first = chars.next()?;
        let valid = value.len() <= MAX_DOCUMENT_ID_CHARS
            && (first.is_ascii_lowercase() || first.is_ascii_digit())
            && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        valid.then(|| Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DocumentId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

macro_rules! serial_id {
    ($(#[$doc:meta])* $name:ident, $prefix:literal) => {
        $(#[$doc])*
        /// Document-scoped identifier. Serials are never reused within a document,
        /// including after deletion or undo.
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(u32);

        impl $name {
            /// Parses the canonical form, e.g. `a1`; zero and leading zeros are rejected.
            pub fn parse(value: &str) -> Option<Self> {
                let digits = value.strip_prefix($prefix)?;
                if digits.is_empty()
                    || digits.starts_with('0')
                    || !digits.bytes().all(|b| b.is_ascii_digit())
                {
                    return None;
                }
                digits.parse().ok().map(Self)
            }

            pub fn from_serial(serial: u32) -> Option<Self> {
                (serial > 0).then_some(Self(serial))
            }

            pub const fn serial(self) -> u32 {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(formatter, "{}{}", $prefix, self.0)
            }
        }
    };
}

serial_id!(AtomId, "a");
serial_id!(BondId, "b");
serial_id!(
    /// A scene object of any kind: a molecule scene instance, a group, or a graphic.
    ObjectId,
    "s"
);
serial_id!(
    /// An enhanced-stereo group (see [`crate::stereo_group`]).
    GroupId,
    "e"
);

#[derive(Clone, Copy)]
enum Direction {
    Backward,
    Forward,
}

#[derive(Clone, Debug, PartialEq)]
struct Content {
    fragment_displays: BTreeMap<PositionKey, FragmentDisplay>,
    abbreviations: BTreeMap<PositionKey, AtomId>,
    display_labels: BTreeMap<PositionKey, String>,
    title: String,
    settings: Settings,
    atoms: BTreeMap<AtomId, Atom>,
    bonds: BTreeMap<BondId, Bond>,
    scene: SceneTree,
    references: BTreeSet<SceneReference>,
    positions: BTreeMap<PositionKey, Point>,
    stereo: StereoTable,
    stereo_groups: StereoGroupTable,
}

/// What a command created, which later references (`$N`) can name.
#[derive(Clone, Copy)]
enum Created {
    Nothing,
    Atom(AtomId),
    /// An `AddAtom` with `bond_to`: the new atom, and the bond it was added with. The index
    /// names the atom where an atom is expected and the bond where a bond is.
    AtomAndBond(AtomId, BondId),
    Bond(BondId),
    Object(ObjectId),
    Group(GroupId),
}

/// Mutable working state for one transaction; discarded unless every command succeeds.
struct Draft {
    content: Content,
    next_atom: u32,
    next_bond: u32,
    next_object: u32,
    next_group: u32,
    /// What each earlier command created, by command index.
    created: Vec<Created>,
    /// Stereo records removed because a change made them meaningless or deleted their atom or
    /// bond, not because a `Set*` command cleared them; it may include records created within
    /// the transaction, which the reply filters out. See [`Draft::discard_stereo`].
    cascaded: BTreeSet<StereoKey>,
}

fn unique<T: Copy + Ord>(ids: &[T]) -> Result<BTreeSet<T>, EditError> {
    if ids.is_empty() {
        Err(EditError::EmptyTargetList)
    } else {
        Ok(ids.iter().copied().collect())
    }
}

/// Persistable representation used by storage providers. `undo` is oldest first;
/// `redo` is a stack whose last entry is redone next.
#[derive(Clone, Debug, PartialEq)]
pub struct DocumentParts {
    /// Explicit graph membership and plain display text; no chemical interpretation.
    pub fragment_displays: Vec<(PositionKey, FragmentDisplay)>,
    /// `(instance, anchor)` to the hidden terminal carbon of an Et display.
    pub abbreviations: Vec<(PositionKey, AtomId)>,
    /// Optional cosmetic text; missing entries display Et. No chemical interpretation.
    pub display_labels: Vec<(PositionKey, String)>,
    pub id: DocumentId,
    pub revision: u64,
    pub title: String,
    pub settings: Settings,
    pub atoms: Vec<(AtomId, Atom)>,
    pub bonds: Vec<(BondId, Bond)>,
    /// The page's root objects in order. Every scene object is on page [`PAGE_ID`], and every
    /// instance draws molecule [`MOLECULE_ID`].
    pub roots: Vec<ObjectId>,
    pub objects: Vec<(ObjectId, SceneObject)>,
    pub references: Vec<SceneReference>,
    pub positions: Vec<(PositionKey, Point)>,
    /// Stated stereo configurations; marks are part of `bonds`.
    pub stereo: Vec<(StereoKey, StereoState)>,
    /// Enhanced-stereo groups.
    pub stereo_groups: Vec<(GroupId, StereoGroup)>,
    /// The current selection: where the history ends (see [`Document::restore`]).
    pub selection: Selection,
    pub next_atom: u32,
    pub next_bond: u32,
    pub next_object: u32,
    pub next_group: u32,
    pub undo: Vec<ChangeSet>,
    pub redo: Vec<ChangeSet>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RestoreError(String);

impl fmt::Display for RestoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for RestoreError {}

#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    id: DocumentId,
    revision: u64,
    content: Content,
    selection: Selection,
    next_atom: u32,
    next_bond: u32,
    next_object: u32,
    next_group: u32,
    undo: VecDeque<ChangeSet>,
    redo: Vec<ChangeSet>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stereo_group::GroupKind;

    #[test]
    fn a_group_change_counts_toward_the_history_limit() {
        let created = StereoGroup::new(GroupKind::Or, None, BTreeSet::from([AtomId(1)]));
        let set = ChangeSet {
            stereo_groups: vec![(
                GroupId(1),
                Change {
                    before: None,
                    after: Some(created),
                },
            )],
            ..ChangeSet::default()
        };
        assert_eq!(set.size(), 1, "one changed group is one counted change");
        assert!(!set.is_empty());
    }
}
