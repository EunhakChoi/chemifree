//! Substructure matching of a query drawing in a target drawing.
//!
//! A *mapping* assigns every query atom a distinct target atom such that every query bond lies
//! on a target bond between the assigned atoms (a subgraph monomorphism: the target may have
//! more bonds between the assigned atoms than the query). Mappings are distinct when their
//! atom assignments differ, so a symmetric query is found once per symmetry operation:
//! benzene in benzene has twelve. The bond assignment follows from the atom assignment because
//! a drawing holds at most one bond between two atoms.
//!
//! What is compared:
//!
//! * atoms: the element, or the set a query atom stands for; a nonzero charge and an isotope
//!   that the query atom states must be equal in the target atom. Hydrogen counts, radicals,
//!   attachment points, positions and everything in the scene are not conditions. A drawn
//!   hydrogen atom is an ordinary atom and is never folded away.
//! * bonds: the *effective* kind. Bonds that [`aromaticity`] interprets as aromatic are
//!   aromatic whether they were drawn Kekulé or aromatic; every other bond keeps its drawn
//!   kind. A query bond is a single kind, any bond, or a list of kinds.
//!
//! Nothing is guessed. A query is refused when its aromaticity cannot be read; a target is
//! refused only if a mapping would depend on an atom or bond the model cannot interpret.
//! Reaching a limit is an error, never a short list.

use std::collections::BTreeMap;

use crate::aromaticity::{self, Classification};
use crate::document::{Atom, AtomId, Bond, BondId, BondOrder};
use crate::graph::MoleculeGraph;
use crate::states::{AtomKind, BondQuery, QueryAtom, QueryBondOrder};

/// The matching rules' identifier, reported with every result.
pub const MODEL_ID: &str = "chemifree-substructure-1";
/// The most atoms a query may have.
pub const MAX_QUERY_ATOMS: usize = 64;
/// The most candidate assignments one search tries. Each try of one target atom for one query
/// atom counts as a step, whether or not it succeeds.
pub const MAX_SEARCH_STEPS: usize = 2_000_000;
/// The most mappings one search returns; finding one more is a limit.
pub const MAX_MAPPINGS: usize = 1_000;

/// Which graph a refusal is about.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Side {
    Query,
    Target,
}

impl Side {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Query => "query",
            Self::Target => "target",
        }
    }
}

/// An atom or a bond of one of the graphs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Item {
    Atom(AtomId),
    Bond(BondId),
}

/// Why no answer is given. Each is a refusal or a limit, never an empty answer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatchError {
    /// The query has no atoms.
    EmptyQuery,
    /// The query has more than [`MAX_QUERY_ATOMS`] atoms.
    QueryTooLarge { atoms: usize },
    /// A query atom or bond that no atom or bond can satisfy.
    Unsatisfiable(Item),
    /// The matcher cannot read this atom or bond under its rules, and the answer depends on
    /// it. For the query this is raised before searching, naming `at`; for the target it is
    /// raised when a mapping would rest on `at`, and `query` names the query item compared
    /// with it.
    Unsupported {
        side: Side,
        reason: &'static str,
        at: Option<Item>,
        query: Option<Item>,
    },
    /// The search tried [`MAX_SEARCH_STEPS`] assignments without finishing.
    StepLimit,
    /// There are more than [`MAX_MAPPINGS`] mappings.
    MappingLimit,
}

impl MatchError {
    /// The stable machine-readable code of this refusal or limit.
    pub const fn code(self) -> &'static str {
        match self {
            Self::EmptyQuery => "empty_query",
            Self::QueryTooLarge { .. } => "query_too_large",
            Self::Unsatisfiable(_) => "query_unsatisfiable",
            Self::Unsupported { .. } => "match_unsupported",
            Self::StepLimit | Self::MappingLimit => "match_limit",
        }
    }
}

/// One way the query lies in the target.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Mapping {
    /// `(query atom, target atom)`, ordered by query atom.
    pub atoms: Vec<(AtomId, AtomId)>,
    /// `(query bond, target bond)`, ordered by query bond; each lies between the target atoms
    /// that its query bond's atoms were assigned.
    pub bonds: Vec<(BondId, BondId)>,
}

