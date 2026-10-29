//! A chemical drawing document: a titled chemical graph of atoms and bonds kept apart from
//! its scene, stable identifiers, atomic transactions, and change-set history.
//!
//! The graph (molecule [`MOLECULE_ID`]) holds atoms and bonds without positions. The scene
//! holds one page ([`PAGE_ID`]) with a containment tree of scene objects (see
//! [`crate::scene`]): molecule scene instances, each drawing the graph with exactly one position
//! per atom, groups, and graphics. Documents, molecules, atoms, bonds, scene objects, and pages
//! are separate identifier namespaces. A document has one graph and one page; several graphs,
//! text, and other scene objects are not represented yet. The document also keeps the
//! selection its latest edit, undo, or redo left, which history entries record.
//!
//! This is a drawing model. It records what the user drew and stated, including hydrogen,
//! radical, query, attachment-point, and bond-kind states (see [`crate::states`]), and enforces
//! structural invariants (no dangling or duplicate bonds, a position per atom in every
//! instance, bounded values, valid state combinations). It does not perceive valence,
//! aromaticity, stereochemistry, or implicit hydrogens, and matches no query.

use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};
use std::fmt;

use crate::element::Element;
use crate::scene::{
    DeleteMode, Graphic, MAX_DEPTH, MAX_OBJECTS, ObjectKind, Parent, SceneError, SceneObject,
    Selection, Tree,
};
use crate::states::{
    AtomKind, AttachmentPoints, BondQuery, Hydrogens, MAX_STATED_HYDROGENS, Radical, StateError,
};

pub const MAX_ATOMS: usize = 2_000;
pub const MAX_BONDS: usize = 4_000;
/// Scene instances per document. With [`MAX_ATOMS`] this bounds the positions a document holds.
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
    ($name:ident, $prefix:literal) => {
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
// A scene object: a molecule scene instance, group, or graphic. They share one namespace.
serial_id!(ObjectId, "s");

/// A finite scene position in points, within ±[`MAX_COORDINATE`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    x: f64,
    y: f64,
}

impl Point {
    pub fn new(x: f64, y: f64) -> Result<Self, EditError> {
        let in_range = |value: f64| value.is_finite() && value.abs() <= MAX_COORDINATE;
        if in_range(x) && in_range(y) {
            Ok(Self { x, y })
        } else {
            Err(EditError::CoordinateOutOfRange)
        }
    }

    pub const fn x(self) -> f64 {
        self.x
    }

    pub const fn y(self) -> f64 {
        self.y
    }

    pub(crate) fn translated(self, delta: Vector) -> Result<Self, EditError> {
        Self::new(self.x + delta.dx, self.y + delta.dy)
    }
}

/// A displacement in points. Validity is checked on the translated position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vector {
    pub dx: f64,
    pub dy: f64,
}

/// An atom of the chemical graph. Its positions belong to the scene instances.
#[derive(Clone, Debug, PartialEq)]
pub struct Atom {
    /// An element, or a query atom (which carries no charge or isotope).
    pub kind: AtomKind,
    pub charge: i8,
    /// Explicit isotope mass number; `None` means natural abundance.
    pub isotope: Option<u16>,
    pub hydrogens: Hydrogens,
    pub radical: Radical,
    pub attachment_points: AttachmentPoints,
}

/// A neutral, natural-abundance atom of `element` with inferred hydrogens, no radical, and no
/// attachment points.
impl From<Element> for Atom {
    fn from(element: Element) -> Self {
        Self::new(AtomKind::Element(element))
    }
}

impl Atom {
    /// A neutral, natural-abundance atom of `kind` with inferred hydrogens, no radical, and no
    /// attachment points.
    pub fn new(kind: AtomKind) -> Self {
        Self {
            kind,
            charge: 0,
            isotope: None,
            hydrogens: Hydrogens::Inferred,
            radical: Radical::None,
            attachment_points: AttachmentPoints::default(),
        }
    }

    /// Checks every value and combination; edits, restored files, and replayed history all
    /// pass through here.
    fn validate(&self) -> Result<(), EditError> {
        validate_charge(self.charge)?;
        validate_isotope(self.isotope)?;
        if let Hydrogens::Specified(count) = self.hydrogens
            && count > MAX_STATED_HYDROGENS
        {
            return Err(StateError::HydrogenCountOutOfRange.into());
        }
        if let AtomKind::Query(query) = &self.kind {
            query.validate()?;
            if self.charge != 0 {
                return Err(StateError::QueryAtomCharge.into());
            }
            if self.isotope.is_some() {
                return Err(StateError::QueryAtomIsotope.into());
            }
        }
        Ok(())
    }
}

/// The changes `update_atom` makes; fields left `None` keep their values.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AtomEdit {
    pub kind: Option<AtomKind>,
    pub charge: Option<i8>,
    /// `Some(None)` restores natural abundance.
    pub isotope: Option<Option<u16>>,
    pub hydrogens: Option<Hydrogens>,
    pub radical: Option<Radical>,
    pub attachment_points: Option<AttachmentPoints>,
}

/// Where an atom's position lives: in one scene instance. Orders by instance, then atom.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PositionKey {
    pub instance: ObjectId,
    pub atom: AtomId,
}

/// A bond's kind as drawn. Aromatic, dative, query, and unknown bonds are recorded only:
/// aromaticity is not perceived, no Kekulé form is computed, and no query is matched.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BondOrder {
    Single,
    Double,
    Triple,
    Aromatic,
    /// A dative bond from its `begin` atom (the donor) to its `end` atom.
    Dative,
    Query(BondQuery),
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Bond {
    pub begin: AtomId,
    pub end: AtomId,
    pub order: BondOrder,
}

impl Bond {
    fn validate(&self) -> Result<(), EditError> {
        if let BondOrder::Query(query) = &self.order {
            query.validate()?;
        }
        Ok(())
    }

    fn connects(&self, a: AtomId, b: AtomId) -> bool {
        (self.begin == a && self.end == b) || (self.begin == b && self.end == a)
    }
}

/// An atom named in a command: an existing atom, or one added earlier in the same
/// transaction, so a single transaction can add several new atoms and bond them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AtomRef {
    Id(AtomId),
    /// The atom added by the `AddAtom` command at this index in the transaction.
    Created(usize),
}

/// A scene object named in a transaction's selection after the edit: an existing object, or
/// the one created by an `AddInstance`, `AddGraphic`, or `Group` command of the same
/// transaction, so a client can select what its edit makes before the identifier is known.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ObjectRef {
    Id(ObjectId),
    /// The object created by the command at this index in the transaction.
    Created(usize),
}

#[derive(Clone, Debug, PartialEq)]
pub struct BondTo {
    pub atom: AtomRef,
    pub order: BondOrder,
}

/// Where a new atom is drawn. Creating an atom states its position in every scene instance.
#[derive(Clone, Debug, PartialEq)]
pub enum NewAtomPositions {
    /// One position, accepted while the document has exactly one scene instance.
    Single(Point),
    /// A position for each scene instance; it must name exactly the current instances.
    PerInstance(BTreeMap<ObjectId, Point>),
}

