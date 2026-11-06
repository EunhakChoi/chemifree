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

use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};
use std::fmt;

use crate::element::Element;
use crate::scene;
use crate::scene::{
    ArrangementPlan, ArrangementUnit, Bounds, Caption, DeleteMode, Graphic, GraphicShape,
    MAX_DEPTH, MAX_OBJECTS, MAX_REFERENCES, ObjectArrangement, ObjectKind, ObjectRef,
    ReferenceTarget, SceneError, SceneObject, SceneReference, SceneTree, Selection,
    SelectionRequest, SelectionScope, SelectionTransform, TransactionSelection, TransformPlan,
    TransformedGraphic, TransformedPosition,
};
use crate::settings::{Settings, SettingsEdit, SettingsError};
use crate::states::{
    AtomKind, AttachmentPoints, BondQuery, Hydrogens, MAX_STATED_HYDROGENS, Radical, StateError,
};
use crate::stereo::{
    AtomConfiguration, BondConfiguration, CenterState, DoubleBondState, Graph, Slot, SlotRef,
    StereoError, StereoKey, StereoMark, StereoState, StereoTable, check_center, check_double_bond,
    read_legacy_reference, relative_double_bond,
};
use crate::stereo_drawing::{DeriveError, Drawing};
use crate::stereo_group::{
    GroupKind, GroupNote, StereoGroup, StereoGroupError, StereoGroupTable, meaning_changes,
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
serial_id!(
    /// An enhanced-stereo group (see [`crate::stereo_group`]).
    GroupId,
    "e"
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

/// A bond's kind as drawn. Recording an order does not itself perceive aromaticity,
/// assign a Kekulé form, or match a query; explicit interpretation is separate.
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
    /// How the bond is drawn, if as a stereo depiction. Setting or clearing it never changes
    /// `begin` or `end`: a wedge records its narrow end in the mark.
    pub mark: Option<StereoMark>,
}

impl Bond {
    /// A plain bond: no stereo mark.
    pub const fn new(begin: AtomId, end: AtomId, order: BondOrder) -> Self {
        Self {
            begin,
            end,
            order,
            mark: None,
        }
    }

    fn validate(&self) -> Result<(), EditError> {
        if let BondOrder::Query(query) = &self.order {
            query.validate()?;
        }
        if let Some(mark) = self.mark
            && !mark.fits(&self.order)
        {
            return Err(StereoError::MarkBondOrder.into());
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

/// A stereo group named in a command: an existing group, or one created earlier in the same
/// transaction by a `CreateStereoGroup` or `SplitStereoGroup` command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GroupRef {
    Id(GroupId),
    /// The group created by the command at this index in the transaction.
    Created(usize),
}

/// A bond named in a command: an existing bond, or one added earlier in the same transaction by
/// an `AddBond` command. (The bond of an `AddAtom`'s `bond_to` cannot be named.)
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BondRef {
    Id(BondId),
    /// The bond added by the `AddBond` command at this index in the transaction.
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

/// Supported independently defined regular carbon-ring recipes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RingSize {
    /// Five carbon atoms and five single bonds.
    Five,
    /// Six carbon atoms and six single bonds; the original recipe.
    #[default]
    Six,
}

/// Explicit bond recipes; these state bond orders without aromaticity inference.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RingPattern {
    /// All five or six bonds are single; the original recipe.
    #[default]
    Single,
    /// Six bonds alternate double/single, starting at the rightmost vertex clockwise.
    Kekule,
}

// Fixed recipes start at the rightmost vertex, clockwise in y-down coordinates.
// Keep the original six-ring arithmetic unchanged for compatibility.
fn ring_offsets(length: f64, size: RingSize) -> Vec<(f64, f64)> {
    match size {
        RingSize::Six => {
            let height = length * (3.0_f64.sqrt() / 2.0);
            vec![
                (length, 0.0),
                (length / 2.0, height),
                (-length / 2.0, height),
                (-length, 0.0),
                (-length / 2.0, -height),
                (length / 2.0, -height),
            ]
        }
        RingSize::Five => {
            // sin(36°), cos(72°), cos(36°), sin(72°), from the golden ratio.
            let root_five = 5.0_f64.sqrt();
            let radius = 2.0 * length / (10.0 - 2.0 * root_five).sqrt();
            let near_x = radius * (root_five - 1.0) / 4.0;
            let far_x = -radius * (root_five + 1.0) / 4.0;
            let high_y = radius * (10.0 + 2.0 * root_five).sqrt() / 4.0;
            vec![
                (radius, 0.0),
                (near_x, high_y),
                (far_x, length / 2.0),
                (far_x, -length / 2.0),
                (near_x, -high_y),
            ]
        }
    }
}

/// One edit. A transaction applies an ordered list of commands all-or-nothing.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// Appends an independent regular carbon ring in the only placement.
    /// Uses the current document bond length; creates no transaction-local `$N` target.
    InsertRing {
        instance: ObjectId,
        center: Point,
        size: RingSize,
        pattern: RingPattern,
    },
    /// Connects a fresh regular carbon ring to an existing atom with one single bond.
    /// Direction is finite and nonzero; its length is ignored. Stereo follows ordinary AddBond.
    AttachRing {
        instance: ObjectId,
        atom: AtomId,
        direction: Vector,
        size: RingSize,
    },
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
    /// Adds a bounded independent caption to the ordered scene.
    AddCaption {
        caption: Caption,
        parent: Option<ObjectId>,
        index: Option<usize>,
    },
    /// Replaces a caption's complete anchor and run state atomically.
    UpdateCaption {
        object: ObjectId,
        caption: Caption,
    },
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
    /// Moves the union of selected object subtrees, explicit atoms and selected bond
    /// endpoints once. Explicit graph targets use the named instance, or the only one.
    /// This selection determines geometry targets; transaction selection before/after
    /// independently controls the selection recorded with history.
    MoveSelection {
        selection: Selection,
        instance: Option<ObjectId>,
        delta: Vector,
    },
    /// Rigidly aligns or distributes the outermost selected scene objects using only
    /// atom positions and graphic points. Targets are recomputed at this command's turn.
    ArrangeObjects {
        objects: Vec<ObjectId>,
        arrangement: ObjectArrangement,
    },
    /// Transforms the resolved union of geometry once around an explicit pivot.
    /// Stored chemical configurations, marks and enhanced group membership do not change.
    TransformSelection {
        selection: Selection,
        instance: Option<ObjectId>,
        pivot: Point,
        transform: SelectionTransform,
    },
    /// Adds or removes a directed scene reference without assigning chemical meaning to it.
    AddReference {
        reference: SceneReference,
    },
    RemoveReference {
        reference: SceneReference,
    },
    /// Changes the named style settings as part of this transaction.
    UpdateSettings(SettingsEdit),
    /// Sets or, with `None`, clears how a bond is drawn. The mark must fit the bond's order. The
    /// bond's `begin` and `end` stay as they are.
    SetBondMark {
        bond: BondRef,
        mark: Option<StereoMark>,
    },
    /// Sets or, with `None`, clears the stated configuration of a stereocenter. The neighbors
    /// are checked against the center's bonds at this point of the transaction.
    SetAtomConfiguration {
        atom: AtomRef,
        configuration: Option<AtomConfiguration>,
    },
    /// Sets or, with `None`, clears the stated configuration of a double bond.
    SetBondConfiguration {
        bond: BondRef,
        configuration: Option<BondConfiguration>,
    },
    /// Stores the configuration the bond marks and the positions of one scene instance show
    /// (see [`crate::stereo_drawing`]).
    ///
    /// For every item it processes it replaces the stored record, whatever it is: a statement
    /// made by [`Self::SetAtomConfiguration`] or [`Self::SetBondConfiguration`], `unknown`, or
    /// another label all give way to what the drawing shows. A drawn mark never blocks a stated
    /// configuration when that is written, but this command reads marks, so it overwrites the
    /// statement for the items it processes. It writes nothing for an item it skips or does not
    /// name, and nothing at all when it refuses.
    ///
    /// With neither list, the items are those a mark bears on: each possible center at the
    /// narrow end of a wedge or hash or at an end of a wavy bond, and each crossed double bond.
    /// A mark whose narrow end could never hold a center record (a terminal atom, a query atom,
    /// an atom with five neighbors) is not in the scope at all. A crossed double bond that
    /// fails structurally is skipped: any structurally impossible item is skipped without a
    /// note, and the reply does not list what was skipped. Two causes can occur there,
    /// [`StereoError::GeometryUnsupported`] (a cumulated double or triple bond at an end) and
    /// [`StereoError::ReferenceInvalid`] (an end with more than two other neighbors);
    /// [`StereoError::CenterUnsupported`] and [`StereoError::DoubleBondRequired`] cannot,
    /// because the scope holds only possible centers and double bonds. Any other item the
    /// drawing does not determine refuses the command.
    ///
    /// With a list (`atoms` and/or `bonds`, each non-empty), exactly the named items are
    /// derived and every one must derive, with its structural reason when the model cannot hold
    /// it. A list goes through the same de-duplication as `move_atoms`: repeats, also two
    /// references that resolve to one atom (`"a1"` and `"$0"`), are one item, and items are
    /// processed centers first and then double bonds, each by ascending identifier, so a failing
    /// item is named by its lowest identifier, not by its position in the list.
    ///
    /// `instance` follows `move_atoms`: it may be left out only while the document has one
    /// scene instance. The command runs against the transaction's state at its position, so it
    /// may name atoms and bonds created earlier in the same transaction. Storing what is already
    /// stored changes nothing.
    SetConfigurationFromDrawing {
        atoms: Option<Vec<AtomRef>>,
        bonds: Option<Vec<BondRef>>,
        instance: Option<ObjectId>,
    },
    /// Creates an enhanced-stereo group (see [`crate::stereo_group`]) of stored tetrahedral
    /// centers that are in no group. `number` is stored as stated (`0` and negatives too); an
    /// absolute group takes none.
    CreateStereoGroup {
        kind: GroupKind,
        number: Option<i32>,
        atoms: Vec<AtomRef>,
    },
    /// Deletes a group; its members stay configured and become ungrouped.
    DeleteStereoGroup {
        group: GroupRef,
    },
    SetStereoGroupKind {
        group: GroupRef,
        kind: GroupKind,
    },
    /// Sets or, with `None`, clears the stored number. A number another group of the kind
    /// already has is refused: merge the groups instead.
    SetStereoGroupNumber {
        group: GroupRef,
        number: Option<i32>,
    },
    /// Adds centers that are in no group; centers already in this group are left as they are.
    AddStereoGroupMembers {
        group: GroupRef,
        atoms: Vec<AtomRef>,
    },
    /// Removes members, which become ungrouped; it never empties the group (delete it instead).
    RemoveStereoGroupMembers {
        group: GroupRef,
        atoms: Vec<AtomRef>,
    },
    /// Moves every member of `from` into `into` and deletes `from`; `into` keeps its kind and
    /// number. The groups must be of one kind.
    MergeStereoGroups {
        into: GroupRef,
        from: GroupRef,
    },
    /// Moves some, not all, members of an `and` or `or` group into a new group of the same
    /// kind with `number`. The new group is `$N` for later commands.
    SplitStereoGroup {
        group: GroupRef,
        atoms: Vec<AtomRef>,
        number: Option<i32>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum EditError {
    CaptionInvalid,
    CaptionUnsupported(ObjectId),
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
    /// Ring attachment needs a finite nonzero direction.
    RingDirectionInvalid,
    /// Kekulé is defined only for the independent six-carbon recipe.
    RingPatternInvalid,
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
    /// A command that reads one scene instance's positions (`move_atoms`,
    /// `set_configuration_from_drawing`) did not name an instance while the document has several.
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
    /// Too few independent outermost units for the requested arrangement.
    ArrangementTooFewUnits {
        required: usize,
        found: usize,
    },
    /// The object has no geometry to bound; no synthetic origin is assigned.
    ArrangementEmptyGeometry(ObjectId),
    /// Fixed endpoints cannot contain all unit extents without overlapping equal gaps.
    ArrangementNegativeGap,
    /// Rounded center alignment would not settle after this single materialization.
    ArrangementCenterUnstable,
    /// A rotation angle is not finite, even for an empty selection.
    TransformInvalidAngle,
    /// This selected rectangle cannot be represented after an arbitrary-angle rotation.
    TransformRectangleUnsupported(ObjectId),
    /// A refused combination of recorded states, with its stable code.
    InvalidState(StateError),
    Settings(SettingsError),
    /// `BondRef::Created` must name an earlier command of the same transaction that added a
    /// bond: an `AddBond`, or an `AddAtom` with `bond_to`.
    UnresolvedBondReference(usize),
    /// `GroupRef::Created` must name an earlier command of the same transaction that created a
    /// stereo group.
    UnresolvedGroupReference(usize),
    /// A refused stereo mark or configuration, with its stable code.
    Stereo(StereoError),
    /// A configuration that could not be read off the drawing for this atom (a center) or bond
    /// (a double bond), with the cause and its stable code, if it has one.
    FromDrawing {
        item: StereoKey,
        cause: DeriveError,
    },
    /// A refused stereo group edit or stored group, with its stable code.
    StereoGroup(StereoGroupError),
}

impl EditError {
    /// The scene rule a command violated, when the error has a scene-specific code.
    pub fn scene_error(&self) -> Option<SceneError> {
        Some(match self {
            Self::CaptionInvalid => SceneError::CaptionInvalid,
            Self::CaptionUnsupported(_) => SceneError::CaptionUnsupported,
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
            Self::ArrangementTooFewUnits { .. } => SceneError::ArrangementTooFewUnits,
            Self::ArrangementEmptyGeometry(_) => SceneError::ArrangementEmptyGeometry,
            Self::ArrangementNegativeGap => SceneError::ArrangementNegativeGap,
            Self::ArrangementCenterUnstable => SceneError::ArrangementCenterUnstable,
            Self::TransformInvalidAngle => SceneError::TransformInvalidAngle,
            Self::TransformRectangleUnsupported(_) => SceneError::TransformRectangleUnsupported,
            _ => return None,
        })
    }

    /// The stable code sent as `reason` with `invalid_command`, when the refusal has one.
    pub fn reason(&self) -> Option<&'static str> {
        match self {
            Self::RingDirectionInvalid => Some("ring_direction_invalid"),
            Self::RingPatternInvalid => Some("ring_pattern_invalid"),
            Self::InvalidState(state) => Some(state.code()),
            Self::Settings(error) => Some(error.code()),
            Self::Stereo(error) => Some(error.code()),
            Self::StereoGroup(error) => Some(error.code()),
            Self::FromDrawing { cause, .. } => cause.code(),
            _ => self.scene_error().map(SceneError::code),
        }
    }
}

impl From<StateError> for EditError {
    fn from(error: StateError) -> Self {
        Self::InvalidState(error)
    }
}

impl From<SettingsError> for EditError {
    fn from(error: SettingsError) -> Self {
        Self::Settings(error)
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
            Self::RingDirectionInvalid => formatter.write_str("ring direction must be finite and nonzero"),
            Self::RingPatternInvalid => formatter.write_str("Kekule ring pattern requires six carbon atoms"),
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
                "the molecule has {instances} scene instances; name the `instance` to use"
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
            Self::ArrangementTooFewUnits { required, found } => write!(
                formatter,
                "this arrangement needs at least {required} outermost units; found {found}"
            ),
            Self::ArrangementEmptyGeometry(object) => {
                write!(formatter, "scene object {object} has no geometry to bound")
            }
            Self::ArrangementNegativeGap => formatter.write_str(
                "fixed endpoint units leave insufficient space for nonoverlapping equal gaps",
            ),
            Self::ArrangementCenterUnstable => formatter.write_str(
                "rounded center alignment would change again on repetition; no geometry was applied",
            ),
            Self::TransformInvalidAngle => formatter.write_str("rotation degrees must be finite"),
            Self::TransformRectangleUnsupported(object) => write!(
                formatter,
                "rectangle {object} requires an exact quarter-turn rotation; arbitrary angles cannot be represented by its two axis-aligned corners"
            ),
            Self::CaptionInvalid => formatter.write_str("Caption needs 1–32 nonempty single-line runs, at most 16384 UTF-8 bytes, and non-whitespace text."),
            Self::CaptionUnsupported(id) => write!(formatter, "This operation cannot preserve caption {id}; nothing was changed."),
            Self::SceneTreeInvalid(detail) => formatter.write_str(detail),
            Self::InvalidState(error) => error.fmt(formatter),
            Self::Settings(error) => error.fmt(formatter),
            Self::UnresolvedBondReference(index) => write!(
                formatter,
                "${index} does not name an earlier add_bond command in this transaction"
            ),
            Self::UnresolvedGroupReference(index) => write!(
                formatter,
                "${index} does not name an earlier command in this transaction that creates a \
                 stereo group"
            ),
            Self::Stereo(error) => error.fmt(formatter),
            Self::StereoGroup(error) => error.fmt(formatter),
            Self::FromDrawing { item, cause } => match item {
                StereoKey::Center(atom) => write!(formatter, "atom {atom}: {cause}"),
                StereoKey::DoubleBond(bond) => write!(formatter, "bond {bond}: {cause}"),
            },
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

/// The result of a transaction that was applied: its outcome, and the stereo records that
/// existed before it and are gone after it because a change invalidated them or deleted what
/// they belong to (the record of a deleted atom or bond is listed), in key order. Records a
/// `Set*` command cleared explicitly are not listed, nor are records created and removed within
/// the transaction, nor a record that was removed and set again. `group_notes` lists, in the
/// order they happened, the changes of meaning the transaction made to groups that existed
/// before it (see [`GroupNote`]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Applied {
    pub outcome: Outcome,
    pub cleared: Vec<StereoKey>,
    pub group_notes: Vec<GroupNote>,
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
/// positions), the title, style settings, and selection are recorded apart, and undo or redo
/// replays all of them as one step.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChangeSet {
    pub title: Option<Change<String>>,
    pub settings: Option<Change<Settings>>,
    pub atoms: Vec<(AtomId, Change<Option<Atom>>)>,
    pub bonds: Vec<(BondId, Change<Option<Bond>>)>,
    /// The page's root objects in order, when that changed.
    pub roots: Option<Change<Vec<ObjectId>>>,
    /// Scene objects; a group's state includes its ordered children.
    pub objects: Vec<(ObjectId, Change<Option<SceneObject>>)>,
    pub references: Vec<(SceneReference, Change<bool>)>,
    pub positions: Vec<(PositionKey, Change<Option<Point>>)>,
    /// Stated stereo configurations. Marks are part of the bond states.
    pub stereo: Vec<(StereoKey, Change<Option<StereoState>>)>,
    /// Enhanced-stereo groups, whole before and after.
    pub stereo_groups: Vec<(GroupId, Change<Option<StereoGroup>>)>,
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
            settings: (before.settings != after.settings).then(|| Change {
                before: before.settings.clone(),
                after: after.settings.clone(),
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
            stereo: diff_maps(before.stereo.records(), after.stereo.records()),
            stereo_groups: diff_maps(before.stereo_groups.groups(), after.stereo_groups.groups()),
            selection,
        }
    }

    /// Whether the content is unchanged; the selection does not count.
    pub fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.settings.is_none()
            && self.atoms.is_empty()
            && self.bonds.is_empty()
            && self.roots.is_none()
            && self.objects.is_empty()
            && self.references.is_empty()
            && self.positions.is_empty()
            && self.stereo.is_empty()
            && self.stereo_groups.is_empty()
    }

    /// Entity changes counted against [`HISTORY_CHANGE_LIMIT`].
    fn size(&self) -> usize {
        usize::from(self.title.is_some())
            + usize::from(self.settings.is_some())
            + self.atoms.len()
            + self.bonds.len()
            + usize::from(self.roots.is_some())
            + self.objects.len()
            + self.references.len()
            + self.positions.len()
            + self.stereo.len()
            + self.stereo_groups.len()
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
                .settings
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
            && entries_valid(&self.stereo)
            && entries_valid(&self.stereo_groups)
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
    settings: Settings,
    atoms: BTreeMap<AtomId, Atom>,
    bonds: BTreeMap<BondId, Bond>,
    scene: SceneTree,
    references: BTreeSet<SceneReference>,
    positions: BTreeMap<PositionKey, Point>,
    stereo: StereoTable,
    stereo_groups: StereoGroupTable,
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

    fn selection_scope(
        &self,
        selection: &Selection,
        instance: Option<ObjectId>,
    ) -> Result<SelectionScope, EditError> {
        self.check_selection(selection)?;
        // Even a scene-only or empty selection must not silently accept a bad placement.
        if let Some(instance) = instance {
            self.require_instance(instance)?;
        }
        let mut scope = SelectionScope {
            objects: self.scene.subtrees(&selection.objects),
            ..SelectionScope::default()
        };
        for object in &scope.objects {
            match self.scene.get(*object)? {
                SceneObject::Instance => {
                    scope
                        .positions
                        .extend(
                            self.instance_positions(*object)
                                .map(|(atom, _)| PositionKey {
                                    instance: *object,
                                    atom,
                                }),
                        );
                }
                SceneObject::Graphic(_) => {
                    scope.graphics.insert(*object);
                }
                SceneObject::Caption(_) | SceneObject::Group { .. } => {}
            }
        }
        let mut atoms = selection.atoms.clone();
        for id in &selection.bonds {
            let bond = self.bonds.get(id).ok_or(EditError::UnknownBond(*id))?;
            atoms.extend([bond.begin, bond.end]);
        }
        if !atoms.is_empty() {
            let instance = self.target_instance(instance)?;
            scope
                .positions
                .extend(atoms.into_iter().map(|atom| PositionKey { instance, atom }));
        }
        Ok(scope)
    }

    fn object_arrangement(
        &self,
        objects: &BTreeSet<ObjectId>,
        arrangement: ObjectArrangement,
    ) -> Result<ArrangementPlan, EditError> {
        let roots = self.scene.outermost(objects)?;
        let mut units = Vec::with_capacity(roots.len());
        for object in roots {
            let scope = self.selection_scope(
                &Selection {
                    objects: BTreeSet::from([object]),
                    ..Selection::default()
                },
                None,
            )?;
            for id in &scope.objects {
                if matches!(self.scene.get(*id)?, SceneObject::Caption(_)) {
                    return Err(EditError::CaptionUnsupported(*id));
                }
            }
            let mut points = Vec::new();
            for key in &scope.positions {
                points.push(
                    *self
                        .positions
                        .get(key)
                        .ok_or(EditError::MissingPosition(key.instance))?,
                );
            }
            for graphic in &scope.graphics {
                if let SceneObject::Graphic(graphic) = self.scene.get(*graphic)? {
                    points.extend(graphic.points);
                }
            }
            let bounds =
                Bounds::from_points(points).ok_or(EditError::ArrangementEmptyGeometry(object))?;
            units.push(ArrangementUnit {
                object,
                bounds,
                delta: Vector { dx: 0.0, dy: 0.0 },
                translation: None,
                scope,
            });
        }
        let plan = scene::plan_arrangement(units, arrangement)?;
        // A preview and the mutation share the same preflight, including bounds on each
        // actual geometry point rather than just an approximate rendered rectangle.
        for unit in &plan.units {
            for key in &unit.scope.positions {
                unit.translated(
                    *self
                        .positions
                        .get(key)
                        .ok_or(EditError::MissingPosition(key.instance))?,
                )?;
            }
            for graphic in &unit.scope.graphics {
                if let SceneObject::Graphic(graphic) = self.scene.get(*graphic)? {
                    for point in graphic.points {
                        unit.translated(point)?;
                    }
                }
            }
        }
        Ok(plan)
    }

    fn selection_transform(
        &self,
        selection: &Selection,
        instance: Option<ObjectId>,
        pivot: Point,
        transform: SelectionTransform,
    ) -> Result<TransformPlan, EditError> {
        let scope = self.selection_scope(selection, instance)?;
        for id in &scope.objects {
            if matches!(self.scene.get(*id)?, SceneObject::Caption(_)) {
                return Err(EditError::CaptionUnsupported(*id));
            }
        }
        let prepared = scene::PreparedTransform::new(transform)?;
        let mut positions = Vec::with_capacity(scope.positions.len());
        let mut graphics = Vec::with_capacity(scope.graphics.len());
        for key in &scope.positions {
            let before = *self
                .positions
                .get(key)
                .ok_or(EditError::MissingPosition(key.instance))?;
            positions.push(TransformedPosition {
                key: *key,
                before,
                after: prepared.point(before, pivot)?,
            });
        }
        for object in &scope.graphics {
            if let SceneObject::Graphic(before) = self.scene.get(*object)? {
                if before.shape == GraphicShape::Rectangle && !prepared.supports_rectangle() {
                    return Err(EditError::TransformRectangleUnsupported(*object));
                }
                let after = Graphic {
                    shape: before.shape,
                    points: [
                        prepared.point(before.points[0], pivot)?,
                        prepared.point(before.points[1], pivot)?,
                    ],
                };
                graphics.push(TransformedGraphic {
                    object: *object,
                    before: before.clone(),
                    after,
                });
            }
        }
        let drawing_stereo_may_change = positions
            .iter()
            .any(|position| position.before != position.after);
        Ok(TransformPlan {
            scope,
            positions,
            graphics,
            drawing_stereo_may_change,
        })
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
        self.settings.validate()?;
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
        self.stereo.validate(&self.atoms, &self.bonds)?;
        self.stereo_groups.validate(&self.stereo)?;
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

    /// Allocates a carbon cycle with explicit orders; callers validate the recipe and positions.
    fn insert_carbon_ring(
        &mut self,
        instance: ObjectId,
        positions: &[Point],
        pattern: RingPattern,
    ) -> Result<AtomId, EditError> {
        let mut atoms = Vec::with_capacity(positions.len());
        for point in positions {
            let atom = self.allocate_atom()?;
            self.content.atoms.insert(atom, Atom::from(Element::CARBON));
            self.content
                .positions
                .insert(PositionKey { instance, atom }, *point);
            atoms.push(atom);
        }
        for index in 0..atoms.len() {
            self.insert_bond(
                atoms[index],
                atoms[(index + 1) % atoms.len()],
                if pattern == RingPattern::Kekule && index % 2 == 0 {
                    BondOrder::Double
                } else {
                    BondOrder::Single
                },
            )?;
        }
        Ok(atoms[0])
    }

    fn insert_bond(
        &mut self,
        begin: AtomId,
        end: AtomId,
        order: BondOrder,
    ) -> Result<BondId, EditError> {
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
        let bond = Bond::new(begin, end, order);
        bond.validate()?;
        let id = BondId(self.next_bond);
        self.next_bond = self
            .next_bond
            .checked_add(1)
            .ok_or(EditError::IdentifierSpaceExhausted)?;
        self.content.bonds.insert(id, bond);
        self.discard_stereo(&BTreeSet::from([begin, end]));
        Ok(id)
    }

    /// Removes the stereo records that a change to the bonded neighbors of the `touched` atoms,
    /// or the deletion of their atom or bond, leaves meaningless and remembers them for the
    /// reply. Every such removal goes through
    /// here, the one place a cleared center is reported.
    fn discard_stereo(&mut self, touched: &BTreeSet<AtomId>) {
        let cleared =
            self.content
                .stereo
                .discard_changed(touched, &self.content.atoms, &self.content.bonds);
        self.cascaded.extend(cleared);
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

    fn translate_scope(&mut self, scope: &SelectionScope, delta: Vector) -> Result<(), EditError> {
        if !delta.dx.is_finite() || !delta.dy.is_finite() {
            return Err(EditError::CoordinateOutOfRange);
        }
        for id in &scope.objects {
            if let Some(SceneObject::Caption(caption)) = self.content.scene.objects.get_mut(id) {
                caption.anchor = caption.anchor.translated(delta)?;
            }
        }
        for key in &scope.positions {
            self.translate(key.instance, [key.atom], delta)?;
        }
        for id in &scope.graphics {
            self.content.scene.require(*id, ObjectKind::Graphic)?;
            if let Some(SceneObject::Graphic(graphic)) = self.content.scene.objects.get_mut(id) {
                *graphic = graphic.translated(delta)?;
            }
        }
        Ok(())
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
                Some(Created::Atom(id) | Created::AtomAndBond(id, _)) => Ok(*id),
                _ => Err(EditError::UnresolvedReference(index)),
            },
        }
    }

    fn resolve_group(&self, group: GroupRef) -> Result<GroupId, EditError> {
        match group {
            GroupRef::Id(id) => Ok(id),
            GroupRef::Created(index) => match self.created.get(index) {
                Some(Created::Group(id)) => Ok(*id),
                _ => Err(EditError::UnresolvedGroupReference(index)),
            },
        }
    }

    /// The atoms a group command names, as a set; at least one.
    fn resolve_group_atoms(&self, atoms: &[AtomRef]) -> Result<BTreeSet<AtomId>, EditError> {
        if atoms.is_empty() {
            return Err(StereoGroupError::Empty.into());
        }
        atoms.iter().map(|atom| self.resolve(*atom)).collect()
    }

    fn allocate_group(&mut self) -> Result<GroupId, EditError> {
        let id = GroupId(self.next_group);
        self.next_group = self
            .next_group
            .checked_add(1)
            .ok_or(EditError::IdentifierSpaceExhausted)?;
        Ok(id)
    }

    /// Applies one stereo group command. Members are checked against the stored configurations
    /// as they are at this point of the transaction.
    fn apply_group_command(&mut self, command: &Command) -> Result<Created, EditError> {
        let mut created = Created::Nothing;
        match command {
            Command::CreateStereoGroup {
                kind,
                number,
                atoms,
            } => {
                let members = self.resolve_group_atoms(atoms)?;
                let id = self.allocate_group()?;
                let group = StereoGroup::new(*kind, *number, members);
                self.content
                    .stereo_groups
                    .create(id, group, &self.content.stereo)?;
                created = Created::Group(id);
            }
            Command::DeleteStereoGroup { group } => {
                let id = self.resolve_group(*group)?;
                self.content.stereo_groups.delete(id)?;
            }
            Command::SetStereoGroupKind { group, kind } => {
                let id = self.resolve_group(*group)?;
                self.content.stereo_groups.set_kind(id, *kind)?;
            }
            Command::SetStereoGroupNumber { group, number } => {
                let id = self.resolve_group(*group)?;
                self.content.stereo_groups.set_number(id, *number)?;
            }
            Command::AddStereoGroupMembers { group, atoms } => {
                let id = self.resolve_group(*group)?;
                let members = self.resolve_group_atoms(atoms)?;
                self.content
                    .stereo_groups
                    .add_members(id, &members, &self.content.stereo)?;
            }
            Command::RemoveStereoGroupMembers { group, atoms } => {
                let id = self.resolve_group(*group)?;
                let members = self.resolve_group_atoms(atoms)?;
                self.content.stereo_groups.remove_members(id, &members)?;
            }
            Command::MergeStereoGroups { into, from } => {
                let (into, from) = (self.resolve_group(*into)?, self.resolve_group(*from)?);
                self.content.stereo_groups.merge(into, from)?;
            }
            Command::SplitStereoGroup {
                group,
                atoms,
                number,
            } => {
                let id = self.resolve_group(*group)?;
                let members = self.resolve_group_atoms(atoms)?;
                let new_group = self.allocate_group()?;
                self.content
                    .stereo_groups
                    .split(id, new_group, &members, *number)?;
                created = Created::Group(new_group);
            }
            _ => unreachable!("only stereo group commands are routed here"),
        }
        Ok(created)
    }

    fn resolve_bond(&self, bond: BondRef) -> Result<BondId, EditError> {
        match bond {
            BondRef::Id(id) => Ok(id),
            BondRef::Created(index) => match self.created.get(index) {
                Some(Created::Bond(id) | Created::AtomAndBond(_, id)) => Ok(*id),
                _ => Err(EditError::UnresolvedBondReference(index)),
            },
        }
    }

    /// The configurations the drawing of `instance` shows for the items a
    /// [`Command::SetConfigurationFromDrawing`] names (see there), stored only when every item
    /// derived, so a refusal leaves the table as it was.
    fn derive_from_drawing(
        &mut self,
        atoms: Option<&[AtomRef]>,
        bonds: Option<&[BondRef]>,
        instance: Option<ObjectId>,
    ) -> Result<(), EditError> {
        let instance = self.content.target_instance(instance)?;
        let atoms = atoms
            .map(|refs| {
                let ids = refs
                    .iter()
                    .map(|atom| self.resolve(*atom))
                    .collect::<Result<Vec<_>, _>>()?;
                let ids = unique(&ids)?;
                ids.iter()
                    .try_for_each(|&id| self.content.require_atom(id))?;
                Ok::<_, EditError>(ids)
            })
            .transpose()?;
        let bonds = bonds
            .map(|refs| {
                let ids = refs
                    .iter()
                    .map(|bond| self.resolve_bond(*bond))
                    .collect::<Result<Vec<_>, _>>()?;
                let ids = unique(&ids)?;
                match ids.iter().find(|id| !self.content.bonds.contains_key(id)) {
                    Some(&missing) => Err(EditError::UnknownBond(missing)),
                    None => Ok(ids),
                }
            })
            .transpose()?;

        let drawing = Drawing::new(
            &self.content.atoms,
            &self.content.bonds,
            self.content.instance_positions(instance),
        );
        let listed = atoms.is_some() || bonds.is_some();
        let centers: Vec<AtomId> = match atoms {
            Some(named) => named.into_iter().collect(),
            None if listed => Vec::new(),
            None => drawing.marked_centers(),
        };
        let double_bonds: Vec<BondId> = match bonds {
            Some(named) => named.into_iter().collect(),
            None if listed => Vec::new(),
            None => drawing.crossed_double_bonds(),
        };
        // Without a list, any structurally impossible item (in this scope only a crossed double
        // bond with `GeometryUnsupported` or `ReferenceInvalid`) is left out without a note;
        // with one, the caller asked for it, so it is refused with its structural reason.
        let skippable = |cause: &DeriveError| !listed && matches!(cause, DeriveError::Structure(_));
        let mut derived = Vec::with_capacity(centers.len() + double_bonds.len());
        for atom in centers {
            match drawing.derive_center(atom) {
                Ok(state) => derived.push((StereoKey::Center(atom), StereoState::Center(state))),
                Err(cause) if skippable(&cause) => {}
                Err(cause) => {
                    return Err(EditError::FromDrawing {
                        item: StereoKey::Center(atom),
                        cause,
                    });
                }
            }
        }
        for bond in double_bonds {
            match drawing.derive_double_bond(bond) {
                Ok(state) => {
                    derived.push((StereoKey::DoubleBond(bond), StereoState::DoubleBond(state)))
                }
                Err(cause) if skippable(&cause) => {}
                Err(cause) => {
                    return Err(EditError::FromDrawing {
                        item: StereoKey::DoubleBond(bond),
                        cause,
                    });
                }
            }
        }
        drop(drawing);
        for (key, state) in derived {
            self.content.stereo.set(key, Some(state));
        }
        Ok(())
    }

    fn resolve_slot(&self, slot: SlotRef) -> Result<Slot, EditError> {
        Ok(match slot {
            SlotRef::Atom(atom) => {
                let id = self.resolve(atom)?;
                self.content.require_atom(id)?;
                Slot::Atom(id)
            }
            SlotRef::ImplicitHydrogen => Slot::ImplicitHydrogen,
            SlotRef::LonePair => Slot::LonePair,
        })
    }

    fn resolve_slots(&self, slots: [SlotRef; 4]) -> Result<[Slot; 4], EditError> {
        let mut resolved = [Slot::ImplicitHydrogen; 4];
        for (target, slot) in resolved.iter_mut().zip(slots) {
            *target = self.resolve_slot(slot)?;
        }
        Ok(resolved)
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
            Command::InsertRing {
                instance,
                center,
                size,
                pattern,
            } => {
                if *pattern == RingPattern::Kekule && *size != RingSize::Six {
                    return Err(EditError::RingPatternInvalid);
                }
                self.content.require_instance(*instance)?;
                let instances = self.content.instance_count();
                if instances != 1 {
                    return Err(EditError::PositionsRequired { instances });
                }
                let length = self.content.settings.bond_length_pt;
                let positions = ring_offsets(length, *size)
                    .into_iter()
                    .map(|(x, y)| Point::new(center.x() + x, center.y() + y))
                    .collect::<Result<Vec<_>, _>>()?;
                self.insert_carbon_ring(*instance, &positions, *pattern)?;
            }
            Command::AttachRing {
                instance,
                atom,
                direction,
                size,
            } => {
                self.content.require_instance(*instance)?;
                let instances = self.content.instance_count();
                if instances != 1 {
                    return Err(EditError::PositionsRequired { instances });
                }
                self.content.require_atom(*atom)?;
                let scale = direction.dx.abs().max(direction.dy.abs());
                if !direction.dx.is_finite() || !direction.dy.is_finite() || scale == 0.0 {
                    return Err(EditError::RingDirectionInvalid);
                }
                // Scale before hypot so even huge and subnormal finite directions normalize.
                let dx = direction.dx / scale;
                let dy = direction.dy / scale;
                let norm = dx.hypot(dy);
                let (ux, uy) = (dx / norm, dy / norm);
                let origin = self.content.positions[&PositionKey {
                    instance: *instance,
                    atom: *atom,
                }];
                let length = self.content.settings.bond_length_pt;
                let offsets = ring_offsets(length, *size);
                let radius = offsets[0].0;
                let positions = offsets
                    .into_iter()
                    .map(|(x, y)| {
                        let axial = length + (radius - x);
                        Point::new(
                            origin.x() + axial * ux + y * uy,
                            origin.y() + axial * uy - y * ux,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let first = self.insert_carbon_ring(*instance, &positions, RingPattern::Single)?;
                self.insert_bond(*atom, first, BondOrder::Single)?;
            }
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
                    created = Created::AtomAndBond(id, self.insert_bond(atom, id, order)?);
                }
            }
            Command::AddBond { begin, end, order } => {
                let (begin, end) = (self.resolve(*begin)?, self.resolve(*end)?);
                created = Created::Bond(self.insert_bond(begin, end, order.clone())?);
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
                if matches!(updated.kind, AtomKind::Query(_))
                    && self.content.stereo.get(StereoKey::Center(*atom)).is_some()
                {
                    return Err(StereoError::CenterUnsupported.into());
                }
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
                // A stored configuration must stay valid; the order change that would break it
                // is refused with the rule it breaks rather than erasing the configuration.
                self.content
                    .stereo
                    .validate(&self.content.atoms, &self.content.bonds)?;
            }
            Command::DeleteAtoms { atoms } => {
                let targets = unique(atoms)?;
                for id in &targets {
                    self.content.require_atom(*id)?;
                }
                let mut touched = targets.clone();
                for bond in self.content.bonds.values() {
                    if targets.contains(&bond.begin) || targets.contains(&bond.end) {
                        touched.extend([bond.begin, bond.end]);
                    }
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
                self.discard_stereo(&touched);
                self.content.refuse_dangling_references()?;
            }
            Command::DeleteBonds { bonds } => {
                let mut touched = BTreeSet::new();
                for id in unique(bonds)? {
                    let removed = self
                        .content
                        .bonds
                        .remove(&id)
                        .ok_or(EditError::UnknownBond(id))?;
                    touched.extend([removed.begin, removed.end]);
                }
                self.discard_stereo(&touched);
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
            Command::AddCaption {
                caption,
                parent,
                index,
            } => {
                caption.validate()?;
                let id = self.allocate_object()?;
                self.content.scene.insert(
                    id,
                    SceneObject::Caption(caption.clone()),
                    *parent,
                    *index,
                )?;
                created = Created::Object(id);
            }
            Command::UpdateCaption { object, caption } => {
                caption.validate()?;
                self.content.scene.require(*object, ObjectKind::Caption)?;
                self.content
                    .scene
                    .objects
                    .insert(*object, SceneObject::Caption(caption.clone()));
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
            Command::MoveSelection {
                selection,
                instance,
                delta,
            } => {
                let scope = self.content.selection_scope(selection, *instance)?;
                self.translate_scope(&scope, *delta)?;
            }
            Command::ArrangeObjects {
                objects,
                arrangement,
            } => {
                let objects: BTreeSet<ObjectId> = objects.iter().copied().collect();
                let plan = self.content.object_arrangement(&objects, *arrangement)?;
                for unit in plan.units {
                    for key in &unit.scope.positions {
                        let point = self
                            .content
                            .positions
                            .get_mut(key)
                            .ok_or(EditError::MissingPosition(key.instance))?;
                        *point = unit.translated(*point)?;
                    }
                    for id in &unit.scope.graphics {
                        if let Some(SceneObject::Graphic(graphic)) =
                            self.content.scene.objects.get_mut(id)
                        {
                            for point in &mut graphic.points {
                                *point = unit.translated(*point)?;
                            }
                        }
                    }
                }
            }
            Command::TransformSelection {
                selection,
                instance,
                pivot,
                transform,
            } => {
                let plan = self
                    .content
                    .selection_transform(selection, *instance, *pivot, *transform)?;
                for position in plan.positions {
                    self.content.positions.insert(position.key, position.after);
                }
                for graphic in plan.graphics {
                    self.content
                        .scene
                        .objects
                        .insert(graphic.object, SceneObject::Graphic(graphic.after));
                }
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
            Command::UpdateSettings(edit) => {
                self.content.settings = self.content.settings.edited(edit)?;
            }
            Command::SetBondMark { bond, mark } => {
                let id = self.resolve_bond(*bond)?;
                let target = self
                    .content
                    .bonds
                    .get_mut(&id)
                    .ok_or(EditError::UnknownBond(id))?;
                let updated = Bond {
                    mark: *mark,
                    ..target.clone()
                };
                updated.validate()?;
                *target = updated;
            }
            Command::SetAtomConfiguration {
                atom,
                configuration,
            } => {
                let atom = self.resolve(*atom)?;
                self.content.require_atom(atom)?;
                let state = match configuration {
                    None => None,
                    Some(configuration) => {
                        let state = match configuration {
                            AtomConfiguration::Tetrahedral { neighbors, turn } => {
                                CenterState::tetrahedral(self.resolve_slots(*neighbors)?, *turn)?
                            }
                            AtomConfiguration::Unknown { neighbors } => {
                                CenterState::unknown(self.resolve_slots(*neighbors)?)?
                            }
                        };
                        let graph = Graph::new(&self.content.atoms, &self.content.bonds);
                        check_center(atom, &state, &graph)?;
                        Some(StereoState::Center(state))
                    }
                };
                self.content.stereo.set(StereoKey::Center(atom), state);
            }
            Command::SetBondConfiguration {
                bond,
                configuration,
            } => {
                let id = self.resolve_bond(*bond)?;
                if !self.content.bonds.contains_key(&id) {
                    return Err(EditError::UnknownBond(id));
                }
                let state = match configuration {
                    None => None,
                    Some(configuration) => {
                        let graph = Graph::new(&self.content.atoms, &self.content.bonds);
                        let state = match configuration {
                            BondConfiguration::Relative {
                                begin_neighbor,
                                end_neighbor,
                                relation,
                            } => relative_double_bond(
                                id,
                                self.resolve_slot(*begin_neighbor)?,
                                self.resolve_slot(*end_neighbor)?,
                                *relation,
                                &graph,
                            )?,
                            BondConfiguration::Unknown => DoubleBondState::Unknown,
                        };
                        check_double_bond(id, &state, &graph)?;
                        Some(StereoState::DoubleBond(state))
                    }
                };
                self.content.stereo.set(StereoKey::DoubleBond(id), state);
            }
            Command::SetConfigurationFromDrawing {
                atoms,
                bonds,
                instance,
            } => self.derive_from_drawing(atoms.as_deref(), bonds.as_deref(), *instance)?,
            Command::CreateStereoGroup { .. }
            | Command::DeleteStereoGroup { .. }
            | Command::SetStereoGroupKind { .. }
            | Command::SetStereoGroupNumber { .. }
            | Command::AddStereoGroupMembers { .. }
            | Command::RemoveStereoGroupMembers { .. }
            | Command::MergeStereoGroups { .. }
            | Command::SplitStereoGroup { .. } => created = self.apply_group_command(command)?,
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
    pub next_group: u32,
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
            next_group: self.next_group.max(other.next_group),
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

impl Document {
    /// Creates an empty document at revision 0 with one scene instance, `s1`, and nothing
    /// selected. Surrounding whitespace is trimmed from the title.
    pub fn new(id: DocumentId, title: &str) -> Result<Self, EditError> {
        Self::new_with_settings(id, title, Settings::default())
    }

    /// Creates a document with its own initial settings, without adding a history entry.
    pub fn new_with_settings(
        id: DocumentId,
        title: &str,
        settings: Settings,
    ) -> Result<Self, EditError> {
        let title = validated_title(title)?;
        settings.validate()?;
        Ok(Self {
            id,
            revision: 0,
            content: Content {
                title: title.to_owned(),
                settings,
                atoms: BTreeMap::new(),
                bonds: BTreeMap::new(),
                scene: SceneTree::with_instance(ObjectId(1)),
                references: BTreeSet::new(),
                positions: BTreeMap::new(),
                stereo: StereoTable::default(),
                stereo_groups: StereoGroupTable::default(),
            },
            selection: Selection::default(),
            next_atom: 1,
            next_bond: 1,
            next_object: 2,
            next_group: 1,
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
            next_group: self.next_group,
            undo: VecDeque::new(),
            redo: Vec::new(),
        })
    }

    /// Rebuilds a stored document, rejecting any state commands could not have produced,
    /// including history entries that do not replay cleanly from the current state, and a
    /// selection that is not where the history ends: the newest undo entry's `after` (left by
    /// an edit or a redo) or the nearest redo entry's `before` (left by an undo), compared as
    /// sets. With no history, any selection of what exists is accepted.
    pub fn restore(mut parts: DocumentParts) -> Result<Self, RestoreError> {
        let invalid = |reason: &str| RestoreError(reason.to_owned());
        read_legacy_references(&mut parts);
        if parts.revision > MAX_REVISION {
            return Err(invalid("revision exceeds the largest revision"));
        }
        let counts = (
            parts.atoms.len(),
            parts.bonds.len(),
            parts.objects.len(),
            parts.references.len(),
            parts.positions.len(),
            parts.stereo.len(),
            parts.stereo_groups.len(),
        );
        let content = Content {
            title: parts.title,
            settings: parts.settings,
            atoms: parts.atoms.into_iter().collect(),
            bonds: parts.bonds.into_iter().collect(),
            scene: SceneTree {
                roots: parts.roots,
                objects: parts.objects.into_iter().collect(),
            },
            references: parts.references.into_iter().collect(),
            positions: parts.positions.into_iter().collect(),
            stereo: StereoTable::from_records(parts.stereo.into_iter().collect()),
            stereo_groups: StereoGroupTable::from_groups(parts.stereo_groups.into_iter().collect()),
        };
        if counts
            != (
                content.atoms.len(),
                content.bonds.len(),
                content.scene.objects.len(),
                content.references.len(),
                content.positions.len(),
                content.stereo.len(),
                content.stereo_groups.len(),
            )
        {
            return Err(invalid(
                "duplicate atom, bond, scene object, reference, position, stereo record, or stereo group",
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
            next_group: parts.next_group,
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
            || reuses_identifier(history().map(|set| set.stereo_groups.as_slice()))
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
        let group_ids = self
            .content
            .stereo_groups
            .iter()
            .map(|(id, _)| id)
            .chain(history().flat_map(|set| set.stereo_groups.iter().map(|(id, _)| *id)));
        if self.next_group == 0
            || group_ids
                .map(GroupId::serial)
                .any(|serial| serial >= self.next_group)
            || self.next_atom == 0
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
            settings: self.content.settings.clone(),
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
            stereo: self
                .content
                .stereo
                .iter()
                .map(|(key, state)| (key, *state))
                .collect(),
            stereo_groups: self
                .content
                .stereo_groups
                .iter()
                .map(|(id, group)| (id, group.clone()))
                .collect(),
            selection: self.selection.clone(),
            next_atom: self.next_atom,
            next_bond: self.next_bond,
            next_object: self.next_object,
            next_group: self.next_group,
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

    /// The document's own settings.
    pub fn settings(&self) -> &Settings {
        &self.content.settings
    }

    pub fn atoms(&self) -> impl ExactSizeIterator<Item = (AtomId, &Atom)> {
        self.content.atoms.iter().map(|(id, atom)| (*id, atom))
    }

    pub fn bonds(&self) -> impl ExactSizeIterator<Item = (BondId, &Bond)> {
        self.content.bonds.iter().map(|(id, bond)| (*id, bond))
    }

    /// The graph's atoms and bonds as the maps the stereo checks read.
    pub(crate) const fn graph_maps(&self) -> (&BTreeMap<AtomId, Atom>, &BTreeMap<BondId, Bond>) {
        (&self.content.atoms, &self.content.bonds)
    }

    /// The bond with this identifier, if the document has it.
    pub fn bond(&self, id: BondId) -> Option<&Bond> {
        self.content.bonds.get(&id)
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

    /// The stated stereo configurations (centers and double bonds). Marks are on [`Bond`]s.
    pub const fn stereo(&self) -> &StereoTable {
        &self.content.stereo
    }

    /// The enhanced-stereo groups (see [`crate::stereo_group`]).
    pub const fn stereo_groups(&self) -> &StereoGroupTable {
        &self.content.stereo_groups
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

    /// Resolves the exact unique geometry targets of a selection without changing it.
    /// Selected scene subtrees may span placements; explicit atoms and selected-bond
    /// endpoints use only `instance` (or the sole instance when omitted). Graph targets
    /// do not implicitly select their containing scene groups or other placements.
    ///
    /// # Errors
    /// Refuses unknown selected objects, atoms or bonds, a missing/incorrect explicit
    /// instance, or an omitted instance for graph targets when several placements exist.
    pub fn selection_scope(
        &self,
        selection: &Selection,
        instance: Option<ObjectId>,
    ) -> Result<SelectionScope, EditError> {
        self.content.selection_scope(selection, instance)
    }

    /// Plans rigid geometry-only alignment or equal-gap distribution without changing state.
    /// Selected descendants of another selected object collapse into that ancestor's unit.
    /// Alignment uses the original union bounds; distribution keeps first/last units fixed,
    /// ordered by leading bound then object ID. No labels, fonts or stroke extents are used.
    ///
    /// # Errors
    /// Refuses unknown objects, geometry-empty units, too few outermost units, negative
    /// distribution gaps or any translated point outside the document coordinate bounds.
    pub fn object_arrangement(
        &self,
        objects: &BTreeSet<ObjectId>,
        arrangement: ObjectArrangement,
    ) -> Result<ArrangementPlan, EditError> {
        self.content.object_arrangement(objects, arrangement)
    }

    /// Checks a geometry-only transform of the authoritative selected union without editing.
    /// Positive angles rotate clockwise in page coordinates; horizontal reflection mirrors
    /// about y=pivot.y, vertical reflection about x=pivot.x. Quarter turns are exact affine
    /// mappings. Arbitrary rotations are supported for atom positions, lines and arrows;
    /// any selected rectangle, including descendants and degenerate rectangles, requires
    /// an exact quarter turn. Diagonal/angled reflection is outside this bounded profile.
    /// Stored stereo remains unchanged, and may disagree with the transformed picture.
    ///
    /// # Errors
    /// Refuses invalid targets/placement, nonfinite angles, unsupported rectangle rotations
    /// or any final geometry point outside the finite document coordinate limits.
    pub fn selection_transform(
        &self,
        selection: &Selection,
        instance: Option<ObjectId>,
        pivot: Point,
        transform: SelectionTransform,
    ) -> Result<TransformPlan, EditError> {
        self.content
            .selection_transform(selection, instance, pivot, transform)
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
            next_group: self.next_group,
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
        self.next_group = self.next_group.max(reached.next_group);
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
        self.apply_reporting(commands, selection)
            .map(|applied| applied.outcome)
    }

    /// Like [`Self::apply_selecting`], and also reports the stereo records the edit removed
    /// because it changed the neighbors they were about (see [`Applied`]).
    pub fn apply_reporting(
        &mut self,
        commands: &[Command],
        selection: &TransactionSelection,
    ) -> Result<Applied, TransactionError> {
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
            next_group: self.next_group,
            created: Vec::with_capacity(commands.len()),
            cascaded: BTreeSet::new(),
        };
        for (index, command) in commands.iter().enumerate() {
            draft.apply(command).map_err(|error| TransactionError {
                index: Some(index),
                error,
            })?;
        }
        draft
            .content
            .stereo
            .validate(&draft.content.atoms, &draft.content.bonds)
            .map_err(whole)?;
        // The one place a group loses a member that stopped being a stored tetrahedral center,
        // whichever command (a configuration set or cleared, a neighbor change, a deletion)
        // caused it. Doing it once, here, means a center cleared and stated again within the
        // transaction keeps its group.
        draft.content.stereo_groups.prune(&draft.content.stereo);
        draft
            .content
            .stereo_groups
            .validate(&draft.content.stereo)
            .map_err(whole)?;
        let after = match &selection.after {
            Some(request) => draft.resolve_selection(request).map_err(whole)?,
            None => draft.content.existing(&before),
        };
        let changes = ChangeSet::between(&self.content, &draft.content, Change { before, after });
        if changes.is_empty() {
            return Ok(Applied {
                outcome: Outcome::Unchanged,
                cleared: Vec::new(),
                group_notes: Vec::new(),
            });
        }
        let cleared: Vec<StereoKey> = draft
            .cascaded
            .iter()
            .copied()
            .filter(|key| {
                self.content.stereo.get(*key).is_some() && draft.content.stereo.get(*key).is_none()
            })
            .collect();
        let group_notes = meaning_changes(
            &self.content.stereo_groups,
            &draft.content.stereo_groups,
            &draft.content.stereo,
        );
        if self.revision >= MAX_EDIT_REVISION {
            return Err(whole(EditError::RevisionLimit));
        }
        self.content = draft.content;
        self.selection.clone_from(&changes.selection.after);
        self.next_atom = draft.next_atom;
        self.next_bond = draft.next_bond;
        self.next_object = draft.next_object;
        self.next_group = draft.next_group;
        self.revision += 1;
        self.undo.push_back(changes);
        self.redo.clear();
        while self.undo.len() > HISTORY_LIMIT || history_size(&self.undo) > HISTORY_CHANGE_LIMIT {
            self.undo.pop_front();
        }
        Ok(Applied {
            outcome: Outcome::Changed,
            cleared,
            group_notes,
        })
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

/// Normalizes the stereo of stored parts before any check: the document's records and both
/// sides of every history entry. See [`read_legacy_reference`].
fn read_legacy_references(parts: &mut DocumentParts) {
    for (_, state) in &mut parts.stereo {
        read_legacy_reference(state);
    }
    for set in parts.undo.iter_mut().chain(&mut parts.redo) {
        for (_, change) in &mut set.stereo {
            for state in [&mut change.before, &mut change.after]
                .into_iter()
                .flatten()
            {
                read_legacy_reference(state);
            }
        }
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
    if let Some(change) = &set.settings {
        let (from, to) = change.sides(direction);
        if next.settings != *from {
            return Err(());
        }
        next.settings.clone_from(to);
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
    replay(next.stereo.records_mut(), &set.stereo, direction)?;
    replay(
        next.stereo_groups.groups_mut(),
        &set.stereo_groups,
        direction,
    )?;
    next.validate().map_err(|_| ())?;
    *content = next;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
