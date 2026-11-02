//! What the SMILES reader and writer ask of the shared valence and aromaticity models, in one
//! place so both sides read an atom's hydrogens and radical the same way.

use std::collections::{BTreeMap, BTreeSet};

use chemifree_domain::aromaticity::{self, Classification, Interpretation};
use chemifree_domain::chemistry::{self, AtomAssessment, Finding};
use chemifree_domain::document::{Atom, AtomId, BondId, BondOrder};
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::states::Radical;

/// Why aromatic bonds cannot be read or written: the first atom of the failing ring system, and
/// the aromaticity model's reason.
pub(super) struct AromaticFailure {
    pub atom: AtomId,
    pub reason: &'static str,
}

/// The aromaticity model's reading of a graph whose aromatic bonds are `aromatic`.
///
/// Every ring system holding an aromatic bond must be classified aromatic. An aromatic bond
/// that lies in no ring system, or in a system the model cannot classify, is refused with the
/// model's own reason.
pub(super) fn interpret(
    graph: &MoleculeGraph,
    aromatic: &BTreeSet<BondId>,
) -> Result<Interpretation, AromaticFailure> {
    let interpretation = aromaticity::perceive(graph);
    let mut covered = BTreeSet::new();
    for system in &interpretation.systems {
        covered.extend(system.bonds.iter().copied());
        if system.classification != Classification::Aromatic
            && system.bonds.iter().any(|bond| aromatic.contains(bond))
        {
            let atom = system
                .atoms
                .iter()
                .copied()
                .min()
                .expect("a ring system has atoms");
            return Err(AromaticFailure {
                atom,
                reason: system.reason.unwrap_or("not_aromatic"),
            });
        }
    }
    if let Some(bond) = aromatic.iter().find(|bond| !covered.contains(bond)) {
        let atom = graph
            .bonds()
            .find(|(id, _)| id == bond)
            .map(|(_, bond)| bond.begin)
            .expect("an aromatic bond belongs to the graph");
        return Err(AromaticFailure {
            atom,
            reason: "aromatic_bond_outside_ring",
        });
    }
    Ok(interpretation)
}

/// Every atom's assessment under the Kekulé assignment of the aromatic bonds, with the bond
/// orders each assessment used.
pub(super) struct Analysis {
    pub assessments: Vec<AtomAssessment>,
    pub orders: BTreeMap<AtomId, Vec<Option<u16>>>,
}

/// Assess every atom of `graph`. Aromatic bonds are given the assignment `interpretation`
/// finds; it must come from [`interpret`] on the same graph, and is `None` exactly when the
/// graph has no aromatic bond.
///
/// `interpret` has already required every ring system holding an aromatic bond to be
/// aromatic, so a Kekulé assignment exists for each. Should the model still find none, the
/// failure names the first atom of the first ring system that holds an aromatic bond.
pub(super) fn analyse(
    graph: &MoleculeGraph,
    interpretation: Option<&Interpretation>,
) -> Result<Analysis, AromaticFailure> {
    let assignment: BTreeMap<_, _> = match interpretation {
        Some(interpretation) => interpretation
            .kekulize()
            .map_err(|reason| AromaticFailure {
                atom: first_aromatic_atom(graph, interpretation),
                reason,
            })?
            .into_iter()
            .collect(),
        None => BTreeMap::new(),
    };
    let orders = chemistry::incident_orders(
        graph.atoms().map(|(id, _)| id),
        graph.bonds(),
        |id, bond| aromaticity::kekule_order_value(&assignment, id, bond),
    );
    let assessments = graph
        .atoms()
        .map(|(id, atom)| chemistry::assess(id, atom, &orders[&id]))
        .collect();
    Ok(Analysis {
        assessments,
        orders,
    })
}

/// The lowest atom of the first ring system that holds an aromatic bond of `graph`.
fn first_aromatic_atom(graph: &MoleculeGraph, interpretation: &Interpretation) -> AtomId {
    let aromatic: BTreeSet<BondId> = graph
        .bonds()
        .filter(|(_, bond)| bond.order == BondOrder::Aromatic)
        .map(|(id, _)| id)
        .collect();
    interpretation
        .systems
        .iter()
        .find(|system| system.bonds.iter().any(|bond| aromatic.contains(bond)))
        .and_then(|system| system.atoms.iter().copied().min())
        .or_else(|| graph.atoms().next().map(|(id, _)| id))
        .expect("a graph with an aromatic bond has atoms")
}

/// The one radical state that makes the atom's valence consistent, when exactly one does.
///
/// A normal valence wins over a radical. Singlet and triplet fill the same valences, so an atom
/// that only they could explain has no single state. An atom outside the valence model has
/// none either.
pub(super) fn determine_radical(
    id: AtomId,
    atom: &Atom,
    orders: &[Option<u16>],
) -> Option<Radical> {
    let mut probe = atom.clone();
    let mut valid = |radical: Radical| {
        probe.radical = radical;
        chemistry::assess(id, &probe, orders).finding == Finding::Valid
    };
    if valid(Radical::None) {
        return Some(Radical::None);
    }
    match (
        valid(Radical::Doublet),
        valid(Radical::Singlet),
        valid(Radical::Triplet),
    ) {
        (true, false, false) => Some(Radical::Doublet),
        (false, true, false) => Some(Radical::Singlet),
        (false, false, true) => Some(Radical::Triplet),
        _ => None,
    }
}

/// The kinds of bond SMILES text can state.
pub(super) fn is_writable_order(order: &BondOrder) -> bool {
    matches!(
        order,
        BondOrder::Single | BondOrder::Double | BondOrder::Triple | BondOrder::Aromatic
    )
}
