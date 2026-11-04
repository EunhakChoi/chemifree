//! Double bonds: the structure test, the ranking of the two ends, the reading of a stored
//! relation as E or Z, and the oracle Rule 3 asks for the descriptor of a ligand double bond.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use crate::budget::{Interrupt, Interrupted};
use crate::document::{AtomId, BondId, BondOrder};
use crate::stereo::{DoubleBondState, Relation, Slot, StereoKey, StereoState};

use super::context::{Context, Step};
use super::label::{Scope, rank_slots};
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
    /// The descriptors the comparisons of the two ends' substituents read, which is `deps`
    /// without the ones only a tie classification asked for.
    pub(super) read: BTreeSet<BondId>,
    /// What the ranking of each end looked at, for the ends that were ranked.
    pub(super) regions: Vec<Region>,
    /// How many descriptors deep the computation went, itself included: 1 for a descriptor
    /// that asked for none, otherwise 1 more than the deepest of those it asked for. It is set
    /// when the analysis is kept (see [`Oracle::analysis`]) and is what [`MAX_RULE_3_NESTING`]
    /// is applied to when the analysis is used again.
    pub(super) height: usize,
}

impl Region {
    /// Whether the ranking expanded an atom of `ends`.
    fn touches(&self, ends: Option<[AtomId; 2]>) -> bool {
        ends.is_some_and(|ends| ends.iter().any(|atom| self.expanded.contains(atom)))
    }
}

/// What the ranking of one end of a double bond looked at.
#[derive(Clone, Debug)]
pub(super) struct Region {
    /// The two substituents at this end tie.
    pub(super) tie: bool,
    /// The atoms the comparisons expanded: the ranking looked at these atoms and their
    /// neighbors.
    pub(super) expanded: BTreeSet<AtomId>,
    /// A digraph of the comparisons closes a ring.
    pub(super) closes_ring: bool,
    /// The descriptors the comparisons read.
    pub(super) read: BTreeSet<BondId>,
}

impl Analysis {
    fn refused(reason: Reason) -> Self {
        Self::without_ranking(Outcome::Unsupported(reason))
    }

    /// The ends whose tie holds seen from any digraph: the ranking that found it saw no ring
    /// closed and no atom of `ends`, the bond being labeled. A bond with such an end is not
    /// stereogenic whatever the other end is, so nothing the other end read matters.
    fn steady_ties(&self, ends: Option<[AtomId; 2]>) -> Vec<&Region> {
        self.regions
            .iter()
            .filter(|region| region.tie && !region.closes_ring && !region.touches(ends))
            .collect()
    }

    /// The descriptors whose being read the same from any digraph decides whether this
    /// analysis is: none when a steady tie read none (that tie alone makes the bond not
    /// stereogenic in every reading, the two hydrogens of a `=CH2` for one, so what another end
    /// read does not matter), otherwise those the steady ties read, or all of them when there
    /// is no steady tie.
    ///
    /// A steady tie that read something is not preferred over another that read something else:
    /// which of two is "the smaller" would follow the bond identifiers, and so the answer would
    /// follow the numbering. They are taken together, which can refuse a bond that either end
    /// alone would have settled, never label one.
    pub(super) fn reads_that_matter(&self, ends: Option<[AtomId; 2]>) -> BTreeSet<BondId> {
        let steady = self.steady_ties(ends);
        if steady.is_empty() {
            return self.read.clone();
        }
        if steady.iter().any(|region| region.read.is_empty()) {
            return BTreeSet::new();
        }
        steady
            .into_iter()
            .flat_map(|region| region.read.iter().copied())
            .collect()
    }

