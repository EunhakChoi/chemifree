//! From a center to a label state: the structure test, the pairwise ranking of the four
//! ligands, the tie classification, and the sign line. The pairwise ranking is shared with the
//! two ends of a double bond (`bond.rs`).

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use crate::budget::{Interrupt, Interrupted};
use crate::chemistry::{self, Finding};
use crate::document::{Atom, AtomId, BondId, BondOrder};
use crate::states::{Hydrogens, Radical};
use crate::stereo::{CenterState, Slot, StereoKey, StereoState, Turn};
use crate::stereo_drawing::fourth_position;

use super::bond::Oracle;
use super::context::Context;
use super::digraph::{Arena, Kind};
use super::rank::{Engine, Halt, dense_ranks};
use super::{CenterReport, Descriptor, MAX_SPHERES, Reason, Rule, State, Stop, Warning};

/// The four positions of a center in canonical slot order.
struct Positions {
    slots: [Slot; 4],
    /// The stored non-atom slot differs from the one the valence model names.
    unusual_slot: bool,
}

/// Whether `id` has the structure of a stereocenter (see [`positions`]), with the record stored
/// for it, if any.
pub(super) fn is_center(context: &Context<'_>, id: AtomId) -> bool {
    let Some(atom) = context.atom(id) else {
        return false;
    };
    let stored = match context.graph.stereo().get(StereoKey::Center(id)) {
        Some(StereoState::Center(state)) => Some(*state),
        Some(StereoState::DoubleBond(_)) => return false,
        None => None,
    };
    positions(context, id, atom, stored.as_ref()).is_ok()
}

/// The structure test of a center, whether or not a record is stored.
///
/// The atom must be an element atom with no attachment point, no radical and known hydrogens,
/// with 3 or 4 bonded neighbors by single, double or triple bonds, that the valence model finds
/// valid, and with three neighbors a fourth position `fourth_position` names (the same test the
/// derivation of a configuration from a drawing uses).
fn positions(
    context: &Context<'_>,
    id: AtomId,
    atom: &Atom,
    stored: Option<&CenterState>,
) -> Result<Positions, Reason> {
    if atom.kind.element().is_none() {
        return Err(Reason::QueryAtom);
    }
    if !atom.attachment_points.is_empty() {
        return Err(Reason::AttachmentPoint);
    }
    if atom.radical != Radical::None || atom.hydrogens == Hydrogens::Unknown {
        return Err(Reason::HydrogensUnknown);
    }
    let neighbors = context.neighbors(id);
    if !(3..=4).contains(&neighbors.len()) {
        return Err(Reason::NotACandidate);
    }
    let mut orders = Vec::with_capacity(neighbors.len());
    for &(_, bond) in neighbors {
        let order = &context.bond(bond).ok_or(Reason::CenterStructure)?.order;
        match order {
            BondOrder::Aromatic => return Err(Reason::CenterStructure),
            BondOrder::Dative | BondOrder::Query(_) | BondOrder::Unknown => {
                return Err(Reason::BondKind);
            }
            BondOrder::Single | BondOrder::Double | BondOrder::Triple => {
                orders.push(chemistry::bond_order_value(order));
            }
        }
    }
    if chemistry::assess(id, atom, &orders).finding != Finding::Valid {
        return Err(Reason::CenterStructure);
    }
    let derived = fourth_position(id, atom, &orders).map_err(|_| Reason::CenterStructure)?;
    let (slots, unusual_slot) = match stored {
        Some(state) => {
            let slots = *state.slots();
            let stored_fourth = slots
                .iter()
                .copied()
                .find(|slot| !matches!(slot, Slot::Atom(_)));
            (slots, stored_fourth != derived)
        }
        None => {
            let mut list: Vec<Slot> = neighbors
                .iter()
                .map(|&(neighbor, _)| Slot::Atom(neighbor))
                .collect();
            list.extend(derived);
            let slots: [Slot; 4] = list.try_into().map_err(|_| Reason::CenterStructure)?;
            (slots, false)
        }
    };
    Ok(Positions {
        slots,
        unusual_slot,
    })
}

