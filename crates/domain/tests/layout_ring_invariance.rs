//! The simple-ring layout's renumbering invariance, root rule and idempotence through the public
//! API. A renumbered drawing is built with its atoms created in another order, and it must place
//! each atom where the original does, matched by before position. The other expected values come
//! from the rules in the documentation of `chemifree_domain::layout`. Random unit trees are checked
//! against an all-pairs breadth-first search over the units they were built from, which shares no
//! code with the rooting.

use std::collections::{BTreeSet, VecDeque};

use chemifree_domain::document::{
    Atom, AtomId, AtomRef, BondOrder, Command, Document, DocumentId, NewAtomPositions, Point,
};
use chemifree_domain::element::Element;
use chemifree_domain::layout::{self, Preview, Scope};
use chemifree_domain::scene::Selection;

/// The tolerance, in pt, of every comparison of a computed position.
const TOLERANCE: f64 = 1e-9;

/// The length of every bond of the drawings below, in pt.
const BOND: f64 = 30.0;

/// The number of seeded random unit trees that the root test checks.
const TREE_COUNT: u64 = 200;

fn carbon() -> Atom {
    Atom::from(Element::CARBON)
}

fn point(x: f64, y: f64) -> Point {
    Point::new(x, y).expect("a finite position within the coordinate limits")
}

fn atom(serial: u32) -> AtomId {
    AtomId::from_serial(serial).expect("a positive serial")
}

/// The vertices of a regular hexagon of side `BOND` centred on the origin, vertex `k` at `60 * k`
/// degrees, as `insert_ring` draws one.
fn hexagon_positions() -> Vec<(f64, f64)> {
    (0..6_u32)
        .map(|k| {
            let angle = (60.0 * f64::from(k)).to_radians();
            (BOND * angle.cos(), BOND * angle.sin())
        })
        .collect()
}

/// The bonds of the cycle of `count` atoms from index `first`, in walk order.
fn cycle_bonds(first: usize, count: usize) -> Vec<(usize, usize)> {
    (0..count)
        .map(|k| (first + k, first + (k + 1) % count))
        .collect()
}

/// The indices `0..count` in increasing order.
fn in_order(count: usize) -> Vec<usize> {
    (0..count).collect()
}

/// A drawing as data: the position of each atom, its single bonds by index, and the atoms that the
/// layout selects.
struct Drawing {
    positions: Vec<(f64, f64)>,
    bonds: Vec<(usize, usize)>,
    selected: Vec<usize>,
}

/// A drawing built as a document, with the identifier of each of its atoms by index.
struct Built {
    document: Document,
    ids: Vec<AtomId>,
}

/// Builds `drawing` in one transaction: its atoms are created in the index order `order`, and its
/// bonds follow them. The atom at index `order[k]` takes the k-th identifier, so another order gives
/// the same atoms other identifiers.
fn build(name: &str, drawing: &Drawing, order: &[usize]) -> Built {
    let mut document = Document::new(DocumentId::parse(name).unwrap(), name).unwrap();
    let mut created: Vec<usize> = vec![0; drawing.positions.len()];
    for (position, &index) in order.iter().enumerate() {
        created[index] = position;
    }
    let mut commands: Vec<Command> = order
        .iter()
        .map(|&index| {
            let (x, y) = drawing.positions[index];
            Command::AddAtom {
                atom: carbon(),
                positions: NewAtomPositions::Single(point(x, y)),
                bond_to: None,
            }
        })
        .collect();
    commands.extend(drawing.bonds.iter().map(|&(begin, end)| Command::AddBond {
        begin: AtomRef::Created(created[begin]),
        end: AtomRef::Created(created[end]),
        order: BondOrder::Single,
    }));
    document.apply(&commands).unwrap();
    // Identifiers grow with each atom added, so the sorted identifiers are the creation order.
    let by_creation: Vec<AtomId> = document.atoms().map(|(id, _)| id).collect();
    let ids: Vec<AtomId> = created
        .iter()
        .map(|&position| by_creation[position])
        .collect();
    Built { document, ids }
}

