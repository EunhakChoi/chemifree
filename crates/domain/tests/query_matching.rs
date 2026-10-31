use chemifree_domain::document::{
    Atom, AtomId, AtomRef, BondOrder, Command, Document, DocumentId, NewAtomPositions, Point,
};
use chemifree_domain::element::Element;
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::query::{
    AtomPredicate, MatchModel, QueryError, SearchLimit, find, find_graphs,
};
use chemifree_domain::states::{AtomKind, BondQuery, QueryAtom, QueryBondOrder};

fn atom(symbol: &str) -> Atom {
    Atom::from(Element::from_symbol(symbol).unwrap())
}
fn graph(name: &str, atoms: Vec<Atom>, edges: &[(usize, usize, BondOrder)]) -> Document {
    let mut doc = Document::new(DocumentId::parse(name).unwrap(), name).unwrap();
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
    doc.apply(&commands).unwrap();
    doc
}
fn id(serial: u32) -> AtomId {
    AtomId::from_serial(serial).unwrap()
}

#[test]
fn ring_neighbor_charge_predicates_use_target_ids_and_do_not_edit_either_document() {
    let query = graph(
        "query",
        vec![Atom::new(AtomKind::Query(QueryAtom::AnyHeavy))],
        &[],
    );
    let mut charged = atom("C");
    charged.charge = 1;
    let target = graph(
        "target",
        vec![charged, atom("C"), atom("C"), atom("Cl")],
        &[
            (0, 1, BondOrder::Single),
            (1, 2, BondOrder::Single),
            (2, 0, BondOrder::Single),
            (0, 3, BondOrder::Single),
        ],
    );
    let before = (query.clone(), target.clone());
    let p = AtomPredicate {
        atom: id(1),
        ring: Some(true),
        min_neighbors: Some(3),
        max_neighbors: Some(3),
        charge: Some(1),
    };
    let found = find(&query, &target, &[p], 100, 10_000).unwrap();
    assert_eq!(found.mappings, vec![vec![(id(1), id(1))]]);
    assert!(found.complete);
    assert_eq!((query, target), before);
}

#[test]
fn symmetric_matches_are_distinct_and_limits_are_explicit() {
    let query = graph(
        "query",
        vec![atom("C"), atom("C")],
        &[(0, 1, BondOrder::Single)],
    );
    let target = graph(
        "target",
        vec![atom("C"); 6],
        &(0..6)
            .map(|i| (i, (i + 1) % 6, BondOrder::Single))
            .collect::<Vec<_>>(),
    );
    let all = find(&query, &target, &[], 100, 100_000).unwrap();
    assert!(all.complete);
    assert_eq!(all.mappings.len(), 12);
    let limited = find(&query, &target, &[], 3, 100_000).unwrap();
    assert!(!limited.complete);
    assert_eq!(limited.mappings.len(), 3);
    assert_eq!(limited.limit, Some(SearchLimit::Mappings));
    let work_limited = find(&query, &target, &[], 100, 1).unwrap();
    assert!(!work_limited.complete);
    assert_eq!(work_limited.limit, Some(SearchLimit::States));
}

#[test]
fn state_bound_also_counts_checks_of_mapped_edges() {
    let query = graph(
        "query",
        vec![atom("C"), atom("C")],
        &[(0, 1, BondOrder::Single)],
    );
    let target = graph(
        "target",
        vec![atom("C"), atom("C")],
        &[(0, 1, BondOrder::Single)],
    );
    // Three candidate attempts reach the first possible edge; checking it needs a
    // fourth unit. Exhaustion cannot be reported as a complete negative result.
    let result = find(&query, &target, &[], 10, 3).unwrap();
    assert_eq!(result.states, 3);
    assert!(!result.complete);
    assert_eq!(result.limit, Some(SearchLimit::States));
    assert!(result.mappings.is_empty());
}