/// A finished search.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Matches {
    /// Every mapping, ordered by the target atoms assigned in query-atom order. Empty only
    /// when the query truly does not occur.
    pub mappings: Vec<Mapping>,
    /// Assignments tried; at most [`MAX_SEARCH_STEPS`].
    pub steps: usize,
}

/// Atomic numbers 1 to 118, one bit each.
const ELEMENTS: u128 = ((1_u128 << 119) - 1) & !1;
const HYDROGEN: u128 = 1 << 1;
const CARBON: u128 = 1 << 6;
/// Group 17: F, Cl, Br, I, At, Ts.
const HALOGENS: u128 = (1 << 9) | (1 << 17) | (1 << 35) | (1 << 53) | (1 << 85) | (1 << 117);

/// What an atom of the query asks of a target atom.
struct AtomTest {
    /// The atomic numbers allowed.
    elements: u128,
    charge: Option<i8>,
    isotope: Option<u16>,
}

/// The answer to one comparison: the matcher may be unable to decide.
#[derive(Clone, Copy, Eq, PartialEq)]
enum Verdict {
    Yes,
    No,
    Undecided(&'static str),
}

impl AtomTest {
    fn compile(id: AtomId, atom: &Atom) -> Result<Self, MatchError> {
        let elements = match &atom.kind {
            AtomKind::Element(element) => 1 << element.atomic_number(),
            AtomKind::Query(QueryAtom::Any) => ELEMENTS,
            AtomKind::Query(QueryAtom::AnyHeavy) => ELEMENTS & !HYDROGEN,
            AtomKind::Query(QueryAtom::Heteroatom) => ELEMENTS & !HYDROGEN & !CARBON,
            AtomKind::Query(QueryAtom::Halogen) => HALOGENS,
            AtomKind::Query(QueryAtom::List { elements, negated }) => {
                let listed = elements
                    .iter()
                    .fold(0_u128, |bits, element| bits | 1 << element.atomic_number());
                if *negated { ELEMENTS & !listed } else { listed }
            }
        };
        if elements == 0 {
            return Err(MatchError::Unsatisfiable(Item::Atom(id)));
        }
        let is_element = atom.kind.element().is_some();
        Ok(Self {
            elements,
            charge: (is_element && atom.charge != 0).then_some(atom.charge),
            isotope: atom.isotope.filter(|_| is_element),
        })
    }

    /// True for a test that every atom satisfies, including an atom that is itself a query.
    fn accepts_everything(&self) -> bool {
        self.elements == ELEMENTS && self.charge.is_none() && self.isotope.is_none()
    }