/// Whether both ends of a double bond could have two different substituents, so that Rule 3
/// could rank a ligand by its geometry. Deliberately over-approximate: an end counts unless
/// its positions are certainly identical (a terminal `=CH2`, a carbonyl oxygen).
fn end_could_be_stereogenic(context: &Context<'_>, end: AtomId, partner: AtomId) -> bool {
    let Some(atom) = context.atom(end) else {
        return true;
    };
    let others: Vec<AtomId> = context
        .neighbors(end)
        .iter()
        .map(|&(neighbor, _)| neighbor)
        .filter(|&neighbor| neighbor != partner)
        .collect();
    let Ok(hydrogens) = context.hydrogens(end) else {
        return true;
    };
    let explicit_hydrogens = others
        .iter()
        .filter(|&&neighbor| {
            context
                .element(neighbor)
                .is_ok_and(|element| element.atomic_number() == 1)
        })
        .count();
    let lone_pair = usize::from(
        atom.charge == 0
            && matches!(
                atom.kind.element().map(|element| element.symbol()),
                Some("N" | "P")
            ),
    );
    let total = others.len() + usize::from(hydrogens) + lone_pair;
    let heavy = others.len() - explicit_hydrogens + lone_pair;
    total >= 2 && heavy >= 1
}

/// Whether the digraphs in `arena` contain two consecutive double bonds whose outer atoms could
/// both carry two different substituents: an axis (allene) that Rules 4 and 5 could use.
fn has_cumulated_axis(arena: &Arena<'_, '_>) -> bool {
    (0..arena.len()).any(|index| {
        let node = arena.node(index);
        let (Kind::Real(atom), Some(parent)) = (node.kind, arena.parent(index)) else {
            return false;
        };
        let middle = arena.parent_atom(index);
        node.bond_in == 2
            && arena.node(parent).bond_in == 2
            && matches!(arena.node(parent).kind, Kind::Real(_))
            && arena.parent(parent).is_some()
            && end_could_be_stereogenic(arena.context(), atom, middle)
            && end_could_be_stereogenic(arena.context(), arena.parent_atom(parent), middle)
    })
}

/// The work counter of one tie classification and the limits that guard it.
struct Budget<'a, 'g> {
    /// What the classification is charged.
    nodes: usize,
    /// The part of it that is arenas, not Rule 3 descriptors.
    own: usize,
    cap: usize,
    spheres: usize,
    interrupt: &'a dyn Interrupt,
    oracle: &'a Oracle<'a, 'g>,
    requested: BTreeSet<BondId>,
    /// The descriptors whose cost, with what they needed, is already in `nodes`: a descriptor
    /// several inner-center checks ask for is paid for once.
    charged: BTreeSet<BondId>,
}

impl Budget<'_, '_> {
    /// Adds the work of one comparison. Every amount is at most `cap + 1`, so the total is
    /// the same whichever order the comparisons ran in, and a total past the cap stops the
    /// classification as a limit.
    fn charge(&mut self, nodes: usize, own: usize) -> Result<(), Halt> {
        let over = self.cap.saturating_add(1);
        self.nodes = self.nodes.saturating_add(nodes.min(over));
        self.own = self.own.saturating_add(own.min(over));
        if self.nodes > self.cap {
            return Err(Reason::DigraphLimit.into());
        }
        Ok(())
    }
}