#[test]
fn ring_condition_handles_spiro_fused_and_disconnected_atoms() {
    let query = graph("ring-query", vec![atom("C")], &[]);
    // Two triangles meet at a1; a6--a7 is an acyclic branch and a8 is separate.
    let target = graph(
        "spiro",
        vec![atom("C"); 8],
        &[
            (0, 1, BondOrder::Single),
            (1, 2, BondOrder::Single),
            (2, 0, BondOrder::Single),
            (0, 3, BondOrder::Single),
            (3, 4, BondOrder::Single),
            (4, 0, BondOrder::Single),
            (0, 5, BondOrder::Single),
            (5, 6, BondOrder::Single),
        ],
    );
    let predicate = |ring, min_neighbors| AtomPredicate {
        atom: id(1),
        ring: Some(ring),
        min_neighbors,
        max_neighbors: None,
        charge: None,
    };
    let in_ring = find(&query, &target, &[predicate(true, None)], 20, 1_000).unwrap();
    assert_eq!(
        in_ring.mappings.iter().map(|m| m[0].1).collect::<Vec<_>>(),
        vec![id(1), id(2), id(3), id(4), id(5)]
    );
    let outside = find(&query, &target, &[predicate(false, None)], 20, 1_000).unwrap();
    assert_eq!(
        outside.mappings.iter().map(|m| m[0].1).collect::<Vec<_>>(),
        vec![id(6), id(7), id(8)]
    );
    let junction = find(&query, &target, &[predicate(true, Some(5))], 20, 1_000).unwrap();
    assert_eq!(junction.mappings, vec![vec![(id(1), id(1))]]);
    let fused = graph(
        "fused",
        vec![atom("C"); 4],
        &[
            (0, 1, BondOrder::Single),
            (1, 2, BondOrder::Single),
            (2, 0, BondOrder::Single),
            (1, 3, BondOrder::Single),
            (3, 2, BondOrder::Single),
        ],
    );
    assert_eq!(
        find(&query, &fused, &[predicate(true, None)], 20, 1_000)
            .unwrap()
            .mappings
            .len(),
        4
    );
}

#[test]
fn exactly_full_mapping_limit_can_still_be_complete() {
    let query = graph(
        "edge-query",
        vec![atom("C"), atom("C")],
        &[(0, 1, BondOrder::Single)],
    );
    let target = graph(
        "edge-target",
        vec![atom("C"), atom("C")],
        &[(0, 1, BondOrder::Single)],
    );
    let found = find(&query, &target, &[], 2, 1_000).unwrap();
    assert_eq!(found.mappings.len(), 2);
    assert!(found.complete);
    assert_eq!(found.limit, None);
}

#[test]
fn query_bond_alternatives_charge_and_dative_direction_are_preserved() {
    let mut n_plus = atom("N");
    n_plus.charge = 1;
    let q = graph(
        "query",
        vec![atom("C"), n_plus.clone()],
        &[(
            0,
            1,
            BondOrder::Query(BondQuery::OneOf(vec![
                QueryBondOrder::Single,
                QueryBondOrder::Double,
            ])),
        )],
    );
    let target = graph(
        "target",
        vec![atom("C"), n_plus, atom("N")],
        &[(0, 1, BondOrder::Double), (0, 2, BondOrder::Single)],
    );
    let result = find(&q, &target, &[], 100, 1000).unwrap();
    assert_eq!(result.mappings, vec![vec![(id(1), id(1)), (id(2), id(2))]]);
    let forward = graph(
        "forward",
        vec![atom("C"), atom("N")],
        &[(0, 1, BondOrder::Dative)],
    );
    let backward = graph(
        "backward",
        vec![atom("C"), atom("N")],
        &[(1, 0, BondOrder::Dative)],
    );
    assert!(
        find(&forward, &backward, &[], 100, 1000)
            .unwrap()
            .mappings
            .is_empty()
    );
}

#[test]
fn uncertain_target_bonds_are_refused_instead_of_guessed() {
    let q = graph("query", vec![atom("C")], &[]);
    let target = graph(
        "target",
        vec![atom("C"), atom("C")],
        &[(0, 1, BondOrder::Unknown)],
    );
    assert_eq!(
        find(&q, &target, &[], 5, 100),
        Err(QueryError::UnsupportedTarget)
    );
}

#[test]
fn halogen_query_includes_heavy_halogen_and_charge_constraints_are_additive() {
    let query = graph(
        "query",
        vec![Atom::new(AtomKind::Query(QueryAtom::Halogen))],
        &[],
    );
    let target = graph("target", vec![atom("At"), atom("Ts"), atom("O")], &[]);
    assert_eq!(
        find(&query, &target, &[], 10, 100).unwrap().mappings,
        vec![vec![(id(1), id(1))], vec![(id(1), id(2))]]
    );
    let mut positive = atom("C");
    positive.charge = 1;
    let mut negative = atom("C");
    negative.charge = -1;
    let query = graph("charged-query", vec![positive], &[]);
    let target = graph("charged-target", vec![negative], &[]);
    let predicate = AtomPredicate {
        atom: id(1),
        ring: None,
        min_neighbors: None,
        max_neighbors: None,
        charge: Some(-1),
    };
    assert!(
        find(&query, &target, &[predicate], 10, 100)
            .unwrap()
            .mappings
            .is_empty()
    );
}

