use chemifree_domain::document::{AtomRef, Command, Document, DocumentId, NewAtomPositions, Point};
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::smiles::{self, CanonicalError};

fn graph(input: &str, reverse: bool) -> MoleculeGraph {
    let source = smiles::parse(input).unwrap();
    let mut document =
        Document::new(DocumentId::parse("smiles-canonical").unwrap(), "test").unwrap();
    let indices: Vec<_> = if reverse {
        (0..source.atoms.len()).rev().collect()
    } else {
        (0..source.atoms.len()).collect()
    };
    let mut positions = vec![0; indices.len()];
    for (new, old) in indices.iter().copied().enumerate() {
        positions[old] = new;
    }
    let mut commands: Vec<_> = indices
        .iter()
        .map(|&old| Command::AddAtom {
            atom: source.atoms[old].atom.clone(),
            positions: NewAtomPositions::Single(Point::new(0.0, 0.0).unwrap()),
            bond_to: None,
        })
        .collect();
    commands.extend(source.bonds.iter().rev().map(|bond| Command::AddBond {
        begin: AtomRef::Created(positions[bond.begin]),
        end: AtomRef::Created(positions[bond.end]),
        order: bond.order.clone(),
    }));
    document.apply(&commands).unwrap();
    MoleculeGraph::from_document(&document)
}

#[test]
fn canonical_output_is_independent_of_document_and_bond_insertion_order() {
    for input in ["CC(O)C", "c1ccccc1", "c1ccc2ccccc2c1", "[13CH3-].[NH4+]"] {
        let first = smiles::write_canonical(&graph(input, false)).unwrap();
        let reordered = smiles::write_canonical(&graph(input, true)).unwrap();
        assert_eq!(first, reordered, "{input}");
        assert!(smiles::parse(&first).is_ok(), "{input}: {first}");
    }
}

#[test]
fn chemical_normalization_matches_aromatic_and_explicit_hydrogen_encodings() {
    for (first, second) in [
        ("c1ccccc1", "C1=CC=CC=C1"),
        ("c1ccc2ccccc2c1", "C1=CC2=CC=CC=C2C=C1"),
        ("C", "C([H])([H])([H])[H]"),
        ("N", "N([H])([H])[H]"),
    ] {
        assert_eq!(
            smiles::write_canonical(&graph(first, false)).unwrap(),
            smiles::write_canonical(&graph(second, true)).unwrap(),
            "{first} vs {second}"
        );
    }
    assert_ne!(
        smiles::write_canonical(&graph("C", false)).unwrap(),
        smiles::write_canonical(&graph("[13CH4]", false)).unwrap()
    );
}

#[test]
fn canonical_writer_refuses_unproved_states_without_an_output() {
    let graph = graph("[CH3]", false);
    assert!(matches!(
        smiles::write_canonical(&graph),
        Err(CanonicalError::Identity(_))
    ));
}
