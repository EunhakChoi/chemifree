//! Comparison of two ligands by Rules 1a, 1b, 2 and 3, each over the whole digraph.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use crate::budget::{Interrupt, Interrupted};
use crate::document::{AtomId, BondId};

use super::bond::{Geometry, Oracle};
use super::context::{Context, Mass};
use super::digraph::{Arena, Kind, NodeIx};
use super::{Reason, Rule};

/// Why a comparison did not finish.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Halt {
    Reason(Reason),
    Interrupted(Interrupted),
}

impl From<Reason> for Halt {
    fn from(reason: Reason) -> Self {
        Self::Reason(reason)
    }
}

impl From<Interrupted> for Halt {
    fn from(interrupted: Interrupted) -> Self {
        Self::Interrupted(interrupted)
    }
}

const RULES: [Rule; 4] = [Rule::R1a, Rule::R1b, Rule::R2, Rule::R3];

/// What one rule reads of a node; a missing node reads as a phantom.
#[derive(Clone, Copy)]
struct View {
    z: u8,
    mass: Mass,
    duplicate_of_depth: Option<u16>,
    mancude: bool,
    /// What Rule 3 reads: the descriptor of the double bond to the parent, if there is one.
    bond: Option<Geometry>,
}

const PHANTOM: View = View {
    z: 0,
    mass: Mass::Number(0),
    duplicate_of_depth: None,
    mancude: false,
    bond: None,
};

/// What a comparison of two nodes under one rule found.
struct Verdict {
    /// `Greater` when the first ranks higher.
    ordering: Ordering,
    /// The model cannot decide something the comparison touched; the order is a stand-in.
    refusal: Option<Reason>,
    /// The answer depends on the geometry of a double bond the document does not state; the
    /// order is a stand-in.
    ambiguous: bool,
}

impl Verdict {
    const fn plain(ordering: Ordering) -> Self {
        Self {
            ordering,
            refusal: None,
            ambiguous: false,
        }
    }
}

/// How Rule 3 ranks the double bonds into a node. `Z` above `E` above a bond that is not
/// stereogenic or no bond at all; an unstated geometry sits between `E` and the rest only as a
/// stand-in, because it is ambiguous whenever it is compared with something it could equal.
fn compare_bonds(a: Option<Geometry>, b: Option<Geometry>) -> Verdict {
    let key = |geometry: Option<Geometry>| match geometry {
        Some(Geometry::Z) => 3_u8,
        Some(Geometry::E) => 2,
        Some(Geometry::Uncertain { .. }) => 1,
        Some(Geometry::Plain) | None => 0,
    };
    // An unstated geometry is some Z or E, which outranks a bond that is not stereogenic, unless
    // its own ends were ambiguous (it may not be stereogenic either).
    let settled = |geometry: Option<Geometry>, other: Option<Geometry>| match geometry {
        Some(Geometry::Uncertain { maybe_plain }) => {
            !maybe_plain && matches!(other, Some(Geometry::Plain) | None)
        }
        _ => true,
    };
    let uncertain =
        |geometry: Option<Geometry>| matches!(geometry, Some(Geometry::Uncertain { .. }));
    Verdict {
        ordering: key(a).cmp(&key(b)),
        refusal: None,
        ambiguous: (uncertain(a) || uncertain(b)) && !(settled(a, b) && settled(b, a)),
    }
}

/// How Rule 2 orders two masses, and the refusal if the order is a reading this model does not
/// decide. The order is always a consistent one (a labeled `n` above an unlabeled atom of
/// table mass `n`, an unlabeled atom of an unlisted element below every labeled one), so that
/// the comparison can go on, and the refusal is reported for whoever asked: see
/// [`Engine::refuse`].
fn compare_masses(a: Mass, b: Mass) -> (Ordering, Option<Reason>) {
    // Doubled, so that a table mass sits between the labels just below and just above it.
    let key = |mass: Mass| match mass {
        Mass::NaturalUnlisted => 0_u32,
        Mass::Table(number) => 2 * u32::from(number),
        Mass::Number(number) => 2 * u32::from(number) + 1,
    };
    let undecided = match (a, b) {
        (Mass::NaturalUnlisted, Mass::Number(_)) | (Mass::Number(_), Mass::NaturalUnlisted) => true,
        (Mass::Table(table), Mass::Number(number)) | (Mass::Number(number), Mass::Table(table)) => {
            table == number
        }
        _ => false,
    };
    (key(a).cmp(&key(b)), undecided.then_some(Reason::MassNumber))
}

