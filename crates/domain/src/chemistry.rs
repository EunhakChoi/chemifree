//! A bounded, read-only interpretation of integral covalent valence.
//!
//! Recorded atom and bond states remain authoritative. This policy cannot infer a numeric
//! result for an unknown bond, a query, an aromatic representation, or an unlisted state.

use std::collections::BTreeMap;

use crate::aromaticity::KekuleOrder;
use crate::document::{Atom, AtomId, BondId, BondOrder, Document};
use crate::graph::MoleculeGraph;
use crate::states::{Hydrogens, Radical};

pub const VALENCE_MODEL: &str = "chemifree-valence-2";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Finding {
    Valid,
    Invalid,
    Unsupported,
}

impl Finding {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Valid => "valid",
            Self::Invalid => "invalid",
            Self::Unsupported => "unsupported",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AtomAssessment {
    pub atom: AtomId,
    pub bond_order_sum: Option<u16>,
    /// Additional, non-vertex hydrogens. A drawn H atom contributes a bond instead.
    pub hydrogen_count: Option<u16>,
    pub finding: Finding,
    pub reason: Option<&'static str>,
}

/// Explicit, versioned working table; it is not a universal definition of valence.
fn targets(symbol: &str, charge: i8) -> Option<&'static [u16]> {
    match (symbol, charge) {
        ("H", 0) => Some(&[1]),
        ("H", -1 | 1) | ("F" | "Cl" | "Br" | "I", -1) => Some(&[0]),
        ("B", 0) => Some(&[3]),
        ("B", -1) => Some(&[4]),
        ("B", 1) => Some(&[2]),
        ("C", 0) => Some(&[4]),
        ("C", -1 | 1) => Some(&[3]),
        ("N" | "P", 0) => Some(&[3, 5]),
        ("N", -1) => Some(&[2]),
        ("N", 1) => Some(&[4]),
        ("O", 0) => Some(&[2]),
        ("O", -1) => Some(&[1]),
        ("O", 1) => Some(&[3]),
        ("S", 0) => Some(&[2, 4, 6]),
        ("F" | "Cl" | "Br" | "I", 0) => Some(&[1]),
        _ => None,
    }
}

pub(crate) fn assess(id: AtomId, atom: &Atom, orders: &[Option<u16>]) -> AtomAssessment {
    let sum = orders
        .iter()
        .copied()
        .collect::<Option<Vec<_>>>()
        .map(|v| v.into_iter().sum());
    assess_sum(id, atom, sum)
}

fn assess_sum(id: AtomId, atom: &Atom, sum: Option<u16>) -> AtomAssessment {
    let stated = match atom.hydrogens {
        Hydrogens::Specified(n) => Some(u16::from(n)),
        _ => None,
    };
    let mut result = AtomAssessment {
        atom: id,
        bond_order_sum: sum,
        hydrogen_count: stated,
        finding: Finding::Unsupported,
        reason: None,
    };
    let Some(element) = atom.kind.element() else {
        result.reason = Some("query_atom");
        return result;
    };
    if !atom.attachment_points.is_empty() {
        result.reason = Some("attachment_point");
        return result;
    }
    if atom.hydrogens == Hydrogens::Unknown {
        result.reason = Some("unknown_hydrogens");
        return result;
    }
    let Some(allowed) = targets(element.symbol(), atom.charge) else {
        result.reason = Some("element_or_charge");
        return result;
    };
    let reduction = match (element.symbol(), atom.charge, atom.radical) {
        (_, _, Radical::None) => 0,
        ("H" | "C" | "N" | "O", 0, Radical::Doublet) => 1,
        ("C" | "N" | "O", 0, Radical::Singlet | Radical::Triplet) => 2,
        _ => {
            result.reason = Some("radical_state");
            return result;
        }
    };
    let Some(sum) = sum else {
        result.reason = Some("non_integral_or_unknown_bond");
        return result;
    };
    let adjusted: Vec<_> = allowed
        .iter()
        .filter_map(|v| v.checked_sub(reduction))
        .collect();
    let total = sum + stated.unwrap_or(0);
    if atom.hydrogens == Hydrogens::Inferred && element.symbol() != "H" {
        if let Some(target) = adjusted.iter().copied().find(|target| *target >= total) {
            result.hydrogen_count = Some(target - total);
            result.finding = Finding::Valid;
        } else {
            result.hydrogen_count = Some(0);
            result.finding = Finding::Invalid;
            result.reason = Some("unusual_valence");
        }
    } else {
        result.hydrogen_count = Some(stated.unwrap_or(0));
        if adjusted.contains(&total) {
            result.finding = Finding::Valid;
        } else {
            result.finding = Finding::Invalid;
            result.reason = Some("unusual_valence");
        }
    }
    result
}

