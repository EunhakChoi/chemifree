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
//! instance, bounded values, valid state combinations). It does not perceive valence,
//! aromaticity, stereochemistry, or implicit hydrogens, and matches no query.

use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};
use std::fmt;

use crate::element::Element;
use crate::scene::{
    DeleteMode, Graphic, MAX_DEPTH, MAX_OBJECTS, MAX_REFERENCES, ObjectKind, ObjectRef,
    ReferenceTarget, SceneError, SceneObject, SceneReference, SceneTree, Selection,
    SelectionRequest, TransactionSelection,
};
use crate::states::{
    AtomKind, AttachmentPoints, BondQuery, Hydrogens, MAX_STATED_HYDROGENS, Radical, StateError,
};

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

/// Where an atom's position lives: in one molecule scene instance. Orders by instance, then
/// atom.
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
    /// Adds a scene instance of the molecule as the last page root, with the positions of
    /// `source` (the first instance when `None`) translated by `offset`.
    AddInstance {
        source: Option<ObjectId>,
        offset: Vector,
    },
    /// Translates every position of one scene instance once.
    MoveInstance {
        instance: ObjectId,
        offset: Vector,
    },
    /// Deletes a scene instance and its positions; the last one is kept.
    DeleteInstance {
        instance: ObjectId,
    },
    /// Adds a graphic under `parent` (a group, or the page when `None`) at `index` among its
    /// children (the end when `None`).
    AddGraphic {
        graphic: Graphic,
        parent: Option<ObjectId>,
        index: Option<usize>,
    },
    /// Puts at least two objects sharing one parent into a new group, which takes the place of
    /// the earliest of them; they keep their order.
    Group {
        objects: Vec<ObjectId>,
    },
    /// Removes a group; its children take its place.
    Ungroup {
        group: ObjectId,
    },
    /// Moves an object to `index` among its parent's children.
    Reorder {
        object: ObjectId,
        index: usize,
    },
    /// Deletes objects with or without what they contain. Deleting an instance deletes its
    /// positions; the last instance is kept.
    DeleteObjects {
        objects: Vec<ObjectId>,
        mode: DeleteMode,
    },
    /// Translates every graphic and instance the listed objects are or contain once, even when
    /// both a group and something inside it are listed.
    MoveObjects {
        objects: Vec<ObjectId>,
        delta: Vector,
    },
    /// Adds or removes a directed scene reference without assigning chemical meaning to it.
    AddReference {
        reference: SceneReference,
    },
    RemoveReference {
        reference: SceneReference,
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
    /// A scene object that does not exist.
    UnknownObject(ObjectId),
    /// A scene object of another kind than the command acts on.
    ObjectKindMismatch {
        object: ObjectId,
        expected: ObjectKind,
    },
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
    ObjectLimit,
    DepthLimit,
    /// A group needs at least two distinct objects.
    GroupTooSmall,
    /// Objects to group do not share one parent.
    GroupMixedParents,
    /// A position among a parent's `len` children that does not exist.
    IndexOutOfRange {
        index: usize,
        len: usize,
    },
    /// A selection names what does not exist; the message says what.
    SelectionUnknown(String),
    /// A containment tree that commands could not have produced; restored data only.
    SceneTreeInvalid(String),
    /// A reference names a missing source or target, or a link that does not exist.
    ReferenceUnknown(SceneReference),
    /// A surviving owner still refers to a target being deleted.
    ReferenceInUse(SceneReference),
    ReferenceLimit,
    /// A refused combination of recorded states, with its stable code.
    InvalidState(StateError),
}

impl EditError {
    /// The scene rule a command violated, when the error has a scene-specific code.
    pub fn scene_error(&self) -> Option<SceneError> {
        Some(match self {
            Self::UnknownObject(_) => SceneError::ObjectUnknown,
            Self::ObjectKindMismatch { .. } => SceneError::ObjectKindMismatch,
            Self::GroupMixedParents => SceneError::GroupMixedParents,
            Self::DepthLimit => SceneError::DepthLimit,
            Self::ObjectLimit => SceneError::ObjectLimit,
            Self::IndexOutOfRange { .. } => SceneError::IndexOutOfRange,
            Self::LastInstance => SceneError::LastInstance,
            Self::SelectionUnknown(_) => SceneError::SelectionUnknown,
            Self::ReferenceUnknown(_) => SceneError::ReferenceUnknown,
            Self::ReferenceInUse(_) => SceneError::ReferenceInUse,
            Self::ReferenceLimit => SceneError::ReferenceLimit,
            _ => return None,
        })
    }

