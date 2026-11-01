use chemifree_domain::document::{
    Atom, AtomRef, BondOrder, Command, Document, DocumentId, NewAtomPositions, Point,
};
use chemifree_domain::element::Element;
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::rings;

fn graph(n: usize, edges: &[(usize, usize)]) -> Document {
    let mut document = Document::new(DocumentId::parse("ring-test").unwrap(), "Ring test").unwrap();
    let mut commands = Vec::new();
    for i in 0..n {
        commands.push(Command::AddAtom {
            atom: Atom::from(Element::CARBON),
            positions: NewAtomPositions::Single(Point::new(i as f64, 0.0).unwrap()),
            bond_to: None,
        });
    }
    for &(a, b) in edges {
        commands.push(Command::AddBond {
            begin: AtomRef::Created(a),
            end: AtomRef::Created(b),
            order: BondOrder::Single,
        });
    }
    document.apply(&commands).unwrap();
    document
}

#[test]
fn fused_cycles_form_one_block_and_bridge_is_not_a_ring_bond() {
    // Two six-membered rings sharing one edge, plus a pendant edge.
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
        (9, 10),
    ];
    let doc = graph(11, &edges);
    let before = doc.clone();
    let systems = rings::perceive(&MoleculeGraph::from_document(&doc));
    assert_eq!(systems.len(), 1);
    assert_eq!(
        (
            systems[0].atoms.len(),
            systems[0].bonds.len(),
            systems[0].cycle_rank
        ),
        (10, 11, 2)
    );
    assert_eq!(
        systems[0]
            .bonds
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        (1..=11).map(|n| format!("b{n}")).collect::<Vec<_>>()
    );
    assert_eq!(doc, before);
}

#[test]
fn spiro_cycles_remain_separate_and_acyclic_components_have_no_system() {
    let doc = graph(
        9,
        &[
            (0, 1),
            (1, 2),
            (2, 0),
            (2, 3),
            (3, 4),
            (4, 5),
            (5, 2),
            (6, 7),
            (7, 8),
        ],
    );
    let systems = rings::perceive(&MoleculeGraph::from_document(&doc));
    assert_eq!(
        systems
            .iter()
            .map(|s| (s.atoms.len(), s.bonds.len(), s.cycle_rank))
            .collect::<Vec<_>>(),
        [(3, 3, 1), (4, 4, 1)]
    );
    assert_eq!(
        systems[0]
            .atoms
            .iter()
            .filter(|id| systems[1].atoms.contains(id))
            .count(),
        1
    );
    assert!(
        rings::perceive(&MoleculeGraph::from_document(&graph(3, &[(0, 1), (1, 2)]))).is_empty()
    );
}

#[test]
fn renumbering_does_not_change_ring_membership_or_rank() {
    let edges = [(0, 1), (1, 2), (2, 3), (3, 0), (0, 2), (3, 4)];
    let permutation = [4, 2, 0, 3, 1];
    let permuted: Vec<_> = edges
        .iter()
        .map(|&(a, b)| (permutation[a], permutation[b]))
        .rev()
        .collect();
    let original = rings::perceive(&MoleculeGraph::from_document(&graph(5, &edges)));
    let reordered = rings::perceive(&MoleculeGraph::from_document(&graph(5, &permuted)));
    assert_eq!(
        original
            .iter()
            .map(|s| (s.atoms.len(), s.bonds.len(), s.cycle_rank))
            .collect::<Vec<_>>(),
        vec![(4, 5, 2)]
    );
    assert_eq!(
        reordered
            .iter()
            .map(|s| (s.atoms.len(), s.bonds.len(), s.cycle_rank))
            .collect::<Vec<_>>(),
        vec![(4, 5, 2)]
    );
}

#[test]
fn every_four_vertex_graph_matches_an_edge_removal_cycle_oracle() {
    use std::collections::{BTreeSet, VecDeque};
    let possible = [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)];
    for mask in 0..(1 << possible.len()) {
        let edges: Vec<_> = possible
            .iter()
            .enumerate()
            .filter_map(|(i, e)| ((mask >> i) & 1 == 1).then_some(*e))
            .collect();
        let actual: BTreeSet<_> = rings::perceive(&MoleculeGraph::from_document(&graph(4, &edges)))
            .into_iter()
            .flat_map(|s| s.bonds)
            .map(|id| id.serial() as usize - 1)
            .collect();
        let mut expected = BTreeSet::new();
        for (excluded, &(from, to)) in edges.iter().enumerate() {
            let mut seen = [false; 4];
            seen[from] = true;
            let mut queue = VecDeque::from([from]);
            while let Some(v) = queue.pop_front() {
                for (i, &(a, b)) in edges.iter().enumerate() {
                    if i == excluded {
                        continue;
                    }
                    let neighbor = if a == v {
                        Some(b)
                    } else if b == v {
                        Some(a)
                    } else {
                        None
                    };
                    if let Some(w) = neighbor
                        && !seen[w]
                    {
                        seen[w] = true;
                        queue.push_back(w);
                    }
                }
            }
            if seen[to] {
                expected.insert(excluded);
            }
        }
        assert_eq!(actual, expected, "four-vertex graph mask {mask:#08b}");
    }
}