/// The selection of the given atoms.
fn selection_of(atoms: impl IntoIterator<Item = AtomId>) -> Selection {
    Selection {
        atoms: atoms.into_iter().collect(),
        ..Selection::default()
    }
}

/// The selection of every atom of `document`.
fn every_atom(document: &Document) -> Selection {
    selection_of(document.atoms().map(|(id, _)| id))
}

/// The selection of the atoms that `drawing` selects in `built`.
fn drawn_selection(built: &Built, drawing: &Drawing) -> Selection {
    selection_of(drawing.selected.iter().map(|&index| built.ids[index]))
}

/// Prepares and places `selection` of `document`. A refusal fails the test, and `label` names the
/// case in its message.
fn place_selection(document: &Document, selection: &Selection, label: &str) -> Preview {
    let prepared = layout::prepare(
        document,
        document.first_instance(),
        selection,
        Scope::Selection,
    )
    .unwrap_or_else(|error| panic!("{label}: the selection is refused: {error:?}"));
    layout::place(&prepared)
        .unwrap_or_else(|error| panic!("{label}: the layout is refused: {error:?}"))
}

/// Whether `a` and `b` lie within `TOLERANCE` of each other on both axes.
fn near(a: Point, b: Point) -> bool {
    (a.x() - b.x()).abs() <= TOLERANCE && (a.y() - b.y()).abs() <= TOLERANCE
}

/// The placed position of the layout atom `target` in `preview`.
fn after_of(preview: &Preview, target: AtomId) -> Point {
    preview
        .positions
        .iter()
        .find(|placement| placement.atom == target)
        .unwrap_or_else(|| panic!("{target:?} is not laid out"))
        .after
}

/// Asserts that `second` places each layout atom of `first` at the same position, matched by the
/// before position of the atom.
fn assert_same_placements(first: &Preview, second: &Preview, label: &str) {
    assert_eq!(
        first.positions.len(),
        second.positions.len(),
        "{label}: the renumbered layout has another number of atoms"
    );
    for placement in &first.positions {
        let twin = second
            .positions
            .iter()
            .find(|other| other.before == placement.before)
            .unwrap_or_else(|| {
                panic!(
                    "{label}: no renumbered atom starts at {:?}",
                    placement.before
                )
            });
        assert!(
            near(placement.after, twin.after),
            "{label}: the atom starting at {:?} is placed at {:?}, renumbered at {:?}",
            placement.before,
            placement.after,
            twin.after
        );
    }
}

/// Asserts that `actual` and `expected` hold the same points: each point of one lies within
/// `TOLERANCE` of a point of the other, and both hold the same number of points.
fn assert_same_points(actual: &[Point], expected: &[Point], label: &str) {
    assert_eq!(
        actual.len(),
        expected.len(),
        "{label}: the point counts differ"
    );
    for (from, to) in [(actual, expected), (expected, actual)] {
        for candidate in from {
            assert!(
                to.iter().any(|other| near(*candidate, *other)),
                "{label}: {candidate:?} is not one of {to:?}"
            );
        }
    }
}

/// Asserts that `drawing` keeps each placed position when its atoms are created in reverse order.
/// Returns the placement of the drawing in index order.
fn assert_renumbering_keeps_positions(drawing: &Drawing, label: &str) -> Preview {
    let order = in_order(drawing.positions.len());
    let reversed: Vec<usize> = order.iter().rev().copied().collect();
    let indexed = build("renumbering", drawing, &order);
    let renumbered = build("renumbering-reversed", drawing, &reversed);
    assert_ne!(
        indexed.ids, renumbered.ids,
        "{label}: the reversed order must renumber the atoms"
    );
    let placed = place_selection(
        &indexed.document,
        &drawn_selection(&indexed, drawing),
        label,
    );
    let replaced = place_selection(
        &renumbered.document,
        &drawn_selection(&renumbered, drawing),
        label,
    );
    assert_same_placements(&placed, &replaced, label);
    placed
}

