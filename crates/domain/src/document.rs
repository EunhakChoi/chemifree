//! A chemical drawing document: a titled chemical graph of atoms and bonds kept apart from
//! its scene, stable identifiers, atomic transactions, and change-set history.
//!
//! The graph (molecule [`MOLECULE_ID`]) holds atoms and bonds without positions. The scene
//! holds one page ([`PAGE_ID`]) and an ordered list of molecule scene instances, each drawing
//! the graph on that page with exactly one position per atom. Documents, molecules, atoms,
//! bonds, scene instances, and pages are separate identifier namespaces. A document has one
//! graph and one page; groups, containment beyond page → instance, and other scene objects
//! are not represented yet.
//!
//! This is a drawing model. It records what the user drew and enforces structural
//! invariants (no dangling or duplicate bonds, a position per atom in every instance,
//! bounded values, and atom and bond states that hold together). It can represent a stated
//! hydrogen count, radicals, query atoms and bonds, aromatic and dative bonds, and attachment
//! points, but it does not perceive or compute anything: no valence, implicit hydrogen
//! count, aromaticity, stereochemistry, or query matching.

use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};
use std::fmt;

use crate::element::Element;

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
/// The most hydrogens an atom can be stated to carry ([`Hydrogens::Specified`]).
pub const MAX_HYDROGEN_COUNT: u8 = 16;
/// The most elements a query atom's element list names.
pub const MAX_QUERY_ELEMENTS: usize = 10;
/// Attachment points are numbered 1 through this.
pub const MAX_ATTACHMENT_POINT: u8 = 4;
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
serial_id!(InstanceId, "s");

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

    fn translated(self, delta: Vector) -> Result<Self, EditError> {
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
    pub kind: AtomKind,
    pub charge: i8,
    /// Explicit isotope mass number; `None` means natural abundance.
    pub isotope: Option<u16>,
    pub hydrogens: Hydrogens,
    pub radical: Radical,
    pub attachment_points: AttachmentPoints,
}

impl Atom {
    /// The element, or `None` for a query atom.
    pub fn element(&self) -> Option<Element> {
        match self.kind {
            AtomKind::Element(element) => Some(element),
            AtomKind::Query(_) => None,
        }
    }
}

/// A neutral, natural-abundance atom of `element` with inferred hydrogens, no radical, and
/// no attachment points: the state a new atom starts in.
impl From<Element> for Atom {
    fn from(element: Element) -> Self {
        Self::from(AtomKind::Element(element))
    }
}

/// An atom of `kind` in the state a new atom starts in.
impl From<AtomKind> for Atom {
    fn from(kind: AtomKind) -> Self {
        Self {
            kind,
            charge: 0,
            isotope: None,
            hydrogens: Hydrogens::default(),
            radical: Radical::default(),
            attachment_points: AttachmentPoints::default(),
        }
    }
}

/// What an atom stands for: one element, or a query naming which atoms it would match.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AtomKind {
    Element(Element),
    /// Recorded only: nothing matches queries yet.
    Query(AtomQuery),
}

/// A query atom: the atoms it stands for. Representation only; no matching is implemented.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AtomQuery {
    /// Any atom (`*`).
    Any,
    /// Any atom but hydrogen (`A`).
    AnyHeavy,
    /// Any atom but carbon and hydrogen (`Q`).
    Heteroatom,
    /// F, Cl, Br, I, or At (`X`).
    Halogen,
    /// One of `elements` (1 to [`MAX_QUERY_ELEMENTS`], distinct, in the order given), or with
    /// `negated` any atom that is none of them.
    List {
        elements: Vec<Element>,
        negated: bool,
    },
}