/// One edit. A transaction applies an ordered list of commands all-or-nothing.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// Adds an atom with the given state, optionally bonded from another atom (`bond_to.atom`
    /// is the bond's `begin`, so the donor of a dative bond).
    AddAtom {
        atom: Atom,
        positions: NewAtomPositions,
        bond_to: Option<BondTo>,
    },
    AddBond {
        begin: AtomRef,
        end: AtomRef,
        order: BondOrder,
    },
    /// Changes only the fields of `edit` that are `Some`. The atom that results must be valid
    /// as a whole: turning an atom into a query needs its charge and isotope cleared too.
    UpdateAtom {
        atom: AtomId,
        edit: AtomEdit,
    },
    /// Translates each listed atom once, even if it is listed repeatedly, in one scene
    /// instance. `instance` may be left out only while the document has exactly one.
    MoveAtoms {
        atoms: Vec<AtomId>,
        delta: Vector,
        instance: Option<ObjectId>,
    },
    UpdateBond {
        bond: BondId,
        order: BondOrder,
    },
    /// Deletes the atoms and every bond incident to them.
    DeleteAtoms {
        atoms: Vec<AtomId>,
    },
    DeleteBonds {
        bonds: Vec<BondId>,
    },
    Rename {
        title: String,
    },
    /// Adds a scene instance of the molecule on the page with the positions of `source` (the
    /// first instance when `None`) translated by `offset`.
    AddInstance {
        source: Option<ObjectId>,
        offset: Vector,
    },
    /// Translates every position of one scene instance once.
    MoveInstance {
        instance: ObjectId,
        offset: Vector,
    },
    /// Deletes a scene instance and its positions, and takes it out of its parent; the last
    /// one is kept.
    DeleteInstance {
        instance: ObjectId,
    },
    /// Adds a graphic at the end of `parent`'s children (a group), or of the page's roots.
    AddGraphic {
        graphic: Graphic,
        parent: Option<ObjectId>,
    },
    /// Replaces objects that share a parent with a new group holding them, in their order,
    /// where the first of them was.
    Group {
        objects: Vec<ObjectId>,
    },
    /// Puts a group's children in its place in its parent and deletes the group.
    Ungroup {
        group: ObjectId,
    },
    /// Deletes objects as `mode` says. Deleted instances leave their groups in the same change
    /// set; a group left empty stays. The last molecule scene instance is kept.
    DeleteObjects {
        objects: Vec<ObjectId>,
        mode: DeleteMode,
    },
    /// Translates the union of the objects' subtrees: every instance (all its positions) and
    /// graphic in it moves exactly once, however many of its ancestors are listed too.
    MoveObjects {
        objects: Vec<ObjectId>,
        offset: Vector,
    },
    /// Moves an object to `index` among its parent's children.
    Reorder {
        object: ObjectId,
        index: usize,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum EditError {
    EmptyTransaction,
    TooManyCommands,
    EmptyTargetList,
    UnknownAtom(AtomId),
    /// `AtomRef::Created` must name an earlier `AddAtom` command of the same transaction.
    UnresolvedReference(usize),
    UnknownBond(BondId),
    SelfBond(AtomId),
    DuplicateBond {
        existing: BondId,
    },
    AtomLimit,
    BondLimit,
    InvalidTitle,
    CoordinateOutOfRange,
    ChargeOutOfRange(i8),
    IsotopeOutOfRange(u16),
    IdentifierSpaceExhausted,
    RevisionLimit,
    UnknownInstance(ObjectId),
    /// A single `position` was given while the document has several scene instances.
    PositionsRequired {
        instances: usize,
    },
    /// The positions of a new atom leave out this scene instance.
    MissingPosition(ObjectId),
    /// `move_atoms` did not name an instance while the document has several.
    InstanceRequired {
        instances: usize,
    },
    /// A document keeps at least one scene instance of its molecule.
    LastInstance,
    InstanceLimit,
    /// A refused combination of recorded states, with its stable code.
    InvalidState(StateError),
    /// A refused scene command or selection, with its stable code.
    Scene(SceneError),
    /// A restored scene object that is not contained exactly once by the page's tree.
    BrokenContainment(ObjectId),
}

impl EditError {
    /// The stable code an `invalid_command` carries as `reason`, for the refusals that have one.
    pub fn reason(&self) -> Option<&'static str> {
        match self {
            Self::InvalidState(error) => Some(error.code()),
            Self::Scene(error) => Some(error.code()),
            Self::LastInstance => Some(LAST_INSTANCE_CODE),
            _ => None,
        }
    }
}

/// The reason code of [`EditError::LastInstance`].
pub const LAST_INSTANCE_CODE: &str = "last_instance";

impl From<StateError> for EditError {
    fn from(error: StateError) -> Self {
        Self::InvalidState(error)
    }
}

impl fmt::Display for EditError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyTransaction => write!(formatter, "a transaction needs at least one command"),
            Self::TooManyCommands => {
                write!(
                    formatter,
                    "a transaction accepts at most {MAX_COMMANDS} commands"
                )
            }
            Self::EmptyTargetList => write!(formatter, "the target list must not be empty"),
            Self::UnknownAtom(id) => write!(formatter, "atom {id} does not exist"),
            Self::UnresolvedReference(index) => write!(
                formatter,
                "${index} does not name an earlier add_atom command in this transaction"
            ),
            Self::UnknownBond(id) => write!(formatter, "bond {id} does not exist"),
            Self::SelfBond(id) => write!(formatter, "atom {id} cannot bond to itself"),
            Self::DuplicateBond { existing } => {
                write!(
                    formatter,
                    "these atoms are already joined by bond {existing}"
                )
            }
            Self::AtomLimit => write!(formatter, "a document holds at most {MAX_ATOMS} atoms"),
            Self::BondLimit => write!(formatter, "a document holds at most {MAX_BONDS} bonds"),
            Self::InvalidTitle => write!(
                formatter,
                "a title needs 1-{MAX_TITLE_CHARS} characters without control characters"
            ),
            Self::CoordinateOutOfRange => write!(
                formatter,
                "coordinates must be finite and within ±{MAX_COORDINATE} pt"
            ),
            Self::ChargeOutOfRange(charge) => write!(
                formatter,
                "charge {charge} is outside {MIN_CHARGE}..={MAX_CHARGE}"
            ),
            Self::IsotopeOutOfRange(mass) => write!(
                formatter,
                "isotope mass number {mass} is outside 1..={MAX_ISOTOPE_MASS_NUMBER}"
            ),
            Self::IdentifierSpaceExhausted => {
                write!(formatter, "this document has no identifiers left")
            }
            Self::RevisionLimit => write!(formatter, "{REVISION_LIMIT_MESSAGE}"),
            Self::UnknownInstance(id) => write!(formatter, "scene instance {id} does not exist"),
            Self::PositionsRequired { instances } => write!(
                formatter,
                "the molecule has {instances} scene instances; give `positions` with a point \
                 for each of them"
            ),
            Self::MissingPosition(id) => {
                write!(formatter, "no position is given for scene instance {id}")
            }
            Self::InstanceRequired { instances } => write!(
                formatter,
                "the molecule has {instances} scene instances; name the `instance` whose atoms \
                 move"
            ),
            Self::LastInstance => write!(formatter, "a molecule keeps at least one scene instance"),
            Self::InstanceLimit => write!(
                formatter,
                "a document holds at most {MAX_INSTANCES} scene instances"
            ),
            Self::InvalidState(error) => error.fmt(formatter),
            Self::Scene(error) => error.fmt(formatter),
            Self::BrokenContainment(id) => write!(
                formatter,
                "scene object {id} is not contained exactly once by the page's tree"
            ),
        }
    }
}

impl std::error::Error for EditError {}

/// A rejected transaction. `index` names the failing command when there is one.
#[derive(Clone, Debug, PartialEq)]
pub struct TransactionError {
    pub index: Option<usize>,
    pub error: EditError,
}

impl fmt::Display for TransactionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.index {
            Some(index) => write!(formatter, "command {index}: {}", self.error),
            None => self.error.fmt(formatter),
        }
    }
}

impl std::error::Error for TransactionError {}

/// The client's selection after an edit as it sends it: its objects may name ones the same
/// transaction creates. The change set records it with every reference resolved.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SelectionAfter {
    pub objects: BTreeSet<ObjectRef>,
    pub atoms: BTreeSet<AtomId>,
    pub bonds: BTreeSet<BondId>,
}

impl From<Selection> for SelectionAfter {
    fn from(selection: Selection) -> Self {
        Self {
            objects: selection.objects.into_iter().map(ObjectRef::Id).collect(),
            atoms: selection.atoms,
            bonds: selection.bonds,
        }
    }
}

