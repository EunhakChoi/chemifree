//! Stereo representation: how a bond is drawn (a mark) kept apart from what is stated about a
//! stereocenter or a double bond (a configuration).
//!
//! This is representation only. It computes no CIP label, does not judge whether an atom is
//! stereogenic or a geometry is chemically plausible, and does not derive a configuration from a
//! drawing. A configuration is a statement about atom identifiers, never about list positions or
//! coordinates, so renumbering atoms in an output cannot change what it says. "Unspecified" is
//! the absence of a record; "unknown" is an explicit record.
//!
//! The mark, slot, and state enums are open to later values: other bond depictions and other
//! center geometries arrive with the formats that need them, each stating what it implies, with
//! a file format version of its own. They are deliberately not `#[non_exhaustive]`: every
//! consumer in this workspace (the file format above all) must then handle a new value, and the
//! compiler says where. There is no "unsupported" state or "unknown" mark yet; the first
//! importer that needs one adds it.
//!
//! Conventions are the public ones: the IUPAC wedge, hashed wedge, wavy, and crossed double bond
//! depictions, and the OpenSMILES neighbor-order handedness (looking from the first neighbor
//! toward the center, the other three run anticlockwise for `@` and clockwise for `@@`).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;

use crate::document::{Atom, AtomId, AtomRef, Bond, BondId, BondOrder, EditError};
use crate::states::AtomKind;

/// Which end of a bond a mark's narrow side is at. Marks never swap a bond's `begin` and
/// `end`; the narrow end is recorded in the mark.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BondEnd {
    Begin,
    End,
}

/// How one bond is drawn: a fact about the picture, not a chemical statement. A mark belongs to
/// the graph bond, so every scene instance shows it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StereoMark {
    /// Solid wedge on a single bond: the other end is toward the viewer.
    Wedge { narrow: BondEnd },
    /// Hashed wedge on a single bond: the other end is away from the viewer.
    Hash { narrow: BondEnd },
    /// Wavy single bond: the arrangement is unknown or either. It has no narrow end.
    Wavy,
    /// Crossed double bond: the cis/trans arrangement is unknown or either. It has no narrow
    /// end.
    Crossed,
}

impl StereoMark {
    /// Whether a bond of `order` may carry this mark: wedge, hash, and wavy on a single bond;
    /// crossed on a double bond.
    pub fn fits(self, order: &BondOrder) -> bool {
        match self {
            Self::Wedge { .. } | Self::Hash { .. } | Self::Wavy => *order == BondOrder::Single,
            Self::Crossed => *order == BondOrder::Double,
        }
    }
}

/// A neighbor position of a stereocenter or double-bond end. The derived order is the canonical
/// slot order: atoms ascending by identifier, then an implicit hydrogen, then a lone pair.
/// Later kinds are appended, so stored orders stay valid.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Slot {
    Atom(AtomId),
    /// A hydrogen that is not an atom of this graph.
    ImplicitHydrogen,
    /// A lone pair.
    LonePair,
}

impl Slot {
    const fn is_atom(self) -> bool {
        matches!(self, Self::Atom(_))
    }
}

/// The direction slots 1, 2, 3 run when looking from slot 0 toward the center (OpenSMILES `@`
/// is [`Turn::Anticlockwise`], `@@` is [`Turn::Clockwise`]).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Turn {
    Anticlockwise,
    Clockwise,
}

impl Turn {
    #[must_use]
    pub const fn flipped(self) -> Self {
        match self {
            Self::Anticlockwise => Self::Clockwise,
            Self::Clockwise => Self::Anticlockwise,
        }
    }
}

/// Whether sorting `items` takes an odd number of swaps, or `None` if two items are equal (the
/// order is then not a permutation). Written for any ordered item, so a codec can ask it of a
/// list with an absent slot (`Option` orders `None` first) as long as no value repeats.
pub fn is_odd_permutation<T: Ord>(items: &[T]) -> Option<bool> {
    let mut inversions = 0_usize;
    for (index, earlier) in items.iter().enumerate() {
        for later in &items[index + 1..] {
            match earlier.cmp(later) {
                std::cmp::Ordering::Equal => return None,
                std::cmp::Ordering::Greater => inversions += 1,
                std::cmp::Ordering::Less => {}
            }
        }
    }
    Some(inversions % 2 == 1)
}

