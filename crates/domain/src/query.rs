//! Bounded, injective subgraph matching of recorded atom and bond predicates.
//!
//! Query -> target is non-induced: target edges not required by the query may be present.
//! Uncertain target states are refused rather than silently treated as a definite mismatch.

use std::collections::{BTreeMap, BTreeSet};

use crate::aromaticity::{self, KekuleOrder};
use crate::chemistry::{self, Finding};
use crate::document::{Atom, AtomId, Bond, BondId, BondOrder, Document, MAX_CHARGE, MIN_CHARGE};
use crate::graph::MoleculeGraph;
use crate::rings;
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
    /// A query atom states a hydrogen count and a candidate target atom's count cannot be
    /// determined (inferred hydrogens need a valid atom with integral bond orders).
    TargetHydrogensUndetermined,
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
}

impl<'a> Graph<'a> {
    fn new(document: &'a MoleculeGraph) -> Self {
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
        Self {
            ids,
            atoms: atoms.into_iter().map(|(_, a)| a).collect(),
            adjacent,
        }
    }

    fn edge(&self, a: usize, b: usize) -> Option<&Bond> {
        self.adjacent[a]
            .binary_search_by_key(&b, |(to, _)| *to)
            .ok()
            .map(|index| self.adjacent[a][index].1)
    }
}

/// Facts about the target that atom conditions need, computed once per search.
struct TargetFacts {
    /// An atom is in a ring when it belongs to a cyclic block of the shared topology.
    in_ring: Vec<bool>,
    /// Hydrogens not drawn as atoms, or `None` when no model can determine them.
    hydrogens: Vec<Option<u16>>,
    perceived_aromatic: BTreeSet<(AtomId, AtomId)>,
}

fn ring_membership(target: &MoleculeGraph, ids: &[AtomId]) -> Vec<bool> {
    let in_ring: BTreeSet<_> = rings::perceive(target)
        .into_iter()
        .flat_map(|system| system.atoms)
        .collect();
    ids.iter().map(|id| in_ring.contains(id)).collect()
}

/// Non-vertex hydrogens per target atom. A stated count is used as recorded; an inferred
/// count comes from `chemifree-valence-2` and exists only for a valid atom whose bonds all
/// have integral orders (aromatic bonds take their assigned Kekule order when one is given).
fn target_hydrogens(
    target: &MoleculeGraph,
    ids: &[AtomId],
    kekule: &BTreeMap<BondId, KekuleOrder>,
) -> Vec<Option<u16>> {
    let mut orders: BTreeMap<AtomId, Vec<Option<u16>>> =
        ids.iter().map(|id| (*id, Vec::new())).collect();
    for (id, bond) in target.bonds() {
        let order = match bond.order {
            BondOrder::Single => Some(1),
            BondOrder::Double => Some(2),
            BondOrder::Triple => Some(3),
            BondOrder::Aromatic => kekule.get(&id).map(|order| match order {
                KekuleOrder::Single => 1,
                KekuleOrder::Double => 2,
            }),
            BondOrder::Dative | BondOrder::Query(_) | BondOrder::Unknown => None,
        };
        for end in [bond.begin, bond.end] {
            if let Some(list) = orders.get_mut(&end) {
                list.push(order);
            }
        }
    }
    target
        .atoms()
        .map(|(id, atom)| match atom.hydrogens {
            Hydrogens::Specified(count) => Some(u16::from(count)),
            Hydrogens::Unknown => None,
            Hydrogens::Inferred => {
                let state = chemistry::assess(id, atom, &orders[&id]);
                (state.finding == Finding::Valid)
                    .then_some(state.hydrogen_count)
                    .flatten()
            }
        })
        .collect()
}

