use chemifree_domain::aromaticity::{self, Classification, KekuleOrder};
use chemifree_domain::document::{
    Atom, AtomRef, BondOrder, Command, Document, DocumentId, NewAtomPositions, Point,
};
use chemifree_domain::element::Element;
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::states::Hydrogens;

fn graph(atoms: Vec<Atom>, edges: &[(usize, usize, BondOrder)]) -> Document {
    let mut d =
        Document::new(DocumentId::parse("aromatic-test").unwrap(), "Aromatic test").unwrap();
    let mut commands: Vec<_> = atoms
        .into_iter()
        .map(|atom| Command::AddAtom {
            atom,
            positions: NewAtomPositions::Single(Point::new(0.0, 0.0).unwrap()),
            bond_to: None,
        })
        .collect();
    commands.extend(edges.iter().map(|(a, b, order)| Command::AddBond {
        begin: AtomRef::Created(*a),
        end: AtomRef::Created(*b),
        order: order.clone(),
    }));
    d.apply(&commands).unwrap();
    d
}

fn atom(symbol: &str) -> Atom {
    Atom::from(Element::from_symbol(symbol).unwrap())
}
fn cycle(n: usize, order: impl Fn(usize) -> BondOrder) -> Vec<(usize, usize, BondOrder)> {
    (0..n).map(|i| (i, (i + 1) % n, order(i))).collect()
}

#[test]
fn benzene_aromatic_input_and_both_kekule_forms_have_six_electrons() {
    let aromatic = graph(vec![atom("C"); 6], &cycle(6, |_| BondOrder::Aromatic));
    let before = aromatic.clone();
    let result = aromaticity::perceive(&MoleculeGraph::from_document(&aromatic));
    assert_eq!(result.systems[0].classification, Classification::Aromatic);
    assert_eq!(result.systems[0].electrons, Some(6));
    let assignment = aromaticity::kekulize(&MoleculeGraph::from_document(&aromatic)).unwrap();
    assert_eq!(
        assignment
            .iter()
            .filter(|(_, order)| *order == KekuleOrder::Double)
            .count(),
        3
    );
    assert_eq!(aromatic, before);
    for start in [0, 1] {
        let kekule = graph(
            vec![atom("C"); 6],
            &cycle(6, |i| {
                if i % 2 == start {
                    BondOrder::Double
                } else {
                    BondOrder::Single
                }
            }),
        );
        let result = aromaticity::perceive(&MoleculeGraph::from_document(&kekule));
        assert_eq!(
            (
                result.systems[0].classification,
                result.systems[0].electrons
            ),
            (Classification::Aromatic, Some(6))
        );
        assert_eq!(result.aromatize().unwrap().len(), 6);
    }
}