    fn check(&self, target: &Atom) -> Verdict {
        let Some(element) = target.kind.element() else {
            return if self.accepts_everything() {
                Verdict::Yes
            } else {
                Verdict::Undecided("target_query_atom")
            };
        };
        let allowed = self.elements >> element.atomic_number() & 1 == 1
            && self.charge.is_none_or(|charge| charge == target.charge)
            && self.isotope.is_none_or(|mass| Some(mass) == target.isotope);
        if allowed { Verdict::Yes } else { Verdict::No }
    }
}

/// What a bond of the query asks of a target bond.
struct BondTest {
    /// `None` accepts every bond; otherwise the bonds whose effective kind is listed.
    kinds: Option<u8>,
}

/// The kind of a bond as the matcher reads it.
#[derive(Clone, Copy, Eq, PartialEq)]
enum Kind {
    Single,
    Double,
    Triple,
    Aromatic,
    Dative,
}

impl Kind {
    const fn bit(self) -> u8 {
        1 << self as u8
    }
}

const fn query_kind(order: QueryBondOrder) -> Kind {
    match order {
        QueryBondOrder::Single => Kind::Single,
        QueryBondOrder::Double => Kind::Double,
        QueryBondOrder::Triple => Kind::Triple,
        QueryBondOrder::Aromatic => Kind::Aromatic,
    }
}

/// A bond's effective kind, or why the matcher cannot say.
#[derive(Clone, Copy, Eq, PartialEq)]
enum Effective {
    Known(Kind),
    Unclassified(&'static str),
}

impl BondTest {
    fn check(&self, target: Effective) -> Verdict {
        match (self.kinds, target) {
            (None, _) => Verdict::Yes,
            (Some(mask), Effective::Known(kind)) => {
                if mask & kind.bit() != 0 {
                    Verdict::Yes
                } else {
                    Verdict::No
                }
            }
            (Some(_), Effective::Unclassified(reason)) => Verdict::Undecided(reason),
        }
    }
}

/// The effective kind of every bond of `graph`, in the graph's bond order.
///
/// A ring system is read by [`aromaticity`]: an aromatic system makes all of its bonds
/// aromatic, and a bond drawn aromatic must lie in one. A system the model cannot decide
/// leaves its bonds unclassified, unless it has no double or aromatic bond: then no spelling
/// of it can be aromatic and the drawn kinds stand.
fn effective_kinds(graph: &MoleculeGraph) -> Vec<Effective> {
    let interpretation = aromaticity::perceive(graph);
    let mut system_of: BTreeMap<BondId, usize> = BTreeMap::new();
    for (index, system) in interpretation.systems.iter().enumerate() {
        for &bond in &system.bonds {
            system_of.insert(bond, index);
        }
    }
    let orders: BTreeMap<BondId, &BondOrder> =
        graph.bonds().map(|(id, b)| (id, &b.order)).collect();
    let has_pi: Vec<bool> = interpretation
        .systems
        .iter()
        .map(|system| {
            system
                .bonds
                .iter()
                .any(|bond| matches!(orders[bond], BondOrder::Double | BondOrder::Aromatic))
        })
        .collect();
    graph
        .bonds()
        .map(|(id, bond)| {
            let system = system_of
                .get(&id)
                .map(|&index| (&interpretation.systems[index], has_pi[index]));
            if system.is_some_and(|(s, _)| s.classification == Classification::Aromatic) {
                return Effective::Known(Kind::Aromatic);
            }
            let reason = system.map_or("aromatic_bond_outside_ring", |(s, _)| {
                s.reason.unwrap_or("not_aromatic")
            });
            if bond.order == BondOrder::Aromatic {
                return Effective::Unclassified(reason);
            }
            if system.is_some_and(|(s, pi)| s.classification == Classification::Unsupported && pi) {
                return Effective::Unclassified(reason);
            }
            match bond.order {
                BondOrder::Single => Effective::Known(Kind::Single),
                BondOrder::Double => Effective::Known(Kind::Double),
                BondOrder::Triple => Effective::Known(Kind::Triple),
                BondOrder::Dative => Effective::Known(Kind::Dative),
                BondOrder::Unknown => Effective::Unclassified("unknown_bond"),
                BondOrder::Query(_) => Effective::Unclassified("query_bond"),
                // Handled above: a bond drawn aromatic is aromatic only inside an aromatic system.
                BondOrder::Aromatic => Effective::Unclassified(reason),
            }
        })
        .collect()
}

fn compile_bond(id: BondId, bond: &Bond, effective: Effective) -> Result<BondTest, MatchError> {
    match &bond.order {
        BondOrder::Query(BondQuery::Any) => Ok(BondTest { kinds: None }),
        BondOrder::Query(BondQuery::OneOf(list)) => {
            let mask = list
                .iter()
                .fold(0_u8, |bits, &order| bits | query_kind(order).bit());
            if mask == 0 {
                Err(MatchError::Unsatisfiable(Item::Bond(id)))
            } else {
                Ok(BondTest { kinds: Some(mask) })
            }
        }
        _ => match effective {
            Effective::Known(Kind::Dative) => Err(unsupported_query("query_dative_bond", id)),
            Effective::Known(kind) => Ok(BondTest {
                kinds: Some(kind.bit()),
            }),
            Effective::Unclassified(reason) => Err(unsupported_query(reason, id)),
        },
    }
}

fn unsupported_query(reason: &'static str, bond: BondId) -> MatchError {
    MatchError::Unsupported {
        side: Side::Query,
        reason,
        at: Some(Item::Bond(bond)),
        query: None,
    }
}

/// The query, read once.
struct Query {
    atoms: Vec<(AtomId, AtomTest)>,
    /// `(id, begin, end, test)` with atom indices.
    bonds: Vec<(BondId, usize, usize, BondTest)>,
    /// Per atom: `(neighbor, bond)` indices.
    adjacency: Vec<Vec<(usize, usize)>>,
}

impl Query {
    fn read(graph: &MoleculeGraph) -> Result<Self, MatchError> {
        let count = graph.atoms().len();
        if count == 0 {
            return Err(MatchError::EmptyQuery);
        }
        if count > MAX_QUERY_ATOMS {
            return Err(MatchError::QueryTooLarge { atoms: count });
        }
        let atoms = graph
            .atoms()
            .map(|(id, atom)| AtomTest::compile(id, atom).map(|test| (id, test)))
            .collect::<Result<Vec<_>, _>>()?;
        let index: BTreeMap<AtomId, usize> = atoms
            .iter()
            .enumerate()
            .map(|(i, (id, _))| (*id, i))
            .collect();
        let effective = effective_kinds(graph);
        let mut bonds = Vec::new();
        let mut adjacency = vec![Vec::new(); atoms.len()];
        for ((id, bond), effective) in graph.bonds().zip(effective) {
            let test = compile_bond(id, bond, effective)?;
            let (a, b) = endpoints(&index, bond);
            adjacency[a].push((b, bonds.len()));
            adjacency[b].push((a, bonds.len()));
            bonds.push((id, a, b, test));
        }
        Ok(Self {
            atoms,
            bonds,
            adjacency,
        })
    }
}

fn endpoints(index: &BTreeMap<AtomId, usize>, bond: &Bond) -> (usize, usize) {
    (
        *index
            .get(&bond.begin)
            .expect("a graph's bonds join its atoms"),
        *index
            .get(&bond.end)
            .expect("a graph's bonds join its atoms"),
    )
}

/// The target, read once.
struct Target<'g> {
    atoms: Vec<(AtomId, &'g Atom)>,
    bond_ids: Vec<BondId>,
    effective: Vec<Effective>,
    /// Per atom: `(neighbor, bond)` indices, ordered by neighbor.
    adjacency: Vec<Vec<(usize, usize)>>,
    /// Atom indices by atomic number.
    by_element: Vec<Vec<usize>>,
    /// Atoms that are themselves queries.
    queries: Vec<usize>,
}

impl<'g> Target<'g> {
    fn read(graph: &'g MoleculeGraph) -> Self {
        let atoms: Vec<_> = graph.atoms().collect();
        let index: BTreeMap<AtomId, usize> = atoms
            .iter()
            .enumerate()
            .map(|(i, (id, _))| (*id, i))
            .collect();
        let mut adjacency = vec![Vec::new(); atoms.len()];
        let mut bond_ids = Vec::new();
        for (id, bond) in graph.bonds() {
            let (a, b) = endpoints(&index, bond);
            adjacency[a].push((b, bond_ids.len()));
            adjacency[b].push((a, bond_ids.len()));
            bond_ids.push(id);
        }
        for neighbors in &mut adjacency {
            neighbors.sort_unstable();
        }
        let mut by_element = vec![Vec::new(); 128];
        let mut queries = Vec::new();
        for (i, (_, atom)) in atoms.iter().enumerate() {
            match atom.kind.element() {
                Some(element) => by_element[usize::from(element.atomic_number())].push(i),
                None => queries.push(i),
            }
        }
        Self {
            atoms,
            bond_ids,
            effective: effective_kinds(graph),
            adjacency,
            by_element,
            queries,
        }
    }

