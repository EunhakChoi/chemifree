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

use std::collections::{HashSet, VecDeque};

use chemifree_application::smiles::{SmilesCode, SmilesDoubleBond, SmilesError, Span};
use chemifree_domain::document::{Atom, BondOrder};
use chemifree_domain::stereo::Relation;

/// Rings with fewer atoms than this have no double-bond configuration here.
const MIN_RING_ATOMS: usize = 8;

/// A directional bond as written.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Mark {
    Slash,
    Backslash,
}

/// One bond as the reader decided it. `begin` is the atom written first.
pub(super) struct Linked<'a> {
    pub begin: usize,
    pub end: usize,
    pub order: &'a BondOrder,
    pub mark: Option<Mark>,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Side {
    Up,
    Down,
}

/// A marked bond beside the end of a double bond, seen from that end.
#[derive(Clone, Copy)]
struct Placed {
    bond: usize,
    neighbor: usize,
    side: Side,
    span: Span,
}

struct Topology<'a> {
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
        faults.push(if topology.cumulated(index) {
            SmilesError::at(SmilesCode::DirectionalBondCumulated, first.span)
                .because("an end of this double bond carries a second double or triple bond")
        } else {
            SmilesError::at(SmilesCode::DirectionalBondIncomplete, first.span)
                .because("the other end of the double bond has no directional bond")
        });
    }
    for (index, bond) in bonds.iter().enumerate() {
        if bond.mark.is_some() && !topology.touches_double_bond(index) {
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
    fn new(atoms: &'a [&'a Atom], aromatic: &'a [bool], bonds: &'a [Linked<'a>]) -> Self {
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
                let after = linked.begin == end;
                let side = match (linked.mark?, after) {
                    (Mark::Slash, true) | (Mark::Backslash, false) => Side::Up,
                    (Mark::Slash, false) | (Mark::Backslash, true) => Side::Down,
                };
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

    fn touches_double_bond(&self, marked: usize) -> bool {
        let bond = &self.bonds[marked];
        [bond.begin, bond.end].into_iter().any(|atom| {
            self.adjacent[atom]
                .iter()
                .any(|&(_, other)| *self.bonds[other].order == BondOrder::Double)
        })
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
        let others = self.adjacent[atom].len() - 1;
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
                    .because("an end of this double bond carries a second double or triple bond"),
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
                    .because("the double bond is in a ring of fewer than eight atoms"),
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