/// How `a` and `b` compare under `rule` (the order is a stand-in when the verdict refuses or is
/// ambiguous).
fn compare_views(rule: Rule, a: View, b: View) -> Verdict {
    match rule {
        Rule::R1a => Verdict::plain(a.z.cmp(&b.z)),
        Rule::R1b => {
            let refusal = (a.mancude || b.mancude).then_some(Reason::MancudeRule1b);
            let ordering = match (a.duplicate_of_depth, b.duplicate_of_depth) {
                // Nearer the root ranks higher. Only two duplicates are compared with each
                // other; a duplicate and an ordinary atom tie.
                (Some(a_depth), Some(b_depth)) => b_depth.cmp(&a_depth),
                _ => Ordering::Equal,
            };
            Verdict {
                ordering,
                refusal,
                ambiguous: false,
            }
        }
        Rule::R2 => {
            let (ordering, refusal) = compare_masses(a.mass, b.mass);
            Verdict {
                ordering,
                refusal,
                ambiguous: false,
            }
        }
        Rule::R3 => compare_bonds(a.bond, b.bond),
    }
}

/// Dense ranks (0 is highest) from the pairwise results `results[i][j]` (`Greater` when item
/// `i` ranks higher than item `j`), or `None` when the results are not a consistent order.
/// The ranks do not depend on the order the items are listed in.
pub(super) fn dense_ranks(results: &[Vec<Ordering>]) -> Option<Vec<usize>> {
    let scores: Vec<i32> = results
        .iter()
        .map(|row| {
            row.iter()
                .map(|result| match result {
                    Ordering::Greater => 1,
                    Ordering::Less => -1,
                    Ordering::Equal => 0,
                })
                .sum()
        })
        .collect();
    for (i, row) in results.iter().enumerate() {
        for (j, result) in row.iter().enumerate() {
            if i != j && *result != scores[i].cmp(&scores[j]) {
                return None;
            }
        }
    }
    Some(
        scores
            .iter()
            .map(|score| {
                let mut higher: Vec<i32> = scores.iter().copied().filter(|s| s > score).collect();
                higher.sort_unstable();
                higher.dedup();
                higher.len()
            })
            .collect(),
    )
}

/// The comparisons of the ligands of one pair, in one arena.
///
/// A refusal (an atom the model cannot expand, a comparison it cannot decide) does not stop
/// the comparison: the refused atom is read as having no children and the stand-in order of
/// [`compare_views`] is used, and the smallest reason is remembered in [`Engine::refusal`].
/// The comparison then finishes on a consistent, if wrong, digraph, so the nodes it
/// materialized and the reason it ends with are a function of the digraph alone, and not of
/// the order its neighbors were visited in (which follows atom identifiers). A caller that
/// finds a refusal discards the order. Only a limit or an interrupt stops a comparison early,
/// and the caller does not use how far it got.
pub(super) struct Engine<'c, 'g> {
    pub(super) arena: Arena<'c, 'g>,
    oracle: &'c Oracle<'c, 'g>,
    interrupt: &'c dyn Interrupt,
    cap: usize,
    refusal: Option<Reason>,
    ambiguous: bool,
    /// The double bonds whose descriptors Rule 3 asked for, and the part of their cost already
    /// counted against the cap.
    requested: BTreeSet<BondId>,
    charged: BTreeSet<BondId>,
    rule_3_nodes: usize,
    geometry: BTreeMap<NodeIx, Option<Geometry>>,
    compared: BTreeMap<(NodeIx, NodeIx, Rule), (Ordering, Option<Rule>)>,
    sorted: BTreeMap<(NodeIx, Rule), Vec<NodeIx>>,
}