/// What is stated about a tetrahedral stereocenter. Slots are always in canonical order (see
/// [`Slot`]) and `turn` is relative to that order, so equal statements compare equal. Build one
/// with [`CenterState::tetrahedral`] or [`CenterState::unknown`], which accept any order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CenterState {
    Tetrahedral {
        slots: [Slot; 4],
        turn: Turn,
    },
    /// Stated unknown or either. The slots name the neighbor set the statement is about.
    Unknown {
        slots: [Slot; 4],
    },
}

impl CenterState {
    /// A configuration given with its slots in any order; the result is in canonical order with
    /// `turn` flipped exactly when that reordering is an odd permutation.
    ///
    /// # Errors
    ///
    /// [`StereoError::CenterNeighbors`] when a slot repeats.
    pub fn tetrahedral(slots: [Slot; 4], turn: Turn) -> Result<Self, StereoError> {
        let odd = is_odd_permutation(&slots).ok_or(StereoError::CenterNeighbors)?;
        let mut sorted = slots;
        sorted.sort_unstable();
        Ok(Self::Tetrahedral {
            slots: sorted,
            turn: if odd { turn.flipped() } else { turn },
        })
    }

    /// An unknown configuration over `slots`, given in any order.
    ///
    /// # Errors
    ///
    /// [`StereoError::CenterNeighbors`] when a slot repeats.
    pub fn unknown(slots: [Slot; 4]) -> Result<Self, StereoError> {
        is_odd_permutation(&slots).ok_or(StereoError::CenterNeighbors)?;
        let mut sorted = slots;
        sorted.sort_unstable();
        Ok(Self::Unknown { slots: sorted })
    }

    /// The slots in canonical order.
    pub const fn slots(&self) -> &[Slot; 4] {
        match self {
            Self::Tetrahedral { slots, .. } | Self::Unknown { slots } => slots,
        }
    }
}

/// A geometric relation of two references across a double bond, not an E/Z label.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Relation {
    SameSide,
    OppositeSides,
}

impl Relation {
    #[must_use]
    pub const fn flipped(self) -> Self {
        match self {
            Self::SameSide => Self::OppositeSides,
            Self::OppositeSides => Self::SameSide,
        }
    }
}

/// What is stated about a double bond.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DoubleBondState {
    /// `begin_ref` is a neighbor position of the bond's `begin` atom and `end_ref` of its `end`
    /// atom, each in canonical form: the lowest-identifier explicit neighbor, or, where the
    /// atom has no other explicit neighbor, an implicit hydrogen. The two positions of an end
    /// are complementary, not interchangeable labels: naming the other one (a lone pair, or an
    /// implicit hydrogen at an end with one neighbor) stores the position above with the
    /// relation flipped.
    Relative {
        begin_ref: Slot,
        end_ref: Slot,
        relation: Relation,
    },
    /// Stated unknown or either.
    Unknown,
}

/// What a stereo record belongs to. Orders centers before double bonds.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum StereoKey {
    Center(AtomId),
    DoubleBond(BondId),
}

/// One stereo record's state. Its kind matches its [`StereoKey`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StereoState {
    Center(CenterState),
    DoubleBond(DoubleBondState),
}

/// The stated configurations of a document, in key order. A table inside a document holds only
/// records that satisfy every structural rule against the document's graph.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StereoTable {
    records: BTreeMap<StereoKey, StereoState>,
}

