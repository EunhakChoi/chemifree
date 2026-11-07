//! Double-bond configurations stated by the directional bonds `/` and `\`.
//!
//! Rules are those of the OpenSMILES specification, section "Cis/Trans Configuration of Double
//! Bonds" and "Partial Stereochemistry"
//! (<https://github.com/opensmiles/OpenSMILES>, asciidoc source of
//! <https://opensmiles.github.io/opensmiles/opensmiles.html>), and of the RDKit Book (section on
//! double bond stereo, <https://rdkit.org/docs/RDKit_Book.html>) for the ring size limit.
//!
//! A mark is read relative to the atom written before it: in `A/B` the bond goes up from `A` to
//! `B`, in `A\B` down. So a neighbour written after the double-bond atom with `/` lies above it
//! and one written before it with `/` lies below it (`F/C=C/F` is trans, `C(\F)=C/F` is trans).
//! Each double bond is read from its own marked neighbours, so a single bond between two double
//! bonds (`C/C=C/C=C/C`) serves both.
//!
//! Read: a mark on a chain single bond beside each end of a C=C, C=N or N=N double bond, in a
//! chain or in a ring of at least eight atoms (the RDKit limit; the specification names none).
//! Two marks at one end must put their atoms on opposite sides. A double bond with marks at one
//! end only states nothing by itself and is refused, except when every mark it has also serves a
//! fully marked neighbouring double bond (`C/C=C/C=CC`, the specification's partial
//! stereochemistry): then it is left without a record and without a loss.
//!
//! Every other mark is refused with the span of the mark, never dropped. A mark on a ring-closure
//! digit or an aromatic bond is refused by the reader before this module runs.
//!
//! The writer (`double_bond_write`) shares this module's table of sides ([`Mark::side_from`] and
//! its inverse [`Mark::toward`]) and its judgements of what an end or a ring can state
//! ([`Topology::write_fault`]), so that it writes only what the reader reads.

use std::collections::{HashSet, VecDeque};

use chemifree_application::smiles::{SmilesCode, SmilesDoubleBond, SmilesError, Span};
use chemifree_domain::document::{Atom, BondOrder};
use chemifree_domain::states::Hydrogens;
use chemifree_domain::stereo::Relation;

/// Rings with fewer atoms than this have no double-bond configuration here.
const MIN_RING_ATOMS: usize = 8;

const CUMULATED: &str = "an end of this double bond carries a second double or triple bond";
const SMALL_RING: &str = "the double bond is in a ring of fewer than eight atoms";

/// A directional bond as written.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Mark {
    Slash,
    Backslash,
}

impl Mark {
    /// The text of the mark.
    pub(super) const fn symbol(self) -> &'static str {
        match self {
            Self::Slash => "/",
            Self::Backslash => "\\",
        }
    }

    /// The mark that reads the other way round.
    pub(super) const fn flipped(self) -> Self {
        match self {
            Self::Slash => Self::Backslash,
            Self::Backslash => Self::Slash,
        }
    }

    /// The side of the atom at the other end of the bond, seen from the atom `from`.
    /// `from_first` says whether `from` is written before the other atom: in `A/B` the bond goes
    /// up from `A` to `B`, so `B` is above `A` and `A` is below `B`.
    pub(super) const fn side_from(self, from_first: bool) -> Side {
        match (self, from_first) {
            (Self::Slash, true) | (Self::Backslash, false) => Side::Up,
            (Self::Slash, false) | (Self::Backslash, true) => Side::Down,
        }
    }

    /// The inverse of [`Mark::side_from`]: the mark that puts the other atom on `side`.
    pub(super) const fn toward(side: Side, from_first: bool) -> Self {
        match (side, from_first) {
            (Side::Up, true) | (Side::Down, false) => Self::Slash,
            (Side::Down, true) | (Side::Up, false) => Self::Backslash,
        }
    }
}

/// One bond as the reader decided it. `begin` is the atom written first.
pub(super) struct Linked<'a> {
    pub begin: usize,
    pub end: usize,
    pub order: &'a BondOrder,
    pub mark: Option<Mark>,
    pub span: Span,
}

/// Above or below the double bond, seen from one of its ends.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Side {
    Up,
    Down,
}