#[test]
fn renumbering_an_asymmetric_1_ethyl_3_methylcyclohexane_keeps_each_position() {
    // Ring carbons 0 to 5 around the origin, an ethyl (carbons 6 and 7) on carbon 0 and a methyl
    // (carbon 8) on carbon 2, so the substituents sit at the 1 and 3 positions of the ring.
    let hexagon = hexagon_positions();
    let methyl = (2.0 * hexagon[2].0, 2.0 * hexagon[2].1);
    let mut positions = hexagon;
    positions.extend([(60.0, 0.0), (90.0, 0.0), methyl]);
    let mut bonds = cycle_bonds(0, 6);
    bonds.extend([(0, 6), (6, 7), (2, 8)]);
    let drawing = Drawing {
        positions,
        bonds,
        selected: in_order(9),
    };
    let placed = assert_renumbering_keeps_positions(&drawing, "1-ethyl-3-methylcyclohexane");
    assert_eq!(placed.positions.len(), 9, "every atom is laid out");
}

#[test]
fn renumbering_a_substituted_ring_in_an_anchored_component_keeps_each_position() {
    // The fixed carbon (index 0) is bonded to ring carbon 1, the boundary atom. A methyl (index 7)
    // sits on ring carbon 3, which is meta to the boundary atom. Only the ring and the methyl are
    // selected.
    let hexagon = hexagon_positions();
    let methyl = (2.0 * hexagon[2].0, 2.0 * hexagon[2].1);
    let mut positions = vec![(60.0, 0.0)];
    positions.extend(hexagon);
    positions.push(methyl);
    let mut bonds = vec![(0, 1)];
    bonds.extend(cycle_bonds(1, 6));
    bonds.push((3, 7));
    let drawing = Drawing {
        positions,
        bonds,
        selected: (1..=7).collect(),
    };
    let placed = assert_renumbering_keeps_positions(&drawing, "anchored substituted ring");
    assert_eq!(placed.components.len(), 1, "one anchored component");
    let component = &placed.components[0];
    // In the identifiers of the first order, the fixed carbon is a1 and the boundary atom is a2.
    assert_eq!(
        component.anchor.as_ref().map(|anchor| anchor.fixed_atom),
        Some(atom(1)),
        "the ring hangs from the fixed carbon a1"
    );
    assert_eq!(
        component.root,
        atom(2),
        "the boundary atom a2 roots the component"
    );
    assert_eq!(placed.positions.len(), 7, "every selected atom is laid out");
}

#[test]
fn renumbering_a_plain_hexagon_keeps_its_set_of_positions() {
    let drawing = Drawing {
        positions: hexagon_positions(),
        bonds: cycle_bonds(0, 6),
        selected: in_order(6),
    };
    let indexed = build("hexagon", &drawing, &in_order(6));
    // Creating the atoms from vertex 3 gives vertex 0 another identifier and another entry atom.
    let from_vertex_three = build("hexagon-from-vertex-3", &drawing, &[3, 4, 5, 0, 1, 2]);
    let placed = place_selection(
        &indexed.document,
        &drawn_selection(&indexed, &drawing),
        "hexagon",
    );
    let renumbered = place_selection(
        &from_vertex_three.document,
        &drawn_selection(&from_vertex_three, &drawing),
        "hexagon from vertex 3",
    );
    let drawn: Vec<Point> = placed
        .positions
        .iter()
        .map(|placement| placement.before)
        .collect();
    let after: Vec<Point> = placed
        .positions
        .iter()
        .map(|placement| placement.after)
        .collect();
    let after_renumbered: Vec<Point> = renumbered
        .positions
        .iter()
        .map(|placement| placement.after)
        .collect();
    assert_same_points(
        &after,
        &after_renumbered,
        "the renumbered hexagon keeps its set of positions",
    );
    // A free hexagon at the origin is already placed (example E1), so the set is the drawn one.
    assert_same_points(
        &after,
        &drawn,
        "the free hexagon stays on its drawn positions",
    );
}