    /// The bond between two atoms, if any.
    fn bond_between(&self, a: usize, b: usize) -> Option<usize> {
        let neighbors = &self.adjacency[a];
        neighbors
            .binary_search_by_key(&b, |&(neighbor, _)| neighbor)
            .ok()
            .map(|at| neighbors[at].1)
    }

    /// Atoms a query atom could be assigned, by its own test and its number of bonds. An atom
    /// whose test cannot be decided is kept.
    fn candidates(&self, test: &AtomTest, degree: usize) -> Vec<usize> {
        let mut found: Vec<usize> = Vec::new();
        for number in 1..=118_u8 {
            if test.elements >> number & 1 == 1 {
                found.extend(&self.by_element[usize::from(number)]);
            }
        }
        found.extend(&self.queries);
        found.retain(|&t| {
            self.adjacency[t].len() >= degree && test.check(self.atoms[t].1) != Verdict::No
        });
        found
    }
}

/// How the search assigns one query atom.
struct Step {
    atom: usize,
    /// Earlier-assigned query neighbors and the query bond to each.
    back: Vec<(usize, usize)>,
}

/// An assignment order that keeps each atom next to an assigned one when the query allows,
/// and starts each component from its most constrained atom.
fn plan(query: &Query, candidates: &[Vec<usize>]) -> Vec<Step> {
    let count = query.atoms.len();
    let mut placed = vec![false; count];
    let mut steps = Vec::with_capacity(count);
    while steps.len() < count {
        let connected = |atom: usize| {
            query.adjacency[atom]
                .iter()
                .filter(|&&(neighbor, _)| placed[neighbor])
                .count()
        };
        let next = (0..count)
            .filter(|&atom| !placed[atom])
            .min_by_key(|&atom| {
                (
                    connected(atom) == 0,
                    candidates[atom].len(),
                    std::cmp::Reverse(connected(atom)),
                    atom,
                )
            })
            .expect("an unplaced atom remains while steps are missing");
        let back = query.adjacency[next]
            .iter()
            .filter(|&&(neighbor, _)| placed[neighbor])
            .copied()
            .collect();
        placed[next] = true;
        steps.push(Step { atom: next, back });
    }
    steps
}

struct Search<'a> {
    query: &'a Query,
    target: &'a Target<'a>,
    plan: &'a [Step],
    candidates: &'a [Vec<usize>],
    /// `allowed[query atom][target atom]`.
    allowed: &'a [Vec<bool>],
    assigned: Vec<usize>,
    used: Vec<bool>,
    steps: usize,
    mappings: Vec<Mapping>,
}

const UNASSIGNED: usize = usize::MAX;

impl Search<'_> {
    fn extend(&mut self, depth: usize) -> Result<(), MatchError> {
        let Some(step) = self.plan.get(depth) else {
            return self.record();
        };
        let (query, target) = (self.query, self.target);
        let atom = step.atom;
        // Candidates come from the neighbors of an assigned atom when there is one; only the
        // first atom of a component draws on the whole prefilter list.
        let anchor = step
            .back
            .first()
            .map(|&(earlier, _)| self.assigned[earlier]);
        let count = anchor.map_or(self.candidates[atom].len(), |a| target.adjacency[a].len());
        for at in 0..count {
            let t = anchor.map_or_else(|| self.candidates[atom][at], |a| target.adjacency[a][at].0);
            self.steps += 1;
            if self.steps > MAX_SEARCH_STEPS {
                return Err(MatchError::StepLimit);
            }
            if self.used[t] || !self.allowed[atom][t] {
                continue;
            }
            let fits = step.back.iter().all(|&(earlier, query_bond)| {
                target
                    .bond_between(t, self.assigned[earlier])
                    .is_some_and(|bond| {
                        query.bonds[query_bond].3.check(target.effective[bond]) != Verdict::No
                    })
            });
            if !fits {
                continue;
            }
            self.assigned[atom] = t;
            self.used[t] = true;
            let result = self.extend(depth + 1);
            self.used[t] = false;
            self.assigned[atom] = UNASSIGNED;
            result?;
        }
        Ok(())
    }

