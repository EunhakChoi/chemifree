use chemifree_domain::document::{
    Atom, AtomRef, BondOrder, Command, Document, DocumentId, NewAtomPositions, Point,
};
use chemifree_domain::element::Element;
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::identity::{self, IdentityError};
use chemifree_domain::states::{Hydrogens, Radical};
use std::collections::BTreeSet;

fn atom(symbol: &str) -> Atom {
    Atom::from(Element::from_symbol(symbol).unwrap())
}
fn graph(atoms: Vec<Atom>, edges: &[(usize, usize, BondOrder)]) -> MoleculeGraph {
    let mut d =
        Document::new(DocumentId::parse("identity-test").unwrap(), "Identity test").unwrap();
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
    MoleculeGraph::from_document(&d)
}
fn cycle(n: usize) -> Vec<(usize, usize, BondOrder)> {
    (0..n)
        .map(|i| (i, (i + 1) % n, BondOrder::Aromatic))
        .collect()
}

#[test]
fn benzene_renumbering_has_identical_code_and_one_six_atom_orbit() {
    let edges = cycle(6);
    let first = identity::canonicalize(&graph(vec![atom("C"); 6], &edges)).unwrap();
    let permutation = [3, 0, 5, 1, 4, 2];
    let renumbered: Vec<_> = edges
        .iter()
        .rev()
        .map(|(a, b, o)| (permutation[*a], permutation[*b], o.clone()))
        .collect();
    let second = identity::canonicalize(&graph(vec![atom("C"); 6], &renumbered)).unwrap();
    assert_eq!(first.code, second.code);
    assert_eq!(first.numbering.len(), 6);
    assert_eq!(first.symmetry.iter().map(Vec::len).collect::<Vec<_>>(), [6]);
    assert_eq!(
        second.symmetry.iter().map(Vec::len).collect::<Vec<_>>(),
        [6]
    );
}

#[test]
fn reindexed_asymmetric_graph_keeps_orbits_and_dative_direction() {
    // The two terminal carbons are interchangeable; the O--N side is asymmetric.
    let atoms = vec![atom("C"), atom("C"), atom("C"), atom("O"), atom("N")];
    let edges = [
        (0, 1, BondOrder::Single),
        (1, 2, BondOrder::Single),
        (1, 3, BondOrder::Single),
        (3, 4, BondOrder::Dative),
    ];
    let permutation = [3, 0, 4, 2, 1];
    let mut permuted_atoms = atoms.clone();
    for (old, &new) in permutation.iter().enumerate() {
        permuted_atoms[new] = atoms[old].clone();
    }
    let permuted_edges: Vec<_> = edges
        .iter()
        .rev()
        .map(|(a, b, order)| (permutation[*a], permutation[*b], order.clone()))
        .collect();
    let first = identity::canonicalize(&graph(atoms, &edges)).unwrap();
    let second = identity::canonicalize(&graph(permuted_atoms, &permuted_edges)).unwrap();
    assert_eq!(first.code, second.code);
    let ids: Vec<_> = (1..=5)
        .map(|n| chemifree_domain::document::AtomId::from_serial(n).unwrap())
        .collect();
    let mapped: BTreeSet<BTreeSet<_>> = first
        .symmetry
        .iter()
        .map(|orbit| {
            orbit
                .iter()
                .map(|id| {
                    let old = ids.iter().position(|candidate| candidate == id).unwrap();
                    ids[permutation[old]]
                })
                .collect()
        })
        .collect();
    let actual: BTreeSet<BTreeSet<_>> = second
        .symmetry
        .iter()
        .map(|orbit| orbit.iter().copied().collect())
        .collect();
    assert_eq!(mapped, actual);
    assert!(
        mapped.contains(
            &[ids[permutation[0]], ids[permutation[2]]]
                .into_iter()
                .collect()
        )
    );
}

#[test]
fn equal_degree_graphs_that_are_not_isomorphic_have_different_codes() {
    // Triangular prism versus K3,3: both are connected cubic graphs with six vertices.
    let prism = [
        (0, 1),
        (1, 2),
        (2, 0),
        (3, 4),
        (4, 5),
        (5, 3),
        (0, 3),
        (1, 4),
        (2, 5),
    ]
    .map(|(a, b)| (a, b, BondOrder::Single));
    let bipartite = [
        (0, 3),
        (0, 4),
        (0, 5),
        (1, 3),
        (1, 4),
        (1, 5),
        (2, 3),
        (2, 4),
        (2, 5),
    ]
    .map(|(a, b)| (a, b, BondOrder::Single));
    let a = identity::canonicalize(&graph(vec![atom("C"); 6], &prism)).unwrap();
    let b = identity::canonicalize(&graph(vec![atom("C"); 6], &bipartite)).unwrap();
    assert_ne!(a.code, b.code);
    assert_eq!(a.symmetry.iter().map(Vec::len).collect::<Vec<_>>(), [6]);
    assert_eq!(b.symmetry.iter().map(Vec::len).collect::<Vec<_>>(), [6]);
}