    /// An outcome that needed no ranking.
    fn without_ranking(outcome: Outcome) -> Self {
        Self {
            outcome,
            own: 0,
            deps: BTreeSet::new(),
            read: BTreeSet::new(),
            regions: Vec::new(),
            height: 0,
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
    without_bond: Option<[AtomId; 2]>,
    interrupt: &dyn Interrupt,
) -> Result<Analysis, Interrupted> {
    let ends = match ends(context, id) {
        Ok(ends) => ends,
        Err(Refusal::Cumulated) => {
            return Ok(Analysis::without_ranking(Outcome::Cumulated));
        }
        Err(Refusal::Reason(reason)) => return Ok(Analysis::refused(reason)),
    };
    let mut own = 0_usize;
    let mut deps = BTreeSet::new();
    let mut read = BTreeSet::new();
    let mut regions = Vec::new();
    let mut found = Vec::new();
    for end in &ends {
        let ranking = rank_slots(
            context,
            oracle,
            end.atom,
            &end.positions,
            Scope {
                cap,
                spheres: MAX_NESTED_SPHERES,
                without_bond,
            },
            interrupt,
        )?;
        own = own.saturating_add(ranking.own_nodes);
        deps.extend(ranking.requested.iter().copied());
        read.extend(ranking.read.iter().copied());
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
        regions.push(Region {
            tie: found.last() == Some(&End3::Tie),
            expanded: ranking.expanded,
            closes_ring: ranking.closes_ring,
            read: ranking.read,
        });
    }
    // The precedence is deliberate: a bond with two tied substituents at one end is not
    // stereogenic, whatever the other end could not be ranked for (a refusal, an ambiguity),
    // so a tie at either end is `Tie` before an unsupported or ambiguous end is reported. Then
    // the smallest reason of an unsupported end, then an ambiguous end. A limit is not a reason
    // here but a charge: see `double_bond`, which makes the item a limit first.
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
    Ok(Analysis {
        outcome,
        own,
        deps,
        read,
        regions,
        height: 0,
    })
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
///
/// An analysis is kept for the whole request (`memo`) only when its computation met no refusal
/// that came from the stack: a bond asked for while it is being computed is a cycle, and the
/// answer it gets (and so the answer of everything that asked, down to the bond that closes the
/// cycle) depends on which bond of the cycle was asked for first. Such an analysis is only
/// reused while the stack is the one it was computed under (`provisional`), so no result depends
/// on the order bonds were asked in, and a bond asked for later is computed afresh.
///
/// The nesting bound is applied to a kept analysis whenever it is used: it records how deep its
/// computation went ([`Analysis::height`]), and one that would go past the bound from where it
/// is asked is computed afresh, which is what a request for it from there always did.
#[derive(Default)]
struct Memory {
    memo: BTreeMap<BondId, Rc<Analysis>>,
    provisional: BTreeMap<(BondId, Vec<BondId>), (Rc<Analysis>, usize)>,
    /// The latest analysis of every bond, kept or not; only the cost of a descriptor is read
    /// from it (see [`Oracle::closure`]).
    latest: BTreeMap<BondId, Rc<Analysis>>,
    stack: Vec<BondId>,
    /// The lowest stack position a cycle refusal pointed at during the computation now running,
    /// or `usize::MAX`.
    cycle_at: usize,
    /// How many requests were refused for nesting too deep; a result that saw one is not kept.
    nesting_hits: usize,
    /// The greatest stack size the computation now running reached, counting what a kept
    /// analysis it used would have reached from where it was asked.
    deepest: usize,
}

/// The descriptors one asker has asked for so far, across the calls that charge it. What it is
/// charged is the cost of everything asked (see [`Oracle::closure`]), not the sum of what each
/// call would cost alone, so a descriptor several requests need is counted once whatever
/// order the requests are made in and however they are split into calls.
#[derive(Default)]
pub(super) struct Counted {
    /// Bonds whose cost the caller counts itself.
    left_out: BTreeSet<BondId>,
    /// Everything asked for directly.
    asked: BTreeSet<BondId>,
}

impl Counted {
    /// A count that leaves out `bond`, the one being labeled, whose own cost the caller counts.
    fn leaving_out(bond: BondId) -> Self {
        Self {
            left_out: BTreeSet::from([bond]),
            asked: BTreeSet::new(),
        }
    }
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
            memory: RefCell::new(Memory {
                cycle_at: usize::MAX,
                ..Memory::default()
            }),
        }
    }

    /// The analysis of `bond`. A bond that is being computed already (its descriptor depends on
    /// itself) answers with a [`Reason::StereoCycle`] refusal, and a request beyond
    /// [`MAX_RULE_3_NESTING`] with a [`Reason::DigraphLimit`] one; neither refusal is kept as
    /// the bond's own. An analysis that met a cycle is reused only under the same stack.
    ///
    /// A kept analysis is used only where the descriptors it needed fit the bound from the
    /// stack position it is asked at; asked deeper, it is computed again, and the descriptors
    /// that do not fit are refused, as in a request that never kept anything.
    ///
    /// # Errors
    ///
    /// An interrupt.
    pub(super) fn analysis(&self, bond: BondId) -> Result<Rc<Analysis>, Interrupted> {
        let (hits, outer_cycle_at, outer_deepest, position) = {
            let mut memory = self.memory.borrow_mut();
            let position = memory.stack.len();
            // A kept analysis that does not fit from here is computed again, and the computation
            // meets the bound, so its result is not kept (nor is a refusal stored for the
            // position): every asker past the bound pays for one computation, at most one per
            // asker, bounded by the nesting and the cap.
            if let Some(found) = memory.memo.get(&bond).cloned()
                && position + found.height <= MAX_RULE_3_NESTING
            {
                memory.deepest = memory.deepest.max(position + found.height);
                return Ok(found);
            }
            if let Some(at) = memory.stack.iter().position(|&asked| asked == bond) {
                memory.cycle_at = memory.cycle_at.min(at);
                return Ok(Rc::new(Analysis::refused(Reason::StereoCycle)));
            }
            if position >= MAX_RULE_3_NESTING {
                memory.nesting_hits += 1;
                return Ok(Rc::new(Analysis::refused(Reason::DigraphLimit)));
            }
            let key = (bond, memory.stack.clone());
            if let Some((found, cycle_at)) = memory.provisional.get(&key).cloned() {
                memory.cycle_at = memory.cycle_at.min(cycle_at);
                memory.deepest = memory.deepest.max(position + found.height);
                // The key holds the whole stack, so the entry was computed from this very
                // position, and a computation that met a nesting refusal is not kept: its
                // height fits as it did then, with no check here.
                debug_assert!(position + found.height <= MAX_RULE_3_NESTING);
                return Ok(found);
            }
            memory.stack.push(bond);
            let outer = std::mem::replace(&mut memory.cycle_at, usize::MAX);
            let outer_deepest = std::mem::replace(&mut memory.deepest, position + 1);
            (memory.nesting_hits, outer, outer_deepest, position)
        };
        let result = analyze(self.context, self, bond, self.cap, None, self.interrupt);
        let mut memory = self.memory.borrow_mut();
        memory.stack.pop();
        let cycle_at = memory.cycle_at;
        memory.cycle_at = outer_cycle_at.min(cycle_at);
        let height = memory.deepest - position;
        memory.deepest = memory.deepest.max(outer_deepest);
        let mut analysis = result?;
        analysis.height = height;
        let analysis = Rc::new(analysis);
        if memory.nesting_hits == hits {
            memory.latest.insert(bond, analysis.clone());
            if cycle_at == usize::MAX {
                memory.memo.insert(bond, analysis.clone());
            } else {
                let key = (bond, memory.stack.clone());
                memory.provisional.insert(key, (analysis.clone(), cycle_at));
            }
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

    /// The cost of `requested` and of the descriptors they need, not counting `left_out`. The
    /// requests are made from stack position `depth`, and the descriptors they need one position
    /// further down, level by level, so that a descriptor is met at the shallowest position any
    /// asker reaches it from. `missing` is what a bond with no analysis kept, or whose kept
    /// analysis needed more levels than the bound allows from where it is met, counts: 0 while
    /// it may still be being computed, `cap + 1` for a final charge. That is the cost a request
    /// from `depth` with nothing kept would have found, so it does not depend on which bonds the
    /// request asked for before.
    fn closure(
        &self,
        requested: &BTreeSet<BondId>,
        depth: usize,
        left_out: &BTreeSet<BondId>,
        missing: usize,
    ) -> usize {
        let memory = self.memory.borrow();
        let mut visited = left_out.clone();
        let mut total = 0_usize;
        let mut level: BTreeSet<BondId> = requested.clone();
        let mut depth = depth;
        while !level.is_empty() {
            let mut next = BTreeSet::new();
            for bond in level {
                if !visited.insert(bond) {
                    continue;
                }
                let kept = memory.memo.get(&bond);
                let Some(found) = kept.or_else(|| memory.latest.get(&bond)) else {
                    // A bond whose computation is on the stack is being paid for by its own
                    // asker.
                    if !memory.stack.contains(&bond) {
                        total = total.saturating_add(missing);
                    }
                    continue;
                };
                // Only a kept analysis has a height that belongs to the bond: one that met a
                // cycle went as deep as the stack it was computed under let it.
                if kept.is_some() && depth.saturating_add(found.height) > MAX_RULE_3_NESTING {
                    total = total.saturating_add(missing);
                    continue;
                }
                total = total.saturating_add(found.own);
                next.extend(found.deps.iter().copied());
            }
            level = next;
            depth += 1;
        }
        total
    }

    /// The cost of everything `counted` has asked for, with `bond` added, for a computation that
    /// has not finished.
    pub(super) fn charge(&self, bond: BondId, counted: &mut Counted) -> usize {
        let depth = self.memory.borrow().stack.len();
        counted.asked.insert(bond);
        self.closure(&counted.asked, depth, &counted.left_out, 0)
    }

    /// The analysis of `bond` with the bond between `ends` left out of the digraphs, not kept.
    ///
    /// Only [`Oracle::seen_from_inside`] asks, and it is asked by a double bond being labeled
    /// ([`double_bond`], after its own analysis has finished; a center passes no ends and never
    /// gets here). No descriptor is being computed then, so the stack is empty and this is the
    /// computation a request for `bond` from the top would make: the same nesting bound, and no
    /// outer computation whose `cycle_at` would have to be kept.
    fn analysis_without(&self, bond: BondId, ends: [AtomId; 2]) -> Result<Analysis, Interrupted> {
        debug_assert!(
            self.memory.borrow().stack.is_empty(),
            "a descriptor is being computed while a unit asks how it reads from inside"
        );
        self.memory.borrow_mut().stack.push(bond);
        let result = analyze(
            self.context,
            self,
            bond,
            self.cap,
            Some(ends),
            self.interrupt,
        );
        let mut memory = self.memory.borrow_mut();
        memory.stack.pop();
        memory.cycle_at = usize::MAX;
        result
    }

    /// Whether the descriptors in `read`, and those they read in turn, are the same seen from
    /// inside the digraph of a double bond with ends `ends`.
    ///
    /// For a center (`ends` is `None`) nothing is ranked again: only a ring closure can differ,
    /// and the walk follows analyses already computed, over bonds that are all in the closure of
    /// the descriptors the center was charged for (a bond's reads are among its direct needs), so
    /// it adds no work to count and `nodes`, `deps` and `over` stay empty.
    ///
    /// A bond that has no analysis kept (it was computed under a nesting limit) cannot be
    /// reached with the item inside its cap: its cost counts as `cap + 1` wherever the item is
    /// charged for it, and the item is a limit before `differs` is read.
    pub(super) fn seen_from_inside(
        &self,
        read: BTreeSet<BondId>,
        ends: Option<[AtomId; 2]>,
        room: usize,
    ) -> Result<Seen, Interrupted> {
        let mut seen = Seen {
            differs: false,
            over: false,
            nodes: 0,
            deps: BTreeSet::new(),
        };
        let mut visited = BTreeSet::new();
        let mut pending: Vec<BondId> = read.into_iter().collect();
        while let Some(bond) = pending.pop() {
            if !visited.insert(bond) {
                continue;
            }
            let kept = {
                let memory = self.memory.borrow();
                memory
                    .memo
                    .get(&bond)
                    .or_else(|| memory.latest.get(&bond))
                    .cloned()
            };
            let Some(found) = kept else {
                // See above: the item is a limit whatever `differs` says.
                seen.differs = true;
                continue;
            };
            if !found.steady_ties(ends).is_empty() {
                pending.extend(found.reads_that_matter(ends));
                continue;
            }
            pending.extend(found.read.iter().copied());
            if found.regions.iter().any(|region| region.closes_ring) {
                seen.differs = true;
                continue;
            }
            let Some(ends) = ends else { continue };
            if !found
                .regions
                .iter()
                .any(|region| region.touches(Some(ends)))
            {
                continue;
            }
            let inside = self.analysis_without(bond, ends)?;
            seen.nodes = seen.nodes.saturating_add(inside.own);
            if seen.nodes > room {
                seen.over = true;
                return Ok(seen);
            }
            seen.deps.extend(inside.deps.iter().copied());
            pending.extend(inside.read.iter().copied());
            if !same_reading(&found.outcome, &inside.outcome) {
                seen.differs = true;
            }
        }
        Ok(seen)
    }

    /// The cost of everything a finished computation asked for, `requested` being what it asked
    /// for directly. It is the same whichever asker ran first, because each descriptor's own
    /// cost, its direct needs and its height are fixed by the bond.
    pub(super) fn closure_cost(&self, requested: &BTreeSet<BondId>) -> usize {
        self.closure_cost_into(&mut Counted::default(), requested)
    }

    /// The cost of everything `counted` has asked for, with `requested` added, asked by the
    /// computation now running (or by a center, when none is).
    pub(super) fn closure_cost_into(
        &self,
        counted: &mut Counted,
        requested: &BTreeSet<BondId>,
    ) -> usize {
        let depth = self.memory.borrow().stack.len();
        counted.asked.extend(requested.iter().copied());
        self.closure(
            &counted.asked,
            depth,
            &counted.left_out,
            self.cap.saturating_add(1),
        )
    }

    /// [`Oracle::closure_cost_into`] for what the double bond being labeled asked for, whose
    /// own cost the caller counts: it is asked from the top, so what it asked for sits one
    /// level below it.
    fn closure_cost_below(&self, counted: &mut Counted, requested: &BTreeSet<BondId>) -> usize {
        let depth = self.memory.borrow().stack.len() + 1;
        counted.asked.extend(requested.iter().copied());
        self.closure(
            &counted.asked,
            depth,
            &counted.left_out,
            self.cap.saturating_add(1),
        )
    }
}

/// What reading the descriptors a double bond's ends asked for from inside its own digraph found.
pub(super) struct Seen {
    /// Some descriptor is not the same.
    pub(super) differs: bool,
    /// The rankings redone cost more than the room left.
    pub(super) over: bool,
    /// The nodes of the rankings that were redone.
    pub(super) nodes: usize,
    /// The descriptors those asked for directly.
    pub(super) deps: BTreeSet<BondId>,
}

/// Whether two outcomes read the same for Rule 3: the same refusal or tie, or the same higher
/// substituent at each end.
fn same_reading(a: &Outcome, b: &Outcome) -> bool {
    match (a, b) {
        (
            Outcome::Distinct {
                atoms: atoms_a,
                higher: higher_a,
                ..
            },
            Outcome::Distinct {
                atoms: atoms_b,
                higher: higher_b,
                ..
            },
        ) => atoms_a == atoms_b && higher_a == higher_b,
        (Outcome::Distinct { .. }, _) | (_, Outcome::Distinct { .. }) => false,
        (a, b) => a == b,
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
    let mut counted = Counted::leaving_out(id);
    let mut total = analysis
        .own
        .saturating_add(oracle.closure_cost_below(&mut counted, &analysis.deps));
    let unit_cost = analysis.own;
    let mut reads_own_atoms = false;
    if total <= item_cap
        && !analysis.read.is_empty()
        && let Some(own) = context.bond(id)
    {
        let seen = oracle
            .seen_from_inside(
                analysis.reads_that_matter(Some([own.begin, own.end])),
                Some([own.begin, own.end]),
                item_cap - total,
            )
            .map_err(Stop::Interrupted)?;
        reads_own_atoms = seen.differs;
        total = if seen.over {
            item_cap.saturating_add(1)
        } else {
            // What the descriptors cost is that of everything asked for, the unit's and the
            // rankings redone.
            unit_cost
                .saturating_add(seen.nodes)
                .saturating_add(oracle.closure_cost_below(&mut counted, &seen.deps))
        };
    }
    report.nodes = total;
    report.state = match &analysis.outcome {
        _ if total > item_cap => State::Unsupported(Reason::DigraphLimit),
        Outcome::Unsupported(reason) => State::Unsupported(*reason),
        Outcome::Cumulated => State::Unsupported(Reason::DoubleBondStructure),
        // Whatever the ends came to, a descriptor they read was ranked through this bond's own
        // atoms: see [`Reason::Rule3NestedDescriptor`].
        _ if reads_own_atoms => State::Unsupported(Reason::Rule3NestedDescriptor),
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
    use crate::budget::NEVER;
    use crate::document::{Atom, AtomRef, Command, Document, DocumentId, NewAtomPositions, Point};
    use crate::element::Element;
    use crate::graph::MoleculeGraph;

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

    /// A chain of single bonds, as a document graph, and the identifiers of its bonds.
    fn chain(bonds: usize) -> (MoleculeGraph, Vec<BondId>) {
        let mut document = Document::new(DocumentId::parse("bond-test").unwrap(), "t").unwrap();
        let adds: Vec<Command> = (0..=bonds)
            .map(|i| Command::AddAtom {
                atom: Atom::from(Element::from_symbol("C").unwrap()),
                positions: NewAtomPositions::Single(Point::new(i as f64, 0.0).unwrap()),
                bond_to: None,
            })
            .collect();
        document.apply(&adds).unwrap();
        let ids: Vec<AtomId> = document.atoms().map(|(id, _)| id).collect();
        let joins: Vec<Command> = ids
            .windows(2)
            .map(|pair| Command::AddBond {
                begin: AtomRef::Id(pair[0]),
                end: AtomRef::Id(pair[1]),
                order: BondOrder::Single,
            })
            .collect();
        document.apply(&joins).unwrap();
        let bond_ids = document.bonds().map(|(id, _)| id).collect();
        (MoleculeGraph::from_document(&document), bond_ids)
    }

    /// An analysis as the oracle keeps it, for a descriptor of the given cost, direct needs
    /// and height.
    fn analysis_of(own: usize, deps: &[BondId], height: usize) -> Rc<Analysis> {
        Rc::new(Analysis {
            outcome: Outcome::Tie,
            own,
            deps: deps.iter().copied().collect(),
            read: BTreeSet::new(),
            regions: Vec::new(),
            height,
        })
    }

    /// What a request is charged for the descriptors it asked for must not depend on whether the
    /// asking was split into calls or in what order they were made. `reaches` (not kept, because
    /// it met a cycle) needs `deep`, a kept descriptor four positions deep that does not fit one
    /// position down, and `deep` is also asked for directly, where it fits.
    #[test]
    fn what_is_charged_for_descriptors_does_not_depend_on_how_the_asking_is_split() {
        const CAP: usize = 100;
        let (graph, bonds) = chain(2);
        let context = Context::new(&graph);
        let (reaches, deep) = (bonds[0], bonds[1]);
        let counts = |calls: &[&[BondId]]| {
            let oracle = Oracle::new(&context, CAP, &NEVER);
            {
                let mut memory = oracle.memory.borrow_mut();
                memory.latest.insert(reaches, analysis_of(3, &[deep], 0));
                memory
                    .memo
                    .insert(deep, analysis_of(10, &[], MAX_RULE_3_NESTING));
            }
            let mut counted = Counted::default();
            let mut charged = 0;
            for requested in calls {
                let requested: BTreeSet<BondId> = requested.iter().copied().collect();
                charged = oracle.closure_cost_into(&mut counted, &requested);
            }
            charged
        };
        let together = counts(&[&[reaches, deep]]);
        // `deep` fits from the item, so it is counted for what it costs, and `reaches` too.
        assert_eq!(together, 3 + 10);
        assert_eq!(counts(&[&[reaches], &[deep]]), together);
        assert_eq!(counts(&[&[deep], &[reaches]]), together);
    }

    /// The bond being labeled is counted by the caller, even when a descriptor it needs needs it
    /// in turn (a cycle): it is not counted again by what it asked for.
    #[test]
    fn the_bond_being_labeled_is_not_counted_again_by_a_descriptor_that_needs_it() {
        let (graph, bonds) = chain(2);
        let context = Context::new(&graph);
        let (unit, needed) = (bonds[0], bonds[1]);
        let oracle = Oracle::new(&context, 100, &NEVER);
        {
            let mut memory = oracle.memory.borrow_mut();
            memory.latest.insert(unit, analysis_of(7, &[needed], 0));
            memory.latest.insert(needed, analysis_of(5, &[unit], 0));
        }
        let mut counted = Counted::leaving_out(unit);
        assert_eq!(
            oracle.closure_cost_below(&mut counted, &BTreeSet::from([needed])),
            5
        );
    }
}