#[test]
fn aromatic_bonds_match_recorded_aromatic_bonds_but_not_kekule_orders() {
    let query = graph(
        "query",
        vec![atom("C"), atom("C")],
        &[(0, 1, BondOrder::Aromatic)],
    );
    let aromatic = graph(
        "aromatic",
        vec![atom("C"), atom("C")],
        &[(0, 1, BondOrder::Aromatic)],
    );
    let kekule = graph(
        "kekule",
        vec![atom("C"), atom("C")],
        &[(0, 1, BondOrder::Double)],
    );
    assert_eq!(
        find(&query, &aromatic, &[], 10, 100)
            .unwrap()
            .mappings
            .len(),
        2
    );
    assert!(
        find(&query, &kekule, &[], 10, 100)
            .unwrap()
            .mappings
            .is_empty()
    );
}

#[test]
fn aromatic_query_can_use_the_named_target_model_without_guessing_invalid_targets() {
    let query = graph(
        "query-aromatic",
        vec![atom("C"), atom("C")],
        &[(0, 1, BondOrder::Aromatic)],
    );
    let edges = (0..6)
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
        .collect::<Vec<_>>();
    let kekule = graph("benzene-kekule", vec![atom("C"); 6], &edges);
    let aromatic = graph(
        "benzene-aromatic",
        vec![atom("C"); 6],
        &(0..6)
            .map(|i| (i, (i + 1) % 6, BondOrder::Aromatic))
            .collect::<Vec<_>>(),
    );
    for target in [&kekule, &aromatic] {
        let result = find_graphs(
            &MoleculeGraph::from_document(&query),
            &MoleculeGraph::from_document(target),
            &[],
            100,
            10_000,
            MatchModel::AromaticTarget,
        )
        .unwrap();
        assert!(result.complete);
        assert_eq!(result.mappings.len(), 12);
    }
    assert!(
        find(&query, &kekule, &[], 100, 10_000)
            .unwrap()
            .mappings
            .is_empty()
    );
    let invalid = graph(
        "invalid-aromatic",
        vec![atom("C"); 4],
        &(0..4)
            .map(|i| (i, (i + 1) % 4, BondOrder::Aromatic))
            .collect::<Vec<_>>(),
    );
    assert!(matches!(
        find_graphs(
            &MoleculeGraph::from_document(&query),
            &MoleculeGraph::from_document(&invalid),
            &[],
            100,
            10_000,
            MatchModel::AromaticTarget
        ),
        Err(QueryError::AromaticTargetUnsupported(_))
    ));
}

#[test]
fn any_heteroatom_and_element_lists_have_distinct_meanings() {
    let target = graph(
        "target",
        vec![atom("H"), atom("C"), atom("N"), atom("O")],
        &[],
    );
    let cases = [
        (QueryAtom::Any, vec![id(1), id(2), id(3), id(4)]),
        (QueryAtom::AnyHeavy, vec![id(2), id(3), id(4)]),
        (QueryAtom::Heteroatom, vec![id(3), id(4)]),
        (
            QueryAtom::List {
                elements: vec![
                    Element::from_symbol("N").unwrap(),
                    Element::from_symbol("O").unwrap(),
                ],
                negated: false,
            },
            vec![id(3), id(4)],
        ),
        (
            QueryAtom::List {
                elements: vec![
                    Element::from_symbol("N").unwrap(),
                    Element::from_symbol("O").unwrap(),
                ],
                negated: true,
            },
            vec![id(1), id(2)],
        ),
    ];
    for (kind, expected) in cases {
        let query = graph("query", vec![Atom::new(AtomKind::Query(kind))], &[]);
        let found = find(&query, &target, &[], 10, 100).unwrap();
        assert_eq!(
            found
                .mappings
                .iter()
                .map(|mapping| mapping[0].1)
                .collect::<Vec<_>>(),
            expected
        );
        assert!(found.complete);
    }
}

#[test]
fn any_bond_and_acyclic_neighbor_bound_are_independent_predicates() {
    let q = graph(
        "query",
        vec![atom("C"), atom("N")],
        &[(0, 1, BondOrder::Query(BondQuery::Any))],
    );
    let t = graph(
        "target",
        vec![atom("C"), atom("N")],
        &[(0, 1, BondOrder::Dative)],
    );
    let p = AtomPredicate {
        atom: id(1),
        ring: Some(false),
        min_neighbors: Some(1),
        max_neighbors: Some(1),
        charge: None,
    };
    assert_eq!(
        find(&q, &t, &[p], 10, 100).unwrap().mappings,
        vec![vec![(id(1), id(1)), (id(2), id(2))]]
    );
}