#[test]
fn isotope_charge_radical_hydrogen_and_directed_bonds_remain_distinct() {
    let base = atom("C");
    let cases = [
        {
            let mut a = base.clone();
            a.isotope = Some(13);
            a
        },
        {
            let mut a = base.clone();
            a.charge = 1;
            a
        },
        {
            let mut a = base.clone();
            a.radical = Radical::Doublet;
            a
        },
        {
            let mut a = base.clone();
            a.hydrogens = Hydrogens::Specified(0);
            a
        },
        {
            let mut a = base.clone();
            a.hydrogens = Hydrogens::Unknown;
            a
        },
    ];
    let reference = identity::canonicalize(&graph(vec![base], &[]))
        .unwrap()
        .code;
    for changed in cases {
        assert_ne!(
            identity::canonicalize(&graph(vec![changed], &[]))
                .unwrap()
                .code,
            reference
        );
    }
    let forward = graph(vec![atom("C"), atom("N")], &[(0, 1, BondOrder::Dative)]);
    let reversed = graph(vec![atom("C"), atom("N")], &[(1, 0, BondOrder::Dative)]);
    assert_ne!(
        identity::canonicalize(&forward).unwrap().code,
        identity::canonicalize(&reversed).unwrap().code
    );
}

#[test]
fn recorded_aromatic_and_kekule_are_deliberately_different_profiles() {
    let aromatic = graph(vec![atom("C"); 6], &cycle(6));
    let kekule: Vec<_> = (0..6)
        .map(|i| {
            (
                i,
                (i + 1) % 6,
                if i % 2 == 0 {
                    BondOrder::Double
                } else {
                    BondOrder::Single
                },
            )
        })
        .collect();
    assert_ne!(
        identity::canonicalize(&aromatic).unwrap().code,
        identity::canonicalize(&graph(vec![atom("C"); 6], &kekule))
            .unwrap()
            .code
    );
}

#[test]
fn atom_limit_returns_no_partial_identity() {
    let graph = graph(vec![atom("C"); 129], &[]);
    assert_eq!(
        identity::canonicalize(&graph),
        Err(IdentityError::AtomLimit)
    );
}

#[test]
fn factorial_symmetry_hits_the_search_limit_without_a_partial_result() {
    let repeated = graph(vec![atom("C"); 9], &[]);
    assert_eq!(
        identity::canonicalize(&repeated),
        Err(IdentityError::SearchLimit)
    );
}

#[test]
fn chemical_profile_normalizes_aromatic_and_both_kekule_benzene_forms() {
    let aromatic = graph(vec![atom("C"); 6], &cycle(6));
    let expected = identity::canonicalize_chemical(&aromatic).unwrap();
    for start in [0, 1] {
        let edges: Vec<_> = (0..6)
            .map(|i| {
                (
                    i,
                    (i + 1) % 6,
                    if i % 2 == start {
                        BondOrder::Double
                    } else {
                        BondOrder::Single
                    },
                )
            })
            .collect();
        let actual = identity::canonicalize_chemical(&graph(vec![atom("C"); 6], &edges)).unwrap();
        assert_eq!(actual.code, expected.code);
        assert_eq!(
            actual.symmetry.iter().map(Vec::len).collect::<Vec<_>>(),
            [6]
        );
    }
}

#[test]
fn chemical_identity_preserves_isotope_and_charge_when_normalizing() {
    let natural = identity::canonicalize_chemical(&graph(vec![atom("C"); 6], &cycle(6))).unwrap();
    let mut labeled = vec![atom("C"); 6];
    labeled[0].isotope = Some(13);
    let aromatic = identity::canonicalize_chemical(&graph(labeled.clone(), &cycle(6))).unwrap();
    let kekule: Vec<_> = (0..6)
        .map(|i| {
            (i, (i + 1) % 6, if i % 2 == 0 { BondOrder::Double } else { BondOrder::Single })
        })
        .collect();
    assert_eq!(identity::canonicalize_chemical(&graph(labeled, &kekule)).unwrap().code, aromatic.code);
    assert_ne!(aromatic.code, natural.code);

    // The supported H+ and H− states both have zero attached H, isolating charge itself.
    let mut cation = atom("H");
    cation.charge = 1;
    cation.hydrogens = Hydrogens::Specified(0);
    let mut anion = cation.clone();
    anion.charge = -1;
    assert_ne!(
        identity::canonicalize_chemical(&graph(vec![cation], &[])).unwrap().code,
        identity::canonicalize_chemical(&graph(vec![anion], &[])).unwrap().code,
    );
}

