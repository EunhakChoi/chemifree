//! A bounded ring-system aromaticity and Kekulé interpretation.
//!
//! The source graph is never rewritten. Ring blocks come from `rings`; this policy accepts
//! only a documented subset of ordinary conjugated C/N/O/S systems. It does not assert that
//! other structures are nonaromatic. A bounded matching search assigns alternating bonds.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::chemistry;
use crate::document::{Atom, AtomId, Bond, BondId, BondOrder};
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct System {
    pub atoms: Vec<AtomId>,
    pub bonds: Vec<BondId>,
    pub cycle_rank: usize,
    pub classification: Classification,
    pub electrons: Option<u16>,
    pub reason: Option<&'static str>,
    /// Aromatic system drawn with single and double bonds only: the uncharged nitrogens that
    /// are pyrrole-type donors because they have inferred hydrogens, two neighbours and no
    /// double bond. With aromatic bonds the same nitrogen would read as pyridine-type, so a
    /// conversion to aromatic bonds states their hydrogens (see [`Interpretation::donor_hydrogens`]).
    donors: Vec<AtomId>,
    assignment: Option<Vec<BondId>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Interpretation {
    pub systems: Vec<System>,
    aromatic_bonds: BTreeSet<BondId>,
}

struct Role {
    electrons: u16,
    needs_double: bool,
    /// Every valence is used by single bonds and stated hydrogens, so the atom cannot take part
    /// in conjugation and contributes no pi electrons.
    saturated: bool,
    /// Read as a pyrrole-type donor only through the Kekule rule (inferred hydrogens, two
    /// neighbours, no double bond, no aromatic bonds in the system).
    pyrrole: bool,
}

/// What the atom contributes to a ring system, given its ring and outside neighbours.
/// `has_double` means a double bond to a ring neighbour is recorded; `aromatic_ring` means the
/// system is drawn with aromatic bonds.
fn role(
    atom: &Atom,
    ring_degree: usize,
    outside: usize,
    has_double: bool,
    aromatic_ring: bool,
) -> Result<Role, &'static str> {
    let Some(element) = atom.kind.element() else {
        return Err("query_atom");
    };
    if atom.radical != Radical::None {
        return Err("radical_state");
    }
    if !atom.attachment_points.is_empty() {
        return Err("attachment_point");
    }
    let stated = match atom.hydrogens {
        Hydrogens::Inferred => None,
        Hydrogens::Specified(count) => Some(usize::from(count)),
        Hydrogens::Unknown => return Err("unknown_hydrogens"),
    };
    let neighbours = ring_degree + outside;
    // A pyrrole-type nitrogen has no ring double bond and inferred hydrogens in a ring drawn
    // with single and double bonds; with aromatic bonds the same atom is pyridine-type.
    let pyrrole_type = stated.is_none() && neighbours == 2 && !has_double && !aromatic_ring;
    let symbol = element.symbol();
    let (electrons, needs_double, target): (u16, bool, usize) =
        match (element.symbol(), atom.charge) {
            ("C", 0) => (1, true, 4),
            ("C", 1) => (0, false, 3),
            ("C", -1) => (2, false, 3),
            ("N", 0) if stated.unwrap_or(0) > 0 || neighbours == 3 || pyrrole_type => (2, false, 3),
            ("N", 0) => (1, true, 3),
            ("N", 1) => (1, true, 4),
            ("N", -1) => (2, false, 2),
            ("O" | "S", 0) => (2, false, 2),
            _ => return Err("element_or_charge"),
        };
    if !needs_double {
        let minimum = neighbours;
        if minimum > target || stated.is_some_and(|h| minimum + h != target) {
            return Err("valence_not_in_model");
        }
        return Ok(Role {
            electrons,
            needs_double,
            saturated: false,
            pyrrole: pyrrole_type && symbol == "N" && atom.charge == 0 && neighbours == 2,
        });
    }
    // The atom either takes one more bond order (a ring double bond) or is saturated.
    let held = stated.unwrap_or(0);
    let double_order = usize::from(has_double);
    let used = neighbours + double_order + held;
    let saturated = match stated {
        Some(_) => !has_double && used == target,
        None => !has_double && neighbours == target,
    };
    if saturated {
        if aromatic_ring {
            return Err("valence_not_in_model");
        }
        return Ok(Role {
            electrons: 0,
            needs_double: false,
            saturated: true,
            pyrrole: false,
        });
    }
    // Inferred hydrogens fill any remaining valence; stated ones must fit exactly once the
    // atom takes its (possibly not yet recorded) double bond.
    let minimum = neighbours + 1;
    if minimum > target || stated.is_some_and(|h| minimum + h != target) {
        return Err("valence_not_in_model");
    }
    Ok(Role {
        electrons,
        needs_double: true,
        saturated: false,
        pyrrole: false,
    })
}

