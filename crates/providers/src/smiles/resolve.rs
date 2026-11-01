//! What the shared aromaticity and valence models say about a graph, computed once and used
//! by the reader (radicals, hydrogens) and by both writers (hydrogens, aromatic atoms).

use std::collections::{BTreeMap, BTreeSet};

use chemifree_domain::aromaticity::{self, Classification, KekuleOrder};
use chemifree_domain::chemistry::{self, AtomAssessment};
use chemifree_domain::document::{AtomId, BondId, BondOrder};
use chemifree_domain::graph::MoleculeGraph;

/// The aromaticity model could not read a system that holds a recorded aromatic bond.
#[derive(Debug)]
pub(super) struct AromaticFailure {
    pub reason: &'static str,
    /// The atoms of the failing system, so a caller can point inside it. Empty when the model
    /// failed without naming a system.
    pub atoms: Vec<AtomId>,
}

pub(super) struct Resolution {
    /// Every bond of an aromatic system, however it is recorded.
    pub aromatic_systems: BTreeSet<BondId>,
    /// The Kekulé order of each recorded aromatic bond.
    pub assignment: BTreeMap<BondId, KekuleOrder>,
}

impl Resolution {
    /// The valence contribution of each bond around each atom, recorded aromatic bonds read
    /// through the Kekulé assignment.
    pub(super) fn incident_orders(
        &self,
        graph: &MoleculeGraph,
    ) -> BTreeMap<AtomId, Vec<Option<u16>>> {
        chemistry::incident_orders(
            graph.atoms().map(|(id, _)| id),
            graph.bonds(),
            |id, bond| aromaticity::kekule_order_value(&self.assignment, id, bond),
        )
    }

    pub(super) fn assess(&self, graph: &MoleculeGraph) -> BTreeMap<AtomId, AtomAssessment> {
        let orders = self.incident_orders(graph);
        graph
            .atoms()
            .map(|(id, atom)| (id, chemistry::assess(id, atom, &orders[&id])))
            .collect()
    }
}

/// Reads the aromatic systems of `graph`. A graph without recorded aromatic bonds always
/// resolves; one with them resolves only when each is in a system the model calls aromatic.
pub(super) fn resolve(graph: &MoleculeGraph) -> Result<Resolution, AromaticFailure> {
    let has_aromatic = graph
        .bonds()
        .any(|(_, bond)| bond.order == BondOrder::Aromatic);
    let interpretation = aromaticity::perceive(graph);
    let recorded: BTreeSet<BondId> = graph
        .bonds()
        .filter(|(_, bond)| bond.order == BondOrder::Aromatic)
        .map(|(id, _)| id)
        .collect();
    let mut aromatic_systems = BTreeSet::new();
    for system in &interpretation.systems {
        if system.classification == Classification::Aromatic {
            aromatic_systems.extend(system.bonds.iter().copied());
        } else if system.bonds.iter().any(|id| recorded.contains(id)) {
            return Err(AromaticFailure {
                reason: system.reason.unwrap_or("not_aromatic"),
                atoms: system.atoms.clone(),
            });
        }
    }
    if !has_aromatic {
        return Ok(Resolution {
            aromatic_systems,
            assignment: BTreeMap::new(),
        });
    }
    match interpretation.kekulize() {
        Ok(assignment) => Ok(Resolution {
            aromatic_systems,
            assignment: assignment.into_iter().collect(),
        }),
        Err(reason) => Err(AromaticFailure {
            reason,
            atoms: Vec::new(),
        }),
    }
}
