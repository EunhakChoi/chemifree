//! Bounded, injective subgraph matching of recorded atom and bond predicates.
//!
//! Query -> target is non-induced: target edges not required by the query may be present.
//! Uncertain target states are refused rather than silently treated as a definite mismatch.

use std::collections::{BTreeMap, BTreeSet};

use crate::aromaticity;
use crate::document::{Atom, AtomId, Bond, BondOrder, Document};
use crate::graph::MoleculeGraph;
use crate::states::{AtomKind, BondQuery, Hydrogens, QueryAtom, QueryBondOrder, Radical};

pub const MATCH_MODEL: &str = "chemifree-query-recorded-1";
pub const AROMATIC_TARGET_MODEL: &str = "chemifree-query-aromatic-target-1";
pub const MAX_QUERY_ATOMS: usize = 64;
pub const MAX_MATCHES: usize = 1000;
pub const MAX_STATES: usize = 1_000_000;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AtomPredicate {
    pub atom: AtomId,
    pub ring: Option<bool>,
    pub min_neighbors: Option<usize>,
    pub max_neighbors: Option<usize>,
    pub charge: Option<i8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueryError {
    EmptyQuery,
    QueryLimit,
    InvalidBounds,
    InvalidPredicate,
    UnsupportedQuery,
    UnsupportedTarget,
    AromaticTargetUnsupported(&'static str),
}

/// Query bonds remain explicit predicates in both modes. The second mode interprets
/// supported target ring bonds with `chemifree-aromaticity-1` before matching.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatchModel {
    Recorded,
    AromaticTarget,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SearchLimit {
    Mappings,
    States,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchResult {
    /// Each entry maps every source query atom ID to a distinct target atom ID.
    pub mappings: Vec<Vec<(AtomId, AtomId)>>,
    /// False means a mapping or state limit stopped the search: absence is not proved.
    pub complete: bool,
    /// Work units spent on target candidates and checks of an already mapped edge.
    pub states: usize,
    pub limit: Option<SearchLimit>,
}

struct Graph<'a> {
    ids: Vec<AtomId>,
    atoms: Vec<&'a Atom>,
    adjacent: Vec<Vec<(usize, &'a Bond)>>,
    in_ring: Vec<bool>,
    perceived_aromatic: BTreeSet<(AtomId, AtomId)>,
}

impl<'a> Graph<'a> {
    fn new(document: &'a MoleculeGraph, perceived_aromatic: BTreeSet<(AtomId, AtomId)>) -> Self {
        let atoms: Vec<_> = document.atoms().collect();
        let ids: Vec<_> = atoms.iter().map(|(id, _)| *id).collect();
        let index: BTreeMap<_, _> = ids.iter().enumerate().map(|(i, id)| (*id, i)).collect();
        let mut adjacent = vec![Vec::new(); ids.len()];
        for (_, bond) in document.bonds() {
            let a = index[&bond.begin];
            let b = index[&bond.end];
            adjacent[a].push((b, bond));
            adjacent[b].push((a, bond));
        }
        for edges in &mut adjacent {
            edges.sort_by_key(|(v, _)| *v);
        }
        let in_ring = ring_membership(&adjacent);
        Self {
            ids,
            atoms: atoms.into_iter().map(|(_, a)| a).collect(),
            adjacent,
            in_ring,
            perceived_aromatic,
        }
    }

    fn edge(&self, a: usize, b: usize) -> Option<&Bond> {
        self.adjacent[a]
            .binary_search_by_key(&b, |(to, _)| *to)
            .ok()
            .map(|index| self.adjacent[a][index].1)
    }
}

/// An undirected edge is in a cycle exactly when it is not a bridge. Iterative low-link DFS
/// avoids recursion on large target drawings; self/duplicate edges are excluded by Document.
fn ring_membership(adjacency: &[Vec<(usize, &Bond)>]) -> Vec<bool> {
    let n = adjacency.len();
    let mut discovery = vec![0usize; n];
    let mut low = vec![0usize; n];
    let mut parent = vec![None; n];
    let mut ring = vec![false; n];
    let mut clock = 0;
    for root in 0..n {
        if discovery[root] != 0 {
            continue;
        }
        clock += 1;
        discovery[root] = clock;
        low[root] = clock;
        let mut stack = vec![(root, 0usize)];
        while let Some((v, cursor)) = stack.last_mut() {
            if *cursor < adjacency[*v].len() {
                let next = adjacency[*v][*cursor].0;
                *cursor += 1;
                if parent[*v] == Some(next) {
                    continue;
                }
                if discovery[next] == 0 {
                    parent[next] = Some(*v);
                    clock += 1;
                    discovery[next] = clock;
                    low[next] = clock;
                    stack.push((next, 0));
                } else {
                    low[*v] = low[*v].min(discovery[next]);
                }
            } else {
                let done = *v;
                stack.pop();
                if let Some(up) = parent[done] {
                    if low[done] <= discovery[up] {
                        ring[done] = true;
                        ring[up] = true;
                    }
                    low[up] = low[up].min(low[done]);
                }
            }
        }
    }
    ring
}

fn atom_matches(
    query: &Atom,
    target: &Atom,
    predicate: Option<&AtomPredicate>,
    degree: usize,
    ring: bool,
) -> bool {
    let Some(element) = target.kind.element() else {
        return false;
    };
    let element_match = match &query.kind {
        AtomKind::Element(q) => *q == element,
        AtomKind::Query(QueryAtom::Any) => true,
        AtomKind::Query(QueryAtom::AnyHeavy) => element.symbol() != "H",
        AtomKind::Query(QueryAtom::Heteroatom) => !matches!(element.symbol(), "C" | "H"),
        AtomKind::Query(QueryAtom::Halogen) => {
            matches!(element.symbol(), "F" | "Cl" | "Br" | "I" | "At" | "Ts")
        }
        AtomKind::Query(QueryAtom::List { elements, negated }) => {
            elements.contains(&element) != *negated
        }
    };
    if !element_match {
        return false;
    }
    if matches!(query.kind, AtomKind::Element(_)) && query.charge != target.charge {
        return false;
    }
    if query.isotope.is_some() && query.isotope != target.isotope {
        return false;
    }
    if (matches!(query.kind, AtomKind::Element(_)) || query.radical != Radical::None)
        && query.radical != target.radical
    {
        return false;
    }
    if let Hydrogens::Specified(count) = query.hydrogens
        && target.hydrogens != Hydrogens::Specified(count)
    {
        return false;
    }
    predicate.is_none_or(|p| {
        p.ring.is_none_or(|wanted| wanted == ring)
            && p.min_neighbors.is_none_or(|min| degree >= min)
            && p.max_neighbors.is_none_or(|max| degree <= max)
            && p.charge.is_none_or(|charge| charge == target.charge)
    })
}

fn bond_matches(
    query: &Bond,
    source: AtomId,
    target: &Bond,
    target_source: AtomId,
    aromatic: bool,
) -> bool {
    let target_order = if aromatic {
        &BondOrder::Aromatic
    } else {
        &target.order
    };
    match &query.order {
        BondOrder::Query(BondQuery::Any) => true,
        BondOrder::Query(BondQuery::OneOf(orders)) => {
            let target = match target_order {
                BondOrder::Single => Some(QueryBondOrder::Single),
                BondOrder::Double => Some(QueryBondOrder::Double),
                BondOrder::Triple => Some(QueryBondOrder::Triple),
                BondOrder::Aromatic => Some(QueryBondOrder::Aromatic),
                _ => None,
            };
            target.is_some_and(|order| orders.contains(&order))
        }
        BondOrder::Dative => {
            *target_order == BondOrder::Dative
                && (query.begin == source) == (target.begin == target_source)
        }
        order => order == target_order,
    }
}

struct Search<'a> {
    query: Graph<'a>,
    target: Graph<'a>,
    predicates: BTreeMap<AtomId, &'a AtomPredicate>,
    order: Vec<usize>,
    assigned: Vec<Option<usize>>,
    used: Vec<bool>,
    result: SearchResult,
    match_limit: usize,
    state_limit: usize,
}

impl Search<'_> {
    // Bound both candidate attempts and checks against already assigned neighbors. An
    // edge lookup is logarithmic in the target degree because each adjacency is sorted.
    fn spend(&mut self) -> bool {
        if self.result.states == self.state_limit {
            self.result.complete = false;
            self.result.limit = Some(SearchLimit::States);
            false
        } else {
            self.result.states += 1;
            true
        }
    }

    fn visit(&mut self, depth: usize) {
        if !self.result.complete {
            return;
        }
        if depth == self.order.len() {
            if self.result.mappings.len() == self.match_limit {
                self.result.complete = false;
                self.result.limit = Some(SearchLimit::Mappings);
                return;
            }
            self.result.mappings.push(
                self.assigned
                    .iter()
                    .enumerate()
                    .map(|(q, t)| {
                        (
                            self.query.ids[q],
                            self.target.ids[t.expect("complete assignment")],
                        )
                    })
                    .collect(),
            );
            return;
        }
        let q = self.order[depth];
        for t in 0..self.target.ids.len() {
            if !self.spend() {
                return;
            }
            if self.used[t]
                || !atom_matches(
                    self.query.atoms[q],
                    self.target.atoms[t],
                    self.predicates.get(&self.query.ids[q]).copied(),
                    self.target.adjacent[t].len(),
                    self.target.in_ring[t],
                )
            {
                continue;
            }
            let mut edges_match = true;
            for edge_index in 0..self.query.adjacent[q].len() {
                let other = self.query.adjacent[q][edge_index].0;
                let Some(mapped) = self.assigned[other] else {
                    continue;
                };
                if !self.spend() {
                    return;
                }
                let bond = self.query.adjacent[q][edge_index].1;
                if !self.target.edge(t, mapped).is_some_and(|target_bond| {
                    let key = (
                        target_bond.begin.min(target_bond.end),
                        target_bond.begin.max(target_bond.end),
                    );
                    bond_matches(
                        bond,
                        self.query.ids[q],
                        target_bond,
                        self.target.ids[t],
                        self.target.perceived_aromatic.contains(&key),
                    )
                }) {
                    edges_match = false;
                    break;
                }
            }
            if !edges_match {
                continue;
            }
            self.assigned[q] = Some(t);
            self.used[t] = true;
            self.visit(depth + 1);
            self.assigned[q] = None;
            self.used[t] = false;
            if !self.result.complete {
                return;
            }
        }
    }
}

/// Find all injective mappings up to explicit bounds. `complete == false` means the result
/// was truncated, so callers must not infer that no further mapping exists.
pub fn find(
    query: &Document,
    target: &Document,
    predicates: &[AtomPredicate],
    match_limit: usize,
    state_limit: usize,
) -> Result<SearchResult, QueryError> {
    find_graphs(
        &MoleculeGraph::from_document(query),
        &MoleculeGraph::from_document(target),
        predicates,
        match_limit,
        state_limit,
        MatchModel::Recorded,
    )
}

/// Match immutable graph snapshots. Aromatic target interpretation is opt-in and fails
/// as a whole when the named aromaticity model cannot justify the target representation.
pub fn find_graphs(
    query: &MoleculeGraph,
    target: &MoleculeGraph,
    predicates: &[AtomPredicate],
    match_limit: usize,
    state_limit: usize,
    model: MatchModel,
) -> Result<SearchResult, QueryError> {
    let perceived_aromatic = if model == MatchModel::AromaticTarget {
        let interpretation = aromaticity::perceive(target);
        interpretation
            .kekulize()
            .map_err(QueryError::AromaticTargetUnsupported)?;
        let bonds: BTreeSet<_> = interpretation
            .aromatize()
            .map_err(QueryError::AromaticTargetUnsupported)?
            .into_iter()
            .collect();
        target
            .bonds()
            .filter_map(|(id, bond)| {
                bonds
                    .contains(&id)
                    .then_some((bond.begin.min(bond.end), bond.begin.max(bond.end)))
            })
            .collect()
    } else {
        BTreeSet::new()
    };
    let query = Graph::new(query, BTreeSet::new());
    let target = Graph::new(target, perceived_aromatic);
    if query.ids.is_empty() {
        return Err(QueryError::EmptyQuery);
    }
    if query.ids.len() > MAX_QUERY_ATOMS {
        return Err(QueryError::QueryLimit);
    }
    if match_limit == 0 || match_limit > MAX_MATCHES || state_limit == 0 || state_limit > MAX_STATES
    {
        return Err(QueryError::InvalidBounds);
    }
    let mut seen = BTreeSet::new();
    for p in predicates {
        if !query.ids.contains(&p.atom)
            || !seen.insert(p.atom)
            || p.min_neighbors
                .zip(p.max_neighbors)
                .is_some_and(|(a, b)| a > b)
            || p.charge.is_some_and(|q| !(-8..=8).contains(&q))
        {
            return Err(QueryError::InvalidPredicate);
        }
    }
    if query.atoms.iter().any(|a| {
        a.radical == Radical::Unknown
            || a.hydrogens == Hydrogens::Unknown
            || !a.attachment_points.is_empty()
    }) || query
        .adjacent
        .iter()
        .flatten()
        .any(|(_, b)| b.order == BondOrder::Unknown)
    {
        return Err(QueryError::UnsupportedQuery);
    }
    if target.atoms.iter().any(|a| {
        a.kind.element().is_none()
            || a.radical == Radical::Unknown
            || a.hydrogens == Hydrogens::Unknown
            || !a.attachment_points.is_empty()
    }) || target
        .adjacent
        .iter()
        .flatten()
        .any(|(_, b)| matches!(b.order, BondOrder::Unknown | BondOrder::Query(_)))
    {
        return Err(QueryError::UnsupportedTarget);
    }
    let mut order: Vec<_> = (0..query.ids.len()).collect();
    order.sort_by_key(|&i| (std::cmp::Reverse(query.adjacent[i].len()), query.ids[i]));
    let predicates = predicates.iter().map(|p| (p.atom, p)).collect();
    let mut search = Search {
        assigned: vec![None; query.ids.len()],
        used: vec![false; target.ids.len()],
        order,
        query,
        target,
        predicates,
        result: SearchResult {
            mappings: Vec::new(),
            complete: true,
            states: 0,
            limit: None,
        },
        match_limit,
        state_limit,
    };
    search.visit(0);
    Ok(search.result)
}