/// Classify cyclic biconnected systems. This is a named policy, not a universal test of
/// aromaticity. The model and work limits are described in the public contract.
pub fn perceive(document: &MoleculeGraph) -> Interpretation {
    let atoms: BTreeMap<_, _> = document.atoms().collect();
    let bonds: BTreeMap<_, _> = document.bonds().collect();
    let systems = rings::perceive(document)
        .into_iter()
        .map(|ring| interpret_ring(&ring, &atoms, &bonds))
        .collect();
    let aromatic_bonds = document
        .bonds()
        .filter_map(|(id, b)| (b.order == BondOrder::Aromatic).then_some(id))
        .collect();
    Interpretation {
        systems,
        aromatic_bonds,
    }
}

fn interpret_ring(
    ring: &RingSystem,
    atoms: &BTreeMap<AtomId, &Atom>,
    bonds: &BTreeMap<BondId, &Bond>,
) -> System {
    let mut result = System {
        atoms: ring.atoms.clone(),
        bonds: ring.bonds.clone(),
        cycle_rank: ring.cycle_rank,
        classification: Classification::Unsupported,
        electrons: None,
        reason: None,
        donors: Vec::new(),
        assignment: None,
    };
    let interpretation = calculate(ring, atoms, bonds);
    match interpretation {
        Ok((classification, electrons, assignment, reason, donors)) => {
            result.donors = donors;
            result.classification = classification;
            result.electrons = electrons;
            result.assignment = assignment;
            result.reason = reason;
        }
        Err(reason) => result.reason = Some(reason),
    }
    result
}

type Calculated = (
    Classification,
    Option<u16>,
    Option<Vec<BondId>>,
    Option<&'static str>,
    Vec<AtomId>,
);

fn calculate(
    ring: &RingSystem,
    atoms: &BTreeMap<AtomId, &Atom>,
    bonds: &BTreeMap<BondId, &Bond>,
) -> Result<Calculated, &'static str> {
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
    let mut outside = vec![0; ring.atoms.len()];
    let mut any_aromatic = false;
    let mut all_aromatic = true;
    let mut explicit_double = Vec::new();
    for (&id, bond) in bonds {
        let a = positions.get(&bond.begin).copied();
        let b = positions.get(&bond.end).copied();
        if ring_edges.contains(&id) {
            let (Some(a), Some(b)) = (a, b) else {
                return Err("invalid_graph");
            };
            match bond.order {
                BondOrder::Aromatic => any_aromatic = true,
                BondOrder::Single => all_aromatic = false,
                BondOrder::Double => {
                    all_aromatic = false;
                    explicit_double.push((a, b));
                }
                _ => return Err("unsupported_ring_bond"),
            }
            inside[a].push((b, id));
            inside[b].push((a, id));
        } else if a.is_some() || b.is_some() {
            if bond.order != BondOrder::Single {
                return Err("unsupported_external_bond");
            }
            if let Some(a) = a {
                outside[a] += 1;
            }
            if let Some(b) = b {
                outside[b] += 1;
            }
        }
    }
    if any_aromatic && !all_aromatic {
        // A fused system drawn partly aromatic is the same refusal in either spelling.
        return Err(if ring.cycle_rank > 1 {
            "fused_partially_aromatic"
        } else {
            "mixed_ring_bonds"
        });
    }
    let mut has_double = vec![false; ring.atoms.len()];
    for &(a, b) in &explicit_double {
        has_double[a] = true;
        has_double[b] = true;
    }
    let mut roles = Vec::with_capacity(ring.atoms.len());
    for (i, id) in ring.atoms.iter().enumerate() {
        roles.push(role(
            atoms[id],
            inside[i].len(),
            outside[i],
            has_double[i],
            any_aromatic,
        )?);
    }
    let electrons: u16 = roles.iter().map(|r| r.electrons).sum();
    if !any_aromatic {
        let mut matched = vec![false; roles.len()];
        let mut partner = vec![None; roles.len()];
        let mut conflict = false;
        for &(a, b) in &explicit_double {
            if !roles[a].needs_double || !roles[b].needs_double || matched[a] || matched[b] {
                conflict = true;
                continue;
            }
            matched[a] = true;
            matched[b] = true;
            partner[a] = Some(b);
            partner[b] = Some(a);
        }
        let reason = if conflict {
            Some("not_conjugated")
        } else if roles.iter().any(|r| r.saturated) {
            Some("saturated_atom")
        } else if roles
            .iter()
            .enumerate()
            .any(|(i, r)| r.needs_double != matched[i])
        {
            Some("not_conjugated")
        } else {
            None
        };
        if let Some(reason) = reason {
            // A fused system that is not aromatic as a whole may still contain a cycle that
            // would be aromatic alone. Its answer would depend on how the rest is drawn, so it
            // is refused instead of being called non-aromatic.
            if ring.cycle_rank > 1 {
                match aromatic_cycle_possible(&roles, &inside, &partner) {
                    Ok(false) => {}
                    Ok(true) => return Err("fused_partially_aromatic"),
                    Err(()) => return Err("search_limit"),
                }
            }
            return Ok((
                Classification::NonAromatic,
                None,
                None,
                Some(reason),
                Vec::new(),
            ));
        }
    }
    // Hückel's 4n+2 rule applies to a single circuit, not to the total π count of an
    // arbitrary fused system. For fused systems this version only recognizes neutral
    // carbocyclic benzenoids whose shortest alternate path at every edge forms a
    // six-membered ring. A shorter alternate path is conservatively unsupported.
    if ring.cycle_rank > 1 {
        if ring.atoms.iter().any(|id| {
            atoms[id].kind.element().map(|e| e.symbol()) != Some("C") || atoms[id].charge != 0
        }) || !six_membered_cover(&inside)
        {
            return Err("fused_system_outside_model");
        }
    } else if electrons < 2 || electrons % 4 != 2 {
        return Ok((
            Classification::NonAromatic,
            Some(electrons),
            None,
            Some("electron_count"),
            Vec::new(),
        ));
    }
    if !any_aromatic {
        let donors = ring
            .atoms
            .iter()
            .zip(&roles)
            .filter_map(|(id, role)| role.pyrrole.then_some(*id))
            .collect();
        return Ok((
            Classification::Aromatic,
            Some(electrons),
            None,
            None,
            donors,
        ));
    }
    let mut used = vec![false; roles.len()];
    let mut selected = Vec::new();
    let mut budget = MAX_MATCH_STEPS;
    match assign(&roles, &inside, &mut used, &mut selected, &mut budget) {
        Ok(true) => {
            selected.sort_unstable();
            Ok((
                Classification::Aromatic,
                Some(electrons),
                Some(selected),
                None,
                Vec::new(),
            ))
        }
        Ok(false) => Err("no_kekule_assignment"),
        Err(()) => Err("search_limit"),
    }
}

