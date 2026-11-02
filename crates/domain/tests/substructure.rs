//! Substructure matching: what a mapping is, what is compared, what is refused, and the bounds.

use chemifree_domain::aromaticity::{self, Classification};
use chemifree_domain::document::{
    Atom, AtomId, AtomRef, BondId, BondOrder, Command, Document, DocumentId, NewAtomPositions,
    Point,
};
use chemifree_domain::element::Element;
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::states::{
    AtomKind, BondQuery, Hydrogens, QueryAtom, QueryBondOrder, Radical,
};
use chemifree_domain::substructure::{
    self, Item, MAX_QUERY_ATOMS, MAX_SEARCH_STEPS, MatchError, Matches, Side,
};

/// An atom: `C`, `N+`, `O-`, `13C`, `N@1` (one stated hydrogen), `*`, `A`, `Q`, `X`,
/// `[N,O]` or `![N,O]`.
fn atom(spec: &str) -> Atom {
    let element = |symbol: &str| Element::from_symbol(symbol).unwrap_or_else(|| panic!("{spec}"));
    match spec {
        "*" => return Atom::new(AtomKind::Query(QueryAtom::Any)),
        "A" => return Atom::new(AtomKind::Query(QueryAtom::AnyHeavy)),
        "Q" => return Atom::new(AtomKind::Query(QueryAtom::Heteroatom)),
        "X" => return Atom::new(AtomKind::Query(QueryAtom::Halogen)),
        _ => {}
    }
    if let Some(list) = spec.strip_prefix("![").or_else(|| spec.strip_prefix('[')) {
        let elements = list
            .trim_end_matches(']')
            .split(',')
            .map(element)
            .collect::<Vec<_>>();
        return Atom::new(AtomKind::Query(QueryAtom::List {
            elements,
            negated: spec.starts_with('!'),
        }));
    }
    let (spec, hydrogens) = match spec.split_once('@') {
        Some((spec, count)) => (spec, Some(count.parse::<u8>().unwrap())),
        None => (spec, None),
    };
    let digits = spec.bytes().take_while(u8::is_ascii_digit).count();
    let isotope = (digits > 0).then(|| spec[..digits].parse::<u16>().unwrap());
    let rest = &spec[digits..];
    let sign = rest.find(['+', '-']).unwrap_or(rest.len());
    let mut result = Atom::from(element(&rest[..sign]));
    result.isotope = isotope;
    result.charge = match &rest[sign..] {
        "" => 0,
        "+" => 1,
        "-" => -1,
        other => other.parse().unwrap(),
    };
    if let Some(count) = hydrogens {
        result.hydrogens = Hydrogens::Specified(count);
    }
    result
}

/// A bond: `-` `=` `#` `:` (aromatic) `>` (dative) `?` (unknown) `~` (query: any), or two or
/// more of `-=#:` for a query list.
fn order(spec: &str) -> BondOrder {
    match spec {
        "-" => BondOrder::Single,
        "=" => BondOrder::Double,
        "#" => BondOrder::Triple,
        ":" => BondOrder::Aromatic,
        ">" => BondOrder::Dative,
        "?" => BondOrder::Unknown,
        "~" => BondOrder::Query(BondQuery::Any),
        list => BondOrder::Query(BondQuery::OneOf(
            list.chars()
                .map(|c| match c {
                    '-' => QueryBondOrder::Single,
                    '=' => QueryBondOrder::Double,
                    '#' => QueryBondOrder::Triple,
                    ':' => QueryBondOrder::Aromatic,
                    other => panic!("bond list {other}"),
                })
                .collect(),
        )),
    }
}

