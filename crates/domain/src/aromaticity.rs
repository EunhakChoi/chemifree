//! A bounded ring-system aromaticity and Kekulé interpretation.
//!
//! The source graph is never rewritten. Ring blocks come from `rings`; this policy accepts
//! only a documented subset of ordinary conjugated C/N/O/S systems. It never asserts
//! non-aromatic for a system it cannot decide: a block that mixes a possible aromatic
//! cycle with atoms outside the conjugated system is `Unsupported`. A bounded matching
//! search assigns alternating bonds.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::chemistry::{self, Finding};
use crate::document::{Atom, AtomEdit, AtomId, Bond, BondId, BondOrder, Command, MAX_COMMANDS};
use crate::graph::MoleculeGraph;
use crate::rings::{self, RingSystem};
use crate::states::{Hydrogens, Radical};

pub const MODEL_ID: &str = "chemifree-aromaticity-2";
const MAX_SYSTEM_ATOMS: usize = 128;
const MAX_MATCH_STEPS: usize = 100_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Classification {
    Aromatic,
    NonAromatic,
    Unsupported,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KekuleOrder {
    Single,
    Double,
}

impl KekuleOrder {
    /// The valence contribution of the assigned bond.
    pub const fn valence(self) -> u16 {
        match self {
            Self::Single => 1,
            Self::Double => 2,
        }
    }

    /// The recorded bond order that realises this assignment.
    pub const fn bond_order(self) -> BondOrder {
        match self {
            Self::Single => BondOrder::Single,
            Self::Double => BondOrder::Double,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct System {
    pub atoms: Vec<AtomId>,
    pub bonds: Vec<BondId>,
    pub cycle_rank: usize,
    pub classification: Classification,
    pub electrons: Option<u16>,
    pub reason: Option<&'static str>,
    assignment: Option<Vec<BondId>>,
    /// Kekulé ring nitrogens whose pyrrole-type role rests on a valence-inferred hydrogen.
    inferred_donors: Vec<(AtomId, u8)>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Interpretation {
    pub systems: Vec<System>,
    aromatic_bonds: BTreeSet<BondId>,
}

/// The complete change aromatic conversion makes: every bond of the aromatic systems, and
/// the stated hydrogen of each Kekulé pyrrole-type nitrogen so the aromatic spelling is read
/// the same way.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AromaticConversion {
    pub bonds: Vec<BondId>,
    pub hydrogens: Vec<(AtomId, u8)>,
}

/// A ring atom's bonds, as the role rules see them.
struct Incidence {
    /// Valence contribution of every incident bond; `None` where no integral value exists.
    orders: Vec<Option<u16>>,
    ring_degree: usize,
    outside: usize,
    ring_double: usize,
    ring_aromatic: usize,
}

struct Role {
    electrons: u16,
    needs_double: bool,
    /// Set when this nitrogen is pyrrole-type only because the valence model infers hydrogen.
    inferred_hydrogens: Option<u8>,
}

/// The single rule deciding what a ring atom contributes. `Ok(None)` is an atom outside the
/// conjugated system (saturated); `Err` is a state the model cannot read.
///
/// The valence model only answers whether an atom's valence is a recognised state and, when
/// the bonds are integral, how many hydrogens fill it. It does not decide which atoms this
/// aromaticity model supports: that is the explicit table below, applied identically to
/// Kekulé and aromatic spellings. Each supported (element, charge) has one total valence
/// (bond orders plus hydrogens); radicals, carbenes and hypervalent atoms are refused.
///
/// A neutral nitrogen is pyrrole-type (a two-electron donor) when it carries hydrogen or
/// has three sigma bonds. Its hydrogen count is the stated one, or the one the valence
/// model infers from its bonds; with an aromatic bond nothing can be inferred, so an
/// aromatic nitrogen without a stated hydrogen is pyridine-type. A neutral two-coordinate
/// nitrogen with a stated hydrogen count of 0 and only single bonds has valence 2, which the
/// valence model calls unusual; it is read as outside the conjugated system, not refused.
fn role(id: AtomId, atom: &Atom, incidence: &Incidence) -> Result<Option<Role>, &'static str> {
    let Some(element) = atom.kind.element() else {
        return Err("query_atom");
    };
    if atom.radical != Radical::None {
        return Err("radical_state");
    }
    let assessed = chemistry::assess(id, atom, &incidence.orders);
    let degree = incidence.ring_degree + incidence.outside;
    let takes_part = incidence.ring_double + incidence.ring_aromatic > 0;
    // A neutral nitrogen with exactly two bonds, both single, and a stated hydrogen count of 0
    // has valence 2: no π electrons can be assigned to it, and nothing is conjugated through
    // it. As in model 1 it is outside the conjugated system, so a ring through it is not
    // aromatic. Any other state that the valence model calls invalid stays refused.
    if element.symbol() == "N"
        && atom.charge == 0
        && atom.hydrogens == Hydrogens::Specified(0)
        && incidence.orders == [Some(1), Some(1)]
        && assessed.finding == Finding::Invalid
    {
        return Ok(None);
    }
    let integral = match (assessed.finding, assessed.reason) {
        (Finding::Valid, _) => true,
        (Finding::Invalid, _) => return Err("valence_not_in_model"),
        (Finding::Unsupported, Some("non_integral_or_unknown_bond")) => false,
        (Finding::Unsupported, Some(reason)) => return Err(reason),
        (Finding::Unsupported, None) => return Err("valence_not_in_model"),
    };
    let hydrogens = assessed.hydrogen_count.map_or(0, usize::from);
    // (π electrons, needs a double bond, inferred hydrogens), or None for a saturated atom.
    let pi: Option<(u16, bool, Option<u8>)> = match (element.symbol(), atom.charge) {
        ("C", 0) if takes_part => Some((1, true, None)),
        ("C", 0) | ("N", 1) if !takes_part => None,
        ("C", 1) => Some((0, false, None)),
        ("C", -1) => Some((2, false, None)),
        ("N", 0) if hydrogens > 0 || degree == 3 => {
            let inferred = (atom.hydrogens == Hydrogens::Inferred && integral)
                .then(|| u8::try_from(hydrogens).ok())
                .flatten()
                .filter(|&h| h > 0);
            Some((2, false, inferred))
        }
        ("N", 0) if takes_part => Some((1, true, None)),
        ("N", 0) => return Err("valence_not_in_model"),
        ("N", 1) => Some((1, true, None)),
        ("N", -1) => Some((2, false, None)),
        ("O" | "S", 0) => Some((2, false, None)),
        _ => return Err("element_or_charge"),
    };
    let target = chemistry::restricted_target(element.symbol(), atom.charge)
        .map(usize::from)
        .ok_or("element_or_charge")?;
    if incidence.ring_double > 0 && pi.is_some_and(|(_, needs_double, _)| !needs_double) {
        return Err("valence_not_in_model");
    }
    let needs_double = pi.is_some_and(|(_, needs_double, _)| needs_double);
    match assessed.bond_order_sum {
        // Integral bonds: bond orders plus hydrogens must equal the supported valence.
        Some(sum) if usize::from(sum) + hydrogens != target => return Err("valence_not_in_model"),
        Some(_) => {}
        // An aromatic bond has no integral order; bound the atom by its target instead.
        None => {
            let stated = match atom.hydrogens {
                Hydrogens::Specified(count) => Some(usize::from(count)),
                _ => None,
            };
            let minimum = degree + usize::from(needs_double);
            if minimum > target || stated.is_some_and(|h| minimum + h != target) {
                return Err("valence_not_in_model");
            }
        }
    }
    Ok(
        pi.map(|(electrons, needs_double, inferred_hydrogens)| Role {
            electrons,
            needs_double,
            inferred_hydrogens,
        }),
    )
}

/// Classify cyclic biconnected systems. This is a named policy, not a universal test of
/// aromaticity. The model and work limits are described in the public contract.
pub fn perceive(document: &MoleculeGraph) -> Interpretation {
    perceive_in(document).0
}

/// [`perceive`] together with the atom and bond lookups it built, so a caller that goes on to
/// read the same graph does not build them again.
fn perceive_in(document: &MoleculeGraph) -> (Interpretation, Context<'_>) {
    let atoms: BTreeMap<_, _> = document.atoms().collect();
    let bonds: BTreeMap<_, _> = document.bonds().collect();
    let mut incident: BTreeMap<AtomId, Vec<BondId>> =
        atoms.keys().map(|&id| (id, Vec::new())).collect();
    for (&id, bond) in &bonds {
        incident.entry(bond.begin).or_default().push(id);
        incident.entry(bond.end).or_default().push(id);
    }
    let context = Context {
        atoms,
        bonds,
        incident,
    };
    let systems = rings::perceive(document)
        .into_iter()
        .map(|ring| interpret_ring(&ring, &context))
        .collect();
    let aromatic_bonds = context
        .bonds
        .iter()
        .filter_map(|(&id, b)| (b.order == BondOrder::Aromatic).then_some(id))
        .collect();
    (
        Interpretation {
            systems,
            aromatic_bonds,
        },
        context,
    )
}

struct Context<'a> {
    atoms: BTreeMap<AtomId, &'a Atom>,
    bonds: BTreeMap<BondId, &'a Bond>,
    incident: BTreeMap<AtomId, Vec<BondId>>,
}

struct Verdict {
    classification: Classification,
    electrons: Option<u16>,
    assignment: Option<Vec<BondId>>,
    reason: Option<&'static str>,
    inferred_donors: Vec<(AtomId, u8)>,
}

impl Verdict {
    fn non_aromatic(electrons: Option<u16>, reason: &'static str) -> Self {
        Self {
            classification: Classification::NonAromatic,
            electrons,
            assignment: None,
            reason: Some(reason),
            inferred_donors: Vec::new(),
        }
    }
}

fn interpret_ring(ring: &RingSystem, context: &Context<'_>) -> System {
    let mut result = System {
        atoms: ring.atoms.clone(),
        bonds: ring.bonds.clone(),
        cycle_rank: ring.cycle_rank,
        classification: Classification::Unsupported,
        electrons: None,
        reason: None,
        assignment: None,
        inferred_donors: Vec::new(),
    };
    match calculate(ring, context) {
        Ok(verdict) => {
            result.classification = verdict.classification;
            result.electrons = verdict.electrons;
            result.assignment = verdict.assignment;
            result.reason = verdict.reason;
            result.inferred_donors = verdict.inferred_donors;
        }
        Err(reason) => result.reason = Some(reason),
    }
    result
}

/// True when some π bond (double or aromatic) between active atoms lies on a cycle of active
/// atoms: the only place a conjugated circuit could be. A ring of donors joined by single
/// bonds has no such bond. Bounded by the block size, with no search budget.
fn has_pi_cycle(
    adjacent: &[Vec<(usize, BondId)>],
    active: &[bool],
    pi_bonds: &BTreeSet<BondId>,
) -> bool {
    for (start, edges) in adjacent.iter().enumerate() {
        for &(end, bond) in edges {
            if start > end || !pi_bonds.contains(&bond) || !active[start] || !active[end] {
                continue;
            }
            // Is `end` still reachable from `start` without this bond?
            let mut seen = vec![false; adjacent.len()];
            seen[start] = true;
            let mut queue = VecDeque::from([start]);
            while let Some(vertex) = queue.pop_front() {
                for &(next, edge) in &adjacent[vertex] {
                    if edge != bond && active[next] && !seen[next] {
                        seen[next] = true;
                        queue.push_back(next);
                    }
                }
            }
            if seen[end] {
                return true;
            }
        }
    }
    false
}

fn calculate(ring: &RingSystem, context: &Context<'_>) -> Result<Verdict, &'static str> {
    if ring.atoms.len() > MAX_SYSTEM_ATOMS {
        return Err("system_limit");
    }
    let ring_edges: BTreeSet<_> = ring.bonds.iter().copied().collect();
    let positions: BTreeMap<_, _> = ring
        .atoms
        .iter()
        .copied()
        .enumerate()
        .map(|(i, id)| (id, i))
        .collect();
    let mut inside = vec![Vec::new(); ring.atoms.len()];
    let mut any_aromatic = false;
    let mut all_aromatic = true;
    let mut explicit_double = Vec::new();
    let mut pi_bonds = BTreeSet::new();
    for &id in &ring.bonds {
        let bond = context.bonds[&id];
        let (Some(&a), Some(&b)) = (positions.get(&bond.begin), positions.get(&bond.end)) else {
            return Err("invalid_graph");
        };
        match bond.order {
            BondOrder::Aromatic => {
                any_aromatic = true;
                pi_bonds.insert(id);
            }
            BondOrder::Single => all_aromatic = false,
            BondOrder::Double => {
                all_aromatic = false;
                explicit_double.push((a, b));
                pi_bonds.insert(id);
            }
            _ => return Err("unsupported_ring_bond"),
        }
        inside[a].push((b, id));
        inside[b].push((a, id));
    }
    // Only the bonds touching this block, each once, feed the shared order summation.
    let touching: BTreeMap<BondId, &Bond> = ring
        .atoms
        .iter()
        .flat_map(|atom| &context.incident[atom])
        .map(|&id| (id, context.bonds[&id]))
        .collect();
    let mut orders = chemistry::incident_orders(
        ring.atoms.iter().copied(),
        touching.iter().map(|(&id, &bond)| (id, bond)),
        |_, bond| chemistry::bond_order_value(&bond.order),
    );
    let mut incidences = Vec::with_capacity(ring.atoms.len());
    for (i, atom_id) in ring.atoms.iter().enumerate() {
        let mut incidence = Incidence {
            orders: orders
                .remove(atom_id)
                .expect("every ring atom has an entry"),
            ring_degree: inside[i].len(),
            outside: 0,
            ring_double: 0,
            ring_aromatic: 0,
        };
        for bond_id in &context.incident[atom_id] {
            let bond = context.bonds[bond_id];
            if ring_edges.contains(bond_id) {
                match bond.order {
                    BondOrder::Double => incidence.ring_double += 1,
                    BondOrder::Aromatic => incidence.ring_aromatic += 1,
                    _ => {}
                }
            } else if bond.order == BondOrder::Single {
                incidence.outside += 1;
            } else {
                return Err("unsupported_external_bond");
            }
        }
        incidences.push(incidence);
    }
    let mut roles = Vec::with_capacity(ring.atoms.len());
    for (i, id) in ring.atoms.iter().enumerate() {
        roles.push(role(*id, context.atoms[id], &incidences[i])?);
    }
    if roles.iter().any(Option::is_none) {
        // Some atoms are saturated. Only a cycle of conjugated atoms through a π bond could
        // be aromatic.
        let active: Vec<_> = roles.iter().map(Option::is_some).collect();
        if has_pi_cycle(&inside, &active, &pi_bonds) {
            return Err("fused_partially_aromatic");
        }
        return if any_aromatic {
            Err("mixed_ring_bonds")
        } else {
            Ok(Verdict::non_aromatic(None, "not_conjugated"))
        };
    }
    let roles: Vec<Role> = roles.into_iter().flatten().collect();
    if pi_bonds.is_empty() {
        // Every atom donates but no bond carries π: nothing is conjugated.
        return Ok(Verdict::non_aromatic(None, "not_conjugated"));
    }
    if any_aromatic && !all_aromatic {
        return Err("mixed_ring_bonds");
    }
    let electrons: u16 = roles.iter().map(|r| r.electrons).sum();
    let mut matched = vec![false; roles.len()];
    if !any_aromatic {
        for (a, b) in explicit_double {
            if !roles[a].needs_double || !roles[b].needs_double || matched[a] || matched[b] {
                return Ok(Verdict::non_aromatic(None, "not_conjugated"));
            }
            matched[a] = true;
            matched[b] = true;
        }
        if roles
            .iter()
            .enumerate()
            .any(|(i, r)| r.needs_double != matched[i])
        {
            return Ok(Verdict::non_aromatic(None, "not_conjugated"));
        }
    }
    // The electron count is decided before neighbouring donors are: a count that fails the
    // 4n+2 rule is non-aromatic whatever the lone-pair arrangement. Only a count that would
    // make the ring aromatic depends on that arrangement, which the model does not decide.
    let donates = |r: &Role| r.electrons == 2 && !r.needs_double;
    let adjacent_donors = inside
        .iter()
        .enumerate()
        .any(|(a, edges)| donates(&roles[a]) && edges.iter().any(|&(b, _)| donates(&roles[b])));
    // Hückel's 4n+2 rule applies to a single circuit, not to the total π count of an
    // arbitrary fused system. For fused systems this version only recognizes neutral
    // carbocyclic benzenoids whose shortest alternate path at every edge forms a
    // six-membered ring. A shorter alternate path is conservatively unsupported.
    if ring.cycle_rank > 1 {
        if adjacent_donors {
            return Err("adjacent_donors");
        }
        if ring.atoms.iter().any(|id| {
            context.atoms[id].kind.element().map(|e| e.symbol()) != Some("C")
                || context.atoms[id].charge != 0
        }) || !six_membered_cover(&inside)
        {
            return Err("fused_system_outside_model");
        }
    } else if electrons < 2 || electrons % 4 != 2 {
        return Ok(Verdict::non_aromatic(Some(electrons), "electron_count"));
    } else if adjacent_donors {
        return Err("adjacent_donors");
    }
    let inferred_donors: Vec<_> = ring
        .atoms
        .iter()
        .zip(&roles)
        .filter_map(|(&id, r)| r.inferred_hydrogens.map(|h| (id, h)))
        .collect();
    if !any_aromatic {
        return Ok(Verdict {
            classification: Classification::Aromatic,
            electrons: Some(electrons),
            assignment: None,
            reason: None,
            inferred_donors,
        });
    }
    let mut used = vec![false; roles.len()];
    let mut selected = Vec::new();
    let mut budget = MAX_MATCH_STEPS;
    match assign(&roles, &inside, &mut used, &mut selected, &mut budget) {
        Ok(true) => {
            selected.sort_unstable();
            Ok(Verdict {
                classification: Classification::Aromatic,
                electrons: Some(electrons),
                assignment: Some(selected),
                reason: None,
                inferred_donors: Vec::new(),
            })
        }
        Ok(false) => Err("no_kekule_assignment"),
        Err(()) => Err("search_limit"),
    }
}

fn six_membered_cover(adjacent: &[Vec<(usize, BondId)>]) -> bool {
    for (start, edges) in adjacent.iter().enumerate() {
        for &(end, excluded) in edges {
            if start > end {
                continue;
            }
            let mut distances = vec![usize::MAX; adjacent.len()];
            distances[start] = 0;
            let mut queue = VecDeque::from([start]);
            while let Some(vertex) = queue.pop_front() {
                if distances[vertex] >= 5 {
                    continue;
                }
                for &(next, edge) in &adjacent[vertex] {
                    if edge != excluded && distances[next] == usize::MAX {
                        distances[next] = distances[vertex] + 1;
                        queue.push_back(next);
                    }
                }
            }
            if distances[end] != 5 {
                return false;
            }
        }
    }
    true
}

fn assign(
    roles: &[Role],
    adjacent: &[Vec<(usize, BondId)>],
    used: &mut [bool],
    selected: &mut Vec<BondId>,
    budget: &mut usize,
) -> Result<bool, ()> {
    let next = (0..roles.len())
        .filter(|&i| roles[i].needs_double && !used[i])
        .min_by_key(|&i| {
            adjacent[i]
                .iter()
                .filter(|&&(j, _)| roles[j].needs_double && !used[j])
                .count()
        });
    let Some(i) = next else { return Ok(true) };
    used[i] = true;
    for &(j, bond) in &adjacent[i] {
        if !roles[j].needs_double || used[j] {
            continue;
        }
        if *budget == 0 {
            used[i] = false;
            return Err(());
        }
        *budget -= 1;
        used[j] = true;
        selected.push(bond);
        if assign(roles, adjacent, used, selected, budget)? {
            return Ok(true);
        }
        selected.pop();
        used[j] = false;
    }
    used[i] = false;
    Ok(false)
}

/// Assign every aromatic input bond as single or double, or fail without partial output.
/// The document is never edited; callers may apply the complete assignment as one transaction.
pub fn kekulize(document: &MoleculeGraph) -> Result<Vec<(BondId, KekuleOrder)>, &'static str> {
    perceive(document).kekulize()
}

impl Interpretation {
    /// Return every bond interpreted as aromatic, with the hydrogen statements that keep
    /// Kekulé pyrrole-type nitrogens pyrrole-type, or fail if any cyclic system is
    /// indeterminate. The caller receives no partial assignment on failure.
    pub fn aromatize(&self) -> Result<AromaticConversion, &'static str> {
        let mut bonds = Vec::new();
        let mut hydrogens = Vec::new();
        let mut covered = BTreeSet::new();
        for system in &self.systems {
            covered.extend(system.bonds.iter().copied());
            match system.classification {
                Classification::Aromatic => {
                    bonds.extend(system.bonds.iter().copied());
                    hydrogens.extend(system.inferred_donors.iter().copied());
                }
                Classification::NonAromatic => {}
                Classification::Unsupported => return Err(system.reason.unwrap_or("unsupported")),
            }
        }
        if !self.aromatic_bonds.is_subset(&covered) {
            return Err("aromatic_bond_outside_ring");
        }
        bonds.sort_unstable();
        bonds.dedup();
        hydrogens.sort_unstable();
        hydrogens.dedup();
        Ok(AromaticConversion { bonds, hydrogens })
    }

    /// Use this interpretation's own input bond set; no second perception is required.
    pub fn kekulize(&self) -> Result<Vec<(BondId, KekuleOrder)>, &'static str> {
        let aromatic = &self.aromatic_bonds;
        let mut result = Vec::new();
        let mut seen = BTreeSet::new();
        for system in &self.systems {
            if !system.bonds.iter().any(|id| aromatic.contains(id)) {
                continue;
            }
            if system.classification != Classification::Aromatic {
                return Err(system.reason.unwrap_or("not_aromatic"));
            }
            let doubles: BTreeSet<_> = system
                .assignment
                .as_ref()
                .ok_or("no_kekule_assignment")?
                .iter()
                .copied()
                .collect();
            for &id in &system.bonds {
                if aromatic.contains(&id) {
                    seen.insert(id);
                    result.push((
                        id,
                        if doubles.contains(&id) {
                            KekuleOrder::Double
                        } else {
                            KekuleOrder::Single
                        },
                    ));
                }
            }
        }
        if &seen != aromatic {
            return Err("aromatic_bond_outside_ring");
        }
        result.sort_unstable_by_key(|(id, _)| *id);
        Ok(result)
    }
}