/// Whether some simple cycle of the system alternates single and recorded double bonds through
/// atoms that can conjugate and carries 4n+2 pi electrons. Such a cycle could be aromatic.
fn aromatic_cycle_possible(
    roles: &[Role],
    inside: &[Vec<(usize, BondId)>],
    partner: &[Option<usize>],
) -> Result<bool, ()> {
    struct Walk<'a> {
        roles: &'a [Role],
        inside: &'a [Vec<(usize, BondId)>],
        partner: &'a [Option<usize>],
        participant: Vec<bool>,
        visited: Vec<bool>,
        budget: usize,
        start: usize,
        first_double: Option<bool>,
    }
    impl Walk<'_> {
        fn electrons(&self, i: usize) -> u16 {
            if self.roles[i].needs_double {
                1
            } else {
                self.roles[i].electrons
            }
        }

        fn walk(
            &mut self,
            v: usize,
            arrived: Option<bool>,
            edges: usize,
            electrons: u16,
        ) -> Result<bool, ()> {
            for index in 0..self.inside[v].len() {
                let (w, _) = self.inside[v][index];
                if w < self.start || !self.participant[w] {
                    continue;
                }
                let double = self.partner[v] == Some(w);
                if v != self.start && self.roles[v].needs_double && arrived == Some(double) {
                    continue;
                }
                if self.budget == 0 {
                    return Err(());
                }
                self.budget -= 1;
                if w == self.start {
                    let blocked = self.roles[w].needs_double && self.first_double == Some(double);
                    if edges >= 2 && !blocked && electrons >= 2 && electrons % 4 == 2 {
                        return Ok(true);
                    }
                    continue;
                }
                if self.visited[w] {
                    continue;
                }
                if v == self.start {
                    self.first_double = Some(double);
                }
                self.visited[w] = true;
                let next = electrons + self.electrons(w);
                if self.walk(w, Some(double), edges + 1, next)? {
                    return Ok(true);
                }
                self.visited[w] = false;
            }
            Ok(false)
        }
    }
    let mut participant: Vec<bool> = roles
        .iter()
        .enumerate()
        .map(|(i, r)| !r.saturated && (!r.needs_double || partner[i].is_some()))
        .collect();
    // A cycle only passes through atoms with two participating neighbours, so peel the rest:
    // a conjugated chain or tree hanging off a ring can never close a cycle.
    loop {
        let peel: Vec<usize> = (0..roles.len())
            .filter(|&i| {
                participant[i] && inside[i].iter().filter(|&&(j, _)| participant[j]).count() < 2
            })
            .collect();
        if peel.is_empty() {
            break;
        }
        for i in peel {
            participant[i] = false;
        }
    }
    let mut walk = Walk {
        roles,
        inside,
        partner,
        participant,
        visited: vec![false; roles.len()],
        budget: MAX_MATCH_STEPS,
        start: 0,
        first_double: None,
    };
    for start in 0..roles.len() {
        if !walk.participant[start] {
            continue;
        }
        walk.start = start;
        walk.visited.iter_mut().for_each(|v| *v = false);
        walk.visited[start] = true;
        let electrons = walk.electrons(start);
        if walk.walk(start, None, 0, electrons)? {
            return Ok(true);
        }
    }
    Ok(false)
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
    /// Return every bond interpreted as aromatic, or fail if any cyclic system is
    /// indeterminate. The caller receives no partial assignment on failure.
    pub fn aromatize(&self) -> Result<Vec<BondId>, &'static str> {
        let mut result = Vec::new();
        let mut covered = BTreeSet::new();
        for system in &self.systems {
            covered.extend(system.bonds.iter().copied());
            match system.classification {
                Classification::Aromatic => result.extend(system.bonds.iter().copied()),
                Classification::NonAromatic => {}
                Classification::Unsupported => return Err(system.reason.unwrap_or("unsupported")),
            }
        }
        if !self.aromatic_bonds.is_subset(&covered) {
            return Err("aromatic_bond_outside_ring");
        }
        result.sort_unstable();
        result.dedup();
        Ok(result)
    }

    /// The nitrogens, with the hydrogen count to record, that must state their hydrogens when the
    /// aromatic systems they belong to are rewritten with aromatic bonds. A ring drawn with
    /// single and double bonds reads such a nitrogen as pyrrole-type from its inferred hydrogens;
    /// with aromatic bonds an inferred-hydrogen nitrogen is pyridine-type, so the count the model
    /// used (one, filling nitrogen's valence of three) becomes explicit in the same edit.
    pub fn donor_hydrogens(&self) -> Vec<(AtomId, u8)> {
        let mut result: Vec<_> = self
            .systems
            .iter()
            .filter(|s| s.classification == Classification::Aromatic)
            .flat_map(|s| s.donors.iter().map(|id| (*id, 1)))
            .collect();
        result.sort_unstable();
        result.dedup();
        result
    }

    /// Nitrogens of aromatic systems with aromatic bonds whose stated hydrogens are exactly
    /// what the valence model infers once those bonds have their Kekule orders. Converting to
    /// Kekule form returns them to inferred, which undoes the statement that conversion to
    /// aromatic bonds made; a count the model would not infer is information and is kept.
    pub fn inferable_nitrogen_hydrogens(&self, graph: &MoleculeGraph) -> Vec<AtomId> {
        let Ok(assignment) = self.kekulize() else {
            return Vec::new();
        };
        let assignment: BTreeMap<_, _> = assignment.into_iter().collect();
        let in_aromatic: BTreeSet<AtomId> = self
            .systems
            .iter()
            .filter(|s| s.bonds.iter().any(|id| self.aromatic_bonds.contains(id)))
            .flat_map(|s| s.atoms.iter().copied())
            .collect();
        let mut sums: BTreeMap<AtomId, Option<u16>> = BTreeMap::new();
        for (id, bond) in graph.bonds() {
            let order = chemistry::valence_order(&bond.order, assignment.get(&id).copied());
            for end in [bond.begin, bond.end] {
                let sum = sums.entry(end).or_insert(Some(0));
                *sum = sum.zip(order).map(|(a, b)| a + b);
            }
        }
        graph
            .atoms()
            .filter(|(id, atom)| {
                let Hydrogens::Specified(count) = atom.hydrogens else {
                    return false;
                };
                let nitrogen = atom.kind.element().filter(|e| e.symbol() == "N");
                in_aromatic.contains(id)
                    && atom.charge == 0
                    && atom.isotope.is_none()
                    && atom.radical == Radical::None
                    && nitrogen.is_some_and(|element| {
                        sums.get(id)
                            .copied()
                            .flatten()
                            .and_then(|sum| chemistry::inferred_hydrogens(element, sum))
                            == Some(u16::from(count))
                    })
            })
            .map(|(id, _)| id)
            .collect()
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