type Bonds<'a> = &'a [(usize, usize, &'a str)];

fn graph_of(atoms: Vec<Atom>, bonds: Bonds<'_>) -> MoleculeGraph {
    let mut document = Document::new(DocumentId::parse("match-test").unwrap(), "match").unwrap();
    let mut commands: Vec<Command> = atoms
        .into_iter()
        .map(|atom| Command::AddAtom {
            atom,
            positions: NewAtomPositions::Single(Point::new(0.0, 0.0).unwrap()),
            bond_to: None,
        })
        .collect();
    // Atoms are numbered from a1 in creation order, which is what the bonds name.
    let named = |index: usize| AtomRef::Id(id(u32::try_from(index + 1).unwrap()));
    commands.extend(bonds.iter().map(|&(a, b, kind)| Command::AddBond {
        begin: named(a),
        end: named(b),
        order: order(kind),
    }));
    for chunk in commands.chunks(500) {
        document.apply(chunk).unwrap();
    }
    MoleculeGraph::from_document(&document)
}

fn mol(atoms: &[&str], bonds: Bonds<'_>) -> MoleculeGraph {
    graph_of(atoms.iter().map(|spec| atom(spec)).collect(), bonds)
}

/// The atom IDs of a graph in creation order, as `a1`, `a2`, ...
fn id(serial: u32) -> AtomId {
    AtomId::from_serial(serial).unwrap()
}

fn bond_id(serial: u32) -> BondId {
    BondId::from_serial(serial).unwrap()
}

/// Every mapping as the target atoms, in query-atom order, as zero-based creation indices.
fn assignments(matches: &Matches) -> Vec<Vec<u32>> {
    matches
        .mappings
        .iter()
        .map(|mapping| mapping.atoms.iter().map(|(_, t)| t.serial() - 1).collect())
        .collect()
}

fn found(query: &MoleculeGraph, target: &MoleculeGraph) -> Vec<Vec<u32>> {
    assignments(&substructure::find(query, target).unwrap())
}

fn refusal(query: &MoleculeGraph, target: &MoleculeGraph) -> MatchError {
    substructure::find(query, target).expect_err("a refusal")
}

fn ring(size: usize, kind: &str) -> (Vec<&'static str>, Vec<(usize, usize, String)>) {
    let atoms = vec!["C"; size];
    let bonds = (0..size)
        .map(|i| (i, (i + 1) % size, kind.to_owned()))
        .collect();
    (atoms, bonds)
}

fn cycle(size: usize, kind: &str) -> MoleculeGraph {
    let (atoms, bonds) = ring(size, kind);
    let bonds: Vec<_> = bonds.iter().map(|(a, b, k)| (*a, *b, k.as_str())).collect();
    mol(&atoms, &bonds)
}

/// Benzene drawn Kekulé: alternate double and single bonds.
fn kekule_benzene() -> MoleculeGraph {
    mol(
        &["C"; 6],
        &[
            (0, 1, "="),
            (1, 2, "-"),
            (2, 3, "="),
            (3, 4, "-"),
            (4, 5, "="),
            (5, 0, "-"),
        ],
    )
}

fn aromatic_benzene() -> MoleculeGraph {
    cycle(6, ":")
}

#[test]
fn a_mapping_names_query_and_target_atoms_and_bonds() {
    // Query C-O in ethanol C-C-O: one mapping, query a1 -> a2, a2 -> a3.
    let query = mol(&["C", "O"], &[(0, 1, "-")]);
    let target = mol(&["C", "C", "O"], &[(0, 1, "-"), (1, 2, "-")]);
    let matches = substructure::find(&query, &target).unwrap();
    assert_eq!(matches.mappings.len(), 1);
    assert_eq!(
        matches.mappings[0].atoms,
        vec![(id(1), id(2)), (id(2), id(3))]
    );
    assert_eq!(matches.mappings[0].bonds, vec![(bond_id(1), bond_id(2))]);
}

#[test]
fn a_symmetric_query_is_found_once_per_symmetry_operation() {
    // Propane in propane: both ends can be either end. Benzene in benzene: 6 rotations x 2
    // reflections. Cyclohexane in cyclohexane: the same twelve.
    let propane = mol(&["C", "C", "C"], &[(0, 1, "-"), (1, 2, "-")]);
    assert_eq!(
        found(&propane, &propane),
        vec![vec![0, 1, 2], vec![2, 1, 0]]
    );
    assert_eq!(found(&kekule_benzene(), &kekule_benzene()).len(), 12);
    assert_eq!(found(&aromatic_benzene(), &aromatic_benzene()).len(), 12);
    assert_eq!(found(&cycle(6, "-"), &cycle(6, "-")).len(), 12);
}

#[test]
fn the_bond_assignment_agrees_with_the_atom_assignment() {
    let matches = substructure::find(&kekule_benzene(), &aromatic_benzene()).unwrap();
    let query = kekule_benzene();
    let target = aromatic_benzene();
    let ends = |graph: &MoleculeGraph| -> std::collections::BTreeMap<BondId, (AtomId, AtomId)> {
        graph
            .bonds()
            .map(|(id, b)| (id, (b.begin, b.end)))
            .collect()
    };
    let (query_ends, target_ends) = (ends(&query), ends(&target));
    for mapping in &matches.mappings {
        let atoms: std::collections::BTreeMap<_, _> = mapping.atoms.iter().copied().collect();
        assert_eq!(mapping.bonds.len(), 6);
        for (qb, tb) in &mapping.bonds {
            let (qa, qz) = query_ends[qb];
            let (ta, tz) = target_ends[tb];
            let image = (atoms[&qa], atoms[&qz]);
            assert!(
                image == (ta, tz) || image == (tz, ta),
                "bond {qb}->{tb} must join the images of its atoms: {mapping:?}"
            );
        }
    }
}

#[test]
fn kekule_and_aromatic_spellings_of_the_same_ring_agree() {
    for query in [kekule_benzene(), aromatic_benzene()] {
        for target in [kekule_benzene(), aromatic_benzene()] {
            assert_eq!(found(&query, &target).len(), 12);
        }
    }
    // The other Kekulé form (double bonds on the other three) is the same ring.
    let shifted = mol(
        &["C"; 6],
        &[
            (0, 1, "-"),
            (1, 2, "="),
            (2, 3, "-"),
            (3, 4, "="),
            (4, 5, "-"),
            (5, 0, "="),
        ],
    );
    assert_eq!(found(&kekule_benzene(), &shifted).len(), 12);
    // Cyclohexane is not benzene, in either direction.
    assert!(found(&aromatic_benzene(), &cycle(6, "-")).is_empty());
    assert!(found(&cycle(6, "-"), &kekule_benzene()).is_empty());
}

#[test]
fn pyridine_and_pyrrole_nitrogens_follow_the_aromaticity_model() {
    let pyridine_kekule = mol(
        &["C", "C", "C", "C", "C", "N"],
        &[
            (0, 1, "="),
            (1, 2, "-"),
            (2, 3, "="),
            (3, 4, "-"),
            (4, 5, "="),
            (5, 0, "-"),
        ],
    );
    let pyridine_aromatic = mol(
        &["C", "C", "C", "C", "C", "N"],
        &(0..6).map(|i| (i, (i + 1) % 6, ":")).collect::<Vec<_>>(),
    );
    // Pyridine in pyridine: the nitrogen fixes the ring, so only the two reflections remain.
    assert_eq!(found(&pyridine_kekule, &pyridine_aromatic).len(), 2);
    assert_eq!(found(&pyridine_aromatic, &pyridine_kekule).len(), 2);
    // Pyrrole drawn Kekulé (nitrogen hydrogen inferred) matches pyrrole drawn with aromatic
    // bonds and a stated hydrogen.
    let pyrrole_kekule = mol(
        &["C", "C", "C", "C", "N"],
        &[
            (0, 1, "="),
            (1, 2, "-"),
            (2, 3, "="),
            (3, 4, "-"),
            (4, 0, "-"),
        ],
    );
    let pyrrole_aromatic = mol(
        &["C", "C", "C", "C", "N@1"],
        &(0..5).map(|i| (i, (i + 1) % 5, ":")).collect::<Vec<_>>(),
    );
    assert_eq!(found(&pyrrole_kekule, &pyrrole_aromatic).len(), 2);
    assert_eq!(found(&pyrrole_aromatic, &pyrrole_kekule).len(), 2);
    // Pyridine is not pyrrole.
    assert!(found(&pyridine_kekule, &pyrrole_kekule).is_empty());
}

#[test]
fn naphthalene_holds_two_benzenes() {
    // C1=CC=C2C=CC=CC2=C1 spelled Kekulé; the shared bond is 4-9.
    let naphthalene = mol(
        &["C"; 10],
        &[
            (0, 1, "="),
            (1, 2, "-"),
            (2, 3, "="),
            (3, 4, "-"),
            (4, 5, "="),
            (5, 6, "-"),
            (6, 7, "="),
            (7, 8, "-"),
            (8, 9, "="),
            (9, 0, "-"),
            (3, 8, "-"),
        ],
    );
    assert_eq!(found(&aromatic_benzene(), &naphthalene).len(), 24);
    // The ten-atom ring is not a benzene ring: no mapping of cyclohexane-like single bonds.
    assert!(found(&cycle(6, "-"), &naphthalene).is_empty());
}

#[test]
fn query_atoms_match_their_element_sets() {
    let target = mol(
        &["C", "N", "O", "Cl", "H"],
        &[(0, 1, "-"), (1, 2, "-"), (2, 3, "-"), (3, 4, "-")],
    );
    let any_atom = |spec: &str| found(&mol(&[spec], &[]), &target).concat();
    assert_eq!(any_atom("*"), vec![0, 1, 2, 3, 4], "any");
    assert_eq!(any_atom("A"), vec![0, 1, 2, 3], "any but hydrogen");
    assert_eq!(
        any_atom("Q"),
        vec![1, 2, 3],
        "heteroatom: not carbon, not hydrogen"
    );
    assert_eq!(any_atom("X"), vec![3], "halogen");
    assert_eq!(any_atom("[N,O]"), vec![1, 2], "list");
    assert_eq!(
        any_atom("![N,O]"),
        vec![0, 3, 4],
        "negated list keeps hydrogen"
    );
    assert_eq!(any_atom("O"), vec![2], "element");
    // Halogens are group 17: fluorine and astatine match, sulfur does not.
    let halides = mol(&["F", "S", "At", "Br"], &[]);
    assert_eq!(found(&mol(&["X"], &[]), &halides).concat(), vec![0, 2, 3]);
}

#[test]
fn query_bonds_match_their_kinds() {
    let target = mol(
        &["C", "C", "C", "C", "C"],
        &[(0, 1, "-"), (1, 2, "="), (2, 3, "#"), (3, 4, ">")],
    );
    let bond_matches = |kind: &str| {
        found(&mol(&["C", "C"], &[(0, 1, kind)]), &target)
            .iter()
            .filter(|m| m[0] < m[1])
            .count()
    };
    assert_eq!(bond_matches("-"), 1);
    assert_eq!(bond_matches("="), 1);
    assert_eq!(bond_matches("#"), 1);
    assert_eq!(bond_matches("-="), 2, "a list of two kinds");
    assert_eq!(bond_matches("~"), 4, "any bond, even a dative one");
    assert_eq!(
        bond_matches("=:"),
        1,
        "no aromatic bond in the target, so only the double one"
    );
}

#[test]
fn a_query_list_with_the_aromatic_kind_matches_both_spellings() {
    let query = mol(&["C", "C"], &[(0, 1, "-:")]);
    // Benzene drawn Kekulé: every bond is aromatic, single or double as drawn.
    assert_eq!(found(&query, &kekule_benzene()).len(), 12);
    assert_eq!(found(&query, &aromatic_benzene()).len(), 12);
    let only_single = mol(&["C", "C"], &[(0, 1, "-")]);
    assert!(
        found(&only_single, &kekule_benzene()).is_empty(),
        "Kekulé single bonds are aromatic"
    );
}

#[test]
fn charge_and_isotope_are_conditions_only_when_the_query_states_them() {
    let target = mol(&["N+", "N", "13C", "C", "N-"], &[]);
    let hits = |spec: &str| found(&mol(&[spec], &[]), &target).concat();
    assert_eq!(
        hits("N"),
        vec![0, 1, 4],
        "a neutral query atom states no charge"
    );
    assert_eq!(hits("N+"), vec![0]);
    assert_eq!(hits("N-"), vec![4]);
    assert_eq!(hits("C"), vec![2, 3], "no isotope stated: any isotope");
    assert_eq!(hits("13C"), vec![2]);
    assert!(hits("14C").is_empty());
    assert_eq!(
        hits("*"),
        vec![0, 1, 2, 3, 4],
        "a query atom has no charge or isotope condition"
    );
}

#[test]
fn hydrogen_counts_radicals_and_attachment_points_are_not_conditions() {
    let mut stated = atom("C@0");
    stated.radical = Radical::Doublet;
    let query = graph_of(vec![stated], &[]);
    let target = mol(&["C@3", "C", "C@1"], &[]);
    assert_eq!(found(&query, &target).concat(), vec![0, 1, 2]);
    let unknown = {
        let mut a = atom("C");
        a.hydrogens = Hydrogens::Unknown;
        graph_of(vec![a], &[])
    };
    assert_eq!(found(&unknown, &target).concat(), vec![0, 1, 2]);
}

#[test]
fn drawn_hydrogen_atoms_are_ordinary_atoms_and_are_never_folded() {
    let methane = mol(
        &["C", "H", "H", "H", "H"],
        &[(0, 1, "-"), (0, 2, "-"), (0, 3, "-"), (0, 4, "-")],
    );
    let bare = mol(&["C"], &[]);
    let with_hydrogen = mol(&["C", "H"], &[(0, 1, "-")]);
    assert_eq!(found(&bare, &methane), vec![vec![0]]);
    assert_eq!(
        found(&with_hydrogen, &methane).len(),
        4,
        "four ways to pick the hydrogen"
    );
    // The same carbon with its hydrogens left implicit has no hydrogen atom to match.
    assert!(found(&with_hydrogen, &bare).is_empty());
    // Any-but-hydrogen never matches a drawn hydrogen; any does.
    assert_eq!(found(&mol(&["A"], &[]), &methane).len(), 1);
    assert_eq!(found(&mol(&["*"], &[]), &methane).len(), 5);
}

#[test]
fn a_disconnected_query_places_its_parts_on_distinct_atoms() {
    let two_carbons = mol(&["C", "C"], &[]);
    let ethane = mol(&["C", "C"], &[(0, 1, "-")]);
    let propane = mol(&["C", "C", "C"], &[(0, 1, "-"), (1, 2, "-")]);
    assert_eq!(found(&two_carbons, &ethane), vec![vec![0, 1], vec![1, 0]]);
    assert_eq!(found(&two_carbons, &propane).len(), 6);
    // Two parts: an O-H and a C=C; they cannot share an atom.
    let parts = mol(&["O", "H", "C", "C"], &[(0, 1, "-"), (2, 3, "=")]);
    let target = mol(
        &["C", "C", "O", "H"],
        &[(0, 1, "="), (1, 2, "-"), (2, 3, "-")],
    );
    assert_eq!(
        found(&parts, &target),
        vec![vec![2, 3, 0, 1], vec![2, 3, 1, 0]]
    );
}

#[test]
fn the_target_may_have_more_bonds_between_the_assigned_atoms() {
    // A path of three atoms occurs in a triangle: the third bond is extra, which is allowed.
    let path = mol(&["C", "C", "C"], &[(0, 1, "-"), (1, 2, "-")]);
    assert_eq!(found(&path, &cycle(3, "-")).len(), 6);
    // But a triangle does not occur in a path.
    assert!(found(&cycle(3, "-"), &path).is_empty());
}

#[test]
fn a_query_that_does_not_occur_is_a_complete_empty_answer() {
    let nitrile = mol(&["C", "N"], &[(0, 1, "#")]);
    let ethane = mol(&["C", "C"], &[(0, 1, "-")]);
    let matches = substructure::find(&nitrile, &ethane).unwrap();
    assert!(matches.mappings.is_empty());
    // A query larger than the target is the same honest zero.
    let long = mol(&["C", "C", "C"], &[(0, 1, "-"), (1, 2, "-")]);
    assert!(found(&long, &ethane).is_empty());
}

#[test]
fn a_target_with_no_atoms_has_no_matches() {
    let empty = MoleculeGraph::from_parts(Vec::new(), Vec::new());
    assert!(found(&mol(&["C"], &[]), &empty).is_empty());
}

#[test]
fn a_query_must_have_atoms_and_at_most_the_limit() {
    let empty = MoleculeGraph::from_parts(Vec::new(), Vec::new());
    let target = mol(&["C"], &[]);
    assert_eq!(refusal(&empty, &target), MatchError::EmptyQuery);
    let big = mol(&vec!["C"; MAX_QUERY_ATOMS + 1], &[]);
    assert_eq!(
        refusal(&big, &target),
        MatchError::QueryTooLarge {
            atoms: MAX_QUERY_ATOMS + 1
        }
    );
    // Exactly the limit is accepted.
    let allowed = mol(&vec!["C"; MAX_QUERY_ATOMS], &[]);
    assert!(
        substructure::find(&allowed, &target)
            .unwrap()
            .mappings
            .is_empty()
    );
}

#[test]
fn a_predicate_nothing_can_satisfy_is_refused_not_answered_empty() {
    // The drawing cannot record these (lists hold 1 to 10 elements), but the matcher must not
    // read them as "matches nothing" if a graph reaches it that way.
    let elements = |range: std::ops::RangeInclusive<u8>| -> Vec<Element> {
        range
            .map(|n| Element::from_atomic_number(n).unwrap())
            .collect()
    };
    let everything_negated = Atom::new(AtomKind::Query(QueryAtom::List {
        elements: elements(1..=118),
        negated: true,
    }));
    let empty_list = Atom::new(AtomKind::Query(QueryAtom::List {
        elements: Vec::new(),
        negated: false,
    }));
    let target = mol(&["C", "N"], &[(0, 1, "-")]);
    for atom in [everything_negated, empty_list] {
        let query = MoleculeGraph::from_parts(vec![(id(1), atom)], Vec::new());
        assert_eq!(
            refusal(&query, &target),
            MatchError::Unsatisfiable(Item::Atom(id(1)))
        );
    }
    let nothing_bond = MoleculeGraph::from_parts(
        vec![(id(1), atom("C")), (id(2), atom("C"))],
        vec![(
            bond_id(1),
            chemifree_domain::document::Bond {
                begin: id(1),
                end: id(2),
                order: BondOrder::Query(BondQuery::OneOf(Vec::new())),
            },
        )],
    );
    assert_eq!(
        refusal(&nothing_bond, &target),
        MatchError::Unsatisfiable(Item::Bond(bond_id(1)))
    );
    // An element list with every element but one is satisfiable.
    let nearly = Atom::new(AtomKind::Query(QueryAtom::List {
        elements: elements(2..=118),
        negated: true,
    }));
    let query = MoleculeGraph::from_parts(vec![(id(1), nearly)], Vec::new());
    assert!(
        substructure::find(&query, &mol(&["H", "C"], &[]))
            .unwrap()
            .mappings
            .len()
            == 1
    );
}

#[test]
fn a_dative_or_unknown_bond_in_the_query_is_unsupported() {
    let target = mol(&["N", "B"], &[(0, 1, ">")]);
    let dative = mol(&["N", "B"], &[(0, 1, ">")]);
    assert_eq!(
        refusal(&dative, &target),
        MatchError::Unsupported {
            side: Side::Query,
            reason: "query_dative_bond",
            at: Some(Item::Bond(bond_id(1))),
            query: None,
        }
    );
    let unknown = mol(&["C", "C"], &[(0, 1, "?")]);
    assert_eq!(
        refusal(&unknown, &target),
        MatchError::Unsupported {
            side: Side::Query,
            reason: "unknown_bond",
            at: Some(Item::Bond(bond_id(1))),
            query: None,
        }
    );
    // The same bonds in a target are matched by the any-bond query and by nothing else.
    let any = mol(&["N", "B"], &[(0, 1, "~")]);
    assert_eq!(found(&any, &target), vec![vec![0, 1]]);
    let single = mol(&["N", "B"], &[(0, 1, "-")]);
    assert!(found(&single, &target).is_empty());
}

#[test]
fn an_undecidable_target_bond_is_refused_when_a_mapping_depends_on_it() {
    // Tetralin drawn Kekulé: the model cannot say whether its ring bonds are aromatic.
    let tetralin = mol(
        &["C"; 10],
        &[
            (0, 1, "="),
            (1, 2, "-"),
            (2, 3, "="),
            (3, 4, "-"),
            (4, 5, "="),
            (5, 0, "-"),
            (0, 6, "-"),
            (6, 7, "-"),
            (7, 8, "-"),
            (8, 9, "-"),
            (9, 5, "-"),
        ],
    );
    let system = aromaticity::perceive(&tetralin);
    assert_eq!(
        system.systems[0].classification,
        Classification::Unsupported
    );
    for query in [
        aromatic_benzene(),
        kekule_benzene(),
        mol(&["C", "C"], &[(0, 1, "-")]),
    ] {
        let error = refusal(&query, &tetralin);
        assert!(
            matches!(
                error,
                MatchError::Unsupported {
                    side: Side::Target,
                    reason: "fused_partially_aromatic",
                    at: Some(Item::Bond(_)),
                    query: Some(Item::Bond(_)),
                }
            ),
            "{error:?}"
        );
    }
    // A query that never compares such a bond is answered: single atoms, and any-bonds.
    assert_eq!(found(&mol(&["C"], &[]), &tetralin).len(), 10);
    assert_eq!(
        found(&mol(&["C", "C"], &[(0, 1, "~")]), &tetralin).len(),
        22
    );
    // A query that no atom of the ring can satisfy does not depend on it either.
    assert!(found(&mol(&["Cl", "C"], &[(0, 1, "-")]), &tetralin).is_empty());
}

#[test]
fn a_substituent_bond_outside_the_unsupported_system_is_still_decided() {
    // Kekulé tetralin with a hydroxyl on the saturated ring: the C-O bond is not in the system.
    let mut atoms = vec!["C"; 10];
    atoms.push("O");
    let target = mol(
        &atoms,
        &[
            (0, 1, "="),
            (1, 2, "-"),
            (2, 3, "="),
            (3, 4, "-"),
            (4, 5, "="),
            (5, 0, "-"),
            (0, 6, "-"),
            (6, 7, "-"),
            (7, 8, "-"),
            (8, 9, "-"),
            (9, 5, "-"),
            (8, 10, "-"),
        ],
    );
    assert_eq!(
        found(&mol(&["C", "O"], &[(0, 1, "-")]), &target),
        vec![vec![8, 10]]
    );
    assert!(found(&mol(&["C", "O"], &[(0, 1, "=")]), &target).is_empty());
}

#[test]
fn a_ring_without_pi_bonds_is_read_as_drawn_even_if_the_model_cannot_classify_it() {
    // Cyclohexane with a silicon atom: the aromaticity model does not support silicon, but no
    // spelling of an all-single ring can be aromatic.
    let silacycle = mol(
        &["Si", "C", "C", "C", "C", "C"],
        &(0..6).map(|i| (i, (i + 1) % 6, "-")).collect::<Vec<_>>(),
    );
    assert_eq!(
        aromaticity::perceive(&silacycle).systems[0].classification,
        Classification::Unsupported
    );
    assert_eq!(
        found(&mol(&["Si", "C"], &[(0, 1, "-")]), &silacycle).len(),
        2
    );
    // With a double bond the ring is undecidable.
    let silabenzene = mol(
        &["Si", "C", "C", "C", "C", "C"],
        &[
            (0, 1, "="),
            (1, 2, "-"),
            (2, 3, "="),
            (3, 4, "-"),
            (4, 5, "="),
            (5, 0, "-"),
        ],
    );
    assert!(matches!(
        refusal(&mol(&["Si", "C"], &[(0, 1, "=")]), &silabenzene),
        MatchError::Unsupported {
            side: Side::Target,
            reason: "element_or_charge",
            ..
        }
    ));
}

#[test]
fn a_query_the_aromaticity_model_cannot_read_is_refused_with_its_reason() {
    // A query atom in a ring with double bonds.
    let query = mol(
        &["Q", "C", "C", "C", "C", "C"],
        &[
            (0, 1, "="),
            (1, 2, "-"),
            (2, 3, "="),
            (3, 4, "-"),
            (4, 5, "="),
            (5, 0, "-"),
        ],
    );
    assert!(matches!(
        refusal(&query, &aromatic_benzene()),
        MatchError::Unsupported {
            side: Side::Query,
            reason: "query_atom",
            at: Some(Item::Bond(_)),
            query: None
        }
    ));
    // A query bond in a ring with double bonds.
    let listed = mol(
        &["C"; 6],
        &[
            (0, 1, "="),
            (1, 2, "-"),
            (2, 3, "="),
            (3, 4, "-"),
            (4, 5, "="),
            (5, 0, "-="),
        ],
    );
    assert!(matches!(
        refusal(&listed, &aromatic_benzene()),
        MatchError::Unsupported {
            side: Side::Query,
            reason: "unsupported_ring_bond",
            ..
        }
    ));
    // A bond drawn aromatic outside a ring.
    let loose = mol(&["C", "C"], &[(0, 1, ":")]);
    assert!(matches!(
        refusal(&loose, &aromatic_benzene()),
        MatchError::Unsupported {
            side: Side::Query,
            reason: "aromatic_bond_outside_ring",
            ..
        }
    ));
    // A pyrrole drawn with aromatic bonds and no stated hydrogen is not aromatic to the model.
    let bare_pyrrole = mol(
        &["C", "C", "C", "C", "N"],
        &(0..5).map(|i| (i, (i + 1) % 5, ":")).collect::<Vec<_>>(),
    );
    assert!(matches!(
        refusal(&bare_pyrrole, &aromatic_benzene()),
        MatchError::Unsupported {
            side: Side::Query,
            reason: "electron_count",
            ..
        }
    ));
}

#[test]
fn a_query_bond_to_a_substituent_of_an_aromatic_ring_is_unsupported() {
    // The aromaticity model reads a ring atom's other bonds as single bonds only.
    let query = mol(
        &["C", "C", "C", "C", "C", "C", "X"],
        &[
            (0, 1, "="),
            (1, 2, "-"),
            (2, 3, "="),
            (3, 4, "-"),
            (4, 5, "="),
            (5, 0, "-"),
            (0, 6, "~"),
        ],
    );
    assert!(matches!(
        refusal(&query, &aromatic_benzene()),
        MatchError::Unsupported {
            side: Side::Query,
            reason: "unsupported_external_bond",
            ..
        }
    ));
}

#[test]
fn query_atoms_and_bonds_outside_rings_and_all_single_rings_are_accepted() {
    // A heteroatom and a halogen substituent on a Kekulé benzene.
    let query = mol(
        &["C", "C", "C", "C", "C", "C", "Q", "X"],
        &[
            (0, 1, "="),
            (1, 2, "-"),
            (2, 3, "="),
            (3, 4, "-"),
            (4, 5, "="),
            (5, 0, "-"),
            (0, 6, "-"),
            (3, 7, "-"),
        ],
    );
    let target = mol(
        &["C", "C", "C", "C", "C", "C", "O", "Cl"],
        &[
            (0, 1, ":"),
            (1, 2, ":"),
            (2, 3, ":"),
            (3, 4, ":"),
            (4, 5, ":"),
            (5, 0, ":"),
            (0, 6, "-"),
            (3, 7, "-"),
        ],
    );
    assert_eq!(
        found(&query, &target),
        vec![vec![0, 1, 2, 3, 4, 5, 6, 7], vec![0, 5, 4, 3, 2, 1, 6, 7]]
    );
    // An all-single ring of wildcards.
    let wild_ring = mol(&["*"; 3], &[(0, 1, "-"), (1, 2, "-"), (2, 0, "-")]);
    assert_eq!(found(&wild_ring, &cycle(3, "-")).len(), 6);
}

#[test]
fn a_target_that_is_itself_a_query_drawing_is_undecided_unless_the_query_accepts_everything() {
    let target = mol(&["Q", "C"], &[(0, 1, "-")]);
    // Any matches a query atom; an element cannot be known to.
    assert_eq!(found(&mol(&["*"], &[]), &target).len(), 2);
    assert!(matches!(
        refusal(&mol(&["O"], &[]), &target),
        MatchError::Unsupported {
            side: Side::Target,
            reason: "target_query_atom",
            at: Some(Item::Atom(_)),
            query: Some(Item::Atom(_))
        }
    ));
    // The element it can rule out is still ruled out.
    let listed = mol(&["C"], &[]);
    assert!(matches!(
        refusal(&listed, &target),
        MatchError::Unsupported {
            reason: "target_query_atom",
            ..
        }
    ));
    let bond_target = mol(&["C", "C"], &[(0, 1, "-=")]);
    assert!(matches!(
        refusal(&mol(&["C", "C"], &[(0, 1, "-")]), &bond_target),
        MatchError::Unsupported {
            side: Side::Target,
            reason: "query_bond",
            ..
        }
    ));
    assert_eq!(
        found(&mol(&["C", "C"], &[(0, 1, "~")]), &bond_target).len(),
        2
    );
}

#[test]
fn too_many_mappings_is_a_limit_not_a_short_list() {
    // The disconnected query of two carbons maps onto every ordered pair of a large target.
    let target = mol(&vec!["C"; 40], &[]);
    let query = mol(&["C", "C"], &[]);
    assert_eq!(
        substructure::find(&query, &target),
        Err(MatchError::MappingLimit)
    );
    assert_eq!(MatchError::MappingLimit.code(), "match_limit");
    // 33 x 32 = 1056 > 1000, 32 x 31 = 992 fits: the boundary is the 1001st mapping.
    let fits = mol(&vec!["C"; 32], &[]);
    assert_eq!(
        substructure::find(&query, &fits).unwrap().mappings.len(),
        32 * 31
    );
    let overflows = mol(&vec!["C"; 33], &[]);
    assert_eq!(
        substructure::find(&query, &overflows),
        Err(MatchError::MappingLimit)
    );
}

/// A `rows` x `columns` grid of carbons joined by single bonds.
fn grid(rows: usize, columns: usize) -> MoleculeGraph {
    let mut bonds = Vec::new();
    for row in 0..rows {
        for column in 0..columns {
            let here = row * columns + column;
            if column + 1 < columns {
                bonds.push((here, here + 1, "-"));
            }
            if row + 1 < rows {
                bonds.push((here, here + columns, "-"));
            }
        }
    }
    mol(&vec!["C"; rows * columns], &bonds)
}

#[test]
fn an_exhausted_step_budget_is_a_limit() {
    // A grid is bipartite, so a 15-ring cannot map, but proving it takes a long search.
    assert_eq!(
        substructure::find(&cycle(15, "-"), &grid(14, 14)),
        Err(MatchError::StepLimit)
    );
    assert_eq!(MatchError::StepLimit.code(), "match_limit");
    // A smaller question about a grid is answered completely: 7 x 7 squares, 8 symmetries each.
    let squares = substructure::find(&cycle(4, "-"), &grid(8, 8)).unwrap();
    assert_eq!(squares.mappings.len(), 8 * 7 * 7);
    assert!(squares.steps < MAX_SEARCH_STEPS);
}

#[test]
fn the_largest_documents_are_searched_within_the_budget() {
    // 40 x 50 is 2,000 atoms and 3,910 bonds: the document maximum is 2,000 and 4,000.
    let target = grid(40, 50);
    assert_eq!(target.atoms().len(), 2_000);
    assert!(target.bonds().len() <= 4_000);
    let path = |element: &str| {
        mol(
            &vec![element; MAX_QUERY_ATOMS],
            &(0..MAX_QUERY_ATOMS - 1)
                .map(|i| (i, i + 1, "-"))
                .collect::<Vec<_>>(),
        )
    };
    // The element prefilter leaves no candidate: no step at all.
    let none = substructure::find(&path("Si"), &target).unwrap();
    assert!(none.mappings.is_empty());
    assert_eq!(none.steps, 0);
    // A 64-atom path has more than 1,000 placements: a limit, reached quickly.
    let started = std::time::Instant::now();
    assert_eq!(
        substructure::find(&path("C"), &target),
        Err(MatchError::MappingLimit)
    );
    // A 15-ring does not occur in a bipartite grid; the search ends at its step budget.
    assert_eq!(
        substructure::find(&cycle(15, "-"), &target),
        Err(MatchError::StepLimit)
    );
    assert!(
        started.elapsed().as_secs() < 30,
        "took {:?}",
        started.elapsed()
    );
}

// ---- Differential fuzz: the search against a brute-force enumeration -----------------------

/// A small deterministic generator (xorshift64*), so a failing seed can be replayed.
struct Random(u64);

impl Random {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, bound: usize) -> usize {
        usize::try_from(self.next() >> 33).unwrap() % bound
    }
}

const FUZZ_ATOMS: [&str; 12] = [
    "C", "C", "C", "N", "O", "N+", "13C", "Cl", "*", "A", "Q", "[C,N]",
];
const FUZZ_BONDS: [&str; 9] = ["-", "-", "-", "=", "#", ":", ">", "~", "-="];

/// A random connected-ish graph of `atoms` atoms: a random tree plus a few extra bonds.
fn random_graph(random: &mut Random, atoms: usize, extra: usize) -> MoleculeGraph {
    let specs: Vec<&str> = (0..atoms)
        .map(|_| FUZZ_ATOMS[random.below(FUZZ_ATOMS.len())])
        .collect();
    let mut bonds: Vec<(usize, usize, &str)> = Vec::new();
    for atom in 1..atoms {
        if random.below(8) > 0 {
            bonds.push((
                random.below(atom),
                atom,
                FUZZ_BONDS[random.below(FUZZ_BONDS.len())],
            ));
        }
    }
    for _ in 0..extra {
        let (a, b) = (random.below(atoms), random.below(atoms));
        let exists = bonds
            .iter()
            .any(|&(x, y, _)| (x, y) == (a, b) || (x, y) == (b, a));
        if a != b && !exists {
            bonds.push((a, b, FUZZ_BONDS[random.below(FUZZ_BONDS.len())]));
        }
    }
    mol(&specs, &bonds)
}

#[derive(Clone, Copy, PartialEq)]
enum Reading {
    Single,
    Double,
    Triple,
    Aromatic,
    Dative,
    Undecided,
}

/// Each bond's reading, found separately from the matcher: a bond is aromatic when the model
/// calls its ring system aromatic; one drawn aromatic anywhere else is undecided, as is
/// every bond of an unsupported system holding a double or aromatic bond and every unknown
/// or query bond.
fn readings(graph: &MoleculeGraph) -> std::collections::BTreeMap<BondId, Reading> {
    let interpretation = aromaticity::perceive(graph);
    let mut readings = std::collections::BTreeMap::new();
    for (bond_id, bond) in graph.bonds() {
        let system = interpretation
            .systems
            .iter()
            .find(|s| s.bonds.contains(&bond_id));
        let pi_in_system = system.is_some_and(|s| {
            graph.bonds().any(|(other, b)| {
                s.bonds.contains(&other)
                    && matches!(b.order, BondOrder::Double | BondOrder::Aromatic)
            })
        });
        let reading = match (system.map(|s| s.classification), &bond.order) {
            (Some(Classification::Aromatic), _) => Reading::Aromatic,
            (_, BondOrder::Aromatic) => Reading::Undecided,
            (Some(Classification::Unsupported), _) if pi_in_system => Reading::Undecided,
            (_, BondOrder::Single) => Reading::Single,
            (_, BondOrder::Double) => Reading::Double,
            (_, BondOrder::Triple) => Reading::Triple,
            (_, BondOrder::Dative) => Reading::Dative,
            _ => Reading::Undecided,
        };
        readings.insert(bond_id, reading);
    }
    readings
}

/// The brute-force answer. `None` means the query itself is refused; `Some(Err(()))` means
/// a mapping rests on an undecided comparison; otherwise the mappings as target indices.
fn brute_force(query: &MoleculeGraph, target: &MoleculeGraph) -> Option<Result<Vec<Vec<u32>>, ()>> {
    let query_atoms: Vec<_> = query.atoms().collect();
    let target_atoms: Vec<_> = target.atoms().collect();
    let query_readings = readings(query);
    let target_readings = readings(target);
    let mut query_tests = Vec::new();
    for (bond_id, bond) in query.bonds() {
        let test: Option<Vec<Reading>> = match &bond.order {
            BondOrder::Query(BondQuery::Any) => None,
            BondOrder::Query(BondQuery::OneOf(list)) => Some(
                list.iter()
                    .map(|k| match k {
                        QueryBondOrder::Single => Reading::Single,
                        QueryBondOrder::Double => Reading::Double,
                        QueryBondOrder::Triple => Reading::Triple,
                        QueryBondOrder::Aromatic => Reading::Aromatic,
                    })
                    .collect(),
            ),
            _ => match query_readings[&bond_id] {
                Reading::Undecided | Reading::Dative => return None,
                other => Some(vec![other]),
            },
        };
        query_tests.push((bond.begin, bond.end, test));
    }
    let count = query_atoms.len();
    let mut found = Vec::new();
    let mut undecided = false;
    let mut assigned = vec![0_usize; count];
    loop {
        // Advance a counter over target^count; reject repeats and mismatches.
        let distinct = (0..count).all(|i| (0..i).all(|j| assigned[i] != assigned[j]));
        if distinct {
            let mut ok = true;
            let mut used_undecided = false;
            for (q, &(_, qatom)) in query_atoms.iter().enumerate() {
                let (_, tatom) = target_atoms[assigned[q]];
                match (qatom.kind.element(), tatom.kind.element()) {
                    (_, None) => {
                        used_undecided |= !matches!(qatom.kind, AtomKind::Query(QueryAtom::Any));
                    }
                    (Some(qe), Some(te)) => {
                        ok &= qe == te
                            && (qatom.charge == 0 || qatom.charge == tatom.charge)
                            && qatom.isotope.is_none_or(|m| Some(m) == tatom.isotope);
                    }
                    (None, Some(te)) => {
                        let number = te.atomic_number();
                        ok &= match &qatom.kind {
                            AtomKind::Query(QueryAtom::Any) => true,
                            AtomKind::Query(QueryAtom::AnyHeavy) => number != 1,
                            AtomKind::Query(QueryAtom::Heteroatom) => number != 1 && number != 6,
                            AtomKind::Query(QueryAtom::Halogen) => {
                                [9, 17, 35, 53, 85, 117].contains(&number)
                            }
                            AtomKind::Query(QueryAtom::List { elements, negated }) => {
                                elements.iter().any(|e| e.atomic_number() == number) != *negated
                            }
                            AtomKind::Element(_) => unreachable!("handled above"),
                        };
                    }
                }
            }
            if ok {
                for (a, b, test) in &query_tests {
                    let qa = query_atoms.iter().position(|(id, _)| id == a).unwrap();
                    let qb = query_atoms.iter().position(|(id, _)| id == b).unwrap();
                    let bond = target.bonds().find(|(_, tb)| {
                        let (x, y) = (target_atoms[assigned[qa]].0, target_atoms[assigned[qb]].0);
                        (tb.begin, tb.end) == (x, y) || (tb.begin, tb.end) == (y, x)
                    });
                    let Some((tid, _)) = bond else {
                        ok = false;
                        break;
                    };
                    if let Some(kinds) = test {
                        match target_readings[&tid] {
                            Reading::Undecided => used_undecided = true,
                            reading => ok &= kinds.contains(&reading),
                        }
                    }
                }
            }
            if ok {
                undecided |= used_undecided;
                found.push(
                    assigned
                        .iter()
                        .map(|&t| u32::try_from(t).unwrap())
                        .collect::<Vec<_>>(),
                );
            }
        }
        // Increment the counter.
        let mut position = 0;
        loop {
            if position == count {
                return Some(if undecided {
                    Err(())
                } else {
                    Ok(sorted(found, &query_atoms, &target_atoms))
                });
            }
            assigned[position] += 1;
            if assigned[position] < target_atoms.len() {
                break;
            }
            assigned[position] = 0;
            position += 1;
        }
    }
}

/// Mappings as the target atoms' creation indices in query-atom ID order.
fn sorted(
    found: Vec<Vec<u32>>,
    query_atoms: &[(AtomId, &Atom)],
    target_atoms: &[(AtomId, &Atom)],
) -> Vec<Vec<u32>> {
    let mut order: Vec<usize> = (0..query_atoms.len()).collect();
    order.sort_by_key(|&i| query_atoms[i].0);
    let mut result: Vec<Vec<u32>> = found
        .into_iter()
        .map(|assigned| {
            order
                .iter()
                .map(|&q| {
                    target_atoms[usize::try_from(assigned[q]).unwrap()]
                        .0
                        .serial()
                        - 1
                })
                .collect()
        })
        .collect();
    result.sort();
    result
}

/// An independent check that a returned mapping is a subgraph mapping.
fn verify(query: &MoleculeGraph, target: &MoleculeGraph, matches: &Matches) {
    for mapping in &matches.mappings {
        assert_eq!(mapping.atoms.len(), query.atoms().len());
        let images: std::collections::BTreeSet<_> = mapping.atoms.iter().map(|p| p.1).collect();
        assert_eq!(images.len(), mapping.atoms.len(), "injective: {mapping:?}");
        let map: std::collections::BTreeMap<_, _> = mapping.atoms.iter().copied().collect();
        assert_eq!(mapping.bonds.len(), query.bonds().len());
        for (qb, tb) in &mapping.bonds {
            let q = query.bonds().find(|(id, _)| id == qb).unwrap().1;
            let t = target.bonds().find(|(id, _)| id == tb).unwrap().1;
            let image = (map[&q.begin], map[&q.end]);
            assert!(
                image == (t.begin, t.end) || image == (t.end, t.begin),
                "bond {qb}->{tb}"
            );
        }
    }
}

#[test]
fn seeded_random_graphs_agree_with_brute_force_or_are_honestly_refused() {
    let (mut answered, mut refused, mut nonempty) = (0, 0, 0);
    for seed in 1..=1_500_u64 {
        let mut random = Random::new(seed);
        let (query_atoms, query_extra) = (1 + random.below(4), random.below(2));
        let query = random_graph(&mut random, query_atoms, query_extra);
        let (target_atoms, target_extra) = (2 + random.below(5), random.below(4));
        let target = random_graph(&mut random, target_atoms, target_extra);
        let expected = brute_force(&query, &target);
        let actual = substructure::find(&query, &target);
        match (expected, actual) {
            (
                None,
                Err(MatchError::Unsupported {
                    side: Side::Query, ..
                }),
            ) => refused += 1,
            (
                Some(Err(())),
                Err(MatchError::Unsupported {
                    side: Side::Target, ..
                }),
            ) => refused += 1,
            (Some(Ok(mappings)), Ok(matches)) => {
                verify(&query, &target, &matches);
                assert_eq!(assignments(&matches), mappings, "seed {seed}");
                answered += 1;
                nonempty += usize::from(!mappings.is_empty());
            }
            (expected, actual) => {
                panic!("seed {seed}: brute force {expected:?}, matcher {actual:?}")
            }
        }
    }
    assert!(
        answered > 300 && refused > 50 && nonempty > 100,
        "{answered} answered ({nonempty} non-empty), {refused} refused"
    );
}
