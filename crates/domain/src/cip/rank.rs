//! Comparison of two ligands by Rules 1a, 1b and 2, each over the whole digraph.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use crate::budget::{Interrupt, Interrupted};

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

const RULES: [Rule; 3] = [Rule::R1a, Rule::R1b, Rule::R2];

/// What one rule reads of a node; a missing node reads as a phantom.
#[derive(Clone, Copy)]
struct View {
    z: u8,
    mass: Mass,
    duplicate_of_depth: Option<u16>,
    mancude: bool,
}

const PHANTOM: View = View {
    z: 0,
    mass: Mass::Number(0),
    duplicate_of_depth: None,
    mancude: false,
};

/// `Greater` when `a` ranks higher than `b` under `rule`.
fn compare_views(rule: Rule, a: View, b: View) -> Result<Ordering, Reason> {
    match rule {
        Rule::R1a => Ok(a.z.cmp(&b.z)),
        Rule::R1b => {
            if a.mancude || b.mancude {
                return Err(Reason::MancudeRule1b);
            }
            Ok(match (a.duplicate_of_depth, b.duplicate_of_depth) {
                // Nearer the root ranks higher. Only two duplicates are compared with each
                // other; a duplicate and an ordinary atom tie.
                (Some(a_depth), Some(b_depth)) => b_depth.cmp(&a_depth),
                _ => Ordering::Equal,
            })
        }
        Rule::R2 => match (a.mass, b.mass) {
            (Mass::Number(a_mass), Mass::Number(b_mass)) => Ok(a_mass.cmp(&b_mass)),
            (Mass::NaturalUnlisted, Mass::NaturalUnlisted) => Ok(Ordering::Equal),
            _ => Err(Reason::MassNumber),
        },
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

pub(super) struct Engine<'c, 'g> {
    pub(super) arena: Arena<'c, 'g>,
    interrupt: &'c dyn Interrupt,
    cap: usize,
    compared: BTreeMap<(NodeIx, NodeIx, Rule), (Ordering, Option<Rule>)>,
    sorted: BTreeMap<(NodeIx, Rule), Vec<NodeIx>>,
}

impl<'c, 'g> Engine<'c, 'g> {
    pub(super) fn new(
        context: &'c Context<'g>,
        center: crate::document::AtomId,
        cap: usize,
        interrupt: &'c dyn Interrupt,
    ) -> Self {
        Self {
            arena: Arena::new(context, center),
            interrupt,
            cap,
            compared: BTreeMap::new(),
            sorted: BTreeMap::new(),
        }
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
    /// phantom. The interrupt and the node cap are checked once per sphere.
    fn compare_by(&mut self, a: NodeIx, b: NodeIx, rule: Rule) -> Result<Ordering, Halt> {
        let mut left = vec![vec![a]];
        let mut right = vec![vec![b]];
        loop {
            for (l, r) in left.iter().zip(&right) {
                for position in 0..l.len().max(r.len()) {
                    let ordering = compare_views(
                        rule,
                        self.view(l.get(position).copied()),
                        self.view(r.get(position).copied()),
                    )?;
                    if ordering != Ordering::Equal {
                        return Ok(ordering);
                    }
                }
            }
            self.interrupt.check()?;
            if self.arena.len() > self.cap {
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
        let children = self.arena.children(node)?;
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