#[test]
fn chemical_profile_normalizes_fused_and_heteroaromatic_encodings() {
    // Naphthalene: two six-membered circuits sharing one edge (C10H8).
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
    ];
    let aromatic: Vec<_> = edges
        .iter()
        .map(|&(a, b)| (a, b, BondOrder::Aromatic))
        .collect();
    let expected = identity::canonicalize_chemical(&graph(vec![atom("C"); 10], &aromatic)).unwrap();
    for doubles in [[0, 2, 6, 8, 10], [1, 3, 5, 7, 9]] {
        let kekule: Vec<_> = edges
            .iter()
            .enumerate()
            .map(|(i, &(a, b))| {
                (
                    a,
                    b,
                    if doubles.contains(&i) {
                        BondOrder::Double
                    } else {
                        BondOrder::Single
                    },
                )
            })
            .collect();
        assert_eq!(
            identity::canonicalize_chemical(&graph(vec![atom("C"); 10], &kekule))
                .unwrap()
                .code,
            expected.code
        );
    }
    // Pyridine: one pyridine-like N and five C atoms in the aromatic ring.
    let mut atoms = vec![atom("C"); 6];
    atoms[0] = atom("N");
    let aromatic = identity::canonicalize_chemical(&graph(atoms.clone(), &cycle(6))).unwrap();
    let kekule: Vec<_> = (0..6)
        .map(|i| {
            (
                i,
                (i + 1) % 6,
                if i % 2 == 0 {
                    BondOrder::Double
                } else {
                    BondOrder::Single
                },
            )
        })
        .collect();
    assert_eq!(
        identity::canonicalize_chemical(&graph(atoms, &kekule))
            .unwrap()
            .code,
        aromatic.code
    );
}

#[test]
fn chemical_profile_folds_only_ordinary_terminal_hydrogens() {
    let implicit = identity::canonicalize_chemical(&graph(vec![atom("C")], &[])).unwrap();
    let mut stated = atom("C");
    stated.hydrogens = Hydrogens::Specified(4);
    let explicit = identity::canonicalize_chemical(&graph(
        vec![atom("C"), atom("H"), atom("H"), atom("H"), atom("H")],
        &[
            (0, 1, BondOrder::Single),
            (0, 2, BondOrder::Single),
            (0, 3, BondOrder::Single),
            (0, 4, BondOrder::Single),
        ],
    ))
    .unwrap();
    assert_eq!(
        implicit.code,
        identity::canonicalize_chemical(&graph(vec![stated], &[]))
            .unwrap()
            .code
    );
    assert_eq!(implicit.code, explicit.code);
    assert_eq!(explicit.folded_hydrogens.len(), 4);
    assert_eq!(
        explicit.symmetry.iter().map(Vec::len).collect::<Vec<_>>(),
        [1, 4]
    );
    let mut deuterium = atom("H");
    deuterium.isotope = Some(2);
    let isotope = graph(
        vec![atom("C"), deuterium, atom("H"), atom("H"), atom("H")],
        &[
            (0, 1, BondOrder::Single),
            (0, 2, BondOrder::Single),
            (0, 3, BondOrder::Single),
            (0, 4, BondOrder::Single),
        ],
    );
    assert_ne!(
        implicit.code,
        identity::canonicalize_chemical(&isotope).unwrap().code
    );
}

#[test]
fn chemical_profile_normalizes_explicit_ammonia_and_water_hydrogens() {
    for (symbol, count) in [("N", 3), ("O", 2)] {
        let implicit = identity::canonicalize_chemical(&graph(vec![atom(symbol)], &[])).unwrap();
        let mut atoms = vec![atom(symbol)];
        atoms.extend((0..count).map(|_| atom("H")));
        let edges: Vec<_> = (1..=count).map(|i| (0, i, BondOrder::Single)).collect();
        let explicit = identity::canonicalize_chemical(&graph(atoms, &edges)).unwrap();
        assert_eq!(implicit.code, explicit.code, "{symbol}");
        assert_eq!(explicit.folded_hydrogens.len(), count, "{symbol}");
    }
}

#[test]
fn chemical_profile_refuses_unsupported_states_instead_of_claiming_equality() {
    assert_eq!(
        identity::canonicalize_chemical(&graph(vec![atom("Fe")], &[])),
        Err(IdentityError::Unsupported("element_or_charge"))
    );
    let mut unknown = atom("C");
    unknown.hydrogens = Hydrogens::Unknown;
    assert_eq!(
        identity::canonicalize_chemical(&graph(vec![unknown], &[])),
        Err(IdentityError::Unsupported("unknown_hydrogens"))
    );
}
