//! Double bonds: the structure test, the ranking of the two ends, the reading of a stored
//! relation as E or Z, and the oracle Rule 3 asks for the descriptor of a ligand double bond.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use crate::budget::{Interrupt, Interrupted};
use crate::document::{AtomId, BondId, BondOrder};
use crate::stereo::{DoubleBondState, Relation, Slot, StereoKey, StereoState};

use super::context::{Context, Step};
use super::label::rank_slots;
use super::{
    BondReport, Descriptor, EndReport, MAX_NESTED_SPHERES, MAX_RULE_3_NESTING, Reason, Rule, State,
    Stop, Warning,
};

/// What Rule 3 reads of a double bond.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Geometry {
    Z,
    E,
    /// Not stereogenic: the two substituents at an end tie.
    Plain,
    /// Stereogenic (or possibly so) but the document does not say which: no record, stored
    /// unknown, or an end whose own ranking is ambiguous (`maybe_plain`).
    Uncertain {
        maybe_plain: bool,
    },
}

/// Why a double bond has no ends to rank.
enum Refusal {
    /// An end has another double or triple bond: an allene axis, which Rule 3 does not read
    /// (the axis is a matter for Rules 4 and 5, which classify a tie that contains one).
    Cumulated,
    Reason(Reason),
}

impl From<Reason> for Refusal {
    fn from(reason: Reason) -> Self {
        Self::Reason(reason)
    }
}

/// One end of a double bond with the two positions its substituents occupy.
struct End {
    atom: AtomId,
    positions: [Slot; 2],
}

/// What ranking the two ends of a double bond found.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Outcome {
    /// An end has two substituents that tie (and nothing left out of the rules could part them).
    Tie,
    /// A cumulated bond: no E or Z, and for Rule 3 a bond that is not stereogenic.
    Cumulated,
    Ambiguous,
    Unsupported(Reason),
    /// Both ends have a higher substituent. Indexed by end (the bond's `begin`, then `end`);
    /// `higher` indexes `positions`.
    Distinct {
        atoms: [AtomId; 2],
        positions: [[Slot; 2]; 2],
        higher: [usize; 2],
        decided_by: [Rule; 2],
    },
}

/// The result of ranking the ends of one double bond, with what it cost.
#[derive(Clone, Debug)]
pub(super) struct Analysis {
    pub(super) outcome: Outcome,
    /// Digraph nodes of the two ends' arenas and tie classifications, not counting the
    /// descriptors they needed.
    pub(super) own: usize,
    /// The descriptors the two ends asked for directly.
    pub(super) deps: BTreeSet<BondId>,
}

impl Analysis {
    fn refused(reason: Reason) -> Self {
        Self {
            outcome: Outcome::Unsupported(reason),
            own: 0,
            deps: BTreeSet::new(),
        }
    }
}