/// `None` means the condition cannot be decided for this target atom (its hydrogens).
fn atom_matches(
    query: &Atom,
    target: &Atom,
    predicate: Option<&AtomPredicate>,
    degree: usize,
    ring: bool,
    target_hydrogens: Option<u16>,
) -> Option<bool> {
    let Some(element) = target.kind.element() else {
        return Some(false);
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
        return Some(false);
    }
    if matches!(query.kind, AtomKind::Element(_)) && query.charge != target.charge {
        return Some(false);
    }
    if query.isotope.is_some() && query.isotope != target.isotope {
        return Some(false);
    }
    if (matches!(query.kind, AtomKind::Element(_)) || query.radical != Radical::None)
        && query.radical != target.radical
    {
        return Some(false);
    }
    let predicate_holds = predicate.is_none_or(|p| {
        p.ring.is_none_or(|wanted| wanted == ring)
            && p.min_neighbors.is_none_or(|min| degree >= min)
            && p.max_neighbors.is_none_or(|max| degree <= max)
            && p.charge.is_none_or(|charge| charge == target.charge)
    });
    if !predicate_holds {
        return Some(false);
    }
    match query.hydrogens {
        Hydrogens::Specified(count) => target_hydrogens.map(|found| found == u16::from(count)),
        Hydrogens::Inferred | Hydrogens::Unknown => Some(true),
    }
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
    perceived_aromatic: BTreeSet<(AtomId, AtomId)>,
    /// Query atoms in matching order, each with an earlier neighbor that anchors candidates.
    plan: Vec<(usize, Option<usize>)>,
    /// `compatible[q][t]`: target atom `t` satisfies every atom condition of query atom `q`.
    compatible: Vec<Vec<bool>>,
    assigned: Vec<Option<usize>>,
    used: Vec<bool>,
    result: SearchResult,
    match_limit: usize,
    state_limit: usize,
}