/// The selections a transaction records with its change set. `None` keeps the default: the
/// selection before is what the previous edit, undo, or redo left, and the selection after is
/// the previous selection wherever it still exists. Only the selection after may name objects
/// the transaction creates: nothing is created before the edit.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SelectionUpdate {
    /// The client's selection before the edit; undo restores it.
    pub before: Option<Selection>,
    /// The client's selection after the edit; redo restores it.
    pub after: Option<SelectionAfter>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    Changed,
    /// The commands were valid but left the document identical; no revision or undo entry.
    Unchanged,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HistoryError {
    NothingToUndo,
    NothingToRedo,
    RevisionLimit,
}

const REVISION_LIMIT_MESSAGE: &str = "this document has reached its last revision number";

impl fmt::Display for HistoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NothingToUndo => "there is nothing to undo",
            Self::NothingToRedo => "there is nothing to redo",
            Self::RevisionLimit => REVISION_LIMIT_MESSAGE,
        })
    }
}

impl std::error::Error for HistoryError {}

/// No revision remains above the ones already handed out.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RevisionsExhausted;

impl fmt::Display for RevisionsExhausted {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(REVISION_LIMIT_MESSAGE)
    }
}

impl std::error::Error for RevisionsExhausted {}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Change<T> {
    pub before: T,
    pub after: T,
}

/// The exact difference made by one transaction; applied backward to undo and forward to redo.
/// Graph changes (atoms, bonds), scene changes (objects, the page's root order, positions), and
/// the title are recorded apart, and undo or redo replays all of them as one step together with
/// the selection before and after the transaction.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChangeSet {
    pub title: Option<Change<String>>,
    pub atoms: Vec<(AtomId, Change<Option<Atom>>)>,
    pub bonds: Vec<(BondId, Change<Option<Bond>>)>,
    /// Scene objects created, deleted, or changed (a group's children, a graphic's points).
    pub objects: Vec<(ObjectId, Change<Option<SceneObject>>)>,
    /// The page's root objects, in order, when they changed.
    pub roots: Option<Change<Vec<ObjectId>>>,
    pub positions: Vec<(PositionKey, Change<Option<Point>>)>,
    /// The selection before and after the transaction. It is not an edit: an entry whose only
    /// difference is the selection is empty.
    pub selection: Change<Selection>,
}

impl ChangeSet {
    fn between(before: &Content, after: &Content, selection: Change<Selection>) -> Self {
        Self {
            title: (before.title != after.title).then(|| Change {
                before: before.title.clone(),
                after: after.title.clone(),
            }),
            atoms: diff_maps(&before.atoms, &after.atoms),
            bonds: diff_maps(&before.bonds, &after.bonds),
            objects: diff_maps(&before.tree.objects, &after.tree.objects),
            roots: (before.tree.roots != after.tree.roots).then(|| Change {
                before: before.tree.roots.clone(),
                after: after.tree.roots.clone(),
            }),
            positions: diff_maps(&before.positions, &after.positions),
            selection,
        }
    }

    /// Whether the entry changes nothing but, possibly, the selection.
    pub fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.atoms.is_empty()
            && self.bonds.is_empty()
            && self.objects.is_empty()
            && self.roots.is_none()
            && self.positions.is_empty()
    }

    /// Changes counted against [`HISTORY_CHANGE_LIMIT`]: one per entity, plus the identifiers a
    /// group's children, the root order, and the selections name.
    fn size(&self) -> usize {
        let object = |state: &Option<SceneObject>| state.as_ref().map_or(0, SceneObject::size);
        usize::from(self.title.is_some())
            + self.atoms.len()
            + self.bonds.len()
            + self
                .objects
                .iter()
                .map(|(_, change)| 1.max(object(&change.before) + object(&change.after)))
                .sum::<usize>()
            + self
                .roots
                .as_ref()
                .map_or(0, |change| change.before.len() + change.after.len())
            + self.positions.len()
            + self.selection.before.len()
            + self.selection.after.len()
    }

    /// The shape `between` produces: non-empty, each entity at most once, and every entry
    /// an actual change. Only such a set replays identically backward and forward, so
    /// restored history that passes the backward check can always be redone.
    fn is_well_formed(&self) -> bool {
        fn entries_valid<K: Ord, V: PartialEq>(entries: &[(K, Change<Option<V>>)]) -> bool {
            let mut seen = BTreeSet::new();
            entries
                .iter()
                .all(|(id, change)| change.before != change.after && seen.insert(id))
        }
        !self.is_empty()
            && self
                .title
                .as_ref()
                .is_none_or(|change| change.before != change.after)
            && self
                .roots
                .as_ref()
                .is_none_or(|change| change.before != change.after)
            && entries_valid(&self.atoms)
            && entries_valid(&self.bonds)
            && entries_valid(&self.objects)
            && entries_valid(&self.positions)
    }
}

fn diff_maps<K: Copy + Ord, V: Clone + PartialEq>(
    before: &BTreeMap<K, V>,
    after: &BTreeMap<K, V>,
) -> Vec<(K, Change<Option<V>>)> {
    let keys: BTreeSet<K> = before.keys().chain(after.keys()).copied().collect();
    keys.into_iter()
        .filter_map(|key| {
            let (old, new) = (before.get(&key), after.get(&key));
            (old != new).then(|| {
                (
                    key,
                    Change {
                        before: old.cloned(),
                        after: new.cloned(),
                    },
                )
            })
        })
        .collect()
}

#[derive(Clone, Copy)]
enum Direction {
    Backward,
    Forward,
}

impl Direction {
    fn sides<T>(self, change: &Change<T>) -> (&T, &T) {
        match self {
            Self::Backward => (&change.after, &change.before),
            Self::Forward => (&change.before, &change.after),
        }
    }
}

fn replay<K: Copy + Ord, V: Clone + PartialEq>(
    map: &mut BTreeMap<K, V>,
    changes: &[(K, Change<Option<V>>)],
    direction: Direction,
) -> Result<(), ()> {
    for (key, change) in changes {
        let (from, to) = direction.sides(change);
        if map.get(key) != from.as_ref() {
            return Err(());
        }
        match to {
            Some(value) => map.insert(*key, value.clone()),
            None => map.remove(key),
        };
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq)]
struct Content {
    title: String,
    atoms: BTreeMap<AtomId, Atom>,
    bonds: BTreeMap<BondId, Bond>,
    /// The page's scene objects and their containment.
    tree: Tree,
    positions: BTreeMap<PositionKey, Point>,
}

impl Content {
    fn bond_between(&self, a: AtomId, b: AtomId) -> Option<BondId> {
        self.bonds
            .iter()
            .find(|(_, bond)| bond.connects(a, b))
            .map(|(id, _)| *id)
    }

    fn require_atom(&self, id: AtomId) -> Result<(), EditError> {
        if self.atoms.contains_key(&id) {
            Ok(())
        } else {
            Err(EditError::UnknownAtom(id))
        }
    }

    /// `id`, when it names a molecule scene instance.
    fn require_instance(&self, id: ObjectId) -> Result<ObjectId, EditError> {
        match self.tree.objects.get(&id) {
            Some(SceneObject::Molecule) => Ok(id),
            Some(_) => Err(SceneError::KindMismatch {
                object: id,
                expected: ObjectKind::Molecule,
            }
            .into()),
            None => Err(EditError::UnknownInstance(id)),
        }
    }