/// Example E2, methylcyclohexane: the hexagon of `insert_ring` at the origin, and a methyl carbon at
/// (60, 0) bonded to the ring carbon at (30, 0). The methyl is the seventh atom, a7, unless
/// `methyl_first`, which makes it a1 and gives the ring the identifiers a2 to a7.
fn methylcyclohexane(methyl_first: bool) -> Built {
    let mut positions = hexagon_positions();
    positions.push((60.0, 0.0));
    let mut bonds = cycle_bonds(0, 6);
    bonds.push((0, 6));
    let drawing = Drawing {
        positions,
        bonds,
        selected: in_order(7),
    };
    let order = if methyl_first {
        vec![6, 0, 1, 2, 3, 4, 5]
    } else {
        in_order(7)
    };
    build("methylcyclohexane", &drawing, &order)
}

#[test]
fn a_free_methylcyclohexane_is_rooted_at_its_methyl() {
    // The ends of the longest path are the methyl and the ring. The ring's identifier is its
    // smallest member, so with the methyl first the ring comes last in the order the ends are
    // compared in, which is the order that a wrong rule would be sensitive to.
    for methyl_first in [false, true] {
        let label = format!("methylcyclohexane, methyl first: {methyl_first}");
        let built = methylcyclohexane(methyl_first);
        let placed = place_selection(&built.document, &every_atom(&built.document), &label);
        assert_eq!(placed.components.len(), 1, "{label}: one free component");
        assert_eq!(
            placed.components[0].root, built.ids[6],
            "{label}: an atom end beats a ring end, so the methyl roots the component"
        );
    }
}

#[test]
fn laying_out_a_laid_out_ring_component_again_moves_nothing() {
    let built = methylcyclohexane(false);
    let mut document = built.document;
    let instance = document.first_instance();
    let first = place_selection(&document, &every_atom(&document), "first layout");
    let moves = layout::move_commands(&first, instance);
    assert!(
        !moves.is_empty(),
        "the first layout must move atoms, or the second one proves nothing"
    );
    document.apply(&moves).unwrap();
    let second = place_selection(&document, &every_atom(&document), "second layout");
    let again = layout::move_commands(&second, instance);
    assert!(
        again.is_empty(),
        "a laid-out ring moved {} atoms when it was laid out again",
        again.len()
    );
}

/// The next value of the linear congruential generator of the chain layout tests, below `bound`.
/// `state` is its seed.
fn lcg_next(state: &mut u64, bound: usize) -> usize {
    *state = state
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    ((*state >> 33) as usize) % bound
}

/// A random tree of units from `seed`, with 2 to 10 units. A unit is a carbon or a simple ring of
/// 3 to 8 carbons. Each unit after the first is joined by one single bond to a random atom of an
/// earlier unit. Atoms are 60 pt apart in a row, so none coincide.
struct UnitTree {
    drawing: Drawing,
    /// The atom indices of each unit: one index for a carbon, and the members of a ring.
    units: Vec<Vec<usize>>,
    /// The pairs of units that a bond joins, by unit index.
    unit_edges: Vec<(usize, usize)>,
}

