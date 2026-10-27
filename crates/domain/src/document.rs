//! A chemical drawing document: a titled graph of atoms and bonds with one set of
//! scene positions, stable identifiers, atomic transactions, and change-set history.
//!
//! This is a drawing model. It records what the user drew and enforces structural
//! invariants (no dangling or duplicate bonds, bounded values). It does not perceive
//! valence, aromaticity, stereochemistry, or implicit hydrogens.

use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};
use std::fmt;

use crate::element::Element;

pub const MAX_ATOMS: usize = 2_000;
pub const MAX_BONDS: usize = 4_000;
pub const MAX_COMMANDS: usize = 500;
pub const MAX_TITLE_CHARS: usize = 200;
/// Scene coordinates are points (1/72 inch); x grows right and y grows down.
pub const MAX_COORDINATE: f64 = 100_000.0;
pub const MIN_CHARGE: i8 = -8;
pub const MAX_CHARGE: i8 = 8;
pub const MAX_ISOTOPE_MASS_NUMBER: u16 = 999;
/// Undo entries beyond this depth are discarded, oldest first.
pub const HISTORY_LIMIT: usize = 100;
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

#[derive(Clone, Debug, PartialEq)]
pub struct Atom {
    pub element: Element,
    pub charge: i8,
    /// Explicit isotope mass number; `None` means natural abundance.
    pub isotope: Option<u16>,
    pub position: Point,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BondOrder {
    Single,
    Double,
    Triple,
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BondTo {
    pub atom: AtomRef,
    pub order: BondOrder,
}

/// One edit. A transaction applies an ordered list of commands all-or-nothing.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// Adds a neutral, natural-abundance atom, optionally bonded to another atom.
    AddAtom {
        element: Element,
        position: Point,
        bond_to: Option<BondTo>,
    },
    AddBond {
        begin: AtomRef,
        end: AtomRef,
        order: BondOrder,
    },
    /// Changes only the fields that are `Some`; `isotope: Some(None)` clears the isotope.
    UpdateAtom {
        atom: AtomId,
        element: Option<Element>,
        charge: Option<i8>,
        isotope: Option<Option<u16>>,
    },
    /// Translates each listed atom once, even if it is listed repeatedly.
    MoveAtoms {
        atoms: Vec<AtomId>,
        delta: Vector,
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
}

impl fmt::Display for HistoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NothingToUndo => "there is nothing to undo",
            Self::NothingToRedo => "there is nothing to redo",
        })
    }
}

impl std::error::Error for HistoryError {}

#[derive(Clone, Debug, PartialEq)]
pub struct Change<T> {
    pub before: T,
    pub after: T,
}