/// The structure test of a double bond: a double bond between two element atoms, each with two
/// positions for substituents (explicit neighbors, hydrogens the valence model gives it, lone
/// pairs on an atom that can have them), and no cumulated bond.
fn ends(context: &Context<'_>, id: BondId) -> Result<[End; 2], Refusal> {
    let bond = context.bond(id).ok_or(Reason::NotACandidate)?;
    match bond.order {
        BondOrder::Double => {}
        BondOrder::Single | BondOrder::Triple => return Err(Reason::NotACandidate.into()),
        BondOrder::Aromatic => return Err(Reason::AromaticSystem.into()),
        BondOrder::Dative | BondOrder::Query(_) | BondOrder::Unknown => {
            return Err(Reason::BondKind.into());
        }
    }
    if !matches!(context.bond_step(id)?, Step::Order(2)) {
        return Err(Reason::AromaticSystem.into());
    }
    let end = |atom: AtomId, partner: AtomId| -> Result<End, Refusal> {
        let element = context.element(atom)?;
        let hydrogens = context.hydrogens(atom)?;
        let others: Vec<(AtomId, BondId)> = context
            .neighbors(atom)
            .iter()
            .copied()
            .filter(|&(neighbor, _)| neighbor != partner)
            .collect();
        for &(_, other) in &others {
            if matches!(context.bond_step(other), Ok(Step::Order(2 | 3))) {
                return Err(Refusal::Cumulated);
            }
        }
        let filled = others.len() + usize::from(hydrogens);
        let lone_pairs = 2_usize
            .checked_sub(filled)
            .ok_or(Reason::DoubleBondStructure)?;
        if lone_pairs > 0 && !matches!(element.symbol(), "N" | "O" | "S" | "P" | "Se") {
            return Err(Reason::DoubleBondStructure.into());
        }
        let mut positions: Vec<Slot> = others.iter().map(|&(n, _)| Slot::Atom(n)).collect();
        positions.extend(std::iter::repeat_n(
            Slot::ImplicitHydrogen,
            usize::from(hydrogens),
        ));
        positions.extend(std::iter::repeat_n(Slot::LonePair, lone_pairs));
        let positions: [Slot; 2] = positions
            .try_into()
            .map_err(|_| Reason::DoubleBondStructure)?;
        Ok(End { atom, positions })
    };
    Ok([end(bond.begin, bond.end)?, end(bond.end, bond.begin)?])
}

/// Whether a double bond has the structure of a unit that could carry an E/Z label.
pub(super) fn is_unit(context: &Context<'_>, id: BondId) -> bool {
    ends(context, id).is_ok()
}