fn random_unit_tree(seed: u64) -> UnitTree {
    let mut state = seed;
    let unit_count = 2 + lcg_next(&mut state, 9);
    let mut drawing = Drawing {
        positions: Vec::new(),
        bonds: Vec::new(),
        selected: Vec::new(),
    };
    let mut units: Vec<Vec<usize>> = Vec::with_capacity(unit_count);
    let mut unit_edges = Vec::with_capacity(unit_count - 1);
    for unit in 0..unit_count {
        let size = if lcg_next(&mut state, 2) == 0 {
            1
        } else {
            3 + lcg_next(&mut state, 6)
        };
        let first = drawing.positions.len();
        let members: Vec<usize> = (first..first + size).collect();
        drawing.positions.extend(
            members
                .iter()
                .map(|&member| (2.0 * BOND * member as f64, 0.0)),
        );
        if size >= 3 {
            drawing.bonds.extend(cycle_bonds(first, size));
        }
        if unit > 0 {
            let parent_atom = lcg_next(&mut state, first);
            drawing.bonds.push((parent_atom, first));
            let parent_unit = units
                .iter()
                .position(|unit_members| unit_members.contains(&parent_atom))
                .expect("every earlier atom belongs to a unit");
            unit_edges.push((parent_unit, unit));
        }
        units.push(members);
    }
    drawing.selected = in_order(drawing.positions.len());
    UnitTree {
        drawing,
        units,
        unit_edges,
    }
}

/// Whether unit members form a ring. A carbon unit has one atom, and a ring has 3 to 8.
fn is_ring(members: &[usize]) -> bool {
    members.len() >= 3
}

/// The distance in unit edges between every two units, by a breadth-first search from each unit.
fn unit_distances(unit_count: usize, unit_edges: &[(usize, usize)]) -> Vec<Vec<usize>> {
    let mut adjacent: Vec<Vec<usize>> = vec![Vec::new(); unit_count];
    for &(first, second) in unit_edges {
        adjacent[first].push(second);
        adjacent[second].push(first);
    }
    (0..unit_count)
        .map(|source| {
            let mut distance: Vec<Option<usize>> = vec![None; unit_count];
            distance[source] = Some(0);
            let mut queue = VecDeque::from([source]);
            while let Some(unit) = queue.pop_front() {
                let here = distance[unit].expect("a queued unit has a distance");
                for &next in &adjacent[unit] {
                    if distance[next].is_none() {
                        distance[next] = Some(here + 1);
                        queue.push_back(next);
                    }
                }
            }
            distance
                .into_iter()
                .map(|found| found.expect("the unit tree is connected"))
                .collect()
        })
        .collect()
}

/// The two atoms of a bond, smaller identifier first.
fn ordered_pair(first: AtomId, second: AtomId) -> (AtomId, AtomId) {
    (first.min(second), first.max(second))
}

/// What one checked tree covered: whether the ends of its longest unit path include both an atom
/// and a ring, and whether its root is a ring member.
#[derive(Clone, Copy, Debug)]
struct Coverage {
    mixed_ends: bool,
    ring_rooted: bool,
}