    /// The molecule scene instances in identifier order, which is creation order.
    fn instances(&self) -> impl Iterator<Item = ObjectId> + '_ {
        self.tree
            .objects
            .iter()
            .filter(|(_, object)| matches!(object, SceneObject::Molecule))
            .map(|(id, _)| *id)
    }

    fn instance_count(&self) -> usize {
        self.instances().count()
    }

    fn first_instance(&self) -> ObjectId {
        self.instances()
            .next()
            .expect("validated content keeps at least one scene instance")
    }

    /// The instance a command acts in: the named one, or the only one.
    fn target_instance(&self, named: Option<ObjectId>) -> Result<ObjectId, EditError> {
        match named {
            Some(id) => self.require_instance(id),
            None => {
                let mut instances = self.instances();
                match (instances.next(), instances.next()) {
                    (Some(only), None) => Ok(only),
                    _ => Err(EditError::InstanceRequired {
                        instances: self.instance_count(),
                    }),
                }
            }
        }
    }

    /// The positions of one instance, by atom.
    fn instance_positions(&self, instance: ObjectId) -> impl Iterator<Item = (AtomId, Point)> + '_ {
        let first = PositionKey {
            instance,
            atom: AtomId(0),
        };
        let last = PositionKey {
            instance,
            atom: AtomId(u32::MAX),
        };
        self.positions
            .range(first..=last)
            .map(|(key, point)| (key.atom, *point))
    }

    /// Refuses a selection naming anything this content does not hold.
    fn check_selection(&self, selection: &Selection) -> Result<(), EditError> {
        let unknown = selection
            .objects
            .iter()
            .find(|id| !self.tree.objects.contains_key(id))
            .map(ToString::to_string)
            .or_else(|| {
                selection
                    .atoms
                    .iter()
                    .find(|id| !self.atoms.contains_key(id))
                    .map(ToString::to_string)
            })
            .or_else(|| {
                selection
                    .bonds
                    .iter()
                    .find(|id| !self.bonds.contains_key(id))
                    .map(ToString::to_string)
            });
        match unknown {
            Some(id) => Err(SceneError::UnknownSelection(id).into()),
            None => Ok(()),
        }
    }

    /// `selection` without what this content no longer holds.
    fn retained(&self, selection: &Selection) -> Selection {
        Selection {
            objects: selection
                .objects
                .iter()
                .filter(|id| self.tree.objects.contains_key(id))
                .copied()
                .collect(),
            atoms: selection
                .atoms
                .iter()
                .filter(|id| self.atoms.contains_key(id))
                .copied()
                .collect(),
            bonds: selection
                .bonds
                .iter()
                .filter(|id| self.bonds.contains_key(id))
                .copied()
                .collect(),
        }
    }

    /// Checks every structural invariant; used for restored and replayed states.
    fn validate(&self) -> Result<(), EditError> {
        validate_title(&self.title)?;
        if self.atoms.len() > MAX_ATOMS {
            return Err(EditError::AtomLimit);
        }
        if self.bonds.len() > MAX_BONDS {
            return Err(EditError::BondLimit);
        }
        for atom in self.atoms.values() {
            atom.validate()?;
        }
        let mut pairs = HashSet::with_capacity(self.bonds.len());
        for (id, bond) in &self.bonds {
            bond.validate()?;
            self.require_atom(bond.begin)?;
            self.require_atom(bond.end)?;
            if bond.begin == bond.end {
                return Err(EditError::SelfBond(bond.begin));
            }
            let pair = (bond.begin.min(bond.end), bond.begin.max(bond.end));
            if !pairs.insert(pair) {
                return Err(EditError::DuplicateBond { existing: *id });
            }
        }
        self.tree.validate()?;
        let instances = self.instance_count();
        if instances == 0 {
            return Err(EditError::LastInstance);
        }
        if instances > MAX_INSTANCES {
            return Err(EditError::InstanceLimit);
        }
        for key in self.positions.keys() {
            self.require_instance(key.instance)?;
            self.require_atom(key.atom)?;
        }
        // Keys are unique and each names an existing instance and atom, so the count is
        // right exactly when every atom has one position in every instance.
        if self.positions.len() != instances * self.atoms.len() {
            for instance in self.instances() {
                for atom in self.atoms.keys() {
                    let key = PositionKey {
                        instance,
                        atom: *atom,
                    };
                    if !self.positions.contains_key(&key) {
                        return Err(EditError::MissingPosition(instance));
                    }
                }
            }
        }
        Ok(())
    }
}

fn validate_title(title: &str) -> Result<(), EditError> {
    let length = title.chars().count();
    if title.trim() != title
        || length == 0
        || length > MAX_TITLE_CHARS
        || title.chars().any(char::is_control)
    {
        Err(EditError::InvalidTitle)
    } else {
        Ok(())
    }
}

fn validate_charge(charge: i8) -> Result<(), EditError> {
    if (MIN_CHARGE..=MAX_CHARGE).contains(&charge) {
        Ok(())
    } else {
        Err(EditError::ChargeOutOfRange(charge))
    }
}

fn validate_isotope(isotope: Option<u16>) -> Result<(), EditError> {
    match isotope {
        Some(mass) if !(1..=MAX_ISOTOPE_MASS_NUMBER).contains(&mass) => {
            Err(EditError::IsotopeOutOfRange(mass))
        }
        _ => Ok(()),
    }
}

/// What one command of a transaction created, which later commands and the selection after
/// the transaction may name by the command's index.
#[derive(Clone, Copy)]
enum Created {
    Atom(AtomId),
    Object(ObjectId),
}

/// Mutable working state for one transaction; discarded unless every command succeeds.
struct Draft {
    content: Content,
    next_atom: u32,
    next_bond: u32,
    next_object: u32,
    /// The atom or scene object each earlier command created, by command index.
    created: Vec<Option<Created>>,
}

impl Draft {
    fn allocate_atom(&mut self) -> Result<AtomId, EditError> {
        if self.content.atoms.len() >= MAX_ATOMS {
            return Err(EditError::AtomLimit);
        }
        let id = AtomId(self.next_atom);
        self.next_atom = self
            .next_atom
            .checked_add(1)
            .ok_or(EditError::IdentifierSpaceExhausted)?;
        Ok(id)
    }

    fn insert_bond(
        &mut self,
        begin: AtomId,
        end: AtomId,
        order: BondOrder,
    ) -> Result<(), EditError> {
        self.content.require_atom(begin)?;
        self.content.require_atom(end)?;
        if begin == end {
            return Err(EditError::SelfBond(begin));
        }
        if let Some(existing) = self.content.bond_between(begin, end) {
            return Err(EditError::DuplicateBond { existing });
        }
        if self.content.bonds.len() >= MAX_BONDS {
            return Err(EditError::BondLimit);
        }
        let bond = Bond { begin, end, order };
        bond.validate()?;
        let id = BondId(self.next_bond);
        self.next_bond = self
            .next_bond
            .checked_add(1)
            .ok_or(EditError::IdentifierSpaceExhausted)?;
        self.content.bonds.insert(id, bond);
        Ok(())
    }

    /// The next scene object identifier, from the one counter every object kind shares.
    fn allocate_object(&mut self) -> Result<ObjectId, EditError> {
        if self.content.tree.objects.len() >= MAX_OBJECTS {
            return Err(SceneError::ObjectLimit.into());
        }
        let id = ObjectId(self.next_object);
        self.next_object = self
            .next_object
            .checked_add(1)
            .ok_or(EditError::IdentifierSpaceExhausted)?;
        Ok(id)
    }

    /// A new atom's position in each current scene instance.
    fn new_atom_positions(
        &self,
        positions: &NewAtomPositions,
    ) -> Result<Vec<(ObjectId, Point)>, EditError> {
        let instances: Vec<ObjectId> = self.content.instances().collect();
        match positions {
            NewAtomPositions::Single(point) if instances.len() == 1 => {
                Ok(vec![(instances[0], *point)])
            }
            NewAtomPositions::Single(_) => Err(EditError::PositionsRequired {
                instances: instances.len(),
            }),
            NewAtomPositions::PerInstance(points) => {
                for id in points.keys() {
                    self.content.require_instance(*id)?;
                }
                instances
                    .iter()
                    .map(|id| {
                        points
                            .get(id)
                            .map(|point| (*id, *point))
                            .ok_or(EditError::MissingPosition(*id))
                    })
                    .collect()
            }
        }
    }