impl AtomQuery {
    /// The label drawn for the query: `*`, `A`, `Q`, `X`, `[N,O]`, or `![N,O]` when negated.
    /// No label is an element symbol.
    pub fn label(&self) -> String {
        match self {
            Self::Any => "*".to_owned(),
            Self::AnyHeavy => "A".to_owned(),
            Self::Heteroatom => "Q".to_owned(),
            Self::Halogen => "X".to_owned(),
            Self::List { elements, negated } => {
                let symbols: Vec<&str> = elements.iter().map(|element| element.symbol()).collect();
                let negation = if *negated { "!" } else { "" };
                format!("{negation}[{}]", symbols.join(","))
            }
        }
    }
}

/// Hydrogens on an atom that are not drawn as atoms of their own. A hydrogen drawn as a
/// separate atom is an ordinary atom of the graph and is never counted here.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Hydrogens {
    /// To be derived from valence. Nothing derives it yet, so no count is known: this is not
    /// a computed result.
    #[default]
    Inferred,
    /// Stated to be unknown.
    Unknown,
    /// Stated: this many hydrogens on the atom, 0 to [`MAX_HYDROGEN_COUNT`].
    Specified(u8),
}

/// Unpaired electrons as drawn; recorded, not derived from valence.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Radical {
    #[default]
    None,
    Singlet,
    Doublet,
    Triplet,
    Unknown,
}

/// The attachment points an atom carries: distinct numbers 1 to [`MAX_ATTACHMENT_POINT`]
/// giving the order in which a substituent attaches. A set, read in increasing order.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AttachmentPoints(u8);

impl AttachmentPoints {
    /// The set of `points`, in any order.
    ///
    /// # Errors
    ///
    /// [`EditError::AttachmentPointOutOfRange`] for a number outside 1 to
    /// [`MAX_ATTACHMENT_POINT`], and [`EditError::RepeatedAttachmentPoint`] for one given twice.
    pub fn try_new(points: &[u64]) -> Result<Self, EditError> {
        let mut bits = 0_u8;
        for &point in points {
            let number = u8::try_from(point)
                .ok()
                .filter(|number| (1..=MAX_ATTACHMENT_POINT).contains(number))
                .ok_or(EditError::AttachmentPointOutOfRange(point))?;
            let bit = 1 << (number - 1);
            if bits & bit != 0 {
                return Err(EditError::RepeatedAttachmentPoint(number));
            }
            bits |= bit;
        }
        Ok(Self(bits))
    }

    /// The numbers in increasing order.
    pub fn iter(self) -> impl Iterator<Item = u8> {
        (1..=MAX_ATTACHMENT_POINT).filter(move |number| self.0 & (1 << (number - 1)) != 0)
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// A molecule scene instance: one drawing of the document's molecule on its page. Its atom
/// positions are kept under [`PositionKey`]s. With one molecule and one page, the instance
/// itself carries nothing else yet.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SceneInstance;

/// Where an atom's position lives: in one scene instance. Orders by instance, then atom.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PositionKey {
    pub instance: InstanceId,
    pub atom: AtomId,
}

/// A bond's kind as drawn. Aromatic, dative, query, and unknown bonds are recorded as such:
/// none is turned into a single bond, and nothing perceives aromaticity or matches queries.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BondOrder {
    Single,
    Double,
    Triple,
    Aromatic,
    /// A dative bond from `begin` (the donor) to `end`.
    Dative,
    Query(BondQuery),
    /// Stated to be unknown.
    Unknown,
}

/// The bonds a query bond stands for. Representation only.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BondQuery {
    Any,
    /// Any of these orders: 1 to 4 distinct, in the order given.
    List(Vec<QueryBondOrder>),
}

/// An order a query bond can list.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum QueryBondOrder {
    Single,
    Double,
    Triple,
    Aromatic,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Bond {
    pub begin: AtomId,
    pub end: AtomId,
    pub order: BondOrder,
}

impl Bond {
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
    PerInstance(BTreeMap<InstanceId, Point>),
}