#[test]
fn fused_naphthalene_is_one_ten_electron_system_and_cyclobutadiene_is_not_aromatic() {
    let edges = [
        (0, 1),
        (1, 2),
        (2, 3),
        (3, 4),
        (4, 5),
        (5, 0),
        (5, 6),
        (6, 7),
        (7, 8),
        (8, 9),
        (9, 4),
    ]
    .map(|(a, b)| (a, b, BondOrder::Aromatic));
    let naphthalene = graph(vec![atom("C"); 10], &edges);
    let result = aromaticity::perceive(&MoleculeGraph::from_document(&naphthalene));
    assert_eq!(result.systems.len(), 1);
    assert_eq!(
        (
            result.systems[0].classification,
            result.systems[0].electrons,
            result.systems[0].cycle_rank
        ),
        (Classification::Aromatic, Some(10), 2)
    );
    assert_eq!(
        aromaticity::kekulize(&MoleculeGraph::from_document(&naphthalene))
            .unwrap()
            .iter()
            .filter(|(_, o)| *o == KekuleOrder::Double)
            .count(),
        5
    );
    for doubles in [[0, 2, 6, 8, 10], [1, 3, 5, 7, 9]] {
        let explicit: Vec<_> = edges
            .iter()
            .enumerate()
            .map(|(i, (a, b, _))| {
                (
                    *a,
                    *b,
                    if doubles.contains(&i) {
                        BondOrder::Double
                    } else {
                        BondOrder::Single
                    },
                )
            })
            .collect();
        let document = graph(vec![atom("C"); 10], &explicit);
        let observed = aromaticity::perceive(&MoleculeGraph::from_document(&document));
        assert_eq!(
            (
                observed.systems[0].classification,
                observed.systems[0].electrons
            ),
            (Classification::Aromatic, Some(10))
        );
    }
    let renumbered: Vec<_> = edges
        .iter()
        .rev()
        .map(|(a, b, order)| ((a * 7) % 10, (b * 7) % 10, order.clone()))
        .collect();
    let permuted = graph(vec![atom("C"); 10], &renumbered);
    let observed = aromaticity::perceive(&MoleculeGraph::from_document(&permuted));
    assert_eq!(
        (
            observed.systems[0].classification,
            observed.systems[0].electrons,
            observed.systems[0].cycle_rank
        ),
        (Classification::Aromatic, Some(10), 2)
    );
    let square = graph(
        vec![atom("C"); 4],
        &cycle(4, |i| {
            if i % 2 == 0 {
                BondOrder::Double
            } else {
                BondOrder::Single
            }
        }),
    );
    let result = aromaticity::perceive(&MoleculeGraph::from_document(&square));
    assert_eq!(
        (
            result.systems[0].classification,
            result.systems[0].electrons
        ),
        (Classification::NonAromatic, Some(4))
    );
    let invalid = graph(vec![atom("C"); 4], &cycle(4, |_| BondOrder::Aromatic));
    let before = invalid.clone();
    assert!(aromaticity::kekulize(&MoleculeGraph::from_document(&invalid)).is_err());
    assert_eq!(invalid, before, "a failed conversion changes nothing");
}

#[test]
fn heteroatoms_and_charges_have_separate_electron_policies() {
    let mut pyridine = vec![atom("C"); 6];
    pyridine[0] = atom("N");
    let pyridine = graph(pyridine, &cycle(6, |_| BondOrder::Aromatic));
    assert_eq!(
        aromaticity::perceive(&MoleculeGraph::from_document(&pyridine)).systems[0].electrons,
        Some(6)
    );
    let mut pyrrole = vec![atom("C"); 5];
    pyrrole[0] = atom("N");
    pyrrole[0].hydrogens = Hydrogens::Specified(1);
    let pyrrole = graph(pyrrole, &cycle(5, |_| BondOrder::Aromatic));
    assert_eq!(
        aromaticity::perceive(&MoleculeGraph::from_document(&pyrrole)).systems[0].electrons,
        Some(6)
    );
    assert_eq!(
        aromaticity::kekulize(&MoleculeGraph::from_document(&pyrrole))
            .unwrap()
            .iter()
            .filter(|(_, o)| *o == KekuleOrder::Double)
            .count(),
        2
    );
    let mut cation = vec![atom("C"); 3];
    cation[0].charge = 1;
    let cation = graph(cation, &cycle(3, |_| BondOrder::Aromatic));
    assert_eq!(
        (
            aromaticity::perceive(&MoleculeGraph::from_document(&cation)).systems[0].classification,
            aromaticity::perceive(&MoleculeGraph::from_document(&cation)).systems[0].electrons
        ),
        (Classification::Aromatic, Some(2))
    );
}

#[test]
fn failed_second_component_returns_no_partial_assignment_and_preserves_input() {
    let mut edges = cycle(6, |_| BondOrder::Aromatic);
    edges.extend((0..3).map(|i| (6 + i, 6 + (i + 1) % 3, BondOrder::Aromatic)));
    let document = graph(vec![atom("C"); 9], &edges);
    let before = document.clone();
    assert_eq!(
        aromaticity::kekulize(&MoleculeGraph::from_document(&document)),
        Err("electron_count")
    );
    assert_eq!(document, before);
}

