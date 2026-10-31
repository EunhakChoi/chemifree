//! A bounded, read-only interpretation of integral covalent valence.
//!
//! Recorded atom and bond states remain authoritative. This policy cannot infer a numeric
//! result for an unknown bond, a query, an aromatic representation, or an unlisted state.

use std::collections::BTreeMap;

use crate::document::{Atom, AtomId, BondOrder, Document};
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

fn assess(id: AtomId, atom: &Atom, orders: &[Option<u16>]) -> AtomAssessment {
    let sum = orders
        .iter()
        .copied()
        .collect::<Option<Vec<_>>>()
        .map(|v| v.into_iter().sum());
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

/// Inspect a single immutable document snapshot; never changes recorded atoms, history or IDs.
pub fn inspect_valence(document: &Document) -> Vec<AtomAssessment> {
    let mut incident: BTreeMap<_, Vec<Option<u16>>> =
        document.atoms().map(|(id, _)| (id, Vec::new())).collect();
    for (_, bond) in document.bonds() {
        let order = match bond.order {
            BondOrder::Single => Some(1),
            BondOrder::Double => Some(2),
            BondOrder::Triple => Some(3),
            BondOrder::Aromatic | BondOrder::Dative | BondOrder::Query(_) | BondOrder::Unknown => {
                None
            }
        };
        incident
            .get_mut(&bond.begin)
            .expect("document validates endpoints")
            .push(order);
        incident
            .get_mut(&bond.end)
            .expect("document validates endpoints")
            .push(order);
    }
    document
        .atoms()
        .map(|(id, atom)| assess(id, atom, &incident[&id]))
        .collect()
}
