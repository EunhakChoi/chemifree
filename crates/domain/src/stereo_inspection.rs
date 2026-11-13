//! Stored against drawn: for each stereocenter and double bond that holds a stated
//! configuration or carries a mark, what the document states, what each scene instance's drawing
//! shows ([`crate::stereo_drawing`]), and whether the two agree.
//!
//! This is a comparison and nothing else. It stores and derives nothing, never edits the
//! document and judges no chemistry: no CIP label, no check that an atom is stereogenic, no
//! ring or strain plausibility. An atom or bond that is neither stored nor marked is not an item
//! and is not assessed: its absence is not a statement that it is not stereogenic.
//!
//! # Agreement
//!
//! A stored state and a drawn state *agree* when they are the same geometry: the same kind,
//! the same neighbor set and, for a tetrahedral center, the same turn in canonical order. The
//! single non-atom position of a center with three neighbors is one position whatever its label,
//! so a stored implicit hydrogen and a drawn lone pair agree; [`AtomDrawn::label_differs`] says
//! so apart from the agreement. An unknown center agrees with an unknown center and with
//! nothing else; a definite state and an unknown one disagree, either way round. Double-bond
//! references are canonical on both sides, so equal states agree.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::budget::{Interrupt, Interrupted, NEVER};
use crate::document::{AtomId, BondId, Document, ObjectId};
use crate::stereo::{CenterState, DoubleBondState, Slot, StereoError, StereoRecord};
use crate::stereo_drawing::{DeriveError, Drawing, MisplacedMark, NotDerivable, WEAK_EVIDENCE};

/// How a stored state and a drawn state relate, for one item in one scene instance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Agreement {
    /// Stored, derivable, same geometry.
    Agrees,
    /// Stored, derivable, different geometry (a definite state against an unknown one counts).
    Disagrees,
    /// Stored, and the drawing does not give a usable state.
    OnlyStored,
    /// Not stored, and the drawing gives a state.
    OnlyDrawn,
    /// Not stored, and the drawing does not give a usable state.
    Neither,
}

impl Agreement {
    /// Every value, in documentation order.
    pub const ALL: [Self; 5] = [
        Self::Agrees,
        Self::Disagrees,
        Self::OnlyStored,
        Self::OnlyDrawn,
        Self::Neither,
    ];

    /// The stable machine-readable name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Agrees => "agrees",
            Self::Disagrees => "disagrees",
            Self::OnlyStored => "only_stored",
            Self::OnlyDrawn => "only_drawn",
            Self::Neither => "neither",
        }
    }

    fn judge<T>(
        stored: Option<&T>,
        drawn: &Result<T, Underivable>,
        agree: fn(&T, &T) -> bool,
    ) -> Self {
        match (stored, drawn) {
            (Some(stored), Ok(drawn)) if agree(stored, drawn) => Self::Agrees,
            (Some(_), Ok(_)) => Self::Disagrees,
            (Some(_), Err(_)) => Self::OnlyStored,
            (None, Ok(_)) => Self::OnlyDrawn,
            (None, Err(_)) => Self::Neither,
        }
    }
}

/// Why the drawing gives no state for an item.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Underivable {
    /// The model cannot hold a configuration here at all.
    Structure(StereoError),
    /// It can, but the drawing does not determine it.
    NotDerivable(NotDerivable),
}

impl Underivable {
    /// The stable code, the same as the refusal of `set_configuration_from_drawing`.
    pub const fn code(self) -> &'static str {
        match self {
            Self::Structure(error) => error.code(),
            Self::NotDerivable(reason) => reason.code(),
        }
    }

    /// Whether this is the plain "no mark bears on this center".
    pub const fn is_no_marks(self) -> bool {
        matches!(self, Self::NotDerivable(NotDerivable::NoMarks))
    }
}

impl TryFrom<DeriveError> for Underivable {
    type Error = DeriveError;