/// Neutral nitrogens whose stated hydrogen count equals what the valence model infers once the
/// given Kekulé orders are applied: exactly the atoms aromatic conversion would give the
/// statement back, so an aromatic, Kekulé, aromatic round trip is exact. Only nitrogen
/// endpoints of the converted bonds are examined. Charged nitrogens, a stated zero and any
/// other stated count are kept.
pub fn redundant_nitrogen_hydrogens(
    graph: &MoleculeGraph,
    assignment: &[(BondId, KekuleOrder)],
) -> Vec<AtomId> {
    let converted: BTreeMap<BondId, KekuleOrder> = assignment.iter().copied().collect();
    let endpoints: BTreeSet<AtomId> = graph
        .bonds()
        .filter(|(id, _)| converted.contains_key(id))
        .flat_map(|(_, bond)| [bond.begin, bond.end])
        .collect();
    let candidates: BTreeMap<AtomId, &Atom> = graph
        .atoms()
        .filter(|(id, atom)| {
            endpoints.contains(id)
                && atom.charge == 0
                && atom.kind.element().map(|e| e.symbol()) == Some("N")
                && matches!(atom.hydrogens, Hydrogens::Specified(count) if count > 0)
        })
        .collect();
    let orders =
        chemistry::incident_orders(candidates.keys().copied(), graph.bonds(), |id, bond| {
            kekule_order_value(&converted, id, bond)
        });
    candidates
        .into_iter()
        .filter(|&(id, atom)| {
            let Hydrogens::Specified(stated) = atom.hydrogens else {
                return false;
            };
            let mut probe = atom.clone();
            probe.hydrogens = Hydrogens::Inferred;
            let assessed = chemistry::assess(id, &probe, &orders[&id]);
            assessed.finding == Finding::Valid && assessed.hydrogen_count == Some(u16::from(stated))
        })
        .map(|(id, _)| id)
        .collect()
}