/// One edit. A transaction applies an ordered list of commands all-or-nothing.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// Adds an atom in the state given, optionally bonded to another atom (the bond begins at
    /// that atom and ends at the new one).
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
    /// Changes only the fields of `update` that are `Some`.
    UpdateAtom {
        atom: AtomId,
        update: AtomUpdate,
    },
    /// Translates each listed atom once, even if it is listed repeatedly, in one scene
    /// instance. `instance` may be left out only while the document has exactly one.
    MoveAtoms {
        atoms: Vec<AtomId>,
        delta: Vector,
        instance: Option<InstanceId>,
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
        source: Option<InstanceId>,
        offset: Vector,
    },
    /// Translates every position of one scene instance once.
    MoveInstance {
        instance: InstanceId,
        offset: Vector,
    },
    /// Deletes a scene instance and its positions; the last one is kept.
    DeleteInstance {
        instance: InstanceId,
    },
}

/// The fields of an atom an `UpdateAtom` command changes; `None` leaves a field as it is.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AtomUpdate {
    pub kind: Option<AtomKind>,
    pub charge: Option<i8>,
    /// `Some(None)` restores natural abundance.
    pub isotope: Option<Option<u16>>,
    pub hydrogens: Option<Hydrogens>,
    pub radical: Option<Radical>,
    pub attachment_points: Option<AttachmentPoints>,
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
    UnknownInstance(InstanceId),
    /// A single `position` was given while the document has several scene instances.
    PositionsRequired {
        instances: usize,
    },
    /// The positions of a new atom leave out this scene instance.
    MissingPosition(InstanceId),
    /// `move_atoms` did not name an instance while the document has several.
    InstanceRequired {
        instances: usize,
    },
    /// A document keeps at least one scene instance of its molecule.
    LastInstance,
    InstanceLimit,
    /// A stated hydrogen count above [`MAX_HYDROGEN_COUNT`].
    HydrogenCountOutOfRange(u64),
    /// A query atom with a formal charge: charges in queries are not represented.
    QueryAtomCharge,
    /// A query atom with an isotope: isotopes in queries are not represented.
    QueryAtomIsotope,
    /// A query list (of elements or bond orders) with no entry.
    EmptyQueryList,
    /// A query list naming an entry twice.
    RepeatedQueryEntry,
    /// An element list longer than [`MAX_QUERY_ELEMENTS`].
    QueryListTooLong,
    AttachmentPointOutOfRange(u64),
    RepeatedAttachmentPoint(u8),
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
            Self::HydrogenCountOutOfRange(count) => write!(
                formatter,
                "hydrogen count {count} is outside 0..={MAX_HYDROGEN_COUNT}"
            ),
            Self::QueryAtomCharge => write!(
                formatter,
                "a query atom cannot carry a charge; set the charge to 0 first"
            ),
            Self::QueryAtomIsotope => write!(
                formatter,
                "a query atom cannot carry an isotope; restore natural abundance first"
            ),
            Self::EmptyQueryList => write!(formatter, "a query list needs at least one entry"),
            Self::RepeatedQueryEntry => write!(formatter, "a query list names an entry twice"),
            Self::QueryListTooLong => write!(
                formatter,
                "a query element list names at most {MAX_QUERY_ELEMENTS} elements"
            ),
            Self::AttachmentPointOutOfRange(point) => write!(
                formatter,
                "attachment point {point} is outside 1..={MAX_ATTACHMENT_POINT}"
            ),
            Self::RepeatedAttachmentPoint(point) => {
                write!(formatter, "attachment point {point} is given twice")
            }
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

#[derive(Clone, Debug, PartialEq)]
pub struct Change<T> {
    pub before: T,
    pub after: T,
}

/// The exact difference made by one transaction; applied backward to undo and forward to redo.
/// Graph changes (atoms, bonds), scene changes (instances, positions), and the title are
/// recorded apart, and undo or redo replays all of them as one step.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChangeSet {
    pub title: Option<Change<String>>,
    pub atoms: Vec<(AtomId, Change<Option<Atom>>)>,
    pub bonds: Vec<(BondId, Change<Option<Bond>>)>,
    pub instances: Vec<(InstanceId, Change<Option<SceneInstance>>)>,
    pub positions: Vec<(PositionKey, Change<Option<Point>>)>,
}