/// Whether two positions of `inner` (an atom inside a tied ligand, reached from
/// `parent_atom`) could differ: the digraphs rooted at `inner` differ, or tie but contain a
/// double bond Rule 3 could use. A comparison the model cannot finish counts as different,
/// because the question is only whether a rule not implemented here could separate them.
fn could_differ(
    context: &Context<'_>,
    inner: AtomId,
    positions: (Slot, Slot),
    budget: &mut Budget<'_, '_>,
) -> Result<bool, Halt> {
    let mut engine = Engine::new(
        context,
        budget.oracle,
        inner,
        budget.cap,
        budget.spheres,
        budget.interrupt,
    );
    let Ok((a, b)) = engine.arena.roots(positions.0, positions.1) else {
        return Ok(true);
    };
    let outcome = engine.compare(a, b, Rule::R3);
    budget.requested.extend(engine.requested().iter().copied());
    match outcome {
        Err(halt @ Halt::Interrupted(_)) => return Err(halt),
        // A limit, or an order that is not consistent: charged as a limit, because how far
        // the comparison got depends on the order its neighbors were visited in.
        Err(Halt::Reason(_)) => {
            let over = budget.cap.saturating_add(1);
            budget.charge(over, over)?;
            return Ok(true);
        }
        Ok(_) => {
            let own = engine.arena.len();
            let descriptors = budget
                .oracle
                .closure_cost_into(&mut budget.charged, engine.requested());
            budget.charge(own.saturating_add(descriptors), own)?;
        }
    }
    // A tie with nothing refused or ambiguous is two positions no rule can tell apart.
    Ok(!matches!(outcome, Ok((Ordering::Equal, _)))
        || engine.refusal().is_some()
        || engine.ambiguous())
}

/// Whether `inner` could be a stereocenter itself: at least three of its positions other than
/// the one toward the root are pairwise different (see [`could_differ`]). A tie that holds such
/// an atom might be broken by Rules 4 and 5, which this slice does not apply.
fn could_be_inner_center(
    context: &Context<'_>,
    inner: AtomId,
    parent_atom: AtomId,
    budget: &mut Budget<'_, '_>,
) -> Result<bool, Halt> {
    let Some(atom) = context.atom(inner) else {
        return Ok(true);
    };
    let hydrogens = context.hydrogens(inner)?;
    let mut candidates: Vec<Slot> = context
        .neighbors(inner)
        .iter()
        .filter(|&&(neighbor, _)| neighbor != parent_atom)
        .map(|&(neighbor, _)| Slot::Atom(neighbor))
        .collect();
    if hydrogens > 0 {
        candidates.push(Slot::ImplicitHydrogen);
    }
    let sigma = context.neighbors(inner).len() + usize::from(hydrogens);
    let pyramidal = atom.charge == 0
        && sigma == 3
        && matches!(
            atom.kind.element().map(|element| element.symbol()),
            Some("N" | "P" | "S")
        );
    if pyramidal {
        candidates.push(Slot::LonePair);
    }
    if candidates.len() < 3 {
        return Ok(false);
    }
    // Every pair is asked, not only each candidate against the classes found so far: which
    // pairs the shorter way asks follows the order the neighbors are listed in, which follows
    // identifiers, and so would the work counted.
    let mut alike = vec![vec![false; candidates.len()]; candidates.len()];
    for later in 1..candidates.len() {
        for earlier in 0..later {
            let pair = (candidates[earlier], candidates[later]);
            let differ = could_differ(context, inner, pair, budget)?;
            alike[earlier][later] = !differ;
        }
    }
    let classes = (0..candidates.len())
        .filter(|&later| !(0..later).any(|earlier| alike[earlier][later]))
        .count();
    Ok(classes >= 3)
}