impl<'c, 'g> Engine<'c, 'g> {
    pub(super) fn new(
        context: &'c Context<'g>,
        oracle: &'c Oracle<'c, 'g>,
        center: AtomId,
        cap: usize,
        spheres: usize,
        interrupt: &'c dyn Interrupt,
    ) -> Self {
        Self {
            arena: Arena::new(context, center, spheres),
            oracle,
            interrupt,
            cap,
            refusal: None,
            ambiguous: false,
            requested: BTreeSet::new(),
            charged: BTreeSet::new(),
            rule_3_nodes: 0,
            geometry: BTreeMap::new(),
            compared: BTreeMap::new(),
            sorted: BTreeMap::new(),
        }
    }

    /// The smallest reason a comparison of this engine met that the model cannot decide.
    pub(super) const fn refusal(&self) -> Option<Reason> {
        self.refusal
    }

    /// Whether a comparison depended on a ligand double bond whose geometry is not stated.
    pub(super) const fn ambiguous(&self) -> bool {
        self.ambiguous
    }

    /// The double bonds Rule 3 asked this engine's oracle about.
    pub(super) const fn requested(&self) -> &BTreeSet<BondId> {
        &self.requested
    }

    /// Nodes materialized: the arena, and every descriptor Rule 3 needed with what it needed.
    pub(super) fn nodes(&self) -> usize {
        self.arena
            .len()
            .saturating_add(self.oracle.closure_cost(&self.requested))
    }

    fn refuse(&mut self, reason: Reason) {
        self.refusal = Some(self.refusal.map_or(reason, |found| found.min(reason)));
    }

    fn view(&self, node: Option<NodeIx>) -> View {
        let Some(index) = node else { return PHANTOM };
        let node = self.arena.node(index);
        View {
            z: node.z,
            mass: node.mass,
            duplicate_of_depth: match node.kind {
                Kind::Duplicate { of_depth, .. } => Some(of_depth),
                _ => None,
            },
            mancude: node.kind == Kind::Mancude,
            bond: self.geometry.get(&index).copied().flatten(),
        }
    }

    /// The descriptor of the double bond into each node of these spheres (only a real node
    /// whose real parent is joined to it by a double bond has one), asked for before the
    /// spheres are compared so that which descriptors a comparison needs does not depend on the
    /// order it reaches them in.
    fn fetch_geometries(&mut self, spheres: [&[Vec<NodeIx>]; 2]) -> Result<(), Halt> {
        for &node in spheres
            .into_iter()
            .flat_map(|groups| groups.iter().flatten())
        {
            if self.geometry.contains_key(&node) {
                continue;
            }
            let found = self.node_geometry(node)?;
            self.geometry.insert(node, found);
        }
        Ok(())
    }

    fn node_geometry(&mut self, node: NodeIx) -> Result<Option<Geometry>, Halt> {
        let arena = &self.arena;
        let current = arena.node(node);
        let (Kind::Real(atom), Some(_), 2) = (current.kind, arena.parent(node), current.bond_in)
        else {
            return Ok(None);
        };
        let Some(bond) = arena.context().bond_between(arena.parent_atom(node), atom) else {
            return Ok(None);
        };
        self.requested.insert(bond);
        let analysis = self.oracle.analysis(bond)?;
        self.rule_3_nodes = self
            .rule_3_nodes
            .saturating_add(self.oracle.charge(bond, &mut self.charged));
        if self.arena.len().saturating_add(self.rule_3_nodes) > self.cap {
            return Err(Reason::DigraphLimit.into());
        }
        match self.oracle.geometry(bond, &analysis) {
            Ok(geometry) => Ok(Some(geometry)),
            Err(Reason::DigraphLimit) => Err(Reason::DigraphLimit.into()),
            Err(reason) => {
                self.refuse(reason);
                Ok(Some(Geometry::Plain))
            }
        }
    }