/// The one valence contribution of a bond once a Kekulé assignment is applied: the assigned
/// order where there is one, otherwise the recorded integral order.
pub fn kekule_order_value(
    assignment: &BTreeMap<BondId, KekuleOrder>,
    id: BondId,
    bond: &Bond,
) -> Option<u16> {
    assignment
        .get(&id)
        .map(|order| order.valence())
        .or_else(|| chemistry::bond_order_value(&bond.order))
}

type ConversionPlan = (Vec<(BondId, BondOrder)>, Vec<(AtomId, Hydrogens)>);

/// Which representation a conversion produces.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Target {
    Aromatic,
    Kekule,
}

/// The complete edit for one conversion of the whole graph: bond orders, plus the nitrogen
/// hydrogen statements that keep the meaning of each ring nitrogen. Commands that would
/// change nothing are omitted, so an already converted graph gives an empty list. Fails
/// with the model's reason, and without a partial result, if any system is unsupported, or
/// with `conversion_command_limit` if the edit needs more commands than one transaction
/// accepts ([`MAX_COMMANDS`]).
pub fn conversion_commands(
    graph: &MoleculeGraph,
    target: Target,
) -> Result<Vec<Command>, &'static str> {
    let (interpretation, context) = perceive_in(graph);
    let (bonds, hydrogens): ConversionPlan = match target {
        Target::Aromatic => {
            let plan = interpretation.aromatize()?;
            (
                plan.bonds
                    .into_iter()
                    .map(|id| (id, BondOrder::Aromatic))
                    .collect(),
                plan.hydrogens
                    .into_iter()
                    .map(|(atom, count)| (atom, Hydrogens::Specified(count)))
                    .collect(),
            )
        }
        Target::Kekule => {
            let assignment = interpretation.kekulize()?;
            let hydrogens = redundant_nitrogen_hydrogens(graph, &assignment)
                .into_iter()
                .map(|atom| (atom, Hydrogens::Inferred))
                .collect();
            let bonds = assignment
                .into_iter()
                .map(|(id, order)| (id, order.bond_order()))
                .collect();
            (bonds, hydrogens)
        }
    };
    let commands = changed_commands(&context, bonds, hydrogens)?;
    // The same constant `Document::apply_selecting` checks (`TooManyCommands`); checking here
    // gives the conversion its own reason instead of the generic transaction error.
    if commands.len() > MAX_COMMANDS {
        return Err("conversion_command_limit");
    }
    Ok(commands)
}