/// What a tie between two ligands means, given the arena that holds their complete digraphs.
/// `None` is a tie no rule left out of this slice could break. The answer never claims `none`
/// where an axis or an inner center (Rules 4 and 5) might separate.
///
/// Every atom of the digraphs is asked about even after one could be an inner center: which
/// atom comes first follows the order the arena was filled in, which follows identifiers, so
/// stopping early would make the work counted depend on them.
fn classify_tie(
    engine: &Engine<'_, '_>,
    budget: &mut Budget<'_, '_>,
) -> Result<Option<Reason>, Halt> {
    let arena = &engine.arena;
    if has_cumulated_axis(arena) {
        return Ok(Some(Reason::Rule4Or5Needed));
    }
    let mut asked: BTreeMap<(AtomId, AtomId), bool> = BTreeMap::new();
    let mut inner_center = false;
    for index in 0..arena.len() {
        let Kind::Real(atom) = arena.node(index).kind else {
            continue;
        };
        let key = (atom, arena.parent_atom(index));
        let candidate = match asked.get(&key) {
            Some(&known) => known,
            None => {
                let found = could_be_inner_center(arena.context(), atom, key.1, budget)?;
                asked.insert(key, found);
                found
            }
        };
        inner_center |= candidate;
    }
    Ok(inner_center.then_some(Reason::Rule4Or5Needed))
}

/// The ranking of the positions around one root (a center, or one end of a double bond).
pub(super) struct Ranking {
    /// `results[i][j]` is `Greater` when position `i` ranks higher than `j`.
    pub(super) results: Vec<Vec<Ordering>>,
    /// The last rule any pair needed.
    pub(super) decided_by: Option<Rule>,
    /// What the ranking is charged: arenas, tie classifications, and for each pair the Rule 3
    /// descriptors it needed.
    pub(super) nodes: usize,
    /// The part of `nodes` that is arenas and tie classifications.
    pub(super) own_nodes: usize,
    /// The double bonds Rule 3 asked about, directly.
    pub(super) requested: BTreeSet<BondId>,
    /// The double bonds the comparisons of the pairs asked about, not counting the tie
    /// classifications (which only ask whether a rule left out of this slice could separate).
    pub(super) read: BTreeSet<BondId>,
    /// The atoms the digraphs of those comparisons expanded.
    pub(super) expanded: BTreeSet<AtomId>,
    /// A digraph of those comparisons closes a ring.
    pub(super) closes_ring: bool,
    pub(super) unsupported: Vec<Reason>,
    /// A comparison depended on a ligand double bond whose geometry is not stated.
    pub(super) ambiguous: bool,
}

/// What bounds one ranking and which bond, if any, its digraphs leave out.
#[derive(Clone, Copy)]
pub(super) struct Scope {
    /// The nodes the ranking may charge.
    pub(super) cap: usize,
    /// The farthest sphere expanded.
    pub(super) spheres: usize,
    /// A bond left out of the digraphs of the pairs (not of the tie classifications): see
    /// [`Arena::without_bond`].
    pub(super) without_bond: Option<[AtomId; 2]>,
}