    /// Translates every listed atom's position in `instance` once.
    fn translate(
        &mut self,
        instance: ObjectId,
        atoms: impl IntoIterator<Item = AtomId>,
        delta: Vector,
    ) -> Result<(), EditError> {
        for atom in atoms {
            let point = self
                .content
                .positions
                .get_mut(&PositionKey { instance, atom })
                .ok_or(EditError::UnknownAtom(atom))?;
            *point = point.translated(delta)?;
        }
        Ok(())
    }

    /// Translates every position of `instance` once.
    fn translate_instance(&mut self, instance: ObjectId, delta: Vector) -> Result<(), EditError> {
        let atoms: Vec<AtomId> = self.content.atoms.keys().copied().collect();
        self.translate(instance, atoms, delta)
    }

    fn resolve(&self, atom: AtomRef) -> Result<AtomId, EditError> {
        match atom {
            AtomRef::Id(id) => Ok(id),
            AtomRef::Created(index) => match self.created.get(index) {
                Some(Some(Created::Atom(id))) => Ok(*id),
                _ => Err(EditError::UnresolvedReference(index)),
            },
        }
    }

    /// `selection` with each object a command created named by its identifier.
    fn resolve_selection(&self, selection: SelectionAfter) -> Result<Selection, EditError> {
        let objects = selection
            .objects
            .into_iter()
            .map(|object| match object {
                ObjectRef::Id(id) => Ok(id),
                ObjectRef::Created(index) => match self.created.get(index) {
                    Some(Some(Created::Object(id))) => Ok(*id),
                    _ => Err(SceneError::UnresolvedSelection(index).into()),
                },
            })
            .collect::<Result<_, EditError>>()?;
        Ok(Selection {
            objects,
            atoms: selection.atoms,
            bonds: selection.bonds,
        })
    }

    /// Each listed object, refusing one that does not exist; repeats count once.
    fn existing_objects(&self, objects: &[ObjectId]) -> Result<BTreeSet<ObjectId>, EditError> {
        let objects = unique(objects)?;
        for id in &objects {
            self.content.tree.get(*id)?;
        }
        Ok(objects)
    }

    /// Takes `id` out of its parent's children.
    fn detach(&mut self, id: ObjectId) {
        let tree = &mut self.content.tree;
        if let Some((parent, _)) = tree.parents().get(&id).copied() {
            tree.children_mut(parent).retain(|child| *child != id);
        }
    }

    /// Applies one command and records which atom or scene object, if any, it created.
    fn apply(&mut self, command: &Command) -> Result<(), EditError> {
        let mut created = None;
        match command {
            Command::AddAtom {
                atom,
                positions,
                bond_to,
            } => {
                atom.validate()?;
                let target = bond_to
                    .as_ref()
                    .map(|target| {
                        Ok::<_, EditError>((self.resolve(target.atom)?, target.order.clone()))
                    })
                    .transpose()?;
                if let Some((atom, _)) = &target {
                    self.content.require_atom(*atom)?;
                }
                let points = self.new_atom_positions(positions)?;
                let id = self.allocate_atom()?;
                created = Some(Created::Atom(id));
                self.content.atoms.insert(id, atom.clone());
                for (instance, point) in points {
                    self.content
                        .positions
                        .insert(PositionKey { instance, atom: id }, point);
                }
                if let Some((atom, order)) = target {
                    self.insert_bond(atom, id, order)?;
                }
            }
            Command::AddBond { begin, end, order } => {
                let (begin, end) = (self.resolve(*begin)?, self.resolve(*end)?);
                self.insert_bond(begin, end, order.clone())?;
            }
            Command::UpdateAtom { atom, edit } => {
                let target = self
                    .content
                    .atoms
                    .get_mut(atom)
                    .ok_or(EditError::UnknownAtom(*atom))?;
                let mut updated = target.clone();
                if let Some(kind) = &edit.kind {
                    updated.kind = kind.clone();
                }
                if let Some(charge) = edit.charge {
                    updated.charge = charge;
                }
                if let Some(isotope) = edit.isotope {
                    updated.isotope = isotope;
                }
                if let Some(hydrogens) = edit.hydrogens {
                    updated.hydrogens = hydrogens;
                }
                if let Some(radical) = edit.radical {
                    updated.radical = radical;
                }
                if let Some(points) = edit.attachment_points {
                    updated.attachment_points = points;
                }
                updated.validate()?;
                *target = updated;
            }
            Command::MoveAtoms {
                atoms,
                delta,
                instance,
            } => {
                let atoms = unique(atoms)?;
                let instance = self.content.target_instance(*instance)?;
                self.translate(instance, atoms, *delta)?;
            }
            Command::UpdateBond { bond, order } => {
                let target = self
                    .content
                    .bonds
                    .get_mut(bond)
                    .ok_or(EditError::UnknownBond(*bond))?;
                let updated = Bond {
                    order: order.clone(),
                    ..target.clone()
                };
                updated.validate()?;
                *target = updated;
            }
            Command::DeleteAtoms { atoms } => {
                let targets = unique(atoms)?;
                for id in &targets {
                    self.content.require_atom(*id)?;
                }
                for id in &targets {
                    self.content.atoms.remove(id);
                }
                self.content.bonds.retain(|_, bond| {
                    !targets.contains(&bond.begin) && !targets.contains(&bond.end)
                });
                self.content
                    .positions
                    .retain(|key, _| !targets.contains(&key.atom));
            }
            Command::DeleteBonds { bonds } => {
                for id in unique(bonds)? {
                    self.content
                        .bonds
                        .remove(&id)
                        .ok_or(EditError::UnknownBond(id))?;
                }
            }
            Command::Rename { title } => {
                let title = title.trim();
                validate_title(title)?;
                title.clone_into(&mut self.content.title);
            }
            Command::AddInstance { source, offset } => {
                let source = match source {
                    Some(id) => self.content.require_instance(*id)?,
                    None => self.content.first_instance(),
                };
                let copied = self
                    .content
                    .instance_positions(source)
                    .map(|(atom, point)| Ok((atom, point.translated(*offset)?)))
                    .collect::<Result<Vec<_>, EditError>>()?;
                if self.content.instance_count() >= MAX_INSTANCES {
                    return Err(EditError::InstanceLimit);
                }
                let id = self.allocate_object()?;
                created = Some(Created::Object(id));
                self.content.tree.objects.insert(id, SceneObject::Molecule);
                self.content.tree.roots.push(id);
                for (atom, point) in copied {
                    self.content
                        .positions
                        .insert(PositionKey { instance: id, atom }, point);
                }
            }
            Command::MoveInstance { instance, offset } => {
                let instance = self.content.require_instance(*instance)?;
                self.translate_instance(instance, *offset)?;
            }
            Command::DeleteInstance { instance } => {
                let instance = self.content.require_instance(*instance)?;
                if self.content.instance_count() == 1 {
                    return Err(EditError::LastInstance);
                }
                self.detach(instance);
                self.content.tree.objects.remove(&instance);
                self.content
                    .positions
                    .retain(|key, _| key.instance != instance);
            }
            Command::AddGraphic { graphic, parent } => {
                let parent = match parent {
                    None => Parent::Page,
                    Some(id) => {
                        if !matches!(self.content.tree.get(*id)?, SceneObject::Group(_)) {
                            return Err(SceneError::KindMismatch {
                                object: *id,
                                expected: ObjectKind::Group,
                            }
                            .into());
                        }
                        let tree = &self.content.tree;
                        if tree.depth(&tree.parents(), *id) + 1 > MAX_DEPTH {
                            return Err(SceneError::DepthLimit.into());
                        }
                        Parent::Group(*id)
                    }
                };
                let id = self.allocate_object()?;
                created = Some(Created::Object(id));
                let tree = &mut self.content.tree;
                tree.objects.insert(id, SceneObject::Graphic(*graphic));
                tree.children_mut(parent).push(id);
            }
            Command::Group { objects } => {
                let members = self.existing_objects(objects)?;
                let tree = &self.content.tree;
                let parents = tree.parents();
                let mut shared = None;
                for id in &members {
                    let (parent, _) = parents[id];
                    if shared.is_some_and(|shared| shared != parent) {
                        return Err(SceneError::MixedParents.into());
                    }
                    shared = Some(parent);
                }
                let parent = shared.expect("a group names at least one object");
                let depth = match parent {
                    Parent::Page => 1,
                    Parent::Group(group) => tree.depth(&parents, group) + 1,
                };
                if members
                    .iter()
                    .any(|id| depth + 1 + tree.height(*id) > MAX_DEPTH)
                {
                    return Err(SceneError::DepthLimit.into());
                }
                let siblings = tree.children(parent);
                let at = siblings
                    .iter()
                    .position(|id| members.contains(id))
                    .expect("every member is a child of the shared parent");
                let children: Vec<ObjectId> = siblings
                    .iter()
                    .filter(|id| members.contains(id))
                    .copied()
                    .collect();
                let id = self.allocate_object()?;
                created = Some(Created::Object(id));
                let tree = &mut self.content.tree;
                let siblings = tree.children_mut(parent);
                siblings.retain(|sibling| !members.contains(sibling));
                siblings.insert(at, id);
                tree.objects.insert(id, SceneObject::Group(children));
            }
            Command::Ungroup { group } => {
                let tree = &mut self.content.tree;
                let SceneObject::Group(children) = tree.get(*group)?.clone() else {
                    return Err(SceneError::KindMismatch {
                        object: *group,
                        expected: ObjectKind::Group,
                    }
                    .into());
                };
                let (parent, index) = tree.parents()[group];
                tree.children_mut(parent).splice(index..=index, children);
                tree.objects.remove(group);
            }
            Command::DeleteObjects { objects, mode } => {
                let named = self.existing_objects(objects)?;
                let tree = &self.content.tree;
                let deleted = match mode {
                    DeleteMode::ObjectOnly => named,
                    DeleteMode::Subtree => {
                        let mut deleted = BTreeSet::new();
                        for id in named {
                            tree.subtree(id, &mut deleted);
                        }
                        deleted
                    }
                };
                let deleted_instances: Vec<ObjectId> = self
                    .content
                    .instances()
                    .filter(|id| deleted.contains(id))
                    .collect();
                if deleted_instances.len() == self.content.instance_count() {
                    return Err(EditError::LastInstance);
                }
                let roots = tree.without(&tree.roots, &deleted);
                let groups: Vec<(ObjectId, Vec<ObjectId>)> = tree
                    .objects
                    .iter()
                    .filter(|(id, _)| !deleted.contains(id))
                    .filter_map(|(id, object)| match object {
                        SceneObject::Group(children) => {
                            Some((*id, tree.without(children, &deleted)))
                        }
                        SceneObject::Molecule | SceneObject::Graphic(_) => None,
                    })
                    .collect();
                let tree = &mut self.content.tree;
                tree.roots = roots;
                for (id, children) in groups {
                    tree.objects.insert(id, SceneObject::Group(children));
                }
                tree.objects.retain(|id, _| !deleted.contains(id));
                self.content
                    .positions
                    .retain(|key, _| !deleted_instances.contains(&key.instance));
            }
            Command::MoveObjects { objects, offset } => {
                if !objects.is_empty() {
                    let named = self.existing_objects(objects)?;
                    let mut moved = BTreeSet::new();
                    for id in named {
                        self.content.tree.subtree(id, &mut moved);
                    }
                    for id in moved {
                        match self.content.tree.objects[&id] {
                            SceneObject::Molecule => self.translate_instance(id, *offset)?,
                            SceneObject::Graphic(_) => {
                                self.content.tree.translate_graphic(id, *offset)?;
                            }
                            SceneObject::Group(_) => {}
                        }
                    }
                }
            }
            Command::Reorder { object, index } => {
                let tree = &mut self.content.tree;
                tree.get(*object)?;
                let (parent, at) = tree.parents()[object];
                let siblings = tree.children_mut(parent);
                if *index >= siblings.len() {
                    return Err(SceneError::IndexOutOfRange {
                        index: *index,
                        len: siblings.len(),
                    }
                    .into());
                }
                siblings.remove(at);
                siblings.insert(*index, *object);
            }
        }
        self.created.push(created);
        Ok(())
    }
}