impl StereoTable {
    pub fn get(&self, key: StereoKey) -> Option<&StereoState> {
        self.records.get(&key)
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = (StereoKey, &StereoState)> {
        self.records.iter().map(|(key, state)| (*key, state))
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub(crate) const fn records(&self) -> &BTreeMap<StereoKey, StereoState> {
        &self.records
    }

    pub(crate) fn records_mut(&mut self) -> &mut BTreeMap<StereoKey, StereoState> {
        &mut self.records
    }

    pub(crate) fn from_records(records: BTreeMap<StereoKey, StereoState>) -> Self {
        Self { records }
    }

    /// Stores or, with `None`, removes the record of `key`. This is the explicit edit; records a
    /// topology change invalidates are removed by [`Self::discard_changed`].
    pub(crate) fn set(&mut self, key: StereoKey, state: Option<StereoState>) {
        match state {
            Some(state) => self.records.insert(key, state),
            None => self.records.remove(&key),
        };
    }

    /// Removes every record that a change to the bonded neighbors of the `touched` atoms (or the
    /// disappearance of its atom or bond) leaves without a stable meaning, and returns their keys
    /// in key order: a center whose atom is touched or gone, and a double bond that is gone or
    /// has a touched end. This is the one place a cleared stereo center is reported, so later
    /// groups of centers can drop a cleared member here.
    pub(crate) fn discard_changed(
        &mut self,
        touched: &BTreeSet<AtomId>,
        atoms: &BTreeMap<AtomId, Atom>,
        bonds: &BTreeMap<BondId, Bond>,
    ) -> Vec<StereoKey> {
        let doomed: Vec<StereoKey> = self
            .records
            .keys()
            .copied()
            .filter(|key| match key {
                StereoKey::Center(atom) => touched.contains(atom) || !atoms.contains_key(atom),
                StereoKey::DoubleBond(bond) => bonds.get(bond).is_none_or(|bond| {
                    touched.contains(&bond.begin) || touched.contains(&bond.end)
                }),
            })
            .collect();
        for key in &doomed {
            self.records.remove(key);
        }
        doomed
    }

    /// Checks every record against the graph in one pass: one adjacency map, then work linear in
    /// the records and their neighborhoods.
    pub(crate) fn validate(
        &self,
        atoms: &BTreeMap<AtomId, Atom>,
        bonds: &BTreeMap<BondId, Bond>,
    ) -> Result<(), EditError> {
        if self.records.is_empty() {
            return Ok(());
        }
        let graph = Graph::indexed(atoms, bonds);
        for (key, state) in &self.records {
            match (key, state) {
                (StereoKey::Center(atom), StereoState::Center(state)) => {
                    check_center(*atom, state, &graph)?;
                }
                (StereoKey::DoubleBond(bond), StereoState::DoubleBond(state)) => {
                    check_double_bond(*bond, state, &graph)?;
                }
                _ => return Err(StereoError::Conflict.into()),
            }
        }
        Ok(())
    }
}

/// Why a stereo mark or configuration is refused. Each has a stable code, sent as `reason` with
/// `invalid_command` once a command reaches the wire.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StereoError {
    /// A mark the bond's order does not allow, including an order change that would leave one.
    MarkBondOrder,
    /// A center that is a query atom or has fewer than 3 or more than 4 bonded neighbors.
    CenterUnsupported,
    /// A neighbor list that is not exactly the center's bonded neighbors plus, with three
    /// bonds, one implicit hydrogen or lone pair, or that repeats a slot.
    CenterNeighbors,
    /// A bond configuration on a bond that is not double.
    DoubleBondRequired,
    /// A double-bond reference that is the partner, not bonded to its end, the wrong kind of
    /// slot for that end (an implicit hydrogen or lone pair where the end has two explicit
    /// neighbors, or an atom where it has none), or on an end with three or more other
    /// neighbors.
    ReferenceInvalid,
    /// A double bond with a cumulated double or triple bond at an end.
    GeometryUnsupported,
    /// A stored record that is not in canonical form or whose kind does not match its key. An
    /// edit that would break a stored configuration reports the rule it breaks instead.
    Conflict,
}

impl StereoError {
    /// Every code, in documentation order.
    pub const ALL: [Self; 7] = [
        Self::MarkBondOrder,
        Self::CenterUnsupported,
        Self::CenterNeighbors,
        Self::DoubleBondRequired,
        Self::ReferenceInvalid,
        Self::GeometryUnsupported,
        Self::Conflict,
    ];

    /// The stable machine-readable code.
    pub const fn code(self) -> &'static str {
        match self {
            Self::MarkBondOrder => "stereo_mark_bond_order",
            Self::CenterUnsupported => "stereo_center_unsupported",
            Self::CenterNeighbors => "stereo_center_neighbors",
            Self::DoubleBondRequired => "stereo_double_bond_required",
            Self::ReferenceInvalid => "stereo_reference_invalid",
            Self::GeometryUnsupported => "stereo_geometry_unsupported",
            Self::Conflict => "stereo_conflict",
        }
    }
}