    /// The stable code sent as `reason` with `invalid_command`, when the refusal has one.
    pub fn reason(&self) -> Option<&'static str> {
        match self {
            Self::InvalidState(state) => Some(state.code()),
            _ => self.scene_error().map(SceneError::code),
        }
    }
}

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
            Self::UnknownObject(id) => write!(formatter, "scene object {id} does not exist"),
            Self::ObjectKindMismatch { object, expected } => {
                write!(formatter, "scene object {object} is not {expected}")
            }
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
            Self::ObjectLimit => write!(
                formatter,
                "a document holds at most {MAX_OBJECTS} scene objects"
            ),
            Self::DepthLimit => write!(
                formatter,
                "scene objects nest at most {MAX_DEPTH} levels deep"
            ),
            Self::GroupTooSmall => write!(formatter, "a group needs at least two objects"),
            Self::GroupMixedParents => write!(formatter, "objects to group must share one parent"),
            Self::IndexOutOfRange { index, len } => write!(
                formatter,
                "index {index} is out of range among {len} children"
            ),
            Self::SelectionUnknown(detail) => write!(formatter, "the selection names {detail}"),
            Self::ReferenceUnknown(reference) => write!(
                formatter,
                "reference from {} to {} is not valid",
                reference.source, reference.target
            ),
            Self::ReferenceInUse(reference) => write!(
                formatter,
                "{} is still referenced by scene object {}",
                reference.target, reference.source
            ),
            Self::ReferenceLimit => write!(
                formatter,
                "a document holds at most {MAX_REFERENCES} references"
            ),
            Self::SceneTreeInvalid(detail) => formatter.write_str(detail),
            Self::InvalidState(error) => error.fmt(formatter),
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
/// Graph changes (atoms, bonds), scene changes (the page's root order, scene objects, and
/// positions), the title, and the selection are recorded apart, and undo or redo replays all of
/// them as one step.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChangeSet {
    pub title: Option<Change<String>>,
    pub atoms: Vec<(AtomId, Change<Option<Atom>>)>,
    pub bonds: Vec<(BondId, Change<Option<Bond>>)>,
    /// The page's root objects in order, when that changed.
    pub roots: Option<Change<Vec<ObjectId>>>,
    /// Scene objects; a group's state includes its ordered children.
    pub objects: Vec<(ObjectId, Change<Option<SceneObject>>)>,
    pub references: Vec<(SceneReference, Change<bool>)>,
    pub positions: Vec<(PositionKey, Change<Option<Point>>)>,
    /// The selection before and after the change. It is recorded with every entry and never
    /// makes one: a change set whose content is unchanged is empty.
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
            roots: (before.scene.roots != after.scene.roots).then(|| Change {
                before: before.scene.roots.clone(),
                after: after.scene.roots.clone(),
            }),
            objects: diff_maps(&before.scene.objects, &after.scene.objects),
            references: diff_sets(&before.references, &after.references),
            positions: diff_maps(&before.positions, &after.positions),
            selection,
        }
    }

    /// Whether the content is unchanged; the selection does not count.
    pub fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.atoms.is_empty()
            && self.bonds.is_empty()
            && self.roots.is_none()
            && self.objects.is_empty()
            && self.references.is_empty()
            && self.positions.is_empty()
    }

    /// Entity changes counted against [`HISTORY_CHANGE_LIMIT`].
    fn size(&self) -> usize {
        usize::from(self.title.is_some())
            + self.atoms.len()
            + self.bonds.len()
            + usize::from(self.roots.is_some())
            + self.objects.len()
            + self.references.len()
            + self.positions.len()
    }

    /// The shape `between` produces: non-empty, each entity at most once, every entry an
    /// actual change, and no object changing its kind or a graphic its shape. Only such a set
    /// replays identically backward and forward, so restored history that passes the backward
    /// check can always be redone.
    fn is_well_formed(&self) -> bool {
        fn entries_valid<K: Ord, V: PartialEq>(entries: &[(K, Change<Option<V>>)]) -> bool {
            let mut seen = BTreeSet::new();
            entries
                .iter()
                .all(|(id, change)| change.before != change.after && seen.insert(id))
        }
        let kinds_kept =
            self.objects
                .iter()
                .all(|(_, change)| match (&change.before, &change.after) {
                    (Some(before), Some(after)) => before.same_kind(after),
                    _ => true,
                });
        let mut references_seen = BTreeSet::new();
        let references_valid = self.references.iter().all(|(reference, change)| {
            change.before != change.after && references_seen.insert(reference)
        });
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
            && references_valid
            && entries_valid(&self.positions)
            && kinds_kept
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

fn diff_sets<T: Copy + Ord>(before: &BTreeSet<T>, after: &BTreeSet<T>) -> Vec<(T, Change<bool>)> {
    before
        .symmetric_difference(after)
        .map(|value| {
            (
                *value,
                Change {
                    before: before.contains(value),
                    after: after.contains(value),
                },
            )
        })
        .collect()
}

#[derive(Clone, Copy)]
enum Direction {
    Backward,
    Forward,
}

impl<T> Change<T> {
    /// The state replaying in `direction` starts from, and the one it leaves.
    fn sides(&self, direction: Direction) -> (&T, &T) {
        match direction {
            Direction::Backward => (&self.after, &self.before),
            Direction::Forward => (&self.before, &self.after),
        }
    }
}

fn replay<K: Copy + Ord, V: Clone + PartialEq>(
    map: &mut BTreeMap<K, V>,
    changes: &[(K, Change<Option<V>>)],
    direction: Direction,
) -> Result<(), ()> {
    for (key, change) in changes {
        let (from, to) = change.sides(direction);
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
    scene: SceneTree,
    references: BTreeSet<SceneReference>,
    positions: BTreeMap<PositionKey, Point>,
}

impl Content {
    fn target_exists(&self, target: ReferenceTarget) -> bool {
        match target {
            ReferenceTarget::Object(id) => self.scene.objects.contains_key(&id),
            ReferenceTarget::Atom(id) => self.atoms.contains_key(&id),
            ReferenceTarget::Bond(id) => self.bonds.contains_key(&id),
        }
    }

    fn invalid_reference(&self) -> Option<SceneReference> {
        self.references.iter().copied().find(|reference| {
            !self.scene.objects.contains_key(&reference.source)
                || !self.target_exists(reference.target)
        })
    }

    /// Called after a deletion has removed the references owned by deleted objects.
    fn refuse_dangling_references(&self) -> Result<(), EditError> {
        match self.invalid_reference() {
            Some(reference) => Err(EditError::ReferenceInUse(reference)),
            None => Ok(()),
        }
    }

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

    fn require_instance(&self, id: ObjectId) -> Result<ObjectId, EditError> {
        self.scene.require(id, ObjectKind::Instance).map(|()| id)
    }

    fn instance_count(&self) -> usize {
        self.scene.instances().count()
    }

    fn first_instance(&self) -> ObjectId {
        self.scene
            .instances()
            .next()
            .expect("validated content keeps at least one scene instance")
    }

    /// The instance a command acts in: the named one, or the only one.
    fn target_instance(&self, named: Option<ObjectId>) -> Result<ObjectId, EditError> {
        match named {
            Some(id) => self.require_instance(id),
            None => match self.instance_count() {
                1 => Ok(self.first_instance()),
                instances => Err(EditError::InstanceRequired { instances }),
            },
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

    /// Fails unless everything `selection` names exists.
    fn check_selection(&self, selection: &Selection) -> Result<(), EditError> {
        let unknown = |what: String| Err(EditError::SelectionUnknown(what));
        if let Some(id) = selection
            .objects
            .iter()
            .find(|id| !self.scene.objects.contains_key(id))
        {
            return unknown(format!("scene object {id}, which does not exist"));
        }
        if let Some(id) = selection
            .atoms
            .iter()
            .find(|id| !self.atoms.contains_key(id))
        {
            return unknown(format!("atom {id}, which does not exist"));
        }
        if let Some(id) = selection
            .bonds
            .iter()
            .find(|id| !self.bonds.contains_key(id))
        {
            return unknown(format!("bond {id}, which does not exist"));
        }
        Ok(())
    }

    /// `selection` less what does not exist here.
    fn existing(&self, selection: &Selection) -> Selection {
        Selection {
            objects: selection
                .objects
                .iter()
                .copied()
                .filter(|id| self.scene.objects.contains_key(id))
                .collect(),
            atoms: selection
                .atoms
                .iter()
                .copied()
                .filter(|id| self.atoms.contains_key(id))
                .collect(),
            bonds: selection
                .bonds
                .iter()
                .copied()
                .filter(|id| self.bonds.contains_key(id))
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
        self.scene.validate()?;
        if self.references.len() > MAX_REFERENCES {
            return Err(EditError::ReferenceLimit);
        }
        if let Some(reference) = self.invalid_reference() {
            return Err(EditError::ReferenceUnknown(reference));
        }
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
            for instance in self.scene.instances() {
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

/// Trims surrounding whitespace and validates a title for creation, rename, or copying.
///
/// # Errors
///
/// Returns [`EditError::InvalidTitle`] when the trimmed title is empty, too long, or contains
/// control characters.
pub fn validated_title(title: &str) -> Result<&str, EditError> {
    let title = title.trim();
    validate_title(title)?;
    Ok(title)
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

/// What a command created, which later references (`$N`) can name.
#[derive(Clone, Copy)]
enum Created {
    Nothing,
    Atom(AtomId),
    Object(ObjectId),
}

/// Mutable working state for one transaction; discarded unless every command succeeds.
struct Draft {
    content: Content,
    next_atom: u32,
    next_bond: u32,
    next_object: u32,
    /// What each earlier command created, by command index.
    created: Vec<Created>,
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

    fn allocate_object(&mut self) -> Result<ObjectId, EditError> {
        if self.content.scene.objects.len() >= MAX_OBJECTS {
            return Err(EditError::ObjectLimit);
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
        match positions {
            NewAtomPositions::Single(point) => match self.content.instance_count() {
                1 => Ok(vec![(self.content.first_instance(), *point)]),
                instances => Err(EditError::PositionsRequired { instances }),
            },
            NewAtomPositions::PerInstance(points) => {
                for id in points.keys() {
                    self.content.require_instance(*id)?;
                }
                self.content
                    .scene
                    .instances()
                    .map(|id| {
                        points
                            .get(&id)
                            .map(|point| (id, *point))
                            .ok_or(EditError::MissingPosition(id))
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

    fn resolve(&self, atom: AtomRef) -> Result<AtomId, EditError> {
        match atom {
            AtomRef::Id(id) => Ok(id),
            AtomRef::Created(index) => match self.created.get(index) {
                Some(Created::Atom(id)) => Ok(*id),
                _ => Err(EditError::UnresolvedReference(index)),
            },
        }
    }

    /// The selection a transaction leaves, as the client named it: every `$N` must name a
    /// command that created a scene object, and everything named must exist after the edit.
    fn resolve_selection(&self, request: &SelectionRequest) -> Result<Selection, EditError> {
        let mut objects = BTreeSet::new();
        for object in &request.objects {
            let id = match object {
                ObjectRef::Id(id) => *id,
                ObjectRef::Created(index) => match self.created.get(*index) {
                    Some(Created::Object(id)) => *id,
                    _ => {
                        return Err(EditError::SelectionUnknown(format!(
                            "${index}, which is not a command that creates a scene object"
                        )));
                    }
                },
            };
            objects.insert(id);
        }
        let selection = Selection {
            objects,
            atoms: request.atoms.iter().copied().collect(),
            bonds: request.bonds.iter().copied().collect(),
        };
        self.content.check_selection(&selection)?;
        Ok(selection)
    }

    /// Moves every position of `instances` once.
    fn translate_instances(
        &mut self,
        instances: &[ObjectId],
        delta: Vector,
    ) -> Result<(), EditError> {
        let atoms: Vec<AtomId> = self.content.atoms.keys().copied().collect();
        for instance in instances {
            self.translate(*instance, atoms.iter().copied(), delta)?;
        }
        Ok(())
    }

    /// Applies one command and records what, if anything, it created.
    fn apply(&mut self, command: &Command) -> Result<(), EditError> {
        let mut created = Created::Nothing;
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
                created = Created::Atom(id);
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
                self.content.refuse_dangling_references()?;
            }
            Command::DeleteBonds { bonds } => {
                for id in unique(bonds)? {
                    self.content
                        .bonds
                        .remove(&id)
                        .ok_or(EditError::UnknownBond(id))?;
                }
                self.content.refuse_dangling_references()?;
            }
            Command::Rename { title } => {
                let title = validated_title(title)?;
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
                self.content
                    .scene
                    .insert(id, SceneObject::Instance, None, None)?;
                for (atom, point) in copied {
                    self.content
                        .positions
                        .insert(PositionKey { instance: id, atom }, point);
                }
                created = Created::Object(id);
            }
            Command::MoveInstance { instance, offset } => {
                let instance = self.content.require_instance(*instance)?;
                self.translate_instances(&[instance], *offset)?;
            }
            Command::DeleteInstance { instance } => {
                let instance = self.content.require_instance(*instance)?;
                self.delete_objects(&BTreeSet::from([instance]), DeleteMode::ObjectOnly)?;
            }
            Command::AddGraphic {
                graphic,
                parent,
                index,
            } => {
                let id = self.allocate_object()?;
                self.content.scene.insert(
                    id,
                    SceneObject::Graphic(graphic.clone()),
                    *parent,
                    *index,
                )?;
                created = Created::Object(id);
            }
            Command::Group { objects } => {
                if objects.is_empty() {
                    return Err(EditError::EmptyTargetList);
                }
                let id = self.allocate_object()?;
                self.content.scene.group(id, objects)?;
                created = Created::Object(id);
            }
            Command::Ungroup { group } => {
                self.content.scene.ungroup(*group)?;
                self.content
                    .references
                    .retain(|reference| reference.source != *group);
                self.content.refuse_dangling_references()?;
            }
            Command::Reorder { object, index } => self.content.scene.reorder(*object, *index)?,
            Command::DeleteObjects { objects, mode } => {
                self.delete_objects(&unique(objects)?, *mode)?;
            }
            Command::MoveObjects { objects, delta } => {
                let instances = self.content.scene.translate(&unique(objects)?, *delta)?;
                self.translate_instances(&instances, *delta)?;
            }
            Command::AddReference { reference } => {
                if !self.content.scene.objects.contains_key(&reference.source)
                    || !self.content.target_exists(reference.target)
                {
                    return Err(EditError::ReferenceUnknown(*reference));
                }
                if self.content.references.len() >= MAX_REFERENCES
                    && !self.content.references.contains(reference)
                {
                    return Err(EditError::ReferenceLimit);
                }
                self.content.references.insert(*reference);
            }
            Command::RemoveReference { reference } => {
                if !self.content.references.remove(reference) {
                    return Err(EditError::ReferenceUnknown(*reference));
                }
            }
        }
        self.created.push(created);
        Ok(())
    }

    fn delete_objects(
        &mut self,
        objects: &BTreeSet<ObjectId>,
        mode: DeleteMode,
    ) -> Result<(), EditError> {
        let removed = self.content.scene.delete(objects, mode)?;
        self.content
            .references
            .retain(|reference| !removed.contains(&reference.source));
        self.content
            .positions
            .retain(|key, _| !removed.contains(&key.instance));
        self.content.refuse_dangling_references()?;
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
    /// The page's root objects in order. Every scene object is on page [`PAGE_ID`], and every
    /// instance draws molecule [`MOLECULE_ID`].
    pub roots: Vec<ObjectId>,
    pub objects: Vec<(ObjectId, SceneObject)>,
    pub references: Vec<SceneReference>,
    pub positions: Vec<(PositionKey, Point)>,
    /// The current selection: where the history ends (see [`Document::restore`]).
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
    selection: Selection,
    next_atom: u32,
    next_bond: u32,
    next_object: u32,
    undo: VecDeque<ChangeSet>,
    redo: Vec<ChangeSet>,
}

impl Document {
    /// Creates an empty document at revision 0 with one scene instance, `s1`, and nothing
    /// selected. Surrounding whitespace is trimmed from the title.
    pub fn new(id: DocumentId, title: &str) -> Result<Self, EditError> {
        let title = validated_title(title)?;
        Ok(Self {
            id,
            revision: 0,
            content: Content {
                title: title.to_owned(),
                atoms: BTreeMap::new(),
                bonds: BTreeMap::new(),
                scene: SceneTree::with_instance(ObjectId(1)),
                references: BTreeSet::new(),
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
    /// and scene object identifiers are kept, so commands that name them mean the same entities
    /// in the copy.
    pub fn copy_as(&self, id: DocumentId, title: &str) -> Result<Self, EditError> {
        let title = validated_title(title)?;
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
    /// including history entries that do not replay cleanly from the current state, and a
    /// selection that is not where the history ends: the newest undo entry's `after` (left by
    /// an edit or a redo) or the nearest redo entry's `before` (left by an undo), compared as
    /// sets. With no history, any selection of what exists is accepted.
    pub fn restore(parts: DocumentParts) -> Result<Self, RestoreError> {
        let invalid = |reason: &str| RestoreError(reason.to_owned());
        if parts.revision > MAX_REVISION {
            return Err(invalid("revision exceeds the largest revision"));
        }
        let counts = (
            parts.atoms.len(),
            parts.bonds.len(),
            parts.objects.len(),
            parts.references.len(),
            parts.positions.len(),
        );
        let content = Content {
            title: parts.title,
            atoms: parts.atoms.into_iter().collect(),
            bonds: parts.bonds.into_iter().collect(),
            scene: SceneTree {
                roots: parts.roots,
                objects: parts.objects.into_iter().collect(),
            },
            references: parts.references.into_iter().collect(),
            positions: parts.positions.into_iter().collect(),
        };
        if counts
            != (
                content.atoms.len(),
                content.bonds.len(),
                content.scene.objects.len(),
                content.references.len(),
                content.positions.len(),
            )
        {
            return Err(invalid(
                "duplicate atom, bond, scene object, reference, or position",
            ));
        }
        content
            .validate()
            .map_err(|error| RestoreError(format!("invalid document content: {error}")))?;
        content
            .check_selection(&parts.selection)
            .map_err(|error| RestoreError(format!("invalid selection: {error}")))?;
        let mut undo: VecDeque<ChangeSet> = parts.undo.into();
        let mut redo = parts.redo;
        let mut selection = parts.selection;
        let history_end = [
            undo.back().map(|set| &set.selection.after),
            redo.last().map(|set| &set.selection.before),
        ];
        if history_end.iter().any(Option::is_some) && !history_end.contains(&Some(&selection)) {
            return Err(invalid(
                "the selection is not where the history ends: it is neither the newest undo \
                 entry's selection after nor the nearest redo entry's selection before",
            ));
        }
        // Editing keeps undo + redo within the limits (undo and redo only move entries between
        // them). A stored history beyond them, e.g. written under looser limits, is trimmed the
        // way editing trims: oldest undo entries first, then the farthest redo entries.
        while undo.len() + redo.len() > HISTORY_LIMIT
            || history_size(&undo) + history_size(&redo) > HISTORY_CHANGE_LIMIT
        {
            if undo.pop_front().is_none() {
                redo.remove(0);
            }
        }
        // With every undo entry gone, the state is where an undo of them all would leave it.
        if undo.is_empty()
            && let Some(next) = redo.last()
        {
            selection.clone_from(&next.selection.before);
        }
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
        document.check_counters()?;
        if document
            .undo
            .iter()
            .chain(&document.redo)
            .any(|set| !set.is_well_formed())
        {
            return Err(invalid(
                "history contains an empty, repeated, or no-op change, or changes an object's \
                 kind or a graphic's shape",
            ));
        }
        // An identifier can be introduced once, including across the current undo/redo
        // boundary. Replay alone cannot detect deleting an object and later creating a
        // different one with its old ID: both individual changes would still replay.
        let history = || document.undo.iter().chain(document.redo.iter().rev());
        if reuses_identifier(history().map(|set| set.atoms.as_slice()))
            || reuses_identifier(history().map(|set| set.bonds.as_slice()))
            || reuses_identifier(history().map(|set| set.objects.as_slice()))
        {
            return Err(invalid("history reuses a deleted identifier"));
        }
        let mut past = document.content.clone();
        for set in document.undo.iter().rev() {
            step_checked(&mut past, set, Direction::Backward)
                .map_err(|()| invalid("undo history does not match the document"))?;
        }
        let mut future = document.content.clone();
        for set in document.redo.iter().rev() {
            step_checked(&mut future, set, Direction::Forward)
                .map_err(|()| invalid("redo history does not match the document"))?;
        }
        Ok(document)
    }

    /// Refuses counters of zero (serial 0 is not an identifier) and counters that would hand
    /// out an identifier the content or history already uses.
    fn check_counters(&self) -> Result<(), RestoreError> {
        let history = || self.undo.iter().chain(&self.redo);
        let atom_ids = self
            .content
            .atoms
            .keys()
            .copied()
            .chain(history().flat_map(|set| set.atoms.iter().map(|(id, _)| *id)));
        let bond_ids = self
            .content
            .bonds
            .keys()
            .copied()
            .chain(history().flat_map(|set| set.bonds.iter().map(|(id, _)| *id)));
        // Positions in history name instances too, and must not name a future one.
        let object_ids = self
            .content
            .scene
            .objects
            .keys()
            .copied()
            .chain(history().flat_map(|set| {
                set.objects
                    .iter()
                    .map(|(id, _)| *id)
                    .chain(set.positions.iter().map(|(key, _)| key.instance))
            }));
        if self.next_atom == 0
            || self.next_bond == 0
            || self.next_object == 0
            || atom_ids
                .map(AtomId::serial)
                .any(|serial| serial >= self.next_atom)
            || bond_ids
                .map(BondId::serial)
                .any(|serial| serial >= self.next_bond)
            || object_ids
                .map(ObjectId::serial)
                .any(|serial| serial >= self.next_object)
        {
            return Err(RestoreError(
                "identifier counter would reuse an existing identifier".to_owned(),
            ));
        }
        Ok(())
    }

    pub fn to_parts(&self) -> DocumentParts {
        DocumentParts {
            id: self.id.clone(),
            revision: self.revision,
            title: self.content.title.clone(),
            atoms: self.atoms().map(|(id, atom)| (id, atom.clone())).collect(),
            bonds: self.bonds().map(|(id, bond)| (id, bond.clone())).collect(),
            roots: self.content.scene.roots.clone(),
            objects: self
                .content
                .scene
                .objects
                .iter()
                .map(|(id, object)| (*id, object.clone()))
                .collect(),
            references: self.content.references.iter().copied().collect(),
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

    /// The molecule's scene instances in identifier (creation) order, wherever they are in the
    /// containment tree; there is always at least one.
    pub fn instances(&self) -> impl Iterator<Item = ObjectId> + '_ {
        self.content.scene.instances()
    }

    /// The first scene instance by identifier, whose positions the v1 view reports as
    /// `atoms[].position`.
    pub fn first_instance(&self) -> ObjectId {
        self.content.first_instance()
    }

    /// The page's root objects in drawing order.
    pub fn roots(&self) -> &[ObjectId] {
        &self.content.scene.roots
    }

    pub fn object(&self, id: ObjectId) -> Option<&SceneObject> {
        self.content.scene.objects.get(&id)
    }

    /// Directed references in deterministic order, separate from scene containment.
    pub fn references(&self) -> impl ExactSizeIterator<Item = SceneReference> + '_ {
        self.content.references.iter().copied()
    }

    /// Every scene object in drawing order (depth first, a group before its children) with
    /// its parent (`None` for the page).
    pub fn scene_objects(&self) -> Vec<(ObjectId, Option<ObjectId>, &SceneObject)> {
        self.content
            .scene
            .depth_first()
            .into_iter()
            .map(|(id, parent)| (id, parent, &self.content.scene.objects[&id]))
            .collect()
    }

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

    /// Applies every command or none, keeping the current selection less what no longer
    /// exists; see [`Self::apply_selecting`].
    pub fn apply(&mut self, commands: &[Command]) -> Result<Outcome, TransactionError> {
        self.apply_selecting(commands, &TransactionSelection::default())
    }

    /// Applies every command or none and records the selection before and after with the
    /// change. `selection.before` is checked against the document before the commands and
    /// `selection.after` against it after them; a refused selection fails the whole
    /// transaction without a command index. A transaction that changes nothing leaves the
    /// revision, identifier counters, history, and selection untouched, whatever selections it
    /// names.
    pub fn apply_selecting(
        &mut self,
        commands: &[Command],
        selection: &TransactionSelection,
    ) -> Result<Outcome, TransactionError> {
        let whole = |error| TransactionError { index: None, error };
        if commands.is_empty() {
            return Err(whole(EditError::EmptyTransaction));
        }
        if commands.len() > MAX_COMMANDS {
            return Err(whole(EditError::TooManyCommands));
        }
        let before = match &selection.before {
            Some(before) => {
                self.content.check_selection(before).map_err(whole)?;
                before.clone()
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
        let after = match &selection.after {
            Some(request) => draft.resolve_selection(request).map_err(whole)?,
            None => draft.content.existing(&before),
        };
        let changes = ChangeSet::between(&self.content, &draft.content, Change { before, after });
        if changes.is_empty() {
            return Ok(Outcome::Unchanged);
        }
        if self.revision >= MAX_EDIT_REVISION {
            return Err(whole(EditError::RevisionLimit));
        }
        self.content = draft.content;
        self.selection.clone_from(&changes.selection.after);
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

    /// Reverts the latest transaction as a new revision and restores the selection it was
    /// made with. Identifier counters are not rewound, so a later addition never reuses an
    /// undone entity's identifier.
    pub fn undo(&mut self) -> Result<(), HistoryError> {
        if self.revision >= MAX_EDIT_REVISION {
            return Err(HistoryError::RevisionLimit);
        }
        let set = self.undo.pop_back().ok_or(HistoryError::NothingToUndo)?;
        step(&mut self.content, &set, Direction::Backward)
            .expect("undo entries are validated against the current state");
        self.selection.clone_from(&set.selection.before);
        self.revision += 1;
        self.redo.push(set);
        Ok(())
    }

    /// Reapplies the latest undone transaction as a new revision with the selection it left.
    pub fn redo(&mut self) -> Result<(), HistoryError> {
        if self.revision >= MAX_EDIT_REVISION {
            return Err(HistoryError::RevisionLimit);
        }
        let set = self.redo.pop().ok_or(HistoryError::NothingToRedo)?;
        step(&mut self.content, &set, Direction::Forward)
            .expect("redo entries are validated against the current state");
        self.selection.clone_from(&set.selection.after);
        self.revision += 1;
        self.undo.push_back(set);
        Ok(())
    }
}

fn history_size<'a>(sets: impl IntoIterator<Item = &'a ChangeSet>) -> usize {
    sets.into_iter().map(ChangeSet::size).sum()
}

/// History is in forward time order. A `before` value also records an identifier whose
/// original creation may have been trimmed from the oldest end of the history.
fn reuses_identifier<'a, K: Copy + Ord + 'a, V: 'a>(
    entries: impl IntoIterator<Item = &'a [(K, Change<Option<V>>)]>,
) -> bool {
    let mut seen = BTreeSet::new();
    for changes in entries {
        for (id, change) in changes {
            if change.before.is_none() && !seen.insert(*id) {
                return true;
            }
            if change.before.is_some() {
                seen.insert(*id);
            }
        }
    }
    false
}

/// Replays one restored change set like [`step`], and checks that its selections name only
/// what exists on their sides of it.
fn step_checked(content: &mut Content, set: &ChangeSet, direction: Direction) -> Result<(), ()> {
    let (from, to) = set.selection.sides(direction);
    content.check_selection(from).map_err(|_| ())?;
    step(content, set, direction)?;
    content.check_selection(to).map_err(|_| ())
}

/// Replays one change set's content on a copy and commits it only if it matches and stays
/// valid. The selection is not content; callers set it.
fn step(content: &mut Content, set: &ChangeSet, direction: Direction) -> Result<(), ()> {
    let mut next = content.clone();
    if let Some(change) = &set.title {
        let (from, to) = change.sides(direction);
        if next.title != *from {
            return Err(());
        }
        next.title.clone_from(to);
    }
    if let Some(change) = &set.roots {
        let (from, to) = change.sides(direction);
        if next.scene.roots != *from {
            return Err(());
        }
        next.scene.roots.clone_from(to);
    }
    replay(&mut next.atoms, &set.atoms, direction)?;
    replay(&mut next.bonds, &set.bonds, direction)?;
    replay(&mut next.scene.objects, &set.objects, direction)?;
    for (reference, change) in &set.references {
        let (from, to) = change.sides(direction);
        if next.references.contains(reference) != *from {
            return Err(());
        }
        if *to {
            next.references.insert(*reference);
        } else {
            next.references.remove(reference);
        }
    }
    replay(&mut next.positions, &set.positions, direction)?;
    next.validate().map_err(|_| ())?;
    *content = next;
    Ok(())
}