fn unique<T: Copy + Ord>(ids: &[T]) -> Result<BTreeSet<T>, EditError> {
    if ids.is_empty() {
        Err(EditError::EmptyTargetList)
    } else {
        Ok(ids.iter().copied().collect())
    }
}

/// How far a document's revision and identifier counters have advanced.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Numbering {
    pub revision: u64,
    pub next_atom: u32,
    pub next_bond: u32,
    /// The next scene object identifier, shared by instances, groups, and graphics.
    pub next_object: u32,
}

impl Numbering {
    /// The later of two numberings, field by field.
    #[must_use]
    pub fn max(self, other: Self) -> Self {
        Self {
            revision: self.revision.max(other.revision),
            next_atom: self.next_atom.max(other.next_atom),
            next_bond: self.next_bond.max(other.next_bond),
            next_object: self.next_object.max(other.next_object),
        }
    }
}

/// Persistable representation used by storage providers. `undo` is oldest first;
/// `redo` is a stack whose last entry is redone next.
#[derive(Clone, Debug, PartialEq)]
pub struct DocumentParts {
    pub id: DocumentId,
    pub revision: u64,
    pub title: String,
    pub atoms: Vec<(AtomId, Atom)>,
    pub bonds: Vec<(BondId, Bond)>,
    /// The scene objects on page [`PAGE_ID`]; every instance draws molecule [`MOLECULE_ID`].
    pub objects: Vec<(ObjectId, SceneObject)>,
    /// The page's root objects in order.
    pub roots: Vec<ObjectId>,
    pub positions: Vec<(PositionKey, Point)>,
    pub selection: Selection,
    pub next_atom: u32,
    pub next_bond: u32,
    pub next_object: u32,
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
    /// What the latest edit, undo, or redo left selected. Not part of the content: changing
    /// only the selection is not an edit.
    selection: Selection,
    next_atom: u32,
    next_bond: u32,
    next_object: u32,
    undo: VecDeque<ChangeSet>,
    redo: Vec<ChangeSet>,
}

impl Document {
    /// Creates an empty document at revision 0 with one scene instance, `s1`, as the page's
    /// only object. Surrounding whitespace is trimmed from the title.
    pub fn new(id: DocumentId, title: &str) -> Result<Self, EditError> {
        let title = title.trim();
        validate_title(title)?;
        Ok(Self {
            id,
            revision: 0,
            content: Content {
                title: title.to_owned(),
                atoms: BTreeMap::new(),
                bonds: BTreeMap::new(),
                tree: Tree {
                    objects: BTreeMap::from([(ObjectId(1), SceneObject::Molecule)]),
                    roots: vec![ObjectId(1)],
                },
                positions: BTreeMap::new(),
            },
            selection: Selection::default(),
            next_atom: 1,
            next_bond: 1,
            next_object: 2,
            undo: VecDeque::new(),
            redo: Vec::new(),
        })
    }

    /// An independent document with this one's graph, scene, selection, and identifier
    /// counters under a new identifier and title, at revision 0 with no history. Atom, bond,
    /// and scene object identifiers are kept, so commands that name them mean the same
    /// entities in the copy.
    pub fn copy_as(&self, id: DocumentId, title: &str) -> Result<Self, EditError> {
        let title = title.trim();
        validate_title(title)?;
        Ok(Self {
            id,
            revision: 0,
            content: Content {
                title: title.to_owned(),
                ..self.content.clone()
            },
            selection: self.selection.clone(),
            next_atom: self.next_atom,
            next_bond: self.next_bond,
            next_object: self.next_object,
            undo: VecDeque::new(),
            redo: Vec::new(),
        })
    }