impl fmt::Display for StereoError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::MarkBondOrder => {
                "a wedge, hash, or wavy mark needs a single bond and a crossed mark a double bond"
            }
            Self::CenterUnsupported => {
                "a stereocenter is an element atom with 3 or 4 bonded neighbors"
            }
            Self::CenterNeighbors => {
                "a center's neighbors are its bonded atoms, plus one implicit hydrogen or lone \
                 pair when it has three bonds, each named once"
            }
            Self::DoubleBondRequired => "a bond configuration needs a double bond",
            Self::ReferenceInvalid => {
                "a double-bond reference is a neighbor of its own end other than the double \
                 bond's partner, on an end with at most two other neighbors, or an implicit \
                 hydrogen or lone pair where the end has at most one"
            }
            Self::GeometryUnsupported => {
                "a double bond configuration does not cover a cumulated double or triple bond"
            }
            Self::Conflict => {
                "a stored stereo record is not in canonical form or does not match its key"
            }
        })
    }
}

impl std::error::Error for StereoError {}

impl From<StereoError> for EditError {
    fn from(error: StereoError) -> Self {
        Self::Stereo(error)
    }
}

/// A neighbor position named in a command: an atom (existing or created earlier in the
/// transaction), an implicit hydrogen, or a lone pair.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SlotRef {
    Atom(AtomRef),
    ImplicitHydrogen,
    LonePair,
}

/// A center configuration a command states. Neighbors may be listed in any order; the core
/// stores the canonical order and adjusts `turn` for the reordering.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AtomConfiguration {
    Tetrahedral {
        neighbors: [SlotRef; 4],
        turn: Turn,
    },
    /// Unknown over the named neighbors, which must be the center's neighbor set.
    Unknown {
        neighbors: [SlotRef; 4],
    },
}

/// A double-bond configuration a command states.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BondConfiguration {
    /// `begin_neighbor` is a neighbor position of the bond's `begin` atom, `end_neighbor` of its
    /// `end` atom. An sp2 end has two positions: naming either one is accepted (either neighbor
    /// of an end with two, or the implicit hydrogen or lone pair opposite the single neighbor of
    /// an end with one), and the core stores the canonical one and adjusts `relation`.
    Relative {
        begin_neighbor: SlotRef,
        end_neighbor: SlotRef,
        relation: Relation,
    },
    Unknown,
}

/// The graph a configuration is checked against: atoms and bonds, and each atom's neighbors in
/// ascending identifier order. [`Graph::new`] looks neighbors up by scanning the bonds, which
/// suits checking one record; [`Graph::indexed`] builds a neighbor map once, which suits
/// checking every record.
pub(crate) struct Graph<'a> {
    atoms: &'a BTreeMap<AtomId, Atom>,
    bonds: &'a BTreeMap<BondId, Bond>,
    index: Option<HashMap<AtomId, Vec<(AtomId, BondId)>>>,
}

impl<'a> Graph<'a> {
    pub(crate) fn new(
        atoms: &'a BTreeMap<AtomId, Atom>,
        bonds: &'a BTreeMap<BondId, Bond>,
    ) -> Self {
        Self {
            atoms,
            bonds,
            index: None,
        }
    }

    fn indexed(atoms: &'a BTreeMap<AtomId, Atom>, bonds: &'a BTreeMap<BondId, Bond>) -> Self {
        let mut index: HashMap<AtomId, Vec<(AtomId, BondId)>> = HashMap::with_capacity(atoms.len());
        for (id, bond) in bonds {
            index.entry(bond.begin).or_default().push((bond.end, *id));
            index.entry(bond.end).or_default().push((bond.begin, *id));
        }
        for list in index.values_mut() {
            list.sort_unstable();
        }
        Self {
            atoms,
            bonds,
            index: Some(index),
        }
    }

    fn neighbors(&self, atom: AtomId) -> Vec<(AtomId, BondId)> {
        if let Some(index) = &self.index {
            return index.get(&atom).cloned().unwrap_or_default();
        }
        let mut found: Vec<(AtomId, BondId)> = self
            .bonds
            .iter()
            .filter_map(|(id, bond)| {
                if bond.begin == atom {
                    Some((bond.end, *id))
                } else if bond.end == atom {
                    Some((bond.begin, *id))
                } else {
                    None
                }
            })
            .collect();
        found.sort_unstable();
        found
    }