/// The integral bond order the valence policy counts for a recorded bond, or `None` when it
/// cannot. An aromatic bond counts only with the Kekulé order a caller obtained from the
/// aromaticity model.
pub(crate) fn valence_order(order: &BondOrder, kekule: Option<KekuleOrder>) -> Option<u16> {
    match (order, kekule) {
        (BondOrder::Single, _) | (BondOrder::Aromatic, Some(KekuleOrder::Single)) => Some(1),
        (BondOrder::Double, _) | (BondOrder::Aromatic, Some(KekuleOrder::Double)) => Some(2),
        (BondOrder::Triple, _) => Some(3),
        (BondOrder::Aromatic | BondOrder::Dative | BondOrder::Query(_) | BondOrder::Unknown, _) => {
            None
        }
    }
}

fn collect_orders<'a>(
    atoms: impl Iterator<Item = AtomId>,
    bonds: impl Iterator<Item = (BondId, AtomId, AtomId, &'a BondOrder)>,
    kekule: &BTreeMap<BondId, KekuleOrder>,
) -> BTreeMap<AtomId, Vec<Option<u16>>> {
    let mut incident: BTreeMap<_, Vec<Option<u16>>> = atoms.map(|id| (id, Vec::new())).collect();
    for (id, begin, end, order) in bonds {
        let order = valence_order(order, kekule.get(&id).copied());
        incident
            .get_mut(&begin)
            .expect("graph validates endpoints")
            .push(order);
        incident
            .get_mut(&end)
            .expect("graph validates endpoints")
            .push(order);
    }
    incident
}

/// Inspect a single immutable document snapshot; never changes recorded atoms, history or IDs.
pub fn inspect_valence(document: &Document) -> Vec<AtomAssessment> {
    let incident = collect_orders(
        document.atoms().map(|(id, _)| id),
        document
            .bonds()
            .map(|(id, bond)| (id, bond.begin, bond.end, &bond.order)),
        &BTreeMap::new(),
    );
    document
        .atoms()
        .map(|(id, atom)| assess(id, atom, &incident[&id]))
        .collect()
}

/// Assess every atom of a graph snapshot, counting each aromatic bond with the Kekulé order in
/// `kekule` (an aromatic bond without one makes its atoms unsupported).
pub fn assess_graph(
    graph: &MoleculeGraph,
    kekule: &BTreeMap<BondId, KekuleOrder>,
) -> BTreeMap<AtomId, AtomAssessment> {
    let incident = collect_orders(
        graph.atoms().map(|(id, _)| id),
        graph
            .bonds()
            .map(|(id, bond)| (id, bond.begin, bond.end, &bond.order)),
        kekule,
    );
    graph
        .atoms()
        .map(|(id, atom)| (id, assess(id, atom, &incident[&id])))
        .collect()
}

/// What the shared valence model says about the radical state of an atom whose hydrogens and
/// bond orders are stated, as a SMILES bracket atom states them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RadicalInference {
    /// The state is determined: a closed shell (or a valence the model finds unusual, which
    /// is a valence finding and not a radical) or a doublet.
    Determined(Radical),
    /// The state is not determined, with the reason.
    Undetermined(&'static str),
}

/// Infer the radical state of `atom` (whose own radical field is ignored) from its stated
/// hydrogens and the sum of the integral bond orders around it, using the same table as [`assess`].
/// A doublet needs one fewer bond order than the closed shell; singlet and triplet both need
/// two fewer and cannot be told apart from the connectivity.
pub fn infer_radical(atom: &Atom, bond_order_sum: Option<u16>) -> RadicalInference {
    let probe = |radical| {
        let mut candidate = atom.clone();
        candidate.radical = radical;
        assess_sum(
            AtomId::from_serial(1).expect("1 is a valid serial"),
            &candidate,
            bond_order_sum,
        )
    };
    let closed = probe(Radical::None);
    if closed.finding == Finding::Valid {
        return RadicalInference::Determined(Radical::None);
    }
    match closed.reason {
        Some("element_or_charge") => {
            return RadicalInference::Undetermined("outside_valence_model");
        }
        Some("non_integral_or_unknown_bond") => {
            return RadicalInference::Undetermined("bond_order_not_integral");
        }
        Some("unusual_valence") => {}
        _ => return RadicalInference::Undetermined("outside_valence_model"),
    }
    if probe(Radical::Doublet).finding == Finding::Valid {
        return RadicalInference::Determined(Radical::Doublet);
    }
    if probe(Radical::Singlet).finding == Finding::Valid {
        return RadicalInference::Undetermined("singlet_or_triplet");
    }
    RadicalInference::Determined(Radical::None)
}

/// The hydrogens a SMILES reader infers for an unbracketed atom of `element` whose bonds sum to
/// `bond_order_sum`, under the same table as [`assess`]; `None` when the model has no answer.
pub fn inferred_hydrogens(element: crate::element::Element, bond_order_sum: u16) -> Option<u16> {
    let atom = Atom::new(crate::states::AtomKind::Element(element));
    let state = assess_sum(
        AtomId::from_serial(1).expect("1 is a valid serial"),
        &atom,
        Some(bond_order_sum),
    );
    (state.finding == Finding::Valid)
        .then_some(state.hydrogen_count)
        .flatten()
}