impl ChangeSet {
    fn between(before: &Content, after: &Content) -> Self {
        Self {
            title: (before.title != after.title).then(|| Change {
                before: before.title.clone(),
                after: after.title.clone(),
            }),
            atoms: diff_maps(&before.atoms, &after.atoms),
            bonds: diff_maps(&before.bonds, &after.bonds),
            instances: diff_maps(&before.instances, &after.instances),
            positions: diff_maps(&before.positions, &after.positions),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.atoms.is_empty()
            && self.bonds.is_empty()
            && self.instances.is_empty()
            && self.positions.is_empty()
    }

    /// Entity changes counted against [`HISTORY_CHANGE_LIMIT`].
    fn size(&self) -> usize {
        usize::from(self.title.is_some())
            + self.atoms.len()
            + self.bonds.len()
            + self.instances.len()
            + self.positions.len()
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
            && entries_valid(&self.atoms)
            && entries_valid(&self.bonds)
            && entries_valid(&self.instances)
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

fn replay<K: Copy + Ord, V: Clone + PartialEq>(
    map: &mut BTreeMap<K, V>,
    changes: &[(K, Change<Option<V>>)],
    direction: Direction,
) -> Result<(), ()> {
    for (key, change) in changes {
        let (from, to) = match direction {
            Direction::Backward => (&change.after, &change.before),
            Direction::Forward => (&change.before, &change.after),
        };
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
    /// In identifier order, which is creation order: identifiers are never reused.
    instances: BTreeMap<InstanceId, SceneInstance>,
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

    fn require_instance(&self, id: InstanceId) -> Result<InstanceId, EditError> {
        if self.instances.contains_key(&id) {
            Ok(id)
        } else {
            Err(EditError::UnknownInstance(id))
        }
    }

    fn first_instance(&self) -> InstanceId {
        *self
            .instances
            .keys()
            .next()
            .expect("validated content keeps at least one scene instance")
    }

    /// The instance a command acts in: the named one, or the only one.
    fn target_instance(&self, named: Option<InstanceId>) -> Result<InstanceId, EditError> {
        match named {
            Some(id) => self.require_instance(id),
            None if self.instances.len() == 1 => Ok(self.first_instance()),
            None => Err(EditError::InstanceRequired {
                instances: self.instances.len(),
            }),
        }
    }

    /// The positions of one instance, by atom.
    fn instance_positions(
        &self,
        instance: InstanceId,
    ) -> impl Iterator<Item = (AtomId, Point)> + '_ {
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
            validate_atom(atom)?;
        }
        let mut pairs = HashSet::with_capacity(self.bonds.len());
        for (id, bond) in &self.bonds {
            validate_order(&bond.order)?;
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
        if self.instances.is_empty() {
            return Err(EditError::LastInstance);
        }
        if self.instances.len() > MAX_INSTANCES {
            return Err(EditError::InstanceLimit);
        }
        for key in self.positions.keys() {
            self.require_instance(key.instance)?;
            self.require_atom(key.atom)?;
        }
        // Keys are unique and each names an existing instance and atom, so the count is
        // right exactly when every atom has one position in every instance.
        if self.positions.len() != self.instances.len() * self.atoms.len() {
            for instance in self.instances.keys() {
                for atom in self.atoms.keys() {
                    let key = PositionKey {
                        instance: *instance,
                        atom: *atom,
                    };
                    if !self.positions.contains_key(&key) {
                        return Err(EditError::MissingPosition(*instance));
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

fn validate_hydrogens(hydrogens: Hydrogens) -> Result<(), EditError> {
    match hydrogens {
        Hydrogens::Specified(count) if count > MAX_HYDROGEN_COUNT => {
            Err(EditError::HydrogenCountOutOfRange(count.into()))
        }
        _ => Ok(()),
    }
}

/// Checks an atom's values and that they hold together: a query atom has a valid query and
/// no charge or isotope.
fn validate_atom(atom: &Atom) -> Result<(), EditError> {
    validate_charge(atom.charge)?;
    validate_isotope(atom.isotope)?;
    validate_hydrogens(atom.hydrogens)?;
    if let AtomKind::Query(query) = &atom.kind {
        if let AtomQuery::List { elements, .. } = query {
            validate_list(elements)?;
            if elements.len() > MAX_QUERY_ELEMENTS {
                return Err(EditError::QueryListTooLong);
            }
        }
        if atom.charge != 0 {
            return Err(EditError::QueryAtomCharge);
        }
        if atom.isotope.is_some() {
            return Err(EditError::QueryAtomIsotope);
        }
    }
    Ok(())
}

fn validate_order(order: &BondOrder) -> Result<(), EditError> {
    match order {
        BondOrder::Query(BondQuery::List(orders)) => validate_list(orders),
        _ => Ok(()),
    }
}

/// A query list is non-empty and names each entry once.
fn validate_list<T: Copy + Ord>(entries: &[T]) -> Result<(), EditError> {
    if entries.is_empty() {
        return Err(EditError::EmptyQueryList);
    }
    let distinct: BTreeSet<T> = entries.iter().copied().collect();
    if distinct.len() == entries.len() {
        Ok(())
    } else {
        Err(EditError::RepeatedQueryEntry)
    }
}

/// Mutable working state for one transaction; discarded unless every command succeeds.
struct Draft {
    content: Content,
    next_atom: u32,
    next_bond: u32,
    next_instance: u32,
    /// The atom each earlier command added, by command index.
    created: Vec<Option<AtomId>>,
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
        validate_order(&order)?;
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
        let id = BondId(self.next_bond);
        self.next_bond = self
            .next_bond
            .checked_add(1)
            .ok_or(EditError::IdentifierSpaceExhausted)?;
        self.content.bonds.insert(id, Bond { begin, end, order });
        Ok(())
    }

    fn allocate_instance(&mut self) -> Result<InstanceId, EditError> {
        if self.content.instances.len() >= MAX_INSTANCES {
            return Err(EditError::InstanceLimit);
        }
        let id = InstanceId(self.next_instance);
        self.next_instance = self
            .next_instance
            .checked_add(1)
            .ok_or(EditError::IdentifierSpaceExhausted)?;
        Ok(id)
    }

    /// A new atom's position in each current scene instance.
    fn new_atom_positions(
        &self,
        positions: &NewAtomPositions,
    ) -> Result<Vec<(InstanceId, Point)>, EditError> {
        let instances = &self.content.instances;
        match positions {
            NewAtomPositions::Single(point) if instances.len() == 1 => {
                Ok(vec![(self.content.first_instance(), *point)])
            }
            NewAtomPositions::Single(_) => Err(EditError::PositionsRequired {
                instances: instances.len(),
            }),
            NewAtomPositions::PerInstance(points) => {
                if let Some(extra) = points.keys().find(|id| !instances.contains_key(id)) {
                    return Err(EditError::UnknownInstance(*extra));
                }
                instances
                    .keys()
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
        instance: InstanceId,
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
            AtomRef::Created(index) => self
                .created
                .get(index)
                .copied()
                .flatten()
                .ok_or(EditError::UnresolvedReference(index)),
        }
    }

    /// Applies one command and records which atom, if any, it added.
    fn apply(&mut self, command: &Command) -> Result<(), EditError> {
        let mut created = None;
        match command {
            Command::AddAtom {
                atom,
                positions,
                bond_to,
            } => {
                validate_atom(atom)?;
                let target = bond_to
                    .as_ref()
                    .map(|target| {
                        Ok::<_, EditError>((self.resolve(target.atom)?, target.order.clone()))
                    })
                    .transpose()?;
                if let Some((atom, _)) = target {
                    self.content.require_atom(atom)?;
                }
                let points = self.new_atom_positions(positions)?;
                let id = self.allocate_atom()?;
                created = Some(id);
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
            Command::UpdateAtom { atom, update } => {
                // Each value on its own first, then the atom it makes.
                if let Some(charge) = update.charge {
                    validate_charge(charge)?;
                }
                if let Some(isotope) = update.isotope {
                    validate_isotope(isotope)?;
                }
                if let Some(hydrogens) = update.hydrogens {
                    validate_hydrogens(hydrogens)?;
                }
                let target = self
                    .content
                    .atoms
                    .get_mut(atom)
                    .ok_or(EditError::UnknownAtom(*atom))?;
                let mut updated = target.clone();
                if let Some(kind) = &update.kind {
                    updated.kind = kind.clone();
                }
                if let Some(charge) = update.charge {
                    updated.charge = charge;
                }
                if let Some(isotope) = update.isotope {
                    updated.isotope = isotope;
                }
                if let Some(hydrogens) = update.hydrogens {
                    updated.hydrogens = hydrogens;
                }
                if let Some(radical) = update.radical {
                    updated.radical = radical;
                }
                if let Some(points) = update.attachment_points {
                    updated.attachment_points = points;
                }
                validate_atom(&updated)?;
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
                validate_order(order)?;
                self.content
                    .bonds
                    .get_mut(bond)
                    .ok_or(EditError::UnknownBond(*bond))?
                    .order = order.clone();
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
                let id = self.allocate_instance()?;
                self.content.instances.insert(id, SceneInstance);
                for (atom, point) in copied {
                    self.content
                        .positions
                        .insert(PositionKey { instance: id, atom }, point);
                }
            }
            Command::MoveInstance { instance, offset } => {
                let instance = self.content.require_instance(*instance)?;
                let atoms: Vec<AtomId> = self.content.atoms.keys().copied().collect();
                self.translate(instance, atoms, *offset)?;
            }
            Command::DeleteInstance { instance } => {
                let instance = self.content.require_instance(*instance)?;
                if self.content.instances.len() == 1 {
                    return Err(EditError::LastInstance);
                }
                self.content.instances.remove(&instance);
                self.content
                    .positions
                    .retain(|key, _| key.instance != instance);
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
    pub next_instance: u32,
}

impl Numbering {
    /// The later of two numberings, field by field.
    #[must_use]
    pub fn max(self, other: Self) -> Self {
        Self {
            revision: self.revision.max(other.revision),
            next_atom: self.next_atom.max(other.next_atom),
            next_bond: self.next_bond.max(other.next_bond),
            next_instance: self.next_instance.max(other.next_instance),
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
    /// Scene instances in order; each is on page [`PAGE_ID`] and draws molecule [`MOLECULE_ID`].
    pub instances: Vec<InstanceId>,
    pub positions: Vec<(PositionKey, Point)>,
    pub next_atom: u32,
    pub next_bond: u32,
    pub next_instance: u32,
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
    next_atom: u32,
    next_bond: u32,
    next_instance: u32,
    undo: VecDeque<ChangeSet>,
    redo: Vec<ChangeSet>,
}

impl Document {
    /// Creates an empty document at revision 0 with one scene instance, `s1`. Surrounding
    /// whitespace is trimmed from the title.
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
                instances: BTreeMap::from([(InstanceId(1), SceneInstance)]),
                positions: BTreeMap::new(),
            },
            next_atom: 1,
            next_bond: 1,
            next_instance: 2,
            undo: VecDeque::new(),
            redo: Vec::new(),
        })
    }

    /// An independent document with this one's graph, scene, and identifier counters under a
    /// new identifier and title, at revision 0 with no history. Atom, bond, and instance
    /// identifiers are kept, so commands that name them mean the same entities in the copy.
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
            next_atom: self.next_atom,
            next_bond: self.next_bond,
            next_instance: self.next_instance,
            undo: VecDeque::new(),
            redo: Vec::new(),
        })
    }

    /// Rebuilds a stored document, rejecting any state commands could not have produced,
    /// including history entries that do not replay cleanly from the current state.
    pub fn restore(parts: DocumentParts) -> Result<Self, RestoreError> {
        let invalid = |reason: &str| RestoreError(reason.to_owned());
        if parts.revision > MAX_REVISION {
            return Err(invalid("revision exceeds the largest revision"));
        }
        let counts = (
            parts.atoms.len(),
            parts.bonds.len(),
            parts.instances.len(),
            parts.positions.len(),
        );
        let content = Content {
            title: parts.title,
            atoms: parts.atoms.into_iter().collect(),
            bonds: parts.bonds.into_iter().collect(),
            instances: parts
                .instances
                .into_iter()
                .map(|id| (id, SceneInstance))
                .collect(),
            positions: parts.positions.into_iter().collect(),
        };
        if counts
            != (
                content.atoms.len(),
                content.bonds.len(),
                content.instances.len(),
                content.positions.len(),
            )
        {
            return Err(invalid(
                "duplicate atom, bond, or scene instance identifier, or a repeated position",
            ));
        }
        content
            .validate()
            .map_err(|error| RestoreError(format!("invalid document content: {error}")))?;
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
        let document = Self {
            id: parts.id,
            revision: parts.revision,
            content,
            next_atom: parts.next_atom,
            next_bond: parts.next_bond,
            next_instance: parts.next_instance,
            undo,
            redo,
        };
        let atom_ids = document.content.atoms.keys().copied().chain(
            document
                .undo
                .iter()
                .chain(&document.redo)
                .flat_map(|set| set.atoms.iter().map(|(id, _)| *id)),
        );
        let bond_ids = document.content.bonds.keys().copied().chain(
            document
                .undo
                .iter()
                .chain(&document.redo)
                .flat_map(|set| set.bonds.iter().map(|(id, _)| *id)),
        );
        // Positions in history name instances too, and must not name a future one.
        let instance_ids = document.content.instances.keys().copied().chain(
            document.undo.iter().chain(&document.redo).flat_map(|set| {
                set.instances
                    .iter()
                    .map(|(id, _)| *id)
                    .chain(set.positions.iter().map(|(key, _)| key.instance))
            }),
        );
        // Serial 0 is not an identifier, so a counter of 0 would allocate an invalid one.
        if document.next_atom == 0
            || document.next_bond == 0
            || document.next_instance == 0
            || atom_ids
                .map(AtomId::serial)
                .any(|serial| serial >= document.next_atom)
            || bond_ids
                .map(BondId::serial)
                .any(|serial| serial >= document.next_bond)
            || instance_ids
                .map(InstanceId::serial)
                .any(|serial| serial >= document.next_instance)
        {
            return Err(invalid(
                "identifier counter would reuse an existing identifier",
            ));
        }
        if document
            .undo
            .iter()
            .chain(&document.redo)
            .any(|set| !set.is_well_formed())
        {
            return Err(invalid(
                "history contains an empty, repeated, or no-op change",
            ));
        }
        let mut past = document.content.clone();
        for set in document.undo.iter().rev() {
            step(&mut past, set, Direction::Backward)
                .map_err(|()| invalid("undo history does not match the document"))?;
        }
        let mut future = document.content.clone();
        for set in document.redo.iter().rev() {
            step(&mut future, set, Direction::Forward)
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
            instances: self.instances().collect(),
            positions: self
                .content
                .positions
                .iter()
                .map(|(key, point)| (*key, *point))
                .collect(),
            next_atom: self.next_atom,
            next_bond: self.next_bond,
            next_instance: self.next_instance,
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

    /// The molecule's scene instances in order; there is always at least one.
    pub fn instances(&self) -> impl ExactSizeIterator<Item = InstanceId> + '_ {
        self.content.instances.keys().copied()
    }

    /// The first scene instance, whose positions the v1 view reports as `atoms[].position`.
    pub fn first_instance(&self) -> InstanceId {
        self.content.first_instance()
    }

    /// Every atom's position in `instance`, by atom; empty for an unknown instance.
    pub fn positions(&self, instance: InstanceId) -> impl Iterator<Item = (AtomId, Point)> + '_ {
        self.content.instance_positions(instance)
    }

    pub fn position(&self, instance: InstanceId, atom: AtomId) -> Option<Point> {
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
            next_instance: self.next_instance,
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
        self.next_instance = self.next_instance.max(reached.next_instance);
        Ok(())
    }

    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    pub fn redo_depth(&self) -> usize {
        self.redo.len()
    }

    /// Applies every command or none. A transaction that changes nothing leaves the
    /// revision, identifier counters, and history untouched.
    pub fn apply(&mut self, commands: &[Command]) -> Result<Outcome, TransactionError> {
        let whole = |error| TransactionError { index: None, error };
        if commands.is_empty() {
            return Err(whole(EditError::EmptyTransaction));
        }
        if commands.len() > MAX_COMMANDS {
            return Err(whole(EditError::TooManyCommands));
        }
        let mut draft = Draft {
            content: self.content.clone(),
            next_atom: self.next_atom,
            next_bond: self.next_bond,
            next_instance: self.next_instance,
            created: Vec::with_capacity(commands.len()),
        };
        for (index, command) in commands.iter().enumerate() {
            draft.apply(command).map_err(|error| TransactionError {
                index: Some(index),
                error,
            })?;
        }
        let changes = ChangeSet::between(&self.content, &draft.content);
        if changes.is_empty() {
            return Ok(Outcome::Unchanged);
        }
        if self.revision >= MAX_EDIT_REVISION {
            return Err(TransactionError {
                index: None,
                error: EditError::RevisionLimit,
            });
        }
        self.content = draft.content;
        self.next_atom = draft.next_atom;
        self.next_bond = draft.next_bond;
        self.next_instance = draft.next_instance;
        self.revision += 1;
        self.undo.push_back(changes);
        self.redo.clear();
        while self.undo.len() > HISTORY_LIMIT || history_size(&self.undo) > HISTORY_CHANGE_LIMIT {
            self.undo.pop_front();
        }
        Ok(Outcome::Changed)
    }

    /// Reverts the latest transaction as a new revision. Identifier counters are not
    /// rewound, so a later addition never reuses an undone atom's or bond's identifier.
    pub fn undo(&mut self) -> Result<(), HistoryError> {
        if self.revision >= MAX_EDIT_REVISION {
            return Err(HistoryError::RevisionLimit);
        }
        let set = self.undo.pop_back().ok_or(HistoryError::NothingToUndo)?;
        step(&mut self.content, &set, Direction::Backward)
            .expect("undo entries are validated against the current state");
        self.revision += 1;
        self.redo.push(set);
        Ok(())
    }

    pub fn redo(&mut self) -> Result<(), HistoryError> {
        if self.revision >= MAX_EDIT_REVISION {
            return Err(HistoryError::RevisionLimit);
        }
        let set = self.redo.pop().ok_or(HistoryError::NothingToRedo)?;
        step(&mut self.content, &set, Direction::Forward)
            .expect("redo entries are validated against the current state");
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
        let (from, to) = match direction {
            Direction::Backward => (&change.after, &change.before),
            Direction::Forward => (&change.before, &change.after),
        };
        if next.title != *from {
            return Err(());
        }
        next.title.clone_from(to);
    }
    replay(&mut next.atoms, &set.atoms, direction)?;
    replay(&mut next.bonds, &set.bonds, direction)?;
    replay(&mut next.instances, &set.instances, direction)?;
    replay(&mut next.positions, &set.positions, direction)?;
    next.validate().map_err(|_| ())?;
    *content = next;
    Ok(())
}