/// Compares every pair of `slots` around `root` and classifies the ties.
///
/// What one ranking counts (see the module comment of `cip.rs`): for each pair of ligands the
/// nodes of its arena, and for each tie the nodes of its classification, each capped at
/// `cap + 1`. A comparison that ends in a limit counts `cap + 1` however far it got, because
/// how far it got can depend on the order the neighbors were visited in; one that ends in a
/// refusal counts what it materialized, which does not.
///
/// # Errors
///
/// Only an interrupt; every other failure is in [`Ranking::unsupported`].
pub(super) fn rank_slots(
    context: &Context<'_>,
    oracle: &Oracle<'_, '_>,
    root: AtomId,
    slots: &[Slot],
    scope: Scope,
    interrupt: &dyn Interrupt,
) -> Result<Ranking, Interrupted> {
    let Scope {
        cap,
        spheres,
        without_bond,
    } = scope;
    let over = cap.saturating_add(1);
    let count = slots.len();
    let mut ranking = Ranking {
        results: vec![vec![Ordering::Equal; count]; count],
        decided_by: None,
        nodes: 0,
        own_nodes: 0,
        requested: BTreeSet::new(),
        read: BTreeSet::new(),
        expanded: BTreeSet::new(),
        closes_ring: false,
        unsupported: Vec::new(),
        ambiguous: false,
    };
    for i in 0..count {
        for j in (i + 1)..count {
            // Every pair is compared, so the sum does not depend on the order of the slots and
            // no pair's failure hides another's.
            let mut engine = Engine::new(context, oracle, root, cap, spheres, interrupt)
                .without_bond(without_bond);
            let outcome = match engine.arena.roots(slots[i], slots[j]) {
                Ok((a, b)) => engine.compare(a, b, Rule::R3),
                Err(reason) => {
                    ranking.unsupported.push(reason);
                    continue;
                }
            };
            ranking.requested.extend(engine.requested().iter().copied());
            let (ordering, rule) = match outcome {
                Err(Halt::Interrupted(interrupted)) => return Err(interrupted),
                Err(Halt::Reason(reason)) => {
                    ranking.nodes = ranking.nodes.saturating_add(over);
                    ranking.own_nodes = ranking.own_nodes.saturating_add(over);
                    ranking.unsupported.push(reason);
                    continue;
                }
                Ok(found) => found,
            };
            let nodes = engine.nodes();
            if nodes > cap {
                ranking.nodes = ranking.nodes.saturating_add(over);
                ranking.own_nodes = ranking.own_nodes.saturating_add(over);
                ranking.unsupported.push(Reason::DigraphLimit);
                continue;
            }
            ranking.nodes = ranking.nodes.saturating_add(nodes);
            ranking.own_nodes = ranking.own_nodes.saturating_add(engine.arena.len());
            // What a comparison read is only kept when it finished: how far one that hit a
            // limit got follows the order its neighbors were visited in.
            ranking.read.extend(engine.requested().iter().copied());
            ranking.expanded.extend(engine.arena.expanded_atoms());
            // The tie of an end of a double bond is steady only if it did not look at the bond's
            // own atoms (see `Analysis::steady_ties`), which is read from `expanded`: a tie
            // expands every node, so no atom it reached is missing there.
            debug_assert!(ordering != Ordering::Equal || engine.arena.all_expanded());
            ranking.closes_ring |= engine.arena.closes_ring();
            if let Some(reason) = engine.refusal() {
                ranking.unsupported.push(reason);
                continue;
            }
            if engine.ambiguous() {
                ranking.ambiguous = true;
                continue;
            }
            ranking.results[i][j] = ordering;
            ranking.results[j][i] = ordering.reverse();
            ranking.decided_by = ranking.decided_by.max(rule);
            if ordering == Ordering::Equal {
                let mut budget = Budget {
                    nodes: 0,
                    own: 0,
                    cap,
                    spheres,
                    interrupt,
                    oracle,
                    requested: BTreeSet::new(),
                    charged: BTreeSet::new(),
                };
                let classified = classify_tie(&engine, &mut budget);
                ranking.nodes = ranking.nodes.saturating_add(budget.nodes.min(over));
                ranking.own_nodes = ranking.own_nodes.saturating_add(budget.own.min(over));
                ranking.requested.extend(budget.requested.iter().copied());
                match classified {
                    Ok(Some(reason)) => ranking.unsupported.push(reason),
                    Ok(None) => {}
                    Err(Halt::Interrupted(interrupted)) => return Err(interrupted),
                    Err(Halt::Reason(reason)) => ranking.unsupported.push(reason),
                }
            }
        }
    }
    if ranking.nodes > cap {
        ranking.unsupported.push(Reason::DigraphLimit);
    }
    Ok(ranking)
}

