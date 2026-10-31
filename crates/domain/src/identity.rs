//! Canonical labeled-graph identity with exhaustive, bounded individualization/refinement.
//!
//! This first profile compares recorded graph states, not normalized chemical meaning.
//! Document identifiers and coordinates never enter the canonical code. It returns no
//! identity or symmetry result when the complete search would exceed its resource limit.

use std::collections::{BTreeMap, BTreeSet};

use crate::aromaticity::{self, Classification};
use crate::chemistry::{self, Finding};
use crate::document::{Atom, AtomId, Bond, BondOrder};
use crate::graph::MoleculeGraph;
use crate::states::{AtomKind, BondQuery, Hydrogens, QueryAtom, QueryBondOrder, Radical};

pub const MODEL_ID: &str = "chemifree-recorded-graph-1";
pub const CHEMICAL_MODEL_ID: &str = "chemifree-chemical-graph-1";
pub const MAX_ATOMS: usize = 128;
pub const MAX_SEARCH_STATES: usize = 100_000;
/// Bounds refinement and serialization work as well as recursion count.
pub const MAX_WORK_UNITS: usize = 10_000_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdentityError {
    AtomLimit,
    SearchLimit,
    Unsupported(&'static str),
}

impl IdentityError {
    pub const fn code(self) -> &'static str {
        match self {
            Self::AtomLimit => "atom_limit",
            Self::SearchLimit => "search_limit",
            Self::Unsupported(reason) => reason,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalGraph {
    /// Stable for graph isomorphisms under this model; not a SMILES or hash.
    pub code: String,
    /// Canonical number `i + 1` maps to this document's atom ID. Symmetric atoms can be
    /// permuted inside an orbit without changing the code.
    pub numbering: Vec<AtomId>,
    /// Exact orbits under the chosen profile, including folded H; IDs are document-local.
    pub symmetry: Vec<Vec<AtomId>>,
    /// Terminal natural H vertices folded into a heavy atom in the chemical profile.
    pub folded_hydrogens: Vec<(AtomId, AtomId)>,
}

type Label = Vec<u8>;

fn radical(value: Radical) -> u8 {
    match value {
        Radical::None => 0,
        Radical::Singlet => 1,
        Radical::Doublet => 2,
        Radical::Triplet => 3,
        Radical::Unknown => 4,
    }
}

fn atom_label(atom: &Atom) -> Label {
    let mut out = Vec::new();
    match &atom.kind {
        AtomKind::Element(element) => out.extend([0, element.atomic_number()]),
        AtomKind::Query(query) => {
            out.push(1);
            match query {
                QueryAtom::Any => out.push(0),
                QueryAtom::AnyHeavy => out.push(1),
                QueryAtom::Heteroatom => out.push(2),
                QueryAtom::Halogen => out.push(3),
                QueryAtom::List { elements, negated } => {
                    out.extend([4, u8::from(*negated), elements.len() as u8]);
                    out.extend(elements.iter().map(|e| e.atomic_number()));
                }
            }
        }
    }
    out.push(atom.charge.to_ne_bytes()[0]);
    match atom.isotope {
        Some(n) => {
            out.push(1);
            out.extend(n.to_be_bytes());
        }
        None => out.push(0),
    }
    match atom.hydrogens {
        Hydrogens::Inferred => out.push(0),
        Hydrogens::Unknown => out.push(1),
        Hydrogens::Specified(count) => out.extend([2, count]),
    }
    out.push(radical(atom.radical));
    let attachments: Vec<_> = atom.attachment_points.values().collect();
    out.push(attachments.len() as u8);
    out.extend(attachments);
    out
}

fn query_order(order: QueryBondOrder) -> u8 {
    match order {
        QueryBondOrder::Single => 1,
        QueryBondOrder::Double => 2,
        QueryBondOrder::Triple => 3,
        QueryBondOrder::Aromatic => 4,
    }
}

/// Edge labels are from the first endpoint's perspective: dative direction is significant.
fn edge_label(bond: &Bond, forward: bool) -> Label {
    match &bond.order {
        BondOrder::Single => vec![1],
        BondOrder::Double => vec![2],
        BondOrder::Triple => vec![3],
        BondOrder::Aromatic => vec![4],
        BondOrder::Dative => vec![if forward { 5 } else { 6 }],
        BondOrder::Unknown => vec![7],
        BondOrder::Query(query) => match query {
            BondQuery::Any => vec![8, 0],
            BondQuery::OneOf(orders) => {
                let mut out = vec![8, 1, orders.len() as u8];
                out.extend(orders.iter().copied().map(query_order));
                out
            }
        },
    }
}

struct Graph {
    ids: Vec<AtomId>,
    labels: Vec<Label>,
    edges: Vec<Vec<Option<Label>>>,
    folded: Vec<(AtomId, AtomId)>,
}

impl Graph {
    fn new(input: &MoleculeGraph) -> Result<Self, IdentityError> {
        if input.atoms().len() > MAX_ATOMS {
            return Err(IdentityError::AtomLimit);
        }
        let atoms: Vec<_> = input.atoms().collect();
        let ids: Vec<_> = atoms.iter().map(|(id, _)| *id).collect();
        let indices: BTreeMap<_, _> = ids
            .iter()
            .copied()
            .enumerate()
            .map(|(i, id)| (id, i))
            .collect();
        let labels = atoms.iter().map(|(_, atom)| atom_label(atom)).collect();
        let mut edges = vec![vec![None; ids.len()]; ids.len()];
        for (_, bond) in input.bonds() {
            let a = indices[&bond.begin];
            let b = indices[&bond.end];
            edges[a][b] = Some(edge_label(bond, true));
            edges[b][a] = Some(edge_label(bond, false));
        }
        Ok(Self {
            ids,
            labels,
            edges,
            folded: Vec::new(),
        })
    }

    fn chemical(input: &MoleculeGraph) -> Result<Self, IdentityError> {
        if input.atoms().len() > MAX_ATOMS {
            return Err(IdentityError::AtomLimit);
        }
        let atoms: BTreeMap<_, _> = input.atoms().collect();
        let mut aromatic_bonds = BTreeSet::new();
        let interpreted = aromaticity::perceive(input);
        for system in &interpreted.systems {
            match system.classification {
                Classification::Aromatic => {
                    aromatic_bonds.extend(system.bonds.iter().copied());
                }
                Classification::Unsupported => {
                    return Err(IdentityError::Unsupported(
                        system.reason.unwrap_or("aromaticity_unsupported"),
                    ));
                }
                Classification::NonAromatic => {}
            }
        }
        if input
            .bonds()
            .any(|(id, bond)| bond.order == BondOrder::Aromatic && !aromatic_bonds.contains(&id))
        {
            return Err(IdentityError::Unsupported("aromaticity_unsupported"));
        }
        // Feed the existing valence policy the model's complete Kekulé assignment for
        // recorded aromatic bonds. Existing single/double inputs remain unchanged; the
        // graph snapshot itself is never rewritten.
        let assignment: BTreeMap<_, _> = interpreted
            .kekulize()
            .map_err(IdentityError::Unsupported)?
            .into_iter()
            .collect();
        let mut orders: BTreeMap<_, Vec<Option<u16>>> =
            atoms.keys().map(|&id| (id, Vec::new())).collect();
        for (id, bond) in input.bonds() {
            let order = match bond.order {
                BondOrder::Single => Some(1),
                BondOrder::Double => Some(2),
                BondOrder::Triple => Some(3),
                BondOrder::Aromatic => match assignment.get(&id) {
                    Some(aromaticity::KekuleOrder::Single) => Some(1),
                    Some(aromaticity::KekuleOrder::Double) => Some(2),
                    None => return Err(IdentityError::Unsupported("aromaticity_unsupported")),
                },
                BondOrder::Dative | BondOrder::Query(_) | BondOrder::Unknown => None,
            };
            orders
                .get_mut(&bond.begin)
                .expect("validated endpoint")
                .push(order);
            orders
                .get_mut(&bond.end)
                .expect("validated endpoint")
                .push(order);
        }
        let mut hydrogens = BTreeMap::new();
        for (&id, atom) in &atoms {
            let state = chemistry::assess(id, atom, &orders[&id]);
            if state.finding != Finding::Valid {
                return Err(IdentityError::Unsupported(
                    state.reason.unwrap_or("valence_unsupported"),
                ));
            }
            let count = state
                .hydrogen_count
                .ok_or(IdentityError::Unsupported("hydrogens_unknown"))?;
            hydrogens.insert(id, count);
        }
        let mut incident: BTreeMap<AtomId, Vec<(AtomId, &Bond)>> =
            atoms.keys().map(|id| (*id, Vec::new())).collect();
        for (_, bond) in input.bonds() {
            incident
                .get_mut(&bond.begin)
                .expect("validated endpoint")
                .push((bond.end, bond));
            incident
                .get_mut(&bond.end)
                .expect("validated endpoint")
                .push((bond.begin, bond));
        }
        let mut suppressed = BTreeSet::new();
        let mut folded = Vec::new();
        for (&id, atom) in &atoms {
            if atom.kind.element().map(|e| e.symbol()) != Some("H")
                || atom.charge != 0
                || atom.isotope.is_some()
                || atom.radical != Radical::None
                || !atom.attachment_points.is_empty()
                || !matches!(
                    atom.hydrogens,
                    Hydrogens::Inferred | Hydrogens::Specified(0)
                )
            {
                continue;
            }
            if let [(other, bond)] = incident[&id].as_slice()
                && bond.order == BondOrder::Single
                && atoms[other]
                    .kind
                    .element()
                    .is_some_and(|e| e.symbol() != "H")
            {
                suppressed.insert(id);
                folded.push((id, *other));
                *hydrogens.get_mut(other).expect("every atom was inspected") += 1;
            }
        }
        let ids: Vec<_> = atoms
            .keys()
            .copied()
            .filter(|id| !suppressed.contains(id))
            .collect();
        let indices: BTreeMap<_, _> = ids
            .iter()
            .copied()
            .enumerate()
            .map(|(i, id)| (id, i))
            .collect();
        let labels = ids
            .iter()
            .map(|id| {
                let mut atom = (*atoms[id]).clone();
                let count = u8::try_from(hydrogens[id])
                    .map_err(|_| IdentityError::Unsupported("hydrogen_limit"))?;
                atom.hydrogens = Hydrogens::Specified(count);
                Ok(atom_label(&atom))
            })
            .collect::<Result<Vec<_>, IdentityError>>()?;
        let mut edges = vec![vec![None; ids.len()]; ids.len()];
        for (id, bond) in input.bonds() {
            let (Some(&a), Some(&b)) = (indices.get(&bond.begin), indices.get(&bond.end)) else {
                continue;
            };
            let mut normalized = bond.clone();
            if aromatic_bonds.contains(&id) {
                normalized.order = BondOrder::Aromatic;
            }
            edges[a][b] = Some(edge_label(&normalized, true));
            edges[b][a] = Some(edge_label(&normalized, false));
        }
        Ok(Self {
            ids,
            labels,
            edges,
            folded,
        })
    }
}

/// Refine by the current partition and each atom's sorted labeled neighborhood.
fn refine(
    graph: &Graph,
    mut colors: Vec<usize>,
    work: &mut usize,
) -> Result<Vec<usize>, IdentityError> {
    loop {
        let cost = graph.ids.len().saturating_mul(graph.ids.len()).max(1);
        *work = work
            .checked_add(cost)
            .filter(|total| *total <= MAX_WORK_UNITS)
            .ok_or(IdentityError::SearchLimit)?;
        let signatures: Vec<_> = (0..graph.ids.len())
            .map(|v| {
                let mut neighbors: Vec<_> = (0..graph.ids.len())
                    .filter_map(|w| {
                        graph.edges[v][w]
                            .as_ref()
                            .map(|edge| (edge.clone(), colors[w]))
                    })
                    .collect();
                neighbors.sort_unstable();
                (colors[v], neighbors)
            })
            .collect();
        let mut unique = signatures.clone();
        unique.sort_unstable();
        unique.dedup();
        let next: Vec<_> = signatures
            .iter()
            .map(|s| unique.binary_search(s).expect("signature is present"))
            .collect();
        if next == colors {
            return Ok(next);
        }
        colors = next;
    }
}

fn initial_colors(graph: &Graph) -> Vec<usize> {
    let mut labels = graph.labels.clone();
    labels.sort_unstable();
    labels.dedup();
    graph
        .labels
        .iter()
        .map(|label| labels.binary_search(label).expect("label is present"))
        .collect()
}

/// Prefix each label with its length; absent edges use length zero.
fn code(graph: &Graph, order: &[usize]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend((order.len() as u16).to_be_bytes());
    for &i in order {
        out.extend((graph.labels[i].len() as u16).to_be_bytes());
        out.extend(&graph.labels[i]);
    }
    for a in 0..order.len() {
        for b in a + 1..order.len() {
            if let Some(edge) = &graph.edges[order[a]][order[b]] {
                out.push(edge.len() as u8);
                out.extend(edge);
            } else {
                out.push(0);
            }
        }
    }
    out
}

struct Search<'a> {
    graph: &'a Graph,
    states: usize,
    work: usize,
    best: Option<(Vec<u8>, Vec<usize>)>,
    /// Disjoint-set parents for permutations yielding the best code.
    orbit: Vec<usize>,
}

impl Search<'_> {
    fn root(&mut self, mut vertex: usize) -> usize {
        while self.orbit[vertex] != vertex {
            self.orbit[vertex] = self.orbit[self.orbit[vertex]];
            vertex = self.orbit[vertex];
        }
        vertex
    }

    fn join(&mut self, a: usize, b: usize) {
        let x = self.root(a);
        let y = self.root(b);
        self.orbit[y] = x;
    }

    fn visit(&mut self, colors: Vec<usize>) -> Result<(), IdentityError> {
        if self.states >= MAX_SEARCH_STATES {
            return Err(IdentityError::SearchLimit);
        }
        self.states += 1;
        let colors = refine(self.graph, colors, &mut self.work)?;
        let mut cells: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for (v, color) in colors.iter().copied().enumerate() {
            cells.entry(color).or_default().push(v);
        }
        if let Some(cell) = cells.values().find(|cell| cell.len() > 1) {
            for &chosen in cell {
                let mut individualized = colors.clone();
                individualized[chosen] = cells.len();
                self.visit(individualized)?;
            }
            return Ok(());
        }
        let order: Vec<_> = cells.values().map(|cell| cell[0]).collect();
        let candidate = code(self.graph, &order);
        match &self.best {
            None => {
                self.orbit = (0..self.graph.ids.len()).collect();
                self.best = Some((candidate, order));
            }
            Some((best, _)) if candidate < *best => {
                self.orbit = (0..self.graph.ids.len()).collect();
                self.best = Some((candidate, order));
            }
            Some((best, best_order)) if candidate == *best => {
                let pairs: Vec<_> = best_order.iter().copied().zip(order).collect();
                for (a, b) in pairs {
                    self.join(a, b);
                }
            }
            _ => {}
        }
        Ok(())
    }
}

/// Return a complete canonical form and exact automorphism orbits of the recorded graph.
/// Isotopes, charge, radical, hydrogen state, query expressions, attachment points and
/// directed dative bonds all participate. Aromatic/Kekulé and explicit/non-vertex hydrogen
/// are distinct representations in this profile.
pub fn canonicalize(input: &MoleculeGraph) -> Result<CanonicalGraph, IdentityError> {
    finish(Graph::new(input)?)
}

/// Canonicalize chemical meaning only when both the bounded valence and aromaticity models
/// can determine it. Natural terminal hydrogen vertices are folded into their heavy atoms;
/// isotope-labeled, charged, radical or H--H vertices remain distinct.
pub fn canonicalize_chemical(input: &MoleculeGraph) -> Result<CanonicalGraph, IdentityError> {
    finish(Graph::chemical(input)?)
}

fn finish(graph: Graph) -> Result<CanonicalGraph, IdentityError> {
    let mut search = Search {
        graph: &graph,
        states: 0,
        work: 0,
        best: None,
        orbit: (0..graph.ids.len()).collect(),
    };
    search.visit(initial_colors(&graph))?;
    let (bytes, order) = search
        .best
        .take()
        .expect("the empty or finite graph has a leaf");
    let mut groups: BTreeMap<usize, BTreeSet<AtomId>> = BTreeMap::new();
    for i in 0..graph.ids.len() {
        groups
            .entry(search.root(i))
            .or_default()
            .insert(graph.ids[i]);
    }
    let mut symmetry: Vec<Vec<_>> = groups
        .into_values()
        .map(|g| g.into_iter().collect())
        .collect();
    let mut hydrogen_orbits = Vec::new();
    for heavy_orbit in &symmetry {
        let folded: Vec<_> = graph
            .folded
            .iter()
            .filter_map(|(h, parent)| heavy_orbit.contains(parent).then_some(*h))
            .collect();
        if !folded.is_empty() {
            hydrogen_orbits.push(folded);
        }
    }
    symmetry.extend(hydrogen_orbits);
    symmetry.sort();
    let code = bytes.iter().map(|b| format!("{b:02x}")).collect();
    Ok(CanonicalGraph {
        code,
        numbering: order.into_iter().map(|i| graph.ids[i]).collect(),
        symmetry,
        folded_hydrogens: graph.folded,
    })
}