/// The commands that change a recorded bond order or hydrogen state, looked up in the graph
/// the plan was made for. An identifier the graph does not hold is an error, not a panic:
/// perception only names recorded items, so this guards a future defect there.
fn changed_commands(
    context: &Context<'_>,
    bonds: Vec<(BondId, BondOrder)>,
    hydrogens: Vec<(AtomId, Hydrogens)>,
) -> Result<Vec<Command>, &'static str> {
    let mut commands = Vec::new();
    for (bond, order) in bonds {
        let current = context.bonds.get(&bond).ok_or("invalid_graph")?;
        if current.order != order {
            commands.push(Command::UpdateBond { bond, order });
        }
    }
    for (atom, state) in hydrogens {
        let current = context.atoms.get(&atom).ok_or("invalid_graph")?;
        if current.hydrogens != state {
            commands.push(Command::UpdateAtom {
                atom,
                edit: AtomEdit {
                    hydrogens: Some(state),
                    ..AtomEdit::default()
                },
            });
        }
    }
    Ok(commands)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Perception only names items of the graph it read, so a lookup that misses is a defect
    /// elsewhere. The conversion must report it as an error instead of panicking, because
    /// it runs while the application holds its session lock.
    #[test]
    fn a_plan_naming_an_item_the_graph_lacks_is_an_error_not_a_panic() {
        let context = Context {
            atoms: BTreeMap::new(),
            bonds: BTreeMap::new(),
            incident: BTreeMap::new(),
        };
        let bond = BondId::from_serial(1).unwrap();
        let atom = AtomId::from_serial(1).unwrap();
        assert_eq!(
            changed_commands(&context, vec![(bond, BondOrder::Aromatic)], Vec::new()),
            Err("invalid_graph")
        );
        assert_eq!(
            changed_commands(&context, Vec::new(), vec![(atom, Hydrogens::Inferred)]),
            Err("invalid_graph")
        );
        assert_eq!(
            changed_commands(&context, Vec::new(), Vec::new()),
            Ok(Vec::new())
        );
    }
}
