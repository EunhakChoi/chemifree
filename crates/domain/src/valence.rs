//! A bounded, versioned valence and hydrogen interpretation of a drawing graph.
//!
//! The policy is deliberately independent of document mutation. It uses integral covalent
//! bond orders and a small set of ordinary valences, with explicit indeterminate results for
//! inputs for which that model cannot make a reliable assignment.

use std::collections::BTreeMap;

use crate::document::{Atom, AtomId, BondOrder, Document};
use crate::states::{AtomKind, Hydrogens, Radical};

pub const MODEL_ID: &str = "chemifree-valence-1";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Status {
    Valid,
    Invalid,
    Unsupported,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AtomValence {
    pub atom_id: AtomId,
    /// Sum of incident drawn covalent bond orders; vertex H atoms are included here.
    pub bond_order_sum: Option<u16>,
    /// Non-vertex hydrogens, stated or inferred. `None` means not determined.
    pub hydrogens: Option<u16>,
    pub status: Status,
    pub reason: Option<&'static str>,
}

#[derive(Default)]
struct Bonds {
    sum: u16,
    unsupported: bool,
}

/// Inspect every atom in a snapshot. This neither changes the graph nor assigns aromaticity.
pub fn inspect(document: &Document) -> Vec<AtomValence> {
    let mut bonds: BTreeMap<AtomId, Bonds> = document
        .atoms()
        .map(|(id, _)| (id, Bonds::default()))
        .collect();
    for (_, bond) in document.bonds() {
        let order = match bond.order {
            BondOrder::Single => Some(1),
            BondOrder::Double => Some(2),
            BondOrder::Triple => Some(3),
            BondOrder::Aromatic | BondOrder::Dative | BondOrder::Query(_) | BondOrder::Unknown => {
                None
            }
        };
        for id in [bond.begin, bond.end] {
            let entry = bonds
                .get_mut(&id)
                .expect("document bonds have valid endpoints");
            if let Some(order) = order {
                entry.sum += order;
            } else {
                entry.unsupported = true;
            }
        }
    }
    document
        .atoms()
        .map(|(id, atom)| inspect_atom(id, atom, &bonds[&id]))
        .collect()
}

fn inspect_atom(atom_id: AtomId, atom: &Atom, bonds: &Bonds) -> AtomValence {
    let stated = match atom.hydrogens {
        Hydrogens::Specified(count) => Some(u16::from(count)),
        Hydrogens::Inferred | Hydrogens::Unknown => None,
    };
    let mut result = AtomValence {
        atom_id,
        bond_order_sum: (!bonds.unsupported).then_some(bonds.sum),
        hydrogens: stated,
        status: Status::Unsupported,
        reason: None,
    };
    let unsupported = |result: &mut AtomValence, reason| {
        result.reason = Some(reason);
    };
    let AtomKind::Element(element) = atom.kind else {
        unsupported(&mut result, "query_atom");
        return result;
    };
    if bonds.unsupported {
        unsupported(&mut result, "non_covalent_or_ambiguous_bond");
        return result;
    }
    if !atom.attachment_points.is_empty() {
        unsupported(&mut result, "attachment_point");
        return result;
    }
    if atom.hydrogens == Hydrogens::Unknown {
        unsupported(&mut result, "unknown_hydrogens");
        return result;
    }
    let Some(normal) = normal_valences(element.symbol(), atom.charge) else {
        unsupported(&mut result, "element_or_charge");
        return result;
    };
    let radical_reduction = match atom.radical {
        Radical::None => 0,
        // A doublet consumes one valence, a singlet or triplet two, under this explicitly
        // bounded Lewis model. Other elements/charges need a different electronic model.
        Radical::Doublet
            if atom.charge == 0 && matches!(element.symbol(), "H" | "C" | "N" | "O") =>
        {
            1
        }
        Radical::Singlet | Radical::Triplet
            if atom.charge == 0 && matches!(element.symbol(), "C" | "N" | "O") =>
        {
            2
        }
        _ => {
            unsupported(&mut result, "radical_state");
            return result;
        }
    };
    let allowed: Vec<u16> = normal
        .iter()
        .filter_map(|v| v.checked_sub(radical_reduction))
        .collect();
    if allowed.is_empty() {
        unsupported(&mut result, "radical_state");
        return result;
    }
    let current = bonds.sum + stated.unwrap_or(0);
    if atom.hydrogens == Hydrogens::Inferred && element.symbol() != "H" {
        if let Some(target) = allowed.iter().copied().find(|&target| target >= current) {
            result.hydrogens = Some(target - current);
            result.status = Status::Valid;
            return result;
        }
        result.hydrogens = Some(0);
    } else if atom.hydrogens == Hydrogens::Inferred {
        // A hydrogen vertex is an atom, not an implicit H2 molecule.
        result.hydrogens = Some(0);
    }
    if allowed.contains(&current) {
        result.status = Status::Valid;
    } else {
        result.status = Status::Invalid;
        result.reason = Some("unusual_valence");
    }
    result
}

/// The table is a policy, not a universal definition of chemically possible valence.
/// Neutral organic-subset values follow OpenSMILES 1.0; selected charged values use common
/// Lewis valences. Charged halogens in this model are free halide ions only.
fn normal_valences(symbol: &str, charge: i8) -> Option<&'static [u16]> {
    match (symbol, charge) {
        ("H", 0) => Some(&[1]),
        ("H", -1 | 1) => Some(&[0]),
        ("B", -1) => Some(&[4]),
        ("B", 0) => Some(&[3]),
        ("B", 1) => Some(&[2]),
        ("C", -1 | 1) => Some(&[3]),
        ("C", 0) => Some(&[4]),
        ("N", -1) => Some(&[2]),
        ("N", 0) => Some(&[3, 5]),
        ("N", 1) => Some(&[4]),
        ("O", -1) => Some(&[1]),
        ("O", 0) => Some(&[2]),
        ("O", 1) => Some(&[3]),
        ("P", 0) => Some(&[3, 5]),
        ("S", 0) => Some(&[2, 4, 6]),
        ("F" | "Cl" | "Br" | "I", -1) => Some(&[0]),
        ("F" | "Cl" | "Br" | "I", 0) => Some(&[1]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::element::Element;

    fn evaluate(
        symbol: &str,
        charge: i8,
        radical: Radical,
        hydrogens: Hydrogens,
        sum: u16,
    ) -> AtomValence {
        let mut atom = Atom::from(Element::from_symbol(symbol).unwrap());
        atom.charge = charge;
        atom.radical = radical;
        atom.hydrogens = hydrogens;
        inspect_atom(
            AtomId::from_serial(1).unwrap(),
            &atom,
            &Bonds {
                sum,
                unsupported: false,
            },
        )
    }

    #[test]
    fn ordinary_charged_and_radical_examples_follow_the_declared_policy() {
        // The first three are the OpenSMILES organic subset examples; the remaining
        // Lewis examples exercise charges and specified electron states separately.
        for (symbol, charge, radical, bonds, expected) in [
            ("C", 0, Radical::None, 0, 4),
            ("N", 0, Radical::None, 0, 3),
            ("Cl", 0, Radical::None, 0, 1),
            ("N", 1, Radical::None, 4, 0),
            ("O", -1, Radical::None, 0, 1),
            ("Cl", -1, Radical::None, 0, 0),
            ("C", 0, Radical::Doublet, 0, 3),
            ("O", 0, Radical::Doublet, 0, 1),
            ("C", 0, Radical::Triplet, 0, 2),
        ] {
            let result = evaluate(symbol, charge, radical, Hydrogens::Inferred, bonds);
            assert_eq!(
                (result.status, result.hydrogens),
                (Status::Valid, Some(expected)),
                "{symbol} charge {charge} radical {radical:?}"
            );
        }
    }

    #[test]
    fn stated_hydrogens_are_checked_and_isotopes_do_not_change_valence() {
        let valid = evaluate("C", 0, Radical::None, Hydrogens::Specified(0), 4);
        assert_eq!((valid.status, valid.hydrogens), (Status::Valid, Some(0)));
        let over = evaluate("C", 0, Radical::None, Hydrogens::Specified(1), 4);
        assert_eq!(
            (over.status, over.hydrogens, over.reason),
            (Status::Invalid, Some(1), Some("unusual_valence"))
        );
        let mut carbon = Atom::from(Element::CARBON);
        let id = AtomId::from_serial(1).unwrap();
        let bonds = Bonds {
            sum: 1,
            unsupported: false,
        };
        let natural = inspect_atom(id, &carbon, &bonds);
        carbon.isotope = Some(13);
        assert_eq!(natural, inspect_atom(id, &carbon, &bonds));
    }

    #[test]
    fn uncertain_input_is_not_converted_to_a_numeric_guess() {
        for (symbol, charge, radical, hydrogens, reason) in [
            (
                "Fe",
                0,
                Radical::None,
                Hydrogens::Inferred,
                "element_or_charge",
            ),
            (
                "C",
                0,
                Radical::Unknown,
                Hydrogens::Inferred,
                "radical_state",
            ),
            (
                "C",
                0,
                Radical::None,
                Hydrogens::Unknown,
                "unknown_hydrogens",
            ),
        ] {
            let result = evaluate(symbol, charge, radical, hydrogens, 0);
            assert_eq!(
                (result.status, result.hydrogens, result.reason),
                (Status::Unsupported, None, Some(reason))
            );
        }
        let carbon = Atom::from(Element::CARBON);
        let result = inspect_atom(
            AtomId::from_serial(1).unwrap(),
            &carbon,
            &Bonds {
                sum: 1,
                unsupported: true,
            },
        );
        assert_eq!(
            (
                result.status,
                result.bond_order_sum,
                result.hydrogens,
                result.reason
            ),
            (
                Status::Unsupported,
                None,
                None,
                Some("non_covalent_or_ambiguous_bond")
            )
        );
    }
}