    /// Keeps a complete assignment, unless some comparison it rests on was undecided.
    fn record(&mut self) -> Result<(), MatchError> {
        let (query, target) = (self.query, self.target);
        for (q, (query_id, test)) in query.atoms.iter().enumerate() {
            let (target_id, atom) = target.atoms[self.assigned[q]];
            if let Verdict::Undecided(reason) = test.check(atom) {
                return Err(undecided(
                    reason,
                    Item::Atom(target_id),
                    Item::Atom(*query_id),
                ));
            }
        }
        let mut bonds = Vec::with_capacity(query.bonds.len());
        for (query_id, a, b, test) in &query.bonds {
            let bond = target
                .bond_between(self.assigned[*a], self.assigned[*b])
                .expect("every query bond was matched while assigning");
            if let Verdict::Undecided(reason) = test.check(target.effective[bond]) {
                return Err(undecided(
                    reason,
                    Item::Bond(target.bond_ids[bond]),
                    Item::Bond(*query_id),
                ));
            }
            bonds.push((*query_id, target.bond_ids[bond]));
        }
        if self.mappings.len() == MAX_MAPPINGS {
            return Err(MatchError::MappingLimit);
        }
        let mut atoms: Vec<_> = query
            .atoms
            .iter()
            .enumerate()
            .map(|(q, (id, _))| (*id, target.atoms[self.assigned[q]].0))
            .collect();
        atoms.sort_unstable();
        bonds.sort_unstable();
        self.mappings.push(Mapping { atoms, bonds });
        Ok(())
    }
}