    /// The neighbors of `atom` other than `partner`, ascending, for one end of a double bond.
    fn other_neighbors(&self, atom: AtomId, partner: AtomId) -> Result<Vec<AtomId>, StereoError> {
        let others: Vec<(AtomId, BondId)> = self
            .neighbors(atom)
            .into_iter()
            .filter(|(neighbor, _)| *neighbor != partner)
            .collect();
        let cumulated = others.iter().any(|(_, bond)| {
            self.bonds
                .get(bond)
                .is_some_and(|bond| matches!(bond.order, BondOrder::Double | BondOrder::Triple))
        });
        if cumulated {
            return Err(StereoError::GeometryUnsupported);
        }
        if others.len() > 2 {
            return Err(StereoError::ReferenceInvalid);
        }
        Ok(others.into_iter().map(|(neighbor, _)| neighbor).collect())
    }
}

/// Checks a center record against the graph: a stereocenter has 3 or 4 bonded neighbors, and its
/// slots, in canonical order, are exactly those neighbors plus one non-atom slot when it has 3.
pub(crate) fn check_center(
    atom: AtomId,
    state: &CenterState,
    graph: &Graph<'_>,
) -> Result<(), EditError> {
    let record = graph.atoms.get(&atom).ok_or(EditError::UnknownAtom(atom))?;
    if matches!(record.kind, AtomKind::Query(_)) {
        return Err(StereoError::CenterUnsupported.into());
    }
    let neighbors = graph.neighbors(atom);
    if !(3..=4).contains(&neighbors.len()) {
        return Err(StereoError::CenterUnsupported.into());
    }
    let slots = state.slots();
    // Strictly ascending: canonical order with no repeated slot.
    if !slots.windows(2).all(|pair| pair[0] < pair[1]) {
        return Err(StereoError::CenterNeighbors.into());
    }
    let explicit = slots.iter().filter_map(|slot| match slot {
        Slot::Atom(id) => Some(*id),
        _ => None,
    });
    if !explicit.eq(neighbors.iter().map(|(id, _)| *id)) {
        return Err(StereoError::CenterNeighbors.into());
    }
    debug_assert!(
        slots.iter().filter(|slot| !slot.is_atom()).count() == 4 - neighbors.len(),
        "four strictly ascending slots with {} atoms leave {} non-atom slots",
        neighbors.len(),
        4 - neighbors.len()
    );
    Ok(())
}

/// Checks a double-bond record against the graph.
pub(crate) fn check_double_bond(
    bond: BondId,
    state: &DoubleBondState,
    graph: &Graph<'_>,
) -> Result<(), EditError> {
    let target = graph.bonds.get(&bond).ok_or(EditError::UnknownBond(bond))?;
    if target.order != BondOrder::Double {
        return Err(StereoError::DoubleBondRequired.into());
    }
    let begin_others = graph.other_neighbors(target.begin, target.end)?;
    let end_others = graph.other_neighbors(target.end, target.begin)?;
    match state {
        DoubleBondState::Unknown => Ok(()),
        DoubleBondState::Relative {
            begin_ref, end_ref, ..
        } => {
            check_reference(*begin_ref, &begin_others)?;
            check_reference(*end_ref, &end_others)?;
            Ok(())
        }
    }
}

/// `others` are the end's neighbors besides the double bond's partner, ascending. A stored
/// reference is canonical: the lowest-identifier neighbor, or, at an end with no other explicit
/// neighbor, an implicit hydrogen. A lone pair, or a non-atom slot at an end that has a
/// neighbor, names the other position of the end and is never stored.
fn check_reference(slot: Slot, others: &[AtomId]) -> Result<(), StereoError> {
    match (slot, others) {
        (Slot::ImplicitHydrogen, []) => Ok(()),
        (Slot::Atom(atom), [lowest, ..]) if atom == *lowest => Ok(()),
        (Slot::Atom(atom), _) if others.contains(&atom) => Err(StereoError::Conflict),
        (Slot::LonePair, []) | (Slot::ImplicitHydrogen | Slot::LonePair, [_]) => {
            Err(StereoError::Conflict)
        }
        _ => Err(StereoError::ReferenceInvalid),
    }
}

