//! A bounded ring-system aromaticity and Kekulé interpretation.
//!
//! The source graph is never rewritten. Ring blocks come from `rings`; this policy accepts
//! only a documented subset of ordinary conjugated C/N/O/S systems. It does not assert that
//! other structures are nonaromatic. A bounded matching search assigns alternating bonds.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::document::{Atom, AtomId, Bond, BondId, BondOrder};
use crate::graph::MoleculeGraph;
use crate::rings::{self, RingSystem};
use crate::states::{Hydrogens, Radical};

pub const MODEL_ID: &str = "chemifree-aromaticity-1";
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
}

fn role(atom: &Atom, ring_degree: usize, outside: usize) -> Result<Role, &'static str> {
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
    let (electrons, needs_double, target): (u16, bool, usize) =
        match (element.symbol(), atom.charge) {
            ("C", 0) => (1, true, 4),
            ("C", 1) => (0, false, 3),
            ("C", -1) => (2, false, 3),
            ("N", 0) if stated.unwrap_or(0) > 0 || ring_degree + outside == 3 => (2, false, 3),
            ("N", 0) => (1, true, 3),
            ("N", 1) => (1, true, 4),
            ("N", -1) => (2, false, 2),
            ("O" | "S", 0) => (2, false, 2),
            _ => return Err("element_or_charge"),
        };
    let minimum = ring_degree + outside + usize::from(needs_double);
    if minimum > target || stated.is_some_and(|h| minimum + h != target) {
        return Err("valence_not_in_model");
    }
    Ok(Role {
        electrons,
        needs_double,
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
        assignment: None,
    };
    let interpretation = calculate(ring, atoms, bonds);
    match interpretation {
        Ok((classification, electrons, assignment, reason)) => {
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
        return Err("mixed_ring_bonds");
    }
    let mut roles = Vec::with_capacity(ring.atoms.len());
    for (i, id) in ring.atoms.iter().enumerate() {
        roles.push(role(atoms[id], inside[i].len(), outside[i])?);
    }
    let electrons: u16 = roles.iter().map(|r| r.electrons).sum();
    let mut matched = vec![false; roles.len()];
    if !any_aromatic {
        for (a, b) in explicit_double {
            if !roles[a].needs_double || !roles[b].needs_double || matched[a] || matched[b] {
                return Ok((
                    Classification::NonAromatic,
                    None,
                    None,
                    Some("not_conjugated"),
                ));
            }
            matched[a] = true;
            matched[b] = true;
        }
        if roles
            .iter()
            .enumerate()
            .any(|(i, r)| r.needs_double != matched[i])
        {
            return Ok((
                Classification::NonAromatic,
                None,
                None,
                Some("not_conjugated"),
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
        ));
    }
    if !any_aromatic {
        return Ok((Classification::Aromatic, Some(electrons), None, None));
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
            ))
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