    /// Compares two ligands (or any two nodes) by the rules up to and including `upto`, in
    /// order: the first rule that separates them decides. `Greater` means `a` ranks higher; the
    /// rule is `None` for a tie.
    pub(super) fn compare(
        &mut self,
        a: NodeIx,
        b: NodeIx,
        upto: Rule,
    ) -> Result<(Ordering, Option<Rule>), Halt> {
        if let Some(found) = self.compared.get(&(a, b, upto)) {
            return Ok(*found);
        }
        let mut result = (Ordering::Equal, None);
        for rule in RULES.into_iter().take_while(|rule| *rule <= upto) {
            let ordering = self.compare_by(a, b, rule)?;
            if ordering != Ordering::Equal {
                result = (ordering, Some(rule));
                break;
            }
        }
        self.compared.insert((a, b, upto), result);
        self.compared
            .insert((b, a, upto), (result.0.reverse(), result.1));
        Ok(result)
    }

    /// One rule over the whole digraph, sphere by sphere. A sphere is a list of groups, one
    /// group per node of the previous sphere in its established order; a missing node is a
    /// phantom. The interrupt is checked at the start of every sphere, before it is compared,
    /// so a sphere that already decides cannot skip it; the node cap and the sphere bound
    /// ([`super::MAX_SPHERES`], applied when a node is expanded) before a sphere is expanded.
    fn compare_by(&mut self, a: NodeIx, b: NodeIx, rule: Rule) -> Result<Ordering, Halt> {
        let mut left = vec![vec![a]];
        let mut right = vec![vec![b]];
        loop {
            self.interrupt.check()?;
            if rule == Rule::R3 {
                self.fetch_geometries([&left, &right])?;
            }
            for (l, r) in left.iter().zip(&right) {
                for position in 0..l.len().max(r.len()) {
                    let verdict = compare_views(
                        rule,
                        self.view(l.get(position).copied()),
                        self.view(r.get(position).copied()),
                    );
                    if let Some(reason) = verdict.refusal {
                        self.refuse(reason);
                    }
                    self.ambiguous |= verdict.ambiguous;
                    if verdict.ordering != Ordering::Equal {
                        return Ok(verdict.ordering);
                    }
                }
            }
            if self.arena.len().saturating_add(self.rule_3_nodes) > self.cap {
                return Err(Reason::DigraphLimit.into());
            }
            let mut next_left = Vec::new();
            let mut next_right = Vec::new();
            for (l, r) in left.iter().zip(&right) {
                for position in 0..l.len().max(r.len()) {
                    for (side, list) in [(l, &mut next_left), (r, &mut next_right)] {
                        list.push(match side.get(position) {
                            Some(&node) => self.children_sorted(node, rule)?,
                            None => Vec::new(),
                        });
                    }
                }
            }
            if next_left.iter().chain(&next_right).all(Vec::is_empty) {
                return Ok(Ordering::Equal);
            }
            left = next_left;
            right = next_right;
        }
    }

    /// The children of `node`, best first under the rules up to `rule`. Children that tie have
    /// identical digraphs below them, so their relative order cannot change a comparison.
    fn children_sorted(&mut self, node: NodeIx, rule: Rule) -> Result<Vec<NodeIx>, Halt> {
        if let Some(found) = self.sorted.get(&(node, rule)) {
            return Ok(found.clone());
        }
        let children = match self.arena.children(node) {
            Ok(children) => children,
            Err(Reason::DigraphLimit) => return Err(Reason::DigraphLimit.into()),
            Err(reason) => {
                self.refuse(reason);
                Vec::new()
            }
        };
        let count = children.len();
        let mut order = children.clone();
        if count > 1 {
            let mut results = vec![vec![Ordering::Equal; count]; count];
            for i in 0..count {
                for j in (i + 1)..count {
                    let (ordering, _) = self.compare(children[i], children[j], rule)?;
                    results[i][j] = ordering;
                    results[j][i] = ordering.reverse();
                }
            }
            let ranks = dense_ranks(&results).ok_or(Reason::RankingInconsistent)?;
            let mut indices: Vec<usize> = (0..count).collect();
            indices.sort_by_key(|&i| ranks[i]);
            order = indices.into_iter().map(|i| children[i]).collect();
        }
        self.sorted.insert((node, rule), order.clone());
        Ok(order)
    }
}