    /// The causes a drawing can show; an unknown atom, bond or position is a broken document,
    /// returned as it is.
    fn try_from(error: DeriveError) -> Result<Self, DeriveError> {
        match error {
            DeriveError::Structure(error) => Ok(Self::Structure(error)),
            DeriveError::NotDerivable(reason) => Ok(Self::NotDerivable(reason)),
            DeriveError::UnknownAtom(_)
            | DeriveError::UnknownBond(_)
            | DeriveError::MissingPosition(_) => Err(error),
        }
    }
}

/// Why a document could not be inspected. A document never causes either: they name a broken
/// invariant (a stored record of the wrong kind, an atom without a position in an instance).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InspectError {
    Record(StereoError),
    Drawing(DeriveError),
}

impl fmt::Display for InspectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Record(error) => error.fmt(formatter),
            Self::Drawing(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for InspectError {}

impl From<StereoError> for InspectError {
    fn from(error: StereoError) -> Self {
        Self::Record(error)
    }
}

impl From<DeriveError> for InspectError {
    fn from(error: DeriveError) -> Self {
        Self::Drawing(error)
    }
}

/// What the drawing of one scene instance shows for a center.
#[derive(Clone, Debug, PartialEq)]
pub struct AtomDrawn {
    pub instance: ObjectId,
    pub drawn: Result<CenterState, Underivable>,
    pub agreement: Agreement,
    /// Both states have one non-atom position and their labels (implicit hydrogen, lone pair)
    /// differ. The agreement ignores the label; this does not.
    pub label_differs: bool,
    /// `|D|` of a tetrahedral result: how far from flat the drawing is. `None` for an unknown
    /// center and when nothing was derived.
    pub strength: Option<f64>,
}

impl AtomDrawn {
    /// Whether a tetrahedral state was derived from a nearly flat drawing
    /// ([`WEAK_EVIDENCE`]).
    pub fn is_weak_evidence(&self) -> bool {
        self.strength
            .is_some_and(|strength| strength < WEAK_EVIDENCE)
    }
}

/// One center: its stored state, if any, and each inspected instance's drawn state.
#[derive(Clone, Debug, PartialEq)]
pub struct AtomEntry {
    pub atom: AtomId,
    /// Whether a wedge, hash or wavy bond bears on the atom ([`Drawing::marked_centers`], the
    /// test that lists a center in the [`Plan`]). It does not depend on the instance. A reason
    /// for which no state is drawn says nothing about it: a structural reason is checked
    /// before the marks are read.
    pub marked: bool,
    pub stored: Option<CenterState>,
    pub drawn: Vec<AtomDrawn>,
}

/// What the drawing of one scene instance shows for a double bond.
#[derive(Clone, Debug, PartialEq)]
pub struct BondDrawn {
    pub instance: ObjectId,
    pub drawn: Result<DoubleBondState, Underivable>,
    pub agreement: Agreement,
    /// The ends whose two substituents are drawn on the same side of the double bond. Only read
    /// for a relative result, which then follows the lowest-identifier substituent of such an
    /// end.
    pub same_side_ends: Vec<AtomId>,
}

/// One double bond: its stored state, if any, and each inspected instance's drawn state.
#[derive(Clone, Debug, PartialEq)]
pub struct BondEntry {
    pub bond: BondId,
    pub stored: Option<DoubleBondState>,
    pub drawn: Vec<BondDrawn>,
}

/// The items of an inspection: every atom with a stored center record or that a mark bears on,
/// and every bond with a stored double-bond record or a crossed mark, in ascending order. They
/// do not depend on the scene instance.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Plan {
    atoms: Vec<AtomId>,
    /// The atoms of `atoms` that a mark bears on, ascending.
    marked: Vec<AtomId>,
    bonds: Vec<BondId>,
}