/// A double-bond reference as stored, and whether storing it swapped to the complementary
/// position of the end (which flips the relation).
///
/// An sp2 end has two substituent positions, so naming one fixes the other:
/// - two explicit neighbors: naming the higher-identifier one is stored as the lowest, flipped;
/// - one explicit neighbor `n`: naming an implicit hydrogen or a lone pair names the position
///   opposite `n`, stored as `n`, flipped;
/// - no explicit neighbor: the positions are an implicit hydrogen and a lone pair, and the
///   canonical one is the implicit hydrogen, so a lone pair is stored as the hydrogen, flipped.
///
/// The model records no hydrogen or lone-pair inventory, so it does not check that the atom
/// really has a lone pair; which label a position has is derived from the atom at derivation
/// time.
fn canonical_reference(slot: Slot, others: &[AtomId]) -> Result<(Slot, bool), StereoError> {
    match (slot, others) {
        (Slot::ImplicitHydrogen, []) => Ok((Slot::ImplicitHydrogen, false)),
        (Slot::LonePair, []) => Ok((Slot::ImplicitHydrogen, true)),
        (Slot::ImplicitHydrogen | Slot::LonePair, [only]) => Ok((Slot::Atom(*only), true)),
        (Slot::Atom(atom), [lowest, ..]) if others.contains(&atom) => {
            Ok((Slot::Atom(*lowest), atom != *lowest))
        }
        _ => Err(StereoError::ReferenceInvalid),
    }
}

/// Reads the one form the first version 6 build stored that is no longer canonical: a lone
/// pair as the reference of a double-bond end with no other explicit neighbor. That build
/// accepted it only at such an end, so no graph is needed to recognize it; it is rewritten as
/// the implicit hydrogen, flipping the relation, by the same rule the commands apply
/// ([`canonical_reference`]). Any other record is left as it is, for validation to accept or
/// refuse.
pub(crate) fn read_legacy_reference(state: &mut StereoState) {
    let StereoState::DoubleBond(DoubleBondState::Relative {
        begin_ref,
        end_ref,
        relation,
    }) = state
    else {
        return;
    };
    let mut swapped_ends = 0_u8;
    for reference in [begin_ref, end_ref] {
        if *reference == Slot::LonePair
            && let Ok((canonical, true)) = canonical_reference(Slot::LonePair, &[])
        {
            *reference = canonical;
            swapped_ends += 1;
        }
    }
    if swapped_ends == 1 {
        *relation = relation.flipped();
    }
}

/// The record a relative configuration of double bond `bond` stores, or the refusal. The
/// references are canonicalized; the result satisfies [`check_double_bond`].
pub(crate) fn relative_double_bond(
    bond: BondId,
    begin_neighbor: Slot,
    end_neighbor: Slot,
    relation: Relation,
    graph: &Graph<'_>,
) -> Result<DoubleBondState, EditError> {
    let target = graph.bonds.get(&bond).ok_or(EditError::UnknownBond(bond))?;
    if target.order != BondOrder::Double {
        return Err(StereoError::DoubleBondRequired.into());
    }
    let begin_others = graph.other_neighbors(target.begin, target.end)?;
    let end_others = graph.other_neighbors(target.end, target.begin)?;
    let (begin_ref, begin_swapped) = canonical_reference(begin_neighbor, &begin_others)?;
    let (end_ref, end_swapped) = canonical_reference(end_neighbor, &end_others)?;
    let relation = if begin_swapped == end_swapped {
        relation
    } else {
        relation.flipped()
    };
    Ok(DoubleBondState::Relative {
        begin_ref,
        end_ref,
        relation,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_fit_only_their_bond_orders() {
        let wedge = StereoMark::Wedge {
            narrow: BondEnd::Begin,
        };
        assert!(wedge.fits(&BondOrder::Single));
        assert!(!wedge.fits(&BondOrder::Double));
        assert!(StereoMark::Crossed.fits(&BondOrder::Double));
        assert!(!StereoMark::Crossed.fits(&BondOrder::Single));
    }

    #[test]
    fn odd_permutations_are_counted_by_swaps() {
        assert_eq!(is_odd_permutation(&[1, 2, 3, 4]), Some(false));
        assert_eq!(is_odd_permutation(&[2, 1, 3, 4]), Some(true));
        assert_eq!(is_odd_permutation(&[2, 1, 4, 3]), Some(false));
        assert_eq!(is_odd_permutation(&[2, 3, 1, 4]), Some(false));
        assert_eq!(is_odd_permutation(&[1, 1, 2, 3]), None);
        // An absent slot orders first, as a codec's zero entry would.
        assert_eq!(
            is_odd_permutation(&[Some(3), None, Some(1), Some(2)]),
            Some(true)
        );
    }

    #[test]
    fn codes_are_distinct() {
        let mut codes: Vec<&str> = StereoError::ALL.iter().map(|error| error.code()).collect();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), StereoError::ALL.len());
    }
}