/// Checks the placement of the random tree of `seed` against the oracle, and reports what the tree
/// covered.
fn check_unit_tree(seed: u64) -> Coverage {
    let label = format!("seed {seed}");
    let tree = random_unit_tree(seed);
    let built = build(
        "unit-tree",
        &tree.drawing,
        &in_order(tree.drawing.positions.len()),
    );
    let preview = place_selection(
        &built.document,
        &drawn_selection(&built, &tree.drawing),
        &label,
    );
    assert_eq!(
        preview.components.len(),
        1,
        "{label}: a connected tree is one component"
    );
    let component = &preview.components[0];

    // The oracle: the eccentricity of a unit is its greatest distance to another unit, and the ends
    // of a longest unit path are the units whose eccentricity is the diameter.
    let distances = unit_distances(tree.units.len(), &tree.unit_edges);
    let eccentricity: Vec<usize> = distances
        .iter()
        .map(|row| *row.iter().max().expect("a unit row is not empty"))
        .collect();
    let diameter = *eccentricity
        .iter()
        .max()
        .expect("a tree has at least one unit");
    let ends: Vec<usize> = (0..tree.units.len())
        .filter(|&unit| eccentricity[unit] == diameter)
        .collect();
    let end_atoms: BTreeSet<usize> = ends
        .iter()
        .flat_map(|&unit| tree.units[unit].iter().copied())
        .collect();
    let ring_end = ends.iter().any(|&unit| is_ring(&tree.units[unit]));
    let atom_end = ends.iter().any(|&unit| !is_ring(&tree.units[unit]));

    let root_index = built
        .ids
        .iter()
        .position(|id| *id == component.root)
        .expect("the root is a layout atom");
    let root_in_ring = tree
        .units
        .iter()
        .any(|members| is_ring(members) && members.contains(&root_index));
    assert!(
        end_atoms.contains(&root_index),
        "{label}: the root {:?} is not in an end of a longest unit path",
        component.root
    );
    if atom_end {
        assert!(
            !root_in_ring,
            "{label}: the root {:?} is a ring member, but an end of a longest path is an atom",
            component.root
        );
    }

    // Every layout atom is in exactly one chain or ring, and no run is empty.
    let mut run_atoms: Vec<AtomId> = Vec::new();
    for run in component.chains.iter().chain(&component.rings) {
        assert!(
            !run.atoms.is_empty(),
            "{label}: a run of the layout is empty"
        );
        run_atoms.extend(run.atoms.iter().copied());
    }
    let covered: BTreeSet<AtomId> = run_atoms.iter().copied().collect();
    assert_eq!(
        covered.len(),
        run_atoms.len(),
        "{label}: an atom is in two runs"
    );
    let every: BTreeSet<AtomId> = built.ids.iter().copied().collect();
    assert_eq!(
        covered, every,
        "{label}: the runs do not cover every layout atom"
    );

    // Each ring is one ring unit of the tree, walked along its bonds at the bond length.
    let placed_rings: BTreeSet<BTreeSet<AtomId>> = component
        .rings
        .iter()
        .map(|ring| ring.atoms.iter().copied().collect::<BTreeSet<AtomId>>())
        .collect();
    let unit_rings: BTreeSet<BTreeSet<AtomId>> = tree
        .units
        .iter()
        .filter(|members| is_ring(members))
        .map(|members| {
            members
                .iter()
                .map(|&index| built.ids[index])
                .collect::<BTreeSet<AtomId>>()
        })
        .collect();
    assert_eq!(
        placed_rings, unit_rings,
        "{label}: the rings are not the ring units of the tree"
    );
    let bonded: BTreeSet<(AtomId, AtomId)> = tree
        .drawing
        .bonds
        .iter()
        .map(|&(first, second)| ordered_pair(built.ids[first], built.ids[second]))
        .collect();
    for ring in &component.rings {
        let successors = ring.atoms.iter().copied().cycle().skip(1);
        for (from, to) in ring.atoms.iter().copied().zip(successors) {
            assert!(
                bonded.contains(&ordered_pair(from, to)),
                "{label}: the ring walks from {from:?} to {to:?}, which is not a bond"
            );
            let (start, end) = (after_of(&preview, from), after_of(&preview, to));
            let length = (start.x() - end.x()).hypot(start.y() - end.y());
            assert!(
                (length - BOND).abs() <= TOLERANCE,
                "{label}: the ring bond {from:?}-{to:?} is {length} pt, not {BOND} pt"
            );
        }
    }

    Coverage {
        mixed_ends: ring_end && atom_end,
        ring_rooted: root_in_ring,
    }
}

#[test]
fn random_unit_trees_are_rooted_at_an_end_of_a_longest_unit_path_and_prefer_atom_ends() {
    let mut mixed_ends = 0;
    let mut ring_rooted = 0;
    for seed in 1..=TREE_COUNT {
        let coverage = check_unit_tree(seed);
        mixed_ends += usize::from(coverage.mixed_ends);
        ring_rooted += usize::from(coverage.ring_rooted);
    }
    assert!(
        mixed_ends > 0,
        "no tree has ends of both kinds, so the preference of atom ends is never tested"
    );
    assert!(
        ring_rooted > 0,
        "no tree is rooted at a ring, so the ring end is never tested"
    );
}