fn undecided(reason: &'static str, at: Item, query: Item) -> MatchError {
    MatchError::Unsupported {
        side: Side::Target,
        reason,
        at: Some(at),
        query: Some(query),
    }
}

/// Finds every mapping of `query` into `target`.
///
/// # Errors
///
/// A [`MatchError`] when the query is empty, too large, cannot be satisfied or cannot be read;
/// when a mapping would rest on a target atom or bond that cannot be read; or when a limit is
/// reached. No error comes with a partial list.
pub fn find(query: &MoleculeGraph, target: &MoleculeGraph) -> Result<Matches, MatchError> {
    let query = Query::read(query)?;
    let target = Target::read(target);
    let candidates: Vec<Vec<usize>> = query
        .atoms
        .iter()
        .enumerate()
        .map(|(q, (_, test))| target.candidates(test, query.adjacency[q].len()))
        .collect();
    if candidates.iter().any(Vec::is_empty) {
        return Ok(Matches {
            mappings: Vec::new(),
            steps: 0,
        });
    }
    let allowed: Vec<Vec<bool>> = candidates
        .iter()
        .map(|list| {
            let mut row = vec![false; target.atoms.len()];
            for &t in list {
                row[t] = true;
            }
            row
        })
        .collect();
    let plan = plan(&query, &candidates);
    let mut search = Search {
        query: &query,
        target: &target,
        plan: &plan,
        candidates: &candidates,
        allowed: &allowed,
        assigned: vec![UNASSIGNED; query.atoms.len()],
        used: vec![false; target.atoms.len()],
        steps: 0,
        mappings: Vec::new(),
    };
    search.extend(0)?;
    let Search {
        steps,
        mut mappings,
        ..
    } = search;
    mappings.sort_unstable_by(|a, b| {
        a.atoms
            .iter()
            .map(|pair| pair.1)
            .cmp(b.atoms.iter().map(|pair| pair.1))
    });
    Ok(Matches { mappings, steps })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::element::Element;

    #[test]
    fn the_element_bits_are_atomic_numbers_one_to_118() {
        assert_eq!(ELEMENTS.count_ones(), 118);
        assert_eq!(ELEMENTS & 1, 0, "there is no element 0");
        assert_eq!(ELEMENTS >> 118 & 1, 1, "oganesson is element 118");
        assert_eq!(ELEMENTS >> 119, 0, "there is no element 119");
    }

    #[test]
    fn the_halogen_bits_are_group_17() {
        let symbols: Vec<&str> = (1..=118_u8)
            .filter(|&number| HALOGENS >> number & 1 == 1)
            .map(|number| Element::from_atomic_number(number).unwrap().symbol())
            .collect();
        assert_eq!(symbols, ["F", "Cl", "Br", "I", "At", "Ts"]);
    }
}