/// Ranks the two substituents at each end of the double bond `id`.
///
/// # Errors
///
/// Only an interrupt; every other failure is an [`Outcome::Unsupported`].
fn analyze(
    context: &Context<'_>,
    oracle: &Oracle<'_, '_>,
    id: BondId,
    cap: usize,
    interrupt: &dyn Interrupt,
) -> Result<Analysis, Interrupted> {
    let ends = match ends(context, id) {
        Ok(ends) => ends,
        Err(Refusal::Cumulated) => {
            return Ok(Analysis {
                outcome: Outcome::Cumulated,
                own: 0,
                deps: BTreeSet::new(),
            });
        }
        Err(Refusal::Reason(reason)) => return Ok(Analysis::refused(reason)),
    };
    let mut own = 0_usize;
    let mut deps = BTreeSet::new();
    let mut found = Vec::new();
    for end in &ends {
        let ranking = rank_slots(
            context,
            oracle,
            end.atom,
            &end.positions,
            cap,
            MAX_NESTED_SPHERES,
            interrupt,
        )?;
        own = own.saturating_add(ranking.own_nodes);
        deps.extend(ranking.requested.iter().copied());
        found.push(if let Some(&reason) = ranking.unsupported.iter().min() {
            End3::Unsupported(reason)
        } else if ranking.ambiguous {
            End3::Ambiguous
        } else {
            match (ranking.results[0][1], ranking.decided_by) {
                (std::cmp::Ordering::Greater, Some(rule)) => End3::Distinct(0, rule),
                (std::cmp::Ordering::Less, Some(rule)) => End3::Distinct(1, rule),
                _ => End3::Tie,
            }
        });
    }
    let outcome = if found.contains(&End3::Tie) {
        Outcome::Tie
    } else if let Some(reason) = found.iter().filter_map(End3::unsupported).min() {
        Outcome::Unsupported(reason)
    } else if found.contains(&End3::Ambiguous) {
        Outcome::Ambiguous
    } else {
        let [End3::Distinct(a, rule_a), End3::Distinct(b, rule_b)] = found[..] else {
            return Ok(Analysis::refused(Reason::DoubleBondStructure));
        };
        Outcome::Distinct {
            atoms: [ends[0].atom, ends[1].atom],
            positions: [ends[0].positions, ends[1].positions],
            higher: [a, b],
            decided_by: [rule_a, rule_b],
        }
    };
    Ok(Analysis { outcome, own, deps })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum End3 {
    Tie,
    Ambiguous,
    Unsupported(Reason),
    Distinct(usize, Rule),
}

impl End3 {
    const fn unsupported(&self) -> Option<Reason> {
        match self {
            Self::Unsupported(reason) => Some(*reason),
            _ => None,
        }
    }
}

/// The stored record of a double bond, if any (`Err` for a record of the wrong kind).
fn stored(context: &Context<'_>, id: BondId) -> Result<Option<DoubleBondState>, Reason> {
    match context.graph.stereo().get(StereoKey::DoubleBond(id)) {
        Some(StereoState::DoubleBond(state)) => Ok(Some(*state)),
        Some(StereoState::Center(_)) => Err(Reason::DoubleBondStructure),
        None => Ok(None),
    }
}

/// `Z` or `E` for a stored relation over two ends whose higher substituents are `higher`; `Err`
/// when a stored reference is none of its end's positions. The record names a relation between
/// its references; naming the other position of an end flips it, so the relation between the
/// higher substituents is the stored one flipped once for each end whose reference is the lower.
fn orient(
    positions: &[[Slot; 2]; 2],
    higher: [usize; 2],
    begin_ref: Slot,
    end_ref: Slot,
    relation: Relation,
) -> Result<Descriptor, Reason> {
    let mut relation = relation;
    for (end, reference) in [begin_ref, end_ref].into_iter().enumerate() {
        let index = positions[end]
            .iter()
            .position(|&slot| slot == reference)
            .ok_or(Reason::DoubleBondStructure)?;
        if index != higher[end] {
            relation = relation.flipped();
        }
    }
    Ok(match relation {
        Relation::SameSide => Descriptor::Z,
        Relation::OppositeSides => Descriptor::E,
    })
}

/// The memo and the stack of the descriptors of one request.
#[derive(Default)]
struct Memory {
    memo: BTreeMap<BondId, Analysis>,
    stack: Vec<BondId>,
    /// How many requests were refused for nesting too deep; a result that saw one is not kept.
    nesting_hits: usize,
}

/// The descriptors of double bonds, computed on demand, once per request.
///
/// The descriptor of a bond is intrinsic: it is the ranking of the two substituents at each end
/// in the whole molecule, whatever digraph asks. The work is charged to every asker, not to the
/// first: see [`Oracle::charge`].
pub(super) struct Oracle<'c, 'g> {
    context: &'c Context<'g>,
    cap: usize,
    interrupt: &'c dyn Interrupt,
    memory: RefCell<Memory>,
}

impl<'c, 'g> Oracle<'c, 'g> {
    pub(super) fn new(context: &'c Context<'g>, cap: usize, interrupt: &'c dyn Interrupt) -> Self {
        Self {
            context,
            cap,
            interrupt,
            memory: RefCell::new(Memory::default()),
        }
    }

    /// The analysis of `bond`. A bond that is being computed already (its descriptor depends on
    /// itself) and a request beyond [`MAX_RULE_3_NESTING`] answer with a refusal that is not
    /// kept.
    ///
    /// # Errors
    ///
    /// An interrupt.
    pub(super) fn analysis(&self, bond: BondId) -> Result<Analysis, Interrupted> {
        {
            let mut memory = self.memory.borrow_mut();
            if let Some(found) = memory.memo.get(&bond) {
                return Ok(found.clone());
            }
            if memory.stack.contains(&bond) {
                return Ok(Analysis::refused(Reason::StereoCycle));
            }
            if memory.stack.len() >= MAX_RULE_3_NESTING {
                memory.nesting_hits += 1;
                return Ok(Analysis::refused(Reason::DigraphLimit));
            }
            memory.stack.push(bond);
        }
        let hits = self.memory.borrow().nesting_hits;
        let result = analyze(self.context, self, bond, self.cap, self.interrupt);
        let mut memory = self.memory.borrow_mut();
        memory.stack.pop();
        let analysis = result?;
        if memory.nesting_hits == hits {
            memory.memo.insert(bond, analysis.clone());
        }
        Ok(analysis)
    }

    /// What Rule 3 reads of `bond`, given its analysis: the stored relation read through the
    /// ranking.
    ///
    /// # Errors
    ///
    /// The reason the descriptor cannot be given.
    pub(super) fn geometry(&self, bond: BondId, analysis: &Analysis) -> Result<Geometry, Reason> {
        match &analysis.outcome {
            Outcome::Tie | Outcome::Cumulated => Ok(Geometry::Plain),
            Outcome::Ambiguous => Ok(Geometry::Uncertain { maybe_plain: true }),
            Outcome::Unsupported(reason) => Err(*reason),
            Outcome::Distinct {
                positions, higher, ..
            } => Ok(match stored(self.context, bond)? {
                Some(DoubleBondState::Relative {
                    begin_ref,
                    end_ref,
                    relation,
                }) => match orient(positions, *higher, begin_ref, end_ref, relation)? {
                    Descriptor::E => Geometry::E,
                    _ => Geometry::Z,
                },
                Some(DoubleBondState::Unknown) | None => Geometry::Uncertain { maybe_plain: false },
            }),
        }
    }

    /// Adds the cost of `bond` and of the descriptors it needs, each at most once per
    /// `visited`, and returns it. `missing` is what a bond with no kept analysis counts: 0 while
    /// it may still be being computed, `cap + 1` for a final charge.
    fn closure(&self, bond: BondId, visited: &mut BTreeSet<BondId>, missing: usize) -> usize {
        if !visited.insert(bond) {
            return 0;
        }
        let Some(found) = self.memory.borrow().memo.get(&bond).cloned() else {
            // A bond whose computation is on the stack is being paid for by its own asker.
            return if self.memory.borrow().stack.contains(&bond) {
                0
            } else {
                missing
            };
        };
        found.deps.iter().fold(found.own, |total, &dep| {
            total.saturating_add(self.closure(dep, visited, missing))
        })
    }

    /// The running cost of asking for `bond`, for a computation that has not finished: what has
    /// not been charged to `visited` yet.
    pub(super) fn charge(&self, bond: BondId, visited: &mut BTreeSet<BondId>) -> usize {
        self.closure(bond, visited, 0)
    }

    /// The cost of everything a finished computation asked for, `requested` being what it asked
    /// for directly. It is the same whichever asker ran first, because each descriptor's own
    /// cost and its direct needs are fixed by the bond.
    pub(super) fn closure_cost(&self, requested: &BTreeSet<BondId>) -> usize {
        self.closure_cost_from(None, requested)
    }

    /// [`Oracle::closure_cost`] for the computation of `root`, whose own cost is counted by the
    /// caller and so is not counted again when a descriptor it needed needs it back.
    pub(super) fn closure_cost_from(
        &self,
        root: Option<BondId>,
        requested: &BTreeSet<BondId>,
    ) -> usize {
        let mut visited: BTreeSet<BondId> = root.into_iter().collect();
        requested.iter().fold(0_usize, |total, &bond| {
            total.saturating_add(self.closure(bond, &mut visited, self.cap.saturating_add(1)))
        })
    }
}

/// The label of one double bond.
///
/// # Errors
///
/// Only an interrupt stops the item; every other failure is an `Unsupported` state.
pub(super) fn double_bond(
    context: &Context<'_>,
    oracle: &Oracle<'_, '_>,
    id: BondId,
    item_cap: usize,
) -> Result<BondReport, Stop> {
    let mut report = BondReport {
        bond: id,
        state: State::Unsupported(Reason::NotACandidate),
        stored: None,
        ends: None,
        warnings: Vec::new(),
        nodes: 0,
    };
    let record = match stored(context, id) {
        Ok(record) => record,
        Err(reason) => {
            report.state = State::Unsupported(reason);
            return Ok(report);
        }
    };
    report.stored = record;
    let analysis = oracle.analysis(id).map_err(Stop::Interrupted)?;
    let requested = analysis.deps.clone();
    let total = analysis
        .own
        .saturating_add(oracle.closure_cost_from(Some(id), &requested));
    report.nodes = total;
    report.state = match &analysis.outcome {
        _ if total > item_cap => State::Unsupported(Reason::DigraphLimit),
        Outcome::Unsupported(reason) => State::Unsupported(*reason),
        Outcome::Cumulated => State::Unsupported(Reason::DoubleBondStructure),
        Outcome::Ambiguous => State::Ambiguous(Reason::LigandStereoUnspecified),
        Outcome::Tie => {
            if record.is_some() {
                report.warnings.push(Warning::StoredNotStereogenic);
            }
            State::None
        }
        Outcome::Distinct {
            atoms,
            positions,
            higher,
            decided_by,
        } => {
            report.ends = Some(std::array::from_fn(|end| EndReport {
                atom: atoms[end],
                higher: positions[end][higher[end]],
                decided_by: decided_by[end],
            }));
            match record {
                None => State::Unspecified,
                Some(DoubleBondState::Unknown) => State::Unknown,
                Some(DoubleBondState::Relative {
                    begin_ref,
                    end_ref,
                    relation,
                }) => match orient(positions, *higher, begin_ref, end_ref, relation) {
                    Ok(descriptor) => State::Assigned(descriptor),
                    Err(reason) => State::Unsupported(reason),
                },
            }
        }
    };
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    const POSITIONS: [[Slot; 2]; 2] = [[Slot::ImplicitHydrogen, Slot::LonePair]; 2];
    const FIRST: Slot = Slot::ImplicitHydrogen;
    const SECOND: Slot = Slot::LonePair;

    fn read(
        higher: [usize; 2],
        refs: (Slot, Slot),
        relation: Relation,
    ) -> Result<Descriptor, Reason> {
        orient(&POSITIONS, higher, refs.0, refs.1, relation)
    }

    /// The stored relation holds between the two named references; naming the other position
    /// at one end flips it, and at both ends flips it back. Every row is written out.
    #[test]
    fn a_relation_between_references_is_flipped_once_for_each_end_naming_the_lower_position() {
        use Descriptor::{E, Z};
        use Relation::{OppositeSides as Opposite, SameSide as Same};
        // (higher at the begin end, higher at the far end, begin ref, far ref, relation, label)
        let rows = [
            (0, 0, FIRST, FIRST, Same, Z),
            (0, 0, FIRST, FIRST, Opposite, E),
            (0, 0, SECOND, FIRST, Same, E),
            (0, 0, FIRST, SECOND, Same, E),
            (0, 0, SECOND, SECOND, Same, Z),
            (0, 0, SECOND, SECOND, Opposite, E),
            (1, 0, FIRST, FIRST, Same, E),
            (1, 0, SECOND, FIRST, Same, Z),
            (1, 0, SECOND, SECOND, Same, E),
            (0, 1, FIRST, SECOND, Same, Z),
            (0, 1, FIRST, FIRST, Opposite, Z),
            (1, 1, FIRST, FIRST, Same, Z),
            (1, 1, SECOND, FIRST, Opposite, Z),
            (1, 1, SECOND, SECOND, Opposite, E),
        ];
        for (begin, far, begin_ref, far_ref, relation, expected) in rows {
            assert_eq!(
                read([begin, far], (begin_ref, far_ref), relation),
                Ok(expected),
                "higher ({begin}, {far}), references ({begin_ref:?}, {far_ref:?}), {relation:?}"
            );
        }
    }

    #[test]
    fn a_reference_that_is_no_position_of_its_end_is_refused() {
        let hydrogens = [[Slot::ImplicitHydrogen; 2]; 2];
        for refs in [(SECOND, FIRST), (FIRST, SECOND)] {
            assert_eq!(
                orient(&hydrogens, [0, 0], refs.0, refs.1, Relation::SameSide),
                Err(Reason::DoubleBondStructure)
            );
        }
    }
}
