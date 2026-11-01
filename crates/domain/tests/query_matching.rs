use chemifree_domain::document::{
    Atom, AtomId, AtomRef, BondOrder, Command, Document, DocumentId, NewAtomPositions, Point,
};
use chemifree_domain::element::Element;
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::query::{
    AtomPredicate, MatchModel, QueryError, SearchLimit, find, find_graphs,
};
use chemifree_domain::states::{AtomKind, BondQuery, Hydrogens, QueryAtom, QueryBondOrder};

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
    // Two candidate attempts reach the first possible edge; checking it needs a third
    // unit. Exhaustion cannot be reported as a complete negative result.
    let result = find(&query, &target, &[], 10, 2).unwrap();
    assert_eq!(result.states, 2);
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
    // The element query atom already fixes charge +1, so a -1 condition is contradictory
    // and refused instead of being reported as a complete empty result.
    assert_eq!(
        find(&query, &target, &[predicate], 10, 100),
        Err(QueryError::InvalidPredicate)
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

#[test]
fn query_side_errors_take_precedence_over_target_aromaticity_failures() {
    let invalid_target = graph(
        "invalid-aromatic-target",
        vec![atom("C"); 4],
        &(0..4)
            .map(|i| (i, (i + 1) % 4, BondOrder::Aromatic))
            .collect::<Vec<_>>(),
    );
    let empty_query = Document::new(DocumentId::parse("empty-query").unwrap(), "empty").unwrap();
    let one = graph("one-atom-query", vec![atom("C")], &[]);
    let target = MoleculeGraph::from_document(&invalid_target);
    let bad_predicate = [AtomPredicate {
        atom: id(99),
        ring: None,
        min_neighbors: None,
        max_neighbors: None,
        charge: None,
    }];
    for (query, predicates, limit, expected) in [
        (&empty_query, &[][..], 10, QueryError::EmptyQuery),
        (&one, &[][..], 0, QueryError::InvalidBounds),
        (&one, &bad_predicate[..], 10, QueryError::InvalidPredicate),
    ] {
        let result = find_graphs(
            &MoleculeGraph::from_document(query),
            &target,
            predicates,
            limit,
            10_000,
            MatchModel::AromaticTarget,
        );
        assert_eq!(
            result,
            Err(expected),
            "query-side error must be reported first"
        );
    }
}

fn no_conditions(atom: AtomId) -> AtomPredicate {
    AtomPredicate {
        atom,
        ring: None,
        min_neighbors: None,
        max_neighbors: None,
        charge: None,
    }
}

fn with_hydrogens(symbol: &str, count: u8) -> Atom {
    let mut atom = atom(symbol);
    atom.hydrogens = Hydrogens::Specified(count);
    atom
}

fn chain(name: &str, length: usize) -> Document {
    // A transaction holds at most 500 commands, so atoms and bonds go in batches.
    let mut doc = Document::new(DocumentId::parse(name).unwrap(), name).unwrap();
    for start in (0..length).step_by(400) {
        let count = 400.min(length - start);
        let atoms: Vec<_> = (0..count)
            .map(|_| Command::AddAtom {
                atom: atom("C"),
                positions: NewAtomPositions::Single(Point::new(0.0, 0.0).unwrap()),
                bond_to: None,
            })
            .collect();
        doc.apply(&atoms).unwrap();
    }
    let ids: Vec<_> = (1..=length as u32).map(id).collect();
    for batch in ids.windows(2).collect::<Vec<_>>().chunks(400) {
        let bonds: Vec<_> = batch
            .iter()
            .map(|pair| Command::AddBond {
                begin: AtomRef::Id(pair[0]),
                end: AtomRef::Id(pair[1]),
                order: BondOrder::Single,
            })
            .collect();
        doc.apply(&bonds).unwrap();
    }
    doc
}

fn aromatic_pair() -> Document {
    graph(
        "aromatic-pair",
        vec![atom("C"), atom("C")],
        &[(0, 1, BondOrder::Aromatic)],
    )
}

fn target_pairs(result: &chemifree_domain::query::SearchResult) -> Vec<Vec<AtomId>> {
    result
        .mappings
        .iter()
        .map(|mapping| mapping.iter().map(|(_, target)| *target).collect())
        .collect()
}

#[test]
fn stated_query_hydrogens_match_the_valence_model_hydrogens_of_a_drawn_target() {
    let ethane = chain("ethane", 2);
    for (count, expected) in [(3, 2), (2, 0), (4, 0)] {
        let query = graph("methyl", vec![with_hydrogens("C", count)], &[]);
        let result = find(&query, &ethane, &[], 10, 1000).unwrap();
        assert!(result.complete);
        assert_eq!(
            result.mappings.len(),
            expected,
            "query H={count} against drawn ethane (each carbon has 3 inferred H)"
        );
    }
    // A stated target count is used as recorded, not recomputed.
    let stated = graph("stated", vec![with_hydrogens("C", 2)], &[]);
    let query = graph("two", vec![with_hydrogens("C", 2)], &[]);
    assert_eq!(
        find(&query, &stated, &[], 10, 100).unwrap().mappings.len(),
        1
    );
}

#[test]
fn stated_query_hydrogens_refuse_a_target_whose_hydrogens_cannot_be_determined() {
    let query = graph("one-h", vec![with_hydrogens("C", 1)], &[]);
    // Recorded aromatic bonds have no integral order in the recorded model.
    let benzene = graph(
        "benzene",
        vec![atom("C"); 6],
        &(0..6)
            .map(|i| (i, (i + 1) % 6, BondOrder::Aromatic))
            .collect::<Vec<_>>(),
    );
    assert_eq!(
        find(&query, &benzene, &[], 10, 1000),
        Err(QueryError::TargetHydrogensUndetermined)
    );
    // A pentavalent carbon has no valid inferred hydrogen count.
    let hub = graph(
        "hub",
        vec![atom("C"); 6],
        &(1..6)
            .map(|i| (0, i, BondOrder::Single))
            .collect::<Vec<_>>(),
    );
    assert_eq!(
        find(&query, &hub, &[], 10, 1000),
        Err(QueryError::TargetHydrogensUndetermined)
    );
    // Without a stated hydrogen condition the same targets are searched as before.
    let plain = graph("plain", vec![atom("C")], &[]);
    assert!(find(&plain, &hub, &[], 10, 1000).unwrap().complete);
}

#[test]
fn isotope_and_radical_conditions_compare_recorded_target_states() {
    let mut labeled = atom("C");
    labeled.isotope = Some(13);
    let query = graph("c13", vec![labeled.clone()], &[]);
    let natural = graph("natural", vec![atom("C")], &[]);
    let heavy = graph("heavy", vec![labeled], &[]);
    assert!(
        find(&query, &natural, &[], 10, 100)
            .unwrap()
            .mappings
            .is_empty()
    );
    assert_eq!(
        find(&query, &heavy, &[], 10, 100).unwrap().mappings.len(),
        1
    );
    let mut radical = atom("C");
    radical.radical = chemifree_domain::states::Radical::Doublet;
    let query = graph("radical", vec![radical], &[]);
    assert!(
        find(&query, &natural, &[], 10, 100)
            .unwrap()
            .mappings
            .is_empty()
    );
}

#[test]
fn a_charge_condition_that_contradicts_an_element_atom_is_refused() {
    let target = graph("target", vec![atom("C")], &[]);
    let neutral = graph("neutral", vec![atom("C")], &[]);
    let mut contradictory = no_conditions(id(1));
    contradictory.charge = Some(1);
    assert_eq!(
        find(&neutral, &target, &[contradictory], 10, 100),
        Err(QueryError::InvalidPredicate)
    );
    // The same value as the drawn charge is redundant, not contradictory.
    let mut redundant = no_conditions(id(1));
    redundant.charge = Some(0);
    assert_eq!(
        find(&neutral, &target, &[redundant], 10, 100)
            .unwrap()
            .mappings
            .len(),
        1
    );
    // A query atom stands for several charges, so a condition on it is meaningful.
    let any = graph("any", vec![Atom::new(AtomKind::Query(QueryAtom::Any))], &[]);
    let mut cation = atom("C");
    cation.charge = 1;
    let cations = graph("cations", vec![cation, atom("C")], &[]);
    let mut positive = no_conditions(id(1));
    positive.charge = Some(1);
    assert_eq!(
        find(&any, &cations, &[positive], 10, 100).unwrap().mappings,
        vec![vec![(id(1), id(1))]]
    );
}

#[test]
fn kekule_nh_heterocycles_with_inferred_hydrogen_are_refused_not_reported_empty() {
    let pyrazole = graph(
        "pyrazole",
        vec![atom("C"), atom("C"), atom("N"), atom("N"), atom("C")],
        &[
            (0, 1, BondOrder::Single),
            (1, 2, BondOrder::Double),
            (2, 3, BondOrder::Single),
            (3, 4, BondOrder::Single),
            (4, 0, BondOrder::Double),
        ],
    );
    let pyrrole = graph(
        "pyrrole",
        vec![atom("C"), atom("C"), atom("C"), atom("C"), atom("N")],
        &[
            (0, 1, BondOrder::Double),
            (1, 2, BondOrder::Single),
            (2, 3, BondOrder::Double),
            (3, 4, BondOrder::Single),
            (4, 0, BondOrder::Single),
        ],
    );
    let query = aromatic_pair();
    for target in [&pyrazole, &pyrrole] {
        assert_eq!(
            find_graphs(
                &MoleculeGraph::from_document(&query),
                &MoleculeGraph::from_document(target),
                &[],
                100,
                10_000,
                MatchModel::AromaticTarget
            ),
            Err(QueryError::AromaticTargetUnsupported(
                "ambiguous_ring_nitrogen_hydrogen"
            ))
        );
    }
}

#[test]
fn only_the_ambiguous_nitrogen_pattern_is_refused_in_aromatic_target_mode() {
    let aromatic_query = MoleculeGraph::from_document(&aromatic_pair());
    // The same pyrrole with its N-H stated is classified, so the search proceeds.
    let pyrrole = graph(
        "pyrrole-nh",
        vec![
            atom("C"),
            atom("C"),
            atom("C"),
            atom("C"),
            with_hydrogens("N", 1),
        ],
        &[
            (0, 1, BondOrder::Double),
            (1, 2, BondOrder::Single),
            (2, 3, BondOrder::Double),
            (3, 4, BondOrder::Single),
            (4, 0, BondOrder::Single),
        ],
    );
    let found = find_graphs(
        &aromatic_query,
        &MoleculeGraph::from_document(&pyrrole),
        &[],
        100,
        10_000,
        MatchModel::AromaticTarget,
    )
    .unwrap();
    assert!(found.complete);
    assert_eq!(found.mappings.len(), 6);
    // A saturated ring N-H is not aromatic under either hydrogen reading.
    let piperidine = graph(
        "piperidine",
        vec![
            atom("N"),
            atom("C"),
            atom("C"),
            atom("C"),
            atom("C"),
            atom("C"),
        ],
        &(0..6)
            .map(|i| (i, (i + 1) % 6, BondOrder::Single))
            .collect::<Vec<_>>(),
    );
    let single = graph(
        "cc",
        vec![atom("C"), atom("C")],
        &[(0, 1, BondOrder::Single)],
    );
    let result = find_graphs(
        &MoleculeGraph::from_document(&single),
        &MoleculeGraph::from_document(&piperidine),
        &[],
        100,
        10_000,
        MatchModel::AromaticTarget,
    )
    .unwrap();
    assert!(result.complete);
    assert_eq!(result.mappings.len(), 8);
}

#[test]
fn a_two_atom_query_completes_against_a_long_chain_under_default_budgets() {
    let query = chain("pair", 2);
    let target = chain("long-chain", 400);
    let result = find(&query, &target, &[], 1000, 100_000).unwrap();
    assert!(result.complete, "work used: {}", result.states);
    assert_eq!(result.mappings.len(), 798);
}

#[test]
fn renumbering_a_path_query_changes_neither_mappings_nor_completeness() {
    let target = chain("thirty", 30);
    let numberings: [[usize; 6]; 3] = [[0, 1, 2, 3, 4, 5], [3, 0, 5, 1, 4, 2], [5, 2, 4, 0, 3, 1]];
    let mut outcomes = Vec::new();
    for numbering in numberings {
        let edges: Vec<_> = numbering
            .windows(2)
            .map(|pair| (pair[0], pair[1], BondOrder::Single))
            .collect();
        let query = graph("scrambled", vec![atom("C"); 6], &edges);
        let result = find(&query, &target, &[], 1000, 100_000).unwrap();
        assert!(
            result.complete,
            "numbering {numbering:?} stopped at {} work",
            result.states
        );
        // Read each mapping along the path, so numbering does not affect the comparison.
        let mut along_path: Vec<Vec<AtomId>> = result
            .mappings
            .iter()
            .map(|mapping| {
                numbering
                    .iter()
                    .map(|&position| {
                        mapping
                            .iter()
                            .find(|(q, _)| *q == id(position as u32 + 1))
                            .unwrap()
                            .1
                    })
                    .collect()
            })
            .collect();
        along_path.sort();
        outcomes.push(along_path);
    }
    assert_eq!(outcomes[0].len(), 2 * (30 - 5));
    assert_eq!(outcomes[0], outcomes[1]);
    assert_eq!(outcomes[0], outcomes[2]);
}

#[test]
fn a_sixty_four_atom_path_completes_against_a_long_chain_with_the_maximum_budget() {
    let query = chain("path-64", 64);
    let target = chain("chain-240", 240);
    let result = find(&query, &target, &[], 1000, 1_000_000).unwrap();
    assert!(result.complete, "work used: {}", result.states);
    assert_eq!(target_pairs(&result).len(), 2 * (240 - 63));
}