#[test]
fn mixed_or_unknown_bonds_are_unsupported_not_assumed_to_be_single() {
    let mut edges = cycle(6, |_| BondOrder::Aromatic);
    edges[0].2 = BondOrder::Unknown;
    let document = graph(vec![atom("C"); 6], &edges);
    let system = &aromaticity::perceive(&MoleculeGraph::from_document(&document)).systems[0];
    assert_eq!(
        (system.classification, system.electrons, system.reason),
        (
            Classification::Unsupported,
            None,
            Some("unsupported_ring_bond")
        )
    );
    assert_eq!(
        aromaticity::kekulize(&MoleculeGraph::from_document(&document)),
        Err("unsupported_ring_bond")
    );
    assert_eq!(
        aromaticity::perceive(&MoleculeGraph::from_document(&document)).aromatize(),
        Err("unsupported_ring_bond")
    );
}

#[test]
fn impossible_matching_and_resource_bound_never_produce_partial_assignments() {
    let mut atoms = vec![atom("C"); 7];
    for index in [0, 2, 4] {
        atoms[index] = atom("N");
        atoms[index].hydrogens = Hydrogens::Specified(1);
    }
    let impossible = graph(atoms, &cycle(7, |_| BondOrder::Aromatic));
    let snapshot = MoleculeGraph::from_document(&impossible);
    let interpreted = aromaticity::perceive(&snapshot);
    assert_eq!(
        (
            interpreted.systems[0].classification,
            interpreted.systems[0].reason
        ),
        (Classification::Unsupported, Some("no_kekule_assignment"))
    );
    assert_eq!(interpreted.kekulize(), Err("no_kekule_assignment"));
    let large = graph(vec![atom("C"); 129], &cycle(129, |_| BondOrder::Aromatic));
    let snapshot = MoleculeGraph::from_document(&large);
    let interpreted = aromaticity::perceive(&snapshot);
    assert_eq!(
        (
            interpreted.systems[0].classification,
            interpreted.systems[0].reason
        ),
        (Classification::Unsupported, Some("system_limit"))
    );
    assert_eq!(interpreted.kekulize(), Err("system_limit"));
}

#[test]
fn fused_benzenoid_pyrene_does_not_apply_single_ring_4n_plus_2_to_the_whole_system() {
    // Public pyrene connectivity: c1cc2ccc3cccc4ccc(c1)c2c34 (RCSB 8P0).
    let edges = [
        (0, 1),
        (1, 2),
        (2, 3),
        (3, 4),
        (4, 5),
        (5, 6),
        (6, 7),
        (7, 8),
        (8, 9),
        (9, 10),
        (10, 11),
        (11, 12),
        (12, 13),
        (13, 0),
        (12, 14),
        (14, 2),
        (14, 15),
        (15, 5),
        (15, 9),
    ]
    .map(|(a, b)| (a, b, BondOrder::Aromatic));
    let document = graph(vec![atom("C"); 16], &edges);
    let snapshot = MoleculeGraph::from_document(&document);
    let result = aromaticity::perceive(&snapshot);
    assert_eq!(result.systems.len(), 1);
    assert_eq!(
        (
            result.systems[0].classification,
            result.systems[0].electrons,
            result.systems[0].cycle_rank
        ),
        (Classification::Aromatic, Some(16), 4)
    );
    assert_eq!(
        result
            .kekulize()
            .unwrap()
            .iter()
            .filter(|(_, order)| *order == KekuleOrder::Double)
            .count(),
        8
    );
}

#[test]
fn fused_biphenylene_is_outside_the_benzenoid_model() {
    // Two six-membered rings share opposite edges of a central four-membered ring.
    let edges = [
        (0, 1),
        (1, 2),
        (2, 3),
        (3, 0),
        (0, 4),
        (4, 5),
        (5, 6),
        (6, 7),
        (7, 1),
        (2, 8),
        (8, 9),
        (9, 10),
        (10, 11),
        (11, 3),
    ]
    .map(|(a, b)| (a, b, BondOrder::Aromatic));
    let document = graph(vec![atom("C"); 12], &edges);
    let interpreted = aromaticity::perceive(&MoleculeGraph::from_document(&document));
    assert_eq!(
        (
            interpreted.systems[0].classification,
            interpreted.systems[0].reason
        ),
        (
            Classification::Unsupported,
            Some("fused_system_outside_model")
        )
    );
    assert_eq!(interpreted.kekulize(), Err("fused_system_outside_model"));
}