/// The exact difference made by one transaction; applied backward to undo and forward to redo.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChangeSet {
    pub title: Option<Change<String>>,
    pub atoms: Vec<(AtomId, Change<Option<Atom>>)>,
    pub bonds: Vec<(BondId, Change<Option<Bond>>)>,
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
        }
    }

    pub fn is_empty(&self) -> bool {
        self.title.is_none() && self.atoms.is_empty() && self.bonds.is_empty()
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
            validate_charge(atom.charge)?;
            validate_isotope(atom.isotope)?;
        }
        let mut pairs = HashSet::with_capacity(self.bonds.len());
        for (id, bond) in &self.bonds {
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

/// Mutable working state for one transaction; discarded unless every command succeeds.
struct Draft {
    content: Content,
    next_atom: u32,
    next_bond: u32,
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
                element,
                position,
                bond_to,
            } => {
                let target = bond_to
                    .map(|target| Ok::<_, EditError>((self.resolve(target.atom)?, target.order)))
                    .transpose()?;
                if let Some((atom, _)) = target {
                    self.content.require_atom(atom)?;
                }
                let id = self.allocate_atom()?;
                created = Some(id);
                self.content.atoms.insert(
                    id,
                    Atom {
                        element: *element,
                        charge: 0,
                        isotope: None,
                        position: *position,
                    },
                );
                if let Some((atom, order)) = target {
                    self.insert_bond(atom, id, order)?;
                }
            }
            Command::AddBond { begin, end, order } => {
                let (begin, end) = (self.resolve(*begin)?, self.resolve(*end)?);
                self.insert_bond(begin, end, *order)?;
            }
            Command::UpdateAtom {
                atom,
                element,
                charge,
                isotope,
            } => {
                if let Some(charge) = charge {
                    validate_charge(*charge)?;
                }
                if let Some(isotope) = isotope {
                    validate_isotope(*isotope)?;
                }
                let target = self
                    .content
                    .atoms
                    .get_mut(atom)
                    .ok_or(EditError::UnknownAtom(*atom))?;
                if let Some(element) = element {
                    target.element = *element;
                }
                if let Some(charge) = charge {
                    target.charge = *charge;
                }
                if let Some(isotope) = isotope {
                    target.isotope = *isotope;
                }
            }
            Command::MoveAtoms { atoms, delta } => {
                for id in unique(atoms)? {
                    let atom = self
                        .content
                        .atoms
                        .get_mut(&id)
                        .ok_or(EditError::UnknownAtom(id))?;
                    atom.position = atom.position.translated(*delta)?;
                }
            }
            Command::UpdateBond { bond, order } => {
                self.content
                    .bonds
                    .get_mut(bond)
                    .ok_or(EditError::UnknownBond(*bond))?
                    .order = *order;
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

/// Persistable representation used by storage providers. `undo` is oldest first;
/// `redo` is a stack whose last entry is redone next.
#[derive(Clone, Debug, PartialEq)]
pub struct DocumentParts {
    pub id: DocumentId,
    pub revision: u64,
    pub title: String,
    pub atoms: Vec<(AtomId, Atom)>,
    pub bonds: Vec<(BondId, Bond)>,
    pub next_atom: u32,
    pub next_bond: u32,
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
    undo: VecDeque<ChangeSet>,
    redo: Vec<ChangeSet>,
}

impl Document {
    /// Creates an empty document at revision 0. Surrounding whitespace is trimmed from the title.
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
            },
            next_atom: 1,
            next_bond: 1,
            undo: VecDeque::new(),
            redo: Vec::new(),
        })
    }

    /// Rebuilds a stored document, rejecting any state commands could not have produced,
    /// including history entries that do not replay cleanly from the current state.
    pub fn restore(parts: DocumentParts) -> Result<Self, RestoreError> {
        let invalid = |reason: &str| RestoreError(reason.to_owned());
        let atom_count = parts.atoms.len();
        let bond_count = parts.bonds.len();
        let content = Content {
            title: parts.title,
            atoms: parts.atoms.into_iter().collect(),
            bonds: parts.bonds.into_iter().collect(),
        };
        if content.atoms.len() != atom_count || content.bonds.len() != bond_count {
            return Err(invalid("duplicate atom or bond identifier"));
        }
        content
            .validate()
            .map_err(|error| RestoreError(format!("invalid document content: {error}")))?;
        if parts.undo.len() > HISTORY_LIMIT || parts.redo.len() > HISTORY_LIMIT {
            return Err(invalid("history exceeds its limit"));
        }
        let document = Self {
            id: parts.id,
            revision: parts.revision,
            content,
            next_atom: parts.next_atom,
            next_bond: parts.next_bond,
            undo: parts.undo.into(),
            redo: parts.redo,
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
        // Serial 0 is not an identifier, so a counter of 0 would allocate an invalid one.
        if document.next_atom == 0
            || document.next_bond == 0
            || atom_ids
                .map(AtomId::serial)
                .any(|serial| serial >= document.next_atom)
            || bond_ids
                .map(BondId::serial)
                .any(|serial| serial >= document.next_bond)
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
            next_atom: self.next_atom,
            next_bond: self.next_bond,
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
        self.content = draft.content;
        self.next_atom = draft.next_atom;
        self.next_bond = draft.next_bond;
        self.revision += 1;
        self.undo.push_back(changes);
        if self.undo.len() > HISTORY_LIMIT {
            self.undo.pop_front();
        }
        self.redo.clear();
        Ok(Outcome::Changed)
    }

    /// Reverts the latest transaction as a new revision. Identifier counters are not
    /// rewound, so a later addition never reuses an undone atom's or bond's identifier.
    pub fn undo(&mut self) -> Result<(), HistoryError> {
        let set = self.undo.pop_back().ok_or(HistoryError::NothingToUndo)?;
        step(&mut self.content, &set, Direction::Backward)
            .expect("undo entries are validated against the current state");
        self.revision += 1;
        self.redo.push(set);
        Ok(())
    }

    pub fn redo(&mut self) -> Result<(), HistoryError> {
        let set = self.redo.pop().ok_or(HistoryError::NothingToRedo)?;
        step(&mut self.content, &set, Direction::Forward)
            .expect("redo entries are validated against the current state");
        self.revision += 1;
        self.undo.push_back(set);
        Ok(())
    }
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
    next.validate().map_err(|_| ())?;
    *content = next;
    Ok(())
}