    /// Rebuilds a stored document, rejecting any state commands could not have produced,
    /// including history entries that do not replay cleanly from the current state and
    /// selections naming what their state does not hold.
    pub fn restore(parts: DocumentParts) -> Result<Self, RestoreError> {
        let invalid = |reason: &str| RestoreError(reason.to_owned());
        if parts.revision > MAX_REVISION {
            return Err(invalid("revision exceeds the largest revision"));
        }
        let counts = (
            parts.atoms.len(),
            parts.bonds.len(),
            parts.objects.len(),
            parts.positions.len(),
        );
        let content = Content {
            title: parts.title,
            atoms: parts.atoms.into_iter().collect(),
            bonds: parts.bonds.into_iter().collect(),
            tree: Tree {
                objects: parts.objects.into_iter().collect(),
                roots: parts.roots,
            },
            positions: parts.positions.into_iter().collect(),
        };
        if counts
            != (
                content.atoms.len(),
                content.bonds.len(),
                content.tree.objects.len(),
                content.positions.len(),
            )
        {
            return Err(invalid(
                "duplicate atom, bond, or scene object identifier, or a repeated position",
            ));
        }
        content
            .validate()
            .map_err(|error| RestoreError(format!("invalid document content: {error}")))?;
        content
            .check_selection(&parts.selection)
            .map_err(|error| RestoreError(format!("invalid selection: {error}")))?;
        // The selection is what the latest edit, undo, or redo left: the newest undo entry's
        // selection after (an edit or a redo) or the nearest redo entry's selection before (an
        // undo). Either may apply when both exist, since a client may send a selection before
        // an edit other than the one the previous edit left; a redo then leaves the next redo
        // entry's selection before apart from the selection. An edit whose entry alone exceeds
        // the history limits leaves no history, and with none any selection that exists is
        // accepted.
        let boundaries = [
            parts.undo.last().map(|set| &set.selection.after),
            parts.redo.last().map(|set| &set.selection.before),
        ];
        if boundaries.iter().any(Option::is_some)
            && !boundaries
                .iter()
                .flatten()
                .any(|end| **end == parts.selection)
        {
            return Err(invalid("the selection is not the one the history ends at"));
        }
        // Editing keeps undo + redo within the limits (undo and redo only move entries between
        // them). A stored history beyond them, e.g. written under looser limits, is trimmed the
        // way editing trims: oldest undo entries first, then the farthest redo entries.
        let mut undo: VecDeque<ChangeSet> = parts.undo.into();
        let mut redo = parts.redo;
        while undo.len() + redo.len() > HISTORY_LIMIT
            || history_size(&undo) + history_size(&redo) > HISTORY_CHANGE_LIMIT
        {
            if undo.pop_front().is_none() {
                redo.remove(0);
            }
        }
        // Trimming every undo entry may drop the one the selection matched; the history left
        // then ends at the nearest redo entry's selection before, so the document restores
        // again once saved.
        let selection = match redo.last() {
            Some(set) if undo.is_empty() => set.selection.before.clone(),
            _ => parts.selection,
        };
        let document = Self {
            id: parts.id,
            revision: parts.revision,
            content,
            selection,
            next_atom: parts.next_atom,
            next_bond: parts.next_bond,
            next_object: parts.next_object,
            undo,
            redo,
        };
        let history = || document.undo.iter().chain(&document.redo);
        let atom_ids = document
            .content
            .atoms
            .keys()
            .copied()
            .chain(history().flat_map(|set| set.atoms.iter().map(|(id, _)| *id)));
        let bond_ids = document
            .content
            .bonds
            .keys()
            .copied()
            .chain(history().flat_map(|set| set.bonds.iter().map(|(id, _)| *id)));
        // Positions in history name instances too, and must not name a future one.
        let object_ids = document
            .content
            .tree
            .objects
            .keys()
            .copied()
            .chain(history().flat_map(|set| {
                set.objects
                    .iter()
                    .map(|(id, _)| *id)
                    .chain(set.positions.iter().map(|(key, _)| key.instance))
            }));
        // Serial 0 is not an identifier, so a counter of 0 would allocate an invalid one.
        if document.next_atom == 0
            || document.next_bond == 0
            || document.next_object == 0
            || atom_ids
                .map(AtomId::serial)
                .any(|serial| serial >= document.next_atom)
            || bond_ids
                .map(BondId::serial)
                .any(|serial| serial >= document.next_bond)
            || object_ids
                .map(ObjectId::serial)
                .any(|serial| serial >= document.next_object)
        {
            return Err(invalid(
                "identifier counter would reuse an existing identifier",
            ));
        }
        if history().any(|set| !set.is_well_formed()) {
            return Err(invalid(
                "history contains an empty, repeated, or no-op change",
            ));
        }
        let mut past = document.content.clone();
        for set in document.undo.iter().rev() {
            past.check_selection(&set.selection.after)
                .map_err(drop)
                .and_then(|()| step(&mut past, set, Direction::Backward))
                .and_then(|()| past.check_selection(&set.selection.before).map_err(drop))
                .map_err(|()| invalid("undo history does not match the document"))?;
        }
        let mut future = document.content.clone();
        for set in document.redo.iter().rev() {
            future
                .check_selection(&set.selection.before)
                .map_err(drop)
                .and_then(|()| step(&mut future, set, Direction::Forward))
                .and_then(|()| future.check_selection(&set.selection.after).map_err(drop))
                .map_err(|()| invalid("redo history does not match the document"))?;
        }
        Ok(document)
    }

    pub fn to_parts(&self) -> DocumentParts {
        DocumentParts {
            id: self.id.clone(),
            revision: self.revision,
            title: self.content.title.clone(),
            atoms: self.atoms().map(|(id, atom)| (id, atom.clone())).collect(),
            bonds: self.bonds().map(|(id, bond)| (id, bond.clone())).collect(),
            objects: self
                .content
                .tree
                .objects
                .iter()
                .map(|(id, object)| (*id, object.clone()))
                .collect(),
            roots: self.content.tree.roots.clone(),
            positions: self
                .content
                .positions
                .iter()
                .map(|(key, point)| (*key, *point))
                .collect(),
            selection: self.selection.clone(),
            next_atom: self.next_atom,
            next_bond: self.next_bond,
            next_object: self.next_object,
            undo: self.undo.iter().cloned().collect(),
            redo: self.redo.clone(),
        }
    }

    pub fn id(&self) -> &DocumentId {
        &self.id
    }

    /// Increases by exactly one for each effective transaction, undo, or redo.
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    pub fn title(&self) -> &str {
        &self.content.title
    }

    pub fn atoms(&self) -> impl ExactSizeIterator<Item = (AtomId, &Atom)> {
        self.content.atoms.iter().map(|(id, atom)| (*id, atom))
    }

    pub fn bonds(&self) -> impl ExactSizeIterator<Item = (BondId, &Bond)> {
        self.content.bonds.iter().map(|(id, bond)| (*id, bond))
    }