impl Side {
    pub(super) const fn opposite(self) -> Self {
        match self {
            Self::Up => Self::Down,
            Self::Down => Self::Up,
        }
    }
}

/// A marked bond beside the end of a double bond, seen from that end.
#[derive(Clone, Copy)]
struct Placed {
    bond: usize,
    neighbor: usize,
    side: Side,
    span: Span,
}

/// The bonds and atoms the reader (and the writer, for what it must not write) judges.
pub(super) struct Topology<'a> {
    atoms: &'a [&'a Atom],
    aromatic: &'a [bool],
    bonds: &'a [Linked<'a>],
    /// For every atom its `(neighbour, bond)` pairs.
    adjacent: Vec<Vec<(usize, usize)>>,
}

/// The configurations the marks of `bonds` state, in bond order, or the refusal that points
/// earliest in the text. `aromatic` tells which atoms were written in lower case.
pub(super) fn read(
    atoms: &[&Atom],
    aromatic: &[bool],
    bonds: &[Linked<'_>],
) -> Result<Vec<SmilesDoubleBond>, SmilesError> {
    let topology = Topology::new(atoms, aromatic, bonds);
    let mut faults = Vec::new();
    let mut records = Vec::new();
    // The marked bonds that a recorded configuration reads.
    let mut used: HashSet<usize> = HashSet::new();
    let mut partial = Vec::new();
    // Ring double bonds whose only marks sit on chains hung from the ring: those marks are
    // about something else (the ring is as unmarkable as an aromatic one), not about the bond.
    let mut ring_only: HashSet<usize> = HashSet::new();
    for (index, bond) in bonds.iter().enumerate() {
        if *bond.order != BondOrder::Double {
            continue;
        }
        let begin = topology.marks_at(bond.begin, index);
        let end = topology.marks_at(bond.end, index);
        match (begin.is_empty(), end.is_empty()) {
            (true, true) => {}
            (false, false) => match topology.configuration(index, &begin, &end) {
                Ok(record) => {
                    used.extend(begin.iter().chain(&end).map(|placed| placed.bond));
                    records.push(record);
                }
                Err(fault) => faults.push(fault),
            },
            _ => partial.push((index, [begin, end].concat())),
        }
    }
    // A one-sided double bond is silent only when a recorded neighbour reads all its marks.
    for (index, marks) in partial {
        let Some(first) = marks
            .iter()
            .filter(|placed| !used.contains(&placed.bond))
            .min_by_key(|placed| placed.span.offset)
        else {
            continue;
        };
        if topology.cumulated(index) {
            faults.push(
                SmilesError::at(SmilesCode::DirectionalBondCumulated, first.span)
                    .because(CUMULATED),
            );
        } else if topology.hangs_from_small_ring(index, &marks) {
            ring_only.insert(index);
        } else {
            faults.push(
                SmilesError::at(SmilesCode::DirectionalBondIncomplete, first.span)
                    .because("the other end of the double bond has no directional bond"),
            );
        }
    }
    for (index, bond) in bonds.iter().enumerate() {
        if bond.mark.is_some() && !topology.touches_double_bond(index, &ring_only) {
            faults.push(
                SmilesError::at(SmilesCode::DirectionalBondWithoutDoubleBond, bond.span)
                    .because("neither atom of this bond is in a double bond"),
            );
        }
    }
    match faults
        .into_iter()
        .min_by_key(|fault| fault.span.map(|span| span.offset))
    {
        Some(fault) => Err(fault),
        None => Ok(records),
    }
}

impl<'a> Topology<'a> {
    pub(super) fn new(
        atoms: &'a [&'a Atom],
        aromatic: &'a [bool],
        bonds: &'a [Linked<'a>],
    ) -> Self {
        let mut adjacent = vec![Vec::new(); atoms.len()];
        for (index, bond) in bonds.iter().enumerate() {
            adjacent[bond.begin].push((bond.end, index));
            adjacent[bond.end].push((bond.begin, index));
        }
        Self {
            atoms,
            aromatic,
            bonds,
            adjacent,
        }
    }

    /// The marked bonds at `end` other than `double`, in text order.
    fn marks_at(&self, end: usize, double: usize) -> Vec<Placed> {
        let mut marks: Vec<Placed> = self.adjacent[end]
            .iter()
            .filter(|&&(_, bond)| bond != double)
            .filter_map(|&(neighbor, bond)| {
                let linked = &self.bonds[bond];
                // The neighbour is written after `end` when `end` begins the bond.
                let side = linked.mark?.side_from(linked.begin == end);
                Some(Placed {
                    bond,
                    neighbor,
                    side,
                    span: linked.span,
                })
            })
            .collect();
        marks.sort_by_key(|placed| placed.span.offset);
        marks
    }

    /// Whether an end of `double` carries another double or a triple bond.
    fn cumulated(&self, double: usize) -> bool {
        let bond = &self.bonds[double];
        [bond.begin, bond.end].into_iter().any(|atom| {
            self.adjacent[atom].iter().any(|&(_, other)| {
                other != double
                    && matches!(
                        self.bonds[other].order,
                        BondOrder::Double | BondOrder::Triple
                    )
            })
        })
    }

    /// Whether an end of the bond `marked` is in a double bond other than those `skipped`.
    fn touches_double_bond(&self, marked: usize, skipped: &HashSet<usize>) -> bool {
        let bond = &self.bonds[marked];
        [bond.begin, bond.end].into_iter().any(|atom| {
            self.adjacent[atom].iter().any(|&(_, other)| {
                *self.bonds[other].order == BondOrder::Double && !skipped.contains(&other)
            })
        })
    }

    /// Whether `double` is in a ring of fewer than eight atoms and none of its `marks` is: its
    /// marks are on chains hung from the ring, as on `C1=CC=CC=C1/C`, written like `c1ccccc1/C`.
    fn hangs_from_small_ring(&self, double: usize, marks: &[Placed]) -> bool {
        self.in_small_ring(double) && marks.iter().all(|placed| !self.in_small_ring(placed.bond))
    }

    /// Why `atom` cannot be an end of a configured double bond, when it cannot.
    fn end_problem(&self, atom: usize) -> Option<&'static str> {
        if self.aromatic[atom]
            || self.adjacent[atom]
                .iter()
                .any(|&(_, bond)| *self.bonds[bond].order == BondOrder::Aromatic)
        {
            return Some("an aromatic atom is not a configurable end");
        }
        let entry = self.atoms[atom];
        if entry.charge != 0 {
            return Some("a charged atom is not a configurable end");
        }
        // Hydrogens stated in brackets are neighbours too; implicit ones are not counted.
        let stated = match entry.hydrogens {
            Hydrogens::Specified(count) => usize::from(count),
            Hydrogens::Inferred | Hydrogens::Unknown => 0,
        };
        let others = self.adjacent[atom].len() - 1 + stated;
        match entry.kind.element().map(|element| element.symbol()) {
            Some("C") if others <= 2 => None,
            Some("C") => Some("a carbon end has at most two other neighbours"),
            Some("N") if others <= 1 => None,
            Some("N") => Some("a neutral nitrogen end has one other neighbour and a lone pair"),
            _ => Some("only carbon and nitrogen ends are read"),
        }
    }

    /// Whether `double` lies in a ring of fewer than [`MIN_RING_ATOMS`] atoms: the shortest
    /// path between its atoms that avoids the bond itself, searched only that far.
    fn in_small_ring(&self, double: usize) -> bool {
        let bond = &self.bonds[double];
        let mut seen = vec![false; self.atoms.len()];
        seen[bond.begin] = true;
        let mut frontier = VecDeque::from([(bond.begin, 0_usize)]);
        while let Some((atom, distance)) = frontier.pop_front() {
            // The path found below has `distance + 1` bonds and closes a ring of one atom more.
            if distance + 2 >= MIN_RING_ATOMS {
                continue;
            }
            for &(next, via) in &self.adjacent[atom] {
                if via == double || seen[next] {
                    continue;
                }
                if next == bond.end {
                    return true;
                }
                seen[next] = true;
                frontier.push_back((next, distance + 1));
            }
        }
        false
    }

    /// Why a configuration of the double bond `double` cannot be written as marks: the reader
    /// would refuse the text. The judgements are the ones [`read`] makes of cumulated bonds,
    /// ends and rings; only the reading of marks (conflicts) is left to the writer's placement.
    pub(super) fn write_fault(&self, double: usize) -> Option<&'static str> {
        let bond = &self.bonds[double];
        if self.cumulated(double) {
            return Some(CUMULATED);
        }
        if let Some(problem) = [bond.begin, bond.end]
            .into_iter()
            .find_map(|atom| self.end_problem(atom))
        {
            return Some(problem);
        }
        self.in_small_ring(double).then_some(SMALL_RING)
    }

    fn configuration(
        &self,
        double: usize,
        begin: &[Placed],
        end: &[Placed],
    ) -> Result<SmilesDoubleBond, SmilesError> {
        let linked = &self.bonds[double];
        let first = begin
            .iter()
            .chain(end)
            .min_by_key(|placed| placed.span.offset)
            .expect("a configured double bond has marks");
        if self.cumulated(double) {
            return Err(
                SmilesError::at(SmilesCode::DirectionalBondCumulated, first.span)
                    .because(CUMULATED),
            );
        }
        // An end that cannot be read is judged before any conflict at either end.
        for (atom, marks) in [(linked.begin, begin), (linked.end, end)] {
            if let Some(problem) = self.end_problem(atom) {
                return Err(SmilesError::at(
                    SmilesCode::DirectionalBondEndUnsupported,
                    marks[0].span,
                )
                .because(problem));
            }
        }
        for marks in [begin, end] {
            if let [one, other] = marks
                && one.side == other.side
            {
                return Err(
                    SmilesError::at(SmilesCode::DirectionalBondConflict, other.span)
                        .because("both neighbours of this end are placed on the same side"),
                );
            }
        }
        if self.in_small_ring(double) {
            return Err(
                SmilesError::at(SmilesCode::DirectionalBondSmallRing, first.span)
                    .because(SMALL_RING),
            );
        }
        let relation = if begin[0].side == end[0].side {
            Relation::SameSide
        } else {
            Relation::OppositeSides
        };
        Ok(SmilesDoubleBond {
            bond: double,
            begin_neighbor: begin[0].neighbor,
            end_neighbor: end[0].neighbor,
            relation,
            span: first.span,
        })
    }
}