/// The label state of one atom treated as a center.
///
/// # Errors
///
/// Only an interrupt stops the item; every other failure is an `Unsupported` state.
pub(super) fn center(
    context: &Context<'_>,
    oracle: &Oracle<'_, '_>,
    id: AtomId,
    item_cap: usize,
    interrupt: &dyn Interrupt,
) -> Result<CenterReport, Stop> {
    let mut report = CenterReport {
        atom: id,
        state: State::Unsupported(Reason::NotACandidate),
        stored: None,
        priority: None,
        decided_by: None,
        warnings: Vec::new(),
        nodes: 0,
    };
    let Some(atom) = context.atom(id) else {
        return Ok(report);
    };
    let stored = match context.graph.stereo().get(StereoKey::Center(id)) {
        Some(StereoState::Center(state)) => Some(*state),
        Some(StereoState::DoubleBond(_)) => {
            report.state = State::Unsupported(Reason::CenterStructure);
            return Ok(report);
        }
        None => None,
    };
    report.stored = stored;
    let positions = match positions(context, id, atom, stored.as_ref()) {
        Ok(positions) => positions,
        Err(reason) => {
            report.state = State::Unsupported(reason);
            return Ok(report);
        }
    };
    if positions.unusual_slot {
        report.warnings.push(Warning::SlotUnusual);
    }
    let slots = positions.slots;
    let mut ranking = rank_slots(
        context,
        oracle,
        id,
        &slots,
        Scope {
            cap: item_cap,
            spheres: MAX_SPHERES,
            without_bond: None,
        },
        interrupt,
    )
    .map_err(Stop::Interrupted)?;
    report.nodes = ranking.nodes;
    // Past the cap the item is a limit: see `rank_slots`.
    if ranking.nodes <= item_cap {
        let seen = oracle
            .seen_from_inside(ranking.read.clone(), None, 0)
            .map_err(Stop::Interrupted)?;
        // Nothing is ranked again for a center, so there is no work of this walk to charge.
        debug_assert!(!seen.over && seen.nodes == 0 && seen.deps.is_empty());
        if seen.differs {
            ranking.unsupported.push(Reason::Rule3NestedDescriptor);
        }
    }
    if let Some(&reason) = ranking.unsupported.iter().min() {
        report.state = State::Unsupported(reason);
        return Ok(report);
    }
    if ranking.ambiguous {
        report.state = State::Ambiguous(Reason::LigandStereoUnspecified);
        return Ok(report);
    }
    let decided_by = ranking.decided_by;
    let Some(ranks) = dense_ranks(&ranking.results) else {
        report.state = State::Unsupported(Reason::RankingInconsistent);
        return Ok(report);
    };
    if ranks.iter().max() != Some(&3) {
        // Two ligands tie and nothing left out of this slice could break the tie.
        report.state = State::None;
        if stored.is_some() {
            report.warnings.push(Warning::StoredNotStereogenic);
        }
        return Ok(report);
    }
    let mut ordered: Vec<(Slot, usize)> = slots.iter().copied().zip(ranks).collect();
    ordered.sort_by_key(|&(_, rank)| rank);
    let ranked = [ordered[0].0, ordered[1].0, ordered[2].0, ordered[3].0];
    report.priority = Some(
        ordered
            .iter()
            .map(|&(slot, rank)| (slot, u8::try_from(rank + 1).unwrap_or(u8::MAX)))
            .collect(),
    );
    report.decided_by = decided_by;
    report.state = match stored {
        None => State::Unspecified,
        Some(CenterState::Unknown { .. }) => State::Unknown,
        Some(state @ CenterState::Tetrahedral { .. }) => match state.turn_for_order(ranked) {
            // The one line that fixes the sign. The stored turn is read looking from the first
            // position, which here is the highest-ranked one; the CIP view has the lowest
            // position pointing away. Moving the lowest to the end of the order is an odd
            // permutation, which flips the turn, so the two statements agree on this: R exactly
            // when the turn of (highest, second, third, lowest) is clockwise.
            Ok(Some(Turn::Clockwise)) => State::Assigned(Descriptor::R),
            Ok(Some(Turn::Anticlockwise)) => State::Assigned(Descriptor::S),
            Ok(None) | Err(_) => State::Unsupported(Reason::CenterStructure),
        },
    };
    Ok(report)
}
