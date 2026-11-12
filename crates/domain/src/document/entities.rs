//! Position, atom, bond, reference and fragment display value types of a document.

use std::collections::BTreeMap;

use super::{
    AtomId, BondId, EditError, GroupId, MAX_CHARGE, MAX_COORDINATE, MAX_ISOTOPE_MASS_NUMBER,
    MIN_CHARGE, ObjectId,
};
use crate::element::Element;
use crate::scene;
use crate::states::{
    AtomKind, AttachmentPoints, BondQuery, Hydrogens, MAX_STATED_HYDROGENS, Radical, StateError,
};
use crate::stereo::{StereoError, StereoMark};

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
    pub(super) fn validate(&self) -> Result<(), EditError> {
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

    pub(super) fn validate(&self) -> Result<(), EditError> {
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

    pub(super) fn connects(&self, a: AtomId, b: AtomId) -> bool {
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
    /// Five carbon vertices using regular pentagon geometry.
    Five,
    /// Six carbon vertices using the original regular hexagon geometry.
    #[default]
    Six,
}

/// Explicit bond recipes; these state bond orders without aromaticity inference.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RingPattern {
    /// All five or six bonds are single; the original recipe.
    #[default]
    Single,
    /// Six bonds alternate double/single in recipe vertex order, starting with double.
    Kekule,
}

/// Plain presentation for explicit retained graph members, including the anchor.
/// Members are unique, stored in identifier order, and bounded by MAX_ATOMS.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FragmentDisplay {
    pub members: Vec<AtomId>,
    pub runs: Vec<FragmentDisplayRun>,
}

/// Explicit display-script treatment, independent of chemical interpretation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FragmentDisplayRun {
    pub text: String,
    pub script: scene::CaptionScript,
}

impl FragmentDisplay {
    /// Plain text operations reset script treatment to one baseline run.
    pub fn plain(members: Vec<AtomId>, text: String) -> Self {
        Self {
            members,
            runs: vec![FragmentDisplayRun {
                text,
                script: scene::CaptionScript::Baseline,
            }],
        }
    }

    /// Concatenated display text, never a chemical definition.
    pub fn text(&self) -> String {
        self.runs.iter().map(|run| run.text.as_str()).collect()
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