impl Search<'_> {
    // Bound candidate attempts and checks against already assigned neighbors. An edge lookup
    // is logarithmic in the target degree because each adjacency is sorted.
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

    fn edges_match(&mut self, q: usize, t: usize) -> Option<bool> {
        for edge_index in 0..self.query.adjacent[q].len() {
            let other = self.query.adjacent[q][edge_index].0;
            let Some(mapped) = self.assigned[other] else {
                continue;
            };
            if !self.spend() {
                return None;
            }
            let bond = self.query.adjacent[q][edge_index].1;
            let holds = self.target.edge(t, mapped).is_some_and(|target_bond| {
                let key = (
                    target_bond.begin.min(target_bond.end),
                    target_bond.begin.max(target_bond.end),
                );
                bond_matches(
                    bond,
                    self.query.ids[q],
                    target_bond,
                    self.target.ids[t],
                    self.perceived_aromatic.contains(&key),
                )
            });
            if !holds {
                return Some(false);
            }
        }
        Some(true)
    }

    fn record(&mut self) {
        if self.result.mappings.len() == self.match_limit {
            self.result.complete = false;
            self.result.limit = Some(SearchLimit::Mappings);
            return;
        }
        let mapping = self
            .assigned
            .iter()
            .enumerate()
            .map(|(q, t)| {
                (
                    self.query.ids[q],
                    self.target.ids[t.expect("complete assignment")],
                )
            })
            .collect();
        self.result.mappings.push(mapping);
    }

    fn visit(&mut self, depth: usize) {
        if !self.result.complete {
            return;
        }
        if depth == self.plan.len() {
            self.record();
            return;
        }
        let (q, anchor) = self.plan[depth];
        // A connected query atom is only tried next to its mapped neighbor; the first atom of
        // each component is tried against every target atom.
        let candidates: Vec<usize> = match anchor {
            Some(a) => {
                let mapped = self.assigned[a].expect("anchor is mapped before its dependents");
                self.target.adjacent[mapped]
                    .iter()
                    .map(|(t, _)| *t)
                    .collect()
            }
            None => (0..self.target.ids.len()).collect(),
        };
        for t in candidates {
            if self.used[t] {
                continue;
            }
            if !self.spend() {
                return;
            }
            if !self.compatible[q][t] {
                continue;
            }
            match self.edges_match(q, t) {
                None => return,
                Some(false) => continue,
                Some(true) => {}
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

/// Order query atoms so each one after a component's first has a mapped neighbor: prefer the
/// atom with the most mapped neighbors, then the fewest compatible target atoms, then the
/// highest degree. IDs only break exact ties, so renumbering rarely changes the work done.
fn plan_order(query: &Graph, candidates: &[usize]) -> Vec<(usize, Option<usize>)> {
    let n = query.ids.len();
    let mut position: Vec<Option<usize>> = vec![None; n];
    let mut plan: Vec<(usize, Option<usize>)> = Vec::with_capacity(n);
    while plan.len() < n {
        let next = (0..n)
            .filter(|&i| position[i].is_none())
            .min_by_key(|&i| {
                let mapped = query.adjacent[i]
                    .iter()
                    .filter(|(j, _)| position[*j].is_some())
                    .count();
                (
                    std::cmp::Reverse(mapped),
                    candidates[i],
                    std::cmp::Reverse(query.adjacent[i].len()),
                    query.ids[i],
                )
            })
            .expect("an unplaced atom remains");
        let anchor = query.adjacent[next]
            .iter()
            .filter_map(|(j, _)| position[*j].map(|at| (at, *j)))
            .min()
            .map(|(_, j)| j);
        position[next] = Some(plan.len());
        plan.push((next, anchor));
    }
    plan
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

fn validate_query(
    query: &Graph,
    predicates: &[AtomPredicate],
    match_limit: usize,
    state_limit: usize,
) -> Result<(), QueryError> {
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
        let Some(index) = query.ids.iter().position(|id| *id == p.atom) else {
            return Err(QueryError::InvalidPredicate);
        };
        // An element query atom already fixes the target charge, so a different charge
        // condition could never be met and is refused rather than reported as "no match".
        let contradictory = p.charge.is_some_and(|charge| {
            matches!(query.atoms[index].kind, AtomKind::Element(_))
                && charge != query.atoms[index].charge
        });
        if !seen.insert(p.atom)
            || p.min_neighbors
                .zip(p.max_neighbors)
                .is_some_and(|(a, b)| a > b)
            || p.charge
                .is_some_and(|q| !(MIN_CHARGE..=MAX_CHARGE).contains(&q))
            || contradictory
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
    Ok(())
}

/// A neutral ring nitrogen with inferred hydrogens and only single bonds is read by the
/// aromaticity model as pyridine-type (no H). If reading it as N-H (pyrrole-type) would make
/// its ring system aromatic, the answer depends on an unrecorded hydrogen, so refuse.
fn refuse_ambiguous_ring_nitrogen(
    target: &MoleculeGraph,
    interpretation: &aromaticity::Interpretation,
) -> Result<(), QueryError> {
    let suspects: BTreeSet<AtomId> = target
        .atoms()
        .filter(|(id, atom)| {
            atom.kind.element().is_some_and(|e| e.symbol() == "N")
                && atom.charge == 0
                && atom.hydrogens == Hydrogens::Inferred
                && atom.radical == Radical::None
                && {
                    let incident: Vec<_> = target
                        .bonds()
                        .filter(|(_, b)| b.begin == *id || b.end == *id)
                        .collect();
                    incident.len() == 2
                        && incident.iter().all(|(_, b)| b.order == BondOrder::Single)
                }
        })
        .map(|(id, _)| id)
        .collect();
    let flagged: Vec<_> = interpretation
        .systems
        .iter()
        .filter(|system| {
            system.classification == aromaticity::Classification::NonAromatic
                && system.reason == Some("not_conjugated")
                && system.atoms.iter().any(|id| suspects.contains(id))
        })
        .collect();
    if flagged.is_empty() {
        return Ok(());
    }
    let as_amine = target.with_hydrogens(&suspects, Hydrogens::Specified(1));
    let reread = aromaticity::perceive(&as_amine);
    // The N-H reading must reproduce the same non-aromatic outcome. An aromatic result, or an
    // unsupported one (fused systems containing a heteroatom are outside the model) for a
    // ring system that has a double bond, means the answer depends on an unrecorded hydrogen.
    // A fully saturated system cannot be aromatic under either reading.
    let ambiguous = flagged.iter().any(|system| {
        let has_ring_double = target
            .bonds()
            .any(|(id, bond)| system.bonds.contains(&id) && bond.order == BondOrder::Double);
        match reread
            .systems
            .iter()
            .find(|other| other.atoms == system.atoms)
        {
            None => true,
            Some(other) => match other.classification {
                aromaticity::Classification::NonAromatic => false,
                aromaticity::Classification::Aromatic => true,
                aromaticity::Classification::Unsupported => has_ring_double,
            },
        }
    });
    if ambiguous {
        return Err(QueryError::AromaticTargetUnsupported(
            "ambiguous_ring_nitrogen_hydrogen",
        ));
    }
    Ok(())
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
    let query_graph = Graph::new(query);
    validate_query(&query_graph, predicates, match_limit, state_limit)?;
    let target_graph = Graph::new(target);
    if target_graph.atoms.iter().any(|a| {
        a.kind.element().is_none()
            || a.radical == Radical::Unknown
            || a.hydrogens == Hydrogens::Unknown
            || !a.attachment_points.is_empty()
    }) || target_graph
        .adjacent
        .iter()
        .flatten()
        .any(|(_, b)| matches!(b.order, BondOrder::Unknown | BondOrder::Query(_)))
    {
        return Err(QueryError::UnsupportedTarget);
    }
    // Perception is the costliest step, so it runs only after every cheap query-side and
    // target-state check has passed and those errors are not masked by a target failure.
    let mut perceived_aromatic = BTreeSet::new();
    let mut kekule = BTreeMap::new();
    if model == MatchModel::AromaticTarget {
        let interpretation = aromaticity::perceive(target);
        kekule = interpretation
            .kekulize()
            .map_err(QueryError::AromaticTargetUnsupported)?
            .into_iter()
            .collect();
        let bonds: BTreeSet<_> = interpretation
            .aromatize()
            .map_err(QueryError::AromaticTargetUnsupported)?
            .into_iter()
            .collect();
        refuse_ambiguous_ring_nitrogen(target, &interpretation)?;
        perceived_aromatic = target
            .bonds()
            .filter_map(|(id, bond)| {
                bonds
                    .contains(&id)
                    .then_some((bond.begin.min(bond.end), bond.begin.max(bond.end)))
            })
            .collect();
    }
    let facts = TargetFacts {
        in_ring: ring_membership(target, &target_graph.ids),
        hydrogens: target_hydrogens(target, &target_graph.ids, &kekule),
        perceived_aromatic,
    };
    let by_atom: BTreeMap<AtomId, &AtomPredicate> =
        predicates.iter().map(|p| (p.atom, p)).collect();
    let mut compatible = vec![vec![false; target_graph.ids.len()]; query_graph.ids.len()];
    let mut undetermined = false;
    for (q, row) in compatible.iter_mut().enumerate() {
        for (t, slot) in row.iter_mut().enumerate() {
            match atom_matches(
                query_graph.atoms[q],
                target_graph.atoms[t],
                by_atom.get(&query_graph.ids[q]).copied(),
                target_graph.adjacent[t].len(),
                facts.in_ring[t],
                facts.hydrogens[t],
            ) {
                Some(holds) => *slot = holds,
                None => undetermined = true,
            }
        }
    }
    if undetermined {
        return Err(QueryError::TargetHydrogensUndetermined);
    }
    let counts: Vec<usize> = compatible
        .iter()
        .map(|row| row.iter().filter(|holds| **holds).count())
        .collect();
    let plan = plan_order(&query_graph, &counts);
    let mut search = Search {
        assigned: vec![None; query_graph.ids.len()],
        used: vec![false; target_graph.ids.len()],
        plan,
        compatible,
        perceived_aromatic: facts.perceived_aromatic,
        query: query_graph,
        target: target_graph,
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