    /// The molecule's scene instances in identifier (creation) order; there is always at
    /// least one, wherever each is contained.
    pub fn instances(&self) -> impl Iterator<Item = ObjectId> + '_ {
        self.content.instances()
    }

    /// The first scene instance, whose positions the v1 view reports as `atoms[].position`.
    pub fn first_instance(&self) -> ObjectId {
        self.content.first_instance()
    }

    /// Every scene object by identifier.
    pub fn objects(&self) -> impl ExactSizeIterator<Item = (ObjectId, &SceneObject)> {
        self.content
            .tree
            .objects
            .iter()
            .map(|(id, object)| (*id, object))
    }

    pub fn object(&self, id: ObjectId) -> Option<&SceneObject> {
        self.content.tree.objects.get(&id)
    }

    /// The page's root objects in order.
    pub fn roots(&self) -> &[ObjectId] {
        &self.content.tree.roots
    }

    /// Every scene object depth first from the page's roots, children in order, with its
    /// parent.
    pub fn objects_in_tree_order(&self) -> Vec<(ObjectId, Parent)> {
        self.content.tree.in_order()
    }

    /// What the latest edit, undo, or redo left selected.
    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    /// Every atom's position in `instance`, by atom; empty for an unknown instance.
    pub fn positions(&self, instance: ObjectId) -> impl Iterator<Item = (AtomId, Point)> + '_ {
        self.content.instance_positions(instance)
    }

    pub fn position(&self, instance: ObjectId, atom: AtomId) -> Option<Point> {
        self.content
            .positions
            .get(&PositionKey { instance, atom })
            .copied()
    }

    /// The revision and identifier counters this document has reached.
    pub const fn numbering(&self) -> Numbering {
        Numbering {
            revision: self.revision,
            next_atom: self.next_atom,
            next_bond: self.next_bond,
            next_object: self.next_object,
        }
    }

    /// Moves the revision above `reached.revision` and the identifier counters up to
    /// `reached`'s, leaving content and history unchanged. Used when this copy replaces one
    /// that handed out revisions or identifiers for states that were never stored, so those
    /// numbers are not handed out again for different states. Never moves anything back, and
    /// changes nothing when no revision above `reached.revision` remains.
    pub fn continue_after(&mut self, reached: Numbering) -> Result<(), RevisionsExhausted> {
        if self.revision <= reached.revision {
            if reached.revision >= MAX_REVISION {
                return Err(RevisionsExhausted);
            }
            self.revision = reached.revision + 1;
        }
        self.next_atom = self.next_atom.max(reached.next_atom);
        self.next_bond = self.next_bond.max(reached.next_bond);
        self.next_object = self.next_object.max(reached.next_object);
        Ok(())
    }

    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    pub fn redo_depth(&self) -> usize {
        self.redo.len()
    }

    /// Applies every command or none, keeping what was selected wherever it still exists.
    /// A transaction that changes nothing leaves the revision, identifier counters, selection,
    /// and history untouched.
    pub fn apply(&mut self, commands: &[Command]) -> Result<Outcome, TransactionError> {
        self.apply_with_selection(commands, None)
    }

    /// Applies every command or none and records `selection` as the selection after it; see
    /// [`Document::apply_with_selections`].
    pub fn apply_with_selection(
        &mut self,
        commands: &[Command],
        selection: Option<Selection>,
    ) -> Result<Outcome, TransactionError> {
        self.apply_with_selections(
            commands,
            SelectionUpdate {
                before: None,
                after: selection.map(SelectionAfter::from),
            },
        )
    }

    /// Applies every command or none, then records the selections with the change set: the
    /// client's selection before the edit, which must name only what the document holds
    /// before it (without one, the selection the previous edit, undo, or redo left), and its
    /// selection after the edit, which must name only what the edited document holds (without
    /// one, the previous selection is kept wherever it still exists). The selection after may
    /// name an object by the index of the command that creates it ([`ObjectRef::Created`]);
    /// the change set records its identifier. Undo restores the first and redo the second. A
    /// transaction whose commands change nothing is not an edit: it leaves the revision,
    /// identifier counters, selection, and history untouched, so changing only the selection
    /// records nothing.
    pub fn apply_with_selections(
        &mut self,
        commands: &[Command],
        selections: SelectionUpdate,
    ) -> Result<Outcome, TransactionError> {
        let SelectionUpdate {
            before: selection_before,
            after: selection,
        } = selections;
        let whole = |error| TransactionError { index: None, error };
        if commands.is_empty() {
            return Err(whole(EditError::EmptyTransaction));
        }
        if commands.len() > MAX_COMMANDS {
            return Err(whole(EditError::TooManyCommands));
        }
        let selection_before = match selection_before {
            Some(selection) => {
                self.content
                    .check_selection(&selection)
                    .map_err(|error| match error {
                        EditError::Scene(SceneError::UnknownSelection(id)) => {
                            whole(SceneError::UnknownSelectionBefore(id).into())
                        }
                        other => whole(other),
                    })?;
                selection
            }
            None => self.selection.clone(),
        };
        let mut draft = Draft {
            content: self.content.clone(),
            next_atom: self.next_atom,
            next_bond: self.next_bond,
            next_object: self.next_object,
            created: Vec::with_capacity(commands.len()),
        };
        for (index, command) in commands.iter().enumerate() {
            draft.apply(command).map_err(|error| TransactionError {
                index: Some(index),
                error,
            })?;
        }
        let selection = match selection {
            Some(selection) => {
                let selection = draft.resolve_selection(selection).map_err(whole)?;
                draft.content.check_selection(&selection).map_err(whole)?;
                selection
            }
            None => draft.content.retained(&self.selection),
        };
        let changes = ChangeSet::between(
            &self.content,
            &draft.content,
            Change {
                before: selection_before,
                after: selection,
            },
        );
        if changes.is_empty() {
            return Ok(Outcome::Unchanged);
        }
        if self.revision >= MAX_EDIT_REVISION {
            return Err(whole(EditError::RevisionLimit));
        }
        self.content = draft.content;
        self.selection = changes.selection.after.clone();
        self.next_atom = draft.next_atom;
        self.next_bond = draft.next_bond;
        self.next_object = draft.next_object;
        self.revision += 1;
        self.undo.push_back(changes);
        self.redo.clear();
        while self.undo.len() > HISTORY_LIMIT || history_size(&self.undo) > HISTORY_CHANGE_LIMIT {
            self.undo.pop_front();
        }
        Ok(Outcome::Changed)
    }

    /// Reverts the latest transaction as a new revision, with the selection from before it.
    /// Identifier counters are not rewound, so a later addition never reuses an undone
    /// atom's, bond's, or scene object's identifier.
    pub fn undo(&mut self) -> Result<(), HistoryError> {
        if self.revision >= MAX_EDIT_REVISION {
            return Err(HistoryError::RevisionLimit);
        }
        let set = self.undo.pop_back().ok_or(HistoryError::NothingToUndo)?;
        step(&mut self.content, &set, Direction::Backward)
            .expect("undo entries are validated against the current state");
        self.selection = set.selection.before.clone();
        self.revision += 1;
        self.redo.push(set);
        Ok(())
    }

    /// Reapplies the latest undone transaction as a new revision, with the selection from
    /// after it.
    pub fn redo(&mut self) -> Result<(), HistoryError> {
        if self.revision >= MAX_EDIT_REVISION {
            return Err(HistoryError::RevisionLimit);
        }
        let set = self.redo.pop().ok_or(HistoryError::NothingToRedo)?;
        step(&mut self.content, &set, Direction::Forward)
            .expect("redo entries are validated against the current state");
        self.selection = set.selection.after.clone();
        self.revision += 1;
        self.undo.push_back(set);
        Ok(())
    }
}

fn history_size<'a>(sets: impl IntoIterator<Item = &'a ChangeSet>) -> usize {
    sets.into_iter().map(ChangeSet::size).sum()
}

/// Replays one change set on a copy and commits it only if it matches and stays valid.
fn step(content: &mut Content, set: &ChangeSet, direction: Direction) -> Result<(), ()> {
    let mut next = content.clone();
    if let Some(change) = &set.title {
        let (from, to) = direction.sides(change);
        if next.title != *from {
            return Err(());
        }
        next.title.clone_from(to);
    }
    if let Some(change) = &set.roots {
        let (from, to) = direction.sides(change);
        if next.tree.roots != *from {
            return Err(());
        }
        next.tree.roots.clone_from(to);
    }
    replay(&mut next.atoms, &set.atoms, direction)?;
    replay(&mut next.bonds, &set.bonds, direction)?;
    replay(&mut next.tree.objects, &set.objects, direction)?;
    replay(&mut next.positions, &set.positions, direction)?;
    next.validate().map_err(|_| ())?;
    *content = next;
    Ok(())
}