#[cfg(test)]
mod tests {
    use chemifree_domain::element::Element;

    use super::*;

    fn atom(symbol: &str) -> Atom {
        Atom::from(Element::from_symbol(symbol).expect("a known symbol"))
    }

    /// The reason `write_fault` gives for the bond `double` of a molecule given as atoms, flags
    /// for atoms written in lower case, and `(begin, end, order)` bonds.
    fn fault(
        atoms: &[Atom],
        aromatic: &[bool],
        bonds: &[(usize, usize, BondOrder)],
        double: usize,
    ) -> Option<&'static str> {
        let refs: Vec<&Atom> = atoms.iter().collect();
        let linked: Vec<Linked<'_>> = bonds
            .iter()
            .map(|(begin, end, order)| Linked {
                begin: *begin,
                end: *end,
                order,
                mark: None,
                span: Span::new(0, 0),
            })
            .collect();
        Topology::new(&refs, aromatic, &linked).write_fault(double)
    }

    fn chain(symbols: &[&str]) -> Vec<Atom> {
        symbols.iter().map(|symbol| atom(symbol)).collect()
    }

    #[test]
    fn a_plain_carbon_double_bond_has_no_fault() {
        let atoms = chain(&["F", "C", "C", "F"]);
        let bonds = [
            (0, 1, BondOrder::Single),
            (1, 2, BondOrder::Double),
            (2, 3, BondOrder::Single),
        ];
        assert_eq!(fault(&atoms, &[false; 4], &bonds, 1), None);
    }

    #[test]
    fn a_cumulated_end_is_a_fault_the_domain_never_stores() {
        let atoms = chain(&["F", "C", "C", "C", "F"]);
        let bonds = [
            (0, 1, BondOrder::Single),
            (1, 2, BondOrder::Double),
            (2, 3, BondOrder::Double),
            (3, 4, BondOrder::Single),
        ];
        assert_eq!(fault(&atoms, &[false; 5], &bonds, 1), Some(CUMULATED));
    }

    #[test]
    fn ends_the_reader_does_not_read_are_faults() {
        let single = BondOrder::Single;
        let double = BondOrder::Double;
        // An aromatic atom, a charged atom, phosphorus.
        let atoms = chain(&["F", "C", "C", "F"]);
        let bonds = [
            (0, 1, single.clone()),
            (1, 2, double.clone()),
            (2, 3, single.clone()),
        ];
        assert_eq!(
            fault(&atoms, &[false, true, false, false], &bonds, 1),
            Some("an aromatic atom is not a configurable end")
        );
        let mut charged = atoms.clone();
        charged[2].charge = 1;
        assert_eq!(
            fault(&charged, &[false; 4], &bonds, 1),
            Some("a charged atom is not a configurable end")
        );
        let phosphorus = chain(&["F", "P", "C", "F"]);
        assert_eq!(
            fault(&phosphorus, &[false; 4], &bonds, 1),
            Some("only carbon and nitrogen ends are read")
        );
        // A carbon with three other neighbours; a nitrogen with two.
        let crowded = chain(&["F", "C", "C", "F", "Cl", "Br"]);
        let more = [
            (0, 1, single.clone()),
            (1, 2, double.clone()),
            (2, 3, single.clone()),
            (2, 4, single.clone()),
            (2, 5, single.clone()),
        ];
        assert_eq!(
            fault(&crowded, &[false; 6], &more, 1),
            Some("a carbon end has at most two other neighbours")
        );
        let nitrogen = chain(&["F", "N", "C", "F", "Cl"]);
        let two = [
            (0, 1, single.clone()),
            (1, 2, double),
            (1, 4, single.clone()),
            (2, 3, single),
        ];
        assert_eq!(
            fault(&nitrogen, &[false; 5], &two, 1),
            Some("a neutral nitrogen end has one other neighbour and a lone pair")
        );
    }

    #[test]
    fn a_hydrogen_stated_in_brackets_is_a_neighbour_of_the_end() {
        let single = BondOrder::Single;
        let mut atoms = chain(&["F", "C", "C", "F", "Cl"]);
        let bonds = [
            (0, 1, single.clone()),
            (1, 2, BondOrder::Double),
            (2, 3, single.clone()),
            (1, 4, single),
        ];
        assert_eq!(fault(&atoms, &[false; 5], &bonds, 1), None);
        atoms[1].hydrogens = Hydrogens::Specified(1);
        assert_eq!(
            fault(&atoms, &[false; 5], &bonds, 1),
            Some("a carbon end has at most two other neighbours")
        );
        // An inferred count is not counted: the reader does not see those.
        atoms[1].hydrogens = Hydrogens::Inferred;
        assert_eq!(fault(&atoms, &[false; 5], &bonds, 1), None);
    }

    #[test]
    fn the_ring_boundary_is_eight_atoms_for_the_writer_as_for_the_reader() {
        for (size, expected) in [(7_usize, Some(SMALL_RING)), (8, None)] {
            let atoms: Vec<Atom> = (0..size).map(|_| atom("C")).collect();
            let mut bonds: Vec<(usize, usize, BondOrder)> = (0..size)
                .map(|i| (i, (i + 1) % size, BondOrder::Single))
                .collect();
            bonds[1].2 = BondOrder::Double;
            assert_eq!(
                fault(&atoms, &vec![false; size], &bonds, 1),
                expected,
                "ring of {size}"
            );
        }
    }

    #[test]
    fn a_mark_and_the_side_it_names_are_inverse_for_both_directions() {
        for mark in [Mark::Slash, Mark::Backslash] {
            for from_first in [true, false] {
                assert_eq!(Mark::toward(mark.side_from(from_first), from_first), mark);
            }
        }
        // `A/B`: B is above A, A is below B.
        assert_eq!(Mark::Slash.side_from(true), Side::Up);
        assert_eq!(Mark::Slash.side_from(false), Side::Down);
        assert_eq!(Mark::Backslash.side_from(true), Side::Down);
        assert_eq!(Mark::Slash.symbol(), "/");
        assert_eq!(Mark::Backslash.symbol(), "\\");
    }
}