impl Plan {
    /// The items of `document`. Work is linear in its atoms, bonds and records.
    ///
    /// # Errors
    ///
    /// [`InspectError::Record`] for a stored record of the wrong kind.
    pub fn of(document: &Document) -> Result<Self, InspectError> {
        let drawing = Drawing::of_document(document, document.first_instance());
        let marked = drawing.marked_centers();
        let mut atoms: BTreeSet<AtomId> = marked.iter().copied().collect();
        let mut bonds: BTreeSet<BondId> = drawing.crossed_double_bonds().into_iter().collect();
        for record in document.stereo().typed()? {
            match record {
                StereoRecord::Center { atom, .. } => atoms.insert(atom),
                StereoRecord::DoubleBond { bond, .. } => bonds.insert(bond),
            };
        }
        Ok(Self {
            atoms: atoms.into_iter().collect(),
            marked,
            bonds: bonds.into_iter().collect(),
        })
    }

    pub fn atoms(&self) -> &[AtomId] {
        &self.atoms
    }

    pub fn bonds(&self) -> &[BondId] {
        &self.bonds
    }

    /// Whether a mark bears on `atom` (see [`AtomEntry::marked`]).
    pub fn is_marked(&self, atom: AtomId) -> bool {
        self.marked.binary_search(&atom).is_ok()
    }

