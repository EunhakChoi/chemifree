//! The valence target the ring-aromaticity model reads for each supported element and charge,
//! and the refusal of an element the valence table lists but the model does not support.

use chemifree_domain::aromaticity::{self, Classification};
use chemifree_domain::chemistry::restricted_target;
use chemifree_domain::document::{
    Atom, AtomRef, BondOrder, Command, Document, DocumentId, NewAtomPositions, Point,
};
use chemifree_domain::element::Element;
use chemifree_domain::graph::MoleculeGraph;

/// Atoms are element symbols, and bonds are `(begin, end, order)` by atom index.
fn graph(atoms: &[&str], bonds: &[(usize, usize, BondOrder)]) -> MoleculeGraph {
    let mut document =
        Document::new(DocumentId::parse("valence-targets").unwrap(), "targets").unwrap();
    let mut commands: Vec<Command> = atoms
        .iter()
        .map(|symbol| Command::AddAtom {
            atom: Atom::from(Element::from_symbol(symbol).unwrap()),
            positions: NewAtomPositions::Single(Point::new(0.0, 0.0).unwrap()),
            bond_to: None,
        })
        .collect();
    commands.extend(bonds.iter().map(|(begin, end, order)| Command::AddBond {
        begin: AtomRef::Created(*begin),
        end: AtomRef::Created(*end),
        order: order.clone(),
    }));
    document.apply(&commands).unwrap();
    MoleculeGraph::from_document(&document)
}

#[test]
fn restricted_targets_are_the_contract_atom_roles() {
    // CONTRACT "Atom role": neutral C 4, C+ and C- 3, neutral N 3, N+ 4, N- 2, neutral O and S 2.
    let expected = [
        ("C", 0, 4),
        ("C", 1, 3),
        ("C", -1, 3),
        ("N", 0, 3),
        ("N", 1, 4),
        ("N", -1, 2),
        ("O", 0, 2),
        ("S", 0, 2),
    ];
    for (symbol, charge, target) in expected {
        assert_eq!(
            restricted_target(symbol, charge),
            Some(target),
            "{symbol} with charge {charge}"
        );
    }
}

#[test]
fn a_ring_with_phosphorus_is_refused_as_element_or_charge() {
    // The valence table lists phosphorus (`restricted_target` gives Some for P with charge 0),
    // but the ring model supports only C, N, O and S, so the ring is unsupported, never
    // classified. Kekulé spelling: P=C, C-C, C=C, C-C, C=C, C-P.
    let ring = graph(
        &["P", "C", "C", "C", "C", "C"],
        &[
            (0, 1, BondOrder::Double),
            (1, 2, BondOrder::Single),
            (2, 3, BondOrder::Double),
            (3, 4, BondOrder::Single),
            (4, 5, BondOrder::Double),
            (5, 0, BondOrder::Single),
        ],
    );
    let interpretation = aromaticity::perceive(&ring);
    assert_eq!(interpretation.systems.len(), 1);
    let system = &interpretation.systems[0];
    assert_eq!(system.classification, Classification::Unsupported);
    assert_eq!(system.reason, Some("element_or_charge"));
}
