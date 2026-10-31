//! Topological ring systems, independent of drawing order and chemical interpretation.
//!
//! A system is a biconnected edge block containing a cycle. Fused and bridged cycles
//! share a block; rings meeting at only one atom (spiro) do not. This deliberately does
//! not select a chemically privileged smallest-cycle basis.

use std::collections::{BTreeMap, BTreeSet};

use crate::document::{AtomId, BondId};
use crate::graph::MoleculeGraph;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RingSystem {
    pub atoms: Vec<AtomId>,
    pub bonds: Vec<BondId>,
    /// The dimension of the cycle space; it is not a unique count of chemical rings.
    pub cycle_rank: usize,
}

struct Frame {
    vertex: usize,
    parent_edge: Option<usize>,
    next: usize,
}

/// Identify cyclic blocks of the stored graph without changing it. All drawn bond kinds
/// participate in this purely topological result; aromaticity is a separate interpretation.
pub fn perceive(document: &MoleculeGraph) -> Vec<RingSystem> {
    let atoms: Vec<_> = document.atoms().map(|(id, _)| id).collect();
    let indices: BTreeMap<_, _> = atoms
        .iter()
        .copied()
        .enumerate()
        .map(|(i, id)| (id, i))
        .collect();
    let edges: Vec<_> = document
        .bonds()
        .map(|(id, b)| (id, indices[&b.begin], indices[&b.end]))
        .collect();
    let mut adjacent = vec![Vec::new(); atoms.len()];
    for (edge, &(_, a, b)) in edges.iter().enumerate() {
        adjacent[a].push((b, edge));
        adjacent[b].push((a, edge));
    }
    let mut discovery = vec![0; atoms.len()];
    let mut low = vec![0; atoms.len()];
    let mut time = 0;
    let mut edge_stack = Vec::new();
    let mut systems = Vec::new();
    for root in 0..atoms.len() {
        if discovery[root] != 0 {
            continue;
        }
        time += 1;
        discovery[root] = time;
        low[root] = time;
        let mut stack = vec![Frame {
            vertex: root,
            parent_edge: None,
            next: 0,
        }];
        while let Some(frame) = stack.last_mut() {
            let v = frame.vertex;
            if frame.next < adjacent[v].len() {
                let (other, edge) = adjacent[v][frame.next];
                frame.next += 1;
                if Some(edge) == frame.parent_edge {
                    continue;
                }
                if discovery[other] == 0 {
                    edge_stack.push(edge);
                    time += 1;
                    discovery[other] = time;
                    low[other] = time;
                    stack.push(Frame {
                        vertex: other,
                        parent_edge: Some(edge),
                        next: 0,
                    });
                } else if discovery[other] < discovery[v] {
                    edge_stack.push(edge);
                    low[v] = low[v].min(discovery[other]);
                }
            } else {
                let done = stack.pop().expect("frame exists");
                if let (Some(edge), Some(parent)) = (done.parent_edge, stack.last()) {
                    let p = parent.vertex;
                    low[p] = low[p].min(low[v]);
                    if low[v] >= discovery[p] {
                        let mut block = Vec::new();
                        loop {
                            let popped = edge_stack.pop().expect("tree edge is on the stack");
                            block.push(popped);
                            if popped == edge {
                                break;
                            }
                        }
                        let mut vertices = BTreeSet::new();
                        for &i in &block {
                            vertices.insert(atoms[edges[i].1]);
                            vertices.insert(atoms[edges[i].2]);
                        }
                        if block.len() >= vertices.len() {
                            let rank = block.len() - vertices.len() + 1;
                            let mut bond_ids: Vec<_> =
                                block.into_iter().map(|i| edges[i].0).collect();
                            bond_ids.sort_unstable();
                            systems.push(RingSystem {
                                atoms: vertices.into_iter().collect(),
                                bonds: bond_ids,
                                cycle_rank: rank,
                            });
                        }
                    }
                }
            }
        }
    }
    systems.sort_unstable_by(|a, b| a.bonds.cmp(&b.bonds));
    systems
}