    /// Items in all.
    pub fn len(&self) -> usize {
        self.atoms.len() + self.bonds.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// The result of an inspection.
#[derive(Clone, Debug, PartialEq)]
pub struct Inspection {
    pub atoms: Vec<AtomEntry>,
    pub bonds: Vec<BondEntry>,
    /// Wedges and hashes whose narrow end cannot hold a center, once per bond.
    pub misplaced_marks: Vec<MisplacedMark>,
    /// Wavy bonds joining two possible centers, once per bond.
    pub wavy_between_centers: Vec<BondId>,
    /// The listed centers that are stored, or derivable in some *inspected* instance, and have
    /// an aromatic bond. It is about the items of this reply, so it depends on `instances`: a
    /// center derivable only in an instance that was left out is neither listed here nor judged,
    /// unless it is stored. Ascending.
    pub aromatic_centers: Vec<AtomId>,
}

/// Compares what `document` states with what each of `instances` draws, for the items of `plan`.
/// `instances` are scene instances of the document; the work is the items times the instances,
/// each costing its neighborhood, plus one neighbor-index build (linear in the atoms and bonds)
/// per inspected instance and one more for the findings that do not depend on an instance;
/// [`Plan::of`] builds one as well.
///
/// # Errors
///
/// [`InspectError`] for a document whose invariants are broken; no document built by edits or
/// restored from a file causes it.
pub fn inspect(
    document: &Document,
    plan: &Plan,
    instances: &[ObjectId],
) -> Result<Inspection, InspectError> {
    inspect_with(document, plan, instances, &NEVER).expect("the never-interrupt cannot interrupt")
}

/// [`inspect`] that asks `interrupt` at its checkpoints: before each instance is drawn, before
/// each center and double bond of each instance, and once before the closing findings. It
/// returns exactly what [`inspect`] returns, or `Interrupted` and nothing else, never a partial
/// inspection.
///
/// # Errors
///
/// `Interrupted` when `interrupt` stops the computation. Otherwise the inner `Result` holds the
/// [`InspectError`] that [`inspect`] returns.
pub fn inspect_with(
    document: &Document,
    plan: &Plan,
    instances: &[ObjectId],
    interrupt: &dyn Interrupt,
) -> Result<Result<Inspection, InspectError>, Interrupted> {
    match compare_items(document, plan, instances, interrupt) {
        Ok(inspection) => Ok(Ok(inspection)),
        Err(Halt::Refused(error)) => Ok(Err(error)),
        Err(Halt::Interrupted(interrupted)) => Err(interrupted),
    }
}

/// Why the comparison ends without an [`Inspection`]: the document is refused, or `interrupt`
/// stopped it. A stop stays a `Halt` until [`inspect_with`] maps it, so it is never read as a
/// refusal.
enum Halt {
    /// A broken document invariant; this does not depend on the machine.
    Refused(InspectError),
    /// The comparison was stopped before it finished; the inspection may well exist.
    Interrupted(Interrupted),
}

impl From<StereoError> for Halt {
    fn from(error: StereoError) -> Self {
        Self::Refused(error.into())
    }
}

impl From<DeriveError> for Halt {
    fn from(error: DeriveError) -> Self {
        Self::Refused(error.into())
    }
}

impl From<Interrupted> for Halt {
    fn from(interrupted: Interrupted) -> Self {
        Self::Interrupted(interrupted)
    }
}

/// The comparison behind [`inspect_with`], which asks `interrupt` at its checkpoints.
fn compare_items(
    document: &Document,
    plan: &Plan,
    instances: &[ObjectId],
    interrupt: &dyn Interrupt,
) -> Result<Inspection, Halt> {
    let mut stored_centers: BTreeMap<AtomId, CenterState> = BTreeMap::new();
    let mut stored_bonds: BTreeMap<BondId, DoubleBondState> = BTreeMap::new();
    for record in document.stereo().typed()? {
        match record {
            StereoRecord::Center { atom, state } => {
                stored_centers.insert(atom, *state);
            }
            StereoRecord::DoubleBond { bond, state } => {
                stored_bonds.insert(bond, *state);
            }
        }
    }
    let mut atoms: Vec<AtomEntry> = plan
        .atoms()
        .iter()
        .map(|&atom| AtomEntry {
            atom,
            marked: plan.is_marked(atom),
            stored: stored_centers.get(&atom).copied(),
            drawn: Vec::with_capacity(instances.len()),
        })
        .collect();
    let mut bonds: Vec<BondEntry> = plan
        .bonds()
        .iter()
        .map(|&bond| BondEntry {
            bond,
            stored: stored_bonds.get(&bond).copied(),
            drawn: Vec::with_capacity(instances.len()),
        })
        .collect();

    for &instance in instances {
        interrupt.check()?;
        let drawing = Drawing::of_document(document, instance);
        for entry in &mut atoms {
            interrupt.check()?;
            let (drawn, strength) = match drawing.derive_center_with_strength(entry.atom) {
                Ok((state, strength)) => (Ok(state), strength),
                Err(error) => (Err(Underivable::try_from(error)?), None),
            };
            entry.drawn.push(AtomDrawn {
                instance,
                agreement: Agreement::judge(entry.stored.as_ref(), &drawn, centers_agree),
                label_differs: match (&entry.stored, &drawn) {
                    (Some(stored), Ok(drawn)) => label_differs(stored, drawn),
                    _ => false,
                },
                drawn,
                strength,
            });
        }
        for entry in &mut bonds {
            interrupt.check()?;
            let drawn = match drawing.derive_double_bond(entry.bond) {
                Ok(state) => Ok(state),
                Err(error) => Err(Underivable::try_from(error)?),
            };
            let same_side_ends = if matches!(drawn, Ok(DoubleBondState::Relative { .. })) {
                same_side_ends_or_none(&drawing, entry.bond)
            } else {
                Vec::new()
            };
            entry.drawn.push(BondDrawn {
                instance,
                agreement: Agreement::judge(entry.stored.as_ref(), &drawn, |a, b| a == b),
                drawn,
                same_side_ends,
            });
        }
    }

    interrupt.check()?;
    // The graph and the marks are the same in every instance.
    let drawing = Drawing::of_document(document, document.first_instance());
    let aromatic_centers = atoms
        .iter()
        .filter(|entry| entry.stored.is_some() || entry.drawn.iter().any(|d| d.drawn.is_ok()))
        .map(|entry| entry.atom)
        .filter(|&atom| drawing.has_aromatic_bond(atom))
        .collect();
    Ok(Inspection {
        atoms,
        bonds,
        misplaced_marks: drawing.misplaced_marks(),
        wavy_between_centers: drawing.wavy_between_centers(),
        aromatic_centers,
    })
}

/// The ends of `bond` with two substituents drawn on one side ([`Drawing::same_side_ends`]), for
/// a bond whose relation was just derived. The finding is advisory and per item: if the ends
/// cannot be read (a position missing for a second substituent, which a document never causes)
/// there is no such finding for this item in this instance, and the inspection of the other
/// items and instances goes on. It is never an [`InspectError`].
fn same_side_ends_or_none(drawing: &Drawing<'_>, bond: BondId) -> Vec<AtomId> {
    drawing.same_side_ends(bond).unwrap_or_default()
}

/// The neighbor positions of `state`, with a non-atom position named the same whatever its label.
fn positions(state: &CenterState) -> [Slot; 4] {
    state.slots().map(|slot| match slot {
        Slot::Atom(_) => slot,
        Slot::ImplicitHydrogen | Slot::LonePair => Slot::ImplicitHydrogen,
    })
}

/// Whether two center states are the same geometry: see the module documentation.
pub fn centers_agree(stored: &CenterState, drawn: &CenterState) -> bool {
    if positions(stored) != positions(drawn) {
        return false;
    }
    match (stored, drawn) {
        (
            CenterState::Tetrahedral { turn: stored, .. },
            CenterState::Tetrahedral { turn: drawn, .. },
        ) => stored == drawn,
        (CenterState::Unknown { .. }, CenterState::Unknown { .. }) => true,
        (CenterState::Tetrahedral { .. }, CenterState::Unknown { .. })
        | (CenterState::Unknown { .. }, CenterState::Tetrahedral { .. }) => false,
    }
}

/// Whether both states have a non-atom position and its label differs.
pub fn label_differs(stored: &CenterState, drawn: &CenterState) -> bool {
    let label = |state: &CenterState| {
        state
            .slots()
            .iter()
            .copied()
            .find(|slot| !matches!(slot, Slot::Atom(_)))
    };
    matches!((label(stored), label(drawn)), (Some(a), Some(b)) if a != b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Atom, Bond, BondOrder, Point};
    use crate::element::Element;

    #[test]
    fn an_unreadable_second_substituent_is_no_same_side_finding_and_not_an_error() {
        // a1=a2 with a3, a4 on a1 and a5 on a2; a4, the second substituent of a1, has no
        // position. The relation reads only the lowest-identifier substituents (a3, a5), so it is
        // derived, while the same-side test needs a4 and cannot be answered.
        let id = |serial| AtomId::from_serial(serial).unwrap();
        let atoms: BTreeMap<AtomId, Atom> = (1..=5)
            .map(|serial| (id(serial), Atom::from(Element::from_symbol("C").unwrap())))
            .collect();
        let bonds: BTreeMap<BondId, Bond> = [(1, 2), (1, 3), (1, 4), (2, 5)]
            .into_iter()
            .zip(1..)
            .map(|((begin, end), serial)| {
                let order = if serial == 1 {
                    BondOrder::Double
                } else {
                    BondOrder::Single
                };
                (
                    BondId::from_serial(serial).unwrap(),
                    Bond::new(id(begin), id(end), order),
                )
            })
            .collect();
        let point = |x, y| Point::new(x, y).unwrap();
        let drawing = Drawing::new(
            &atoms,
            &bonds,
            [
                (id(1), point(30.0, 100.0)),
                (id(2), point(60.0, 100.0)),
                (id(3), point(15.0, 85.0)),
                (id(5), point(75.0, 115.0)),
            ],
        );
        let bond = BondId::from_serial(1).unwrap();
        assert!(matches!(
            drawing.derive_double_bond(bond),
            Ok(DoubleBondState::Relative { .. })
        ));
        assert_eq!(
            drawing.same_side_ends(bond),
            Err(DeriveError::MissingPosition(id(4)))
        );
        assert_eq!(same_side_ends_or_none(&drawing, bond), Vec::<AtomId>::new());
    }
}
