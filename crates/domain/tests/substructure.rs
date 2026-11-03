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
    self, Item, MAX_MAPPINGS, MAX_QUERY_ATOMS, MAX_SEARCH_STEPS, MatchError, Matches, Side,
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
    stated.attachment_points = chemifree_domain::states::AttachmentPoints::new(&[1, 3]).unwrap();
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
            chemifree_domain::document::Bond::new(
                id(1),
                id(2),
                BondOrder::Query(BondQuery::OneOf(Vec::new())),
            ),
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
    random_graph_of(random, atoms, extra, &FUZZ_ATOMS, &FUZZ_BONDS)
}

/// Like [`random_graph`], with the atom specs and bond kinds to draw from.
fn random_graph_of(
    random: &mut Random,
    atoms: usize,
    extra: usize,
    atom_specs: &[&'static str],
    bond_kinds: &[&'static str],
) -> MoleculeGraph {
    let specs: Vec<&str> = (0..atoms)
        .map(|_| atom_specs[random.below(atom_specs.len())])
        .collect();
    let mut bonds: Vec<(usize, usize, &str)> = Vec::new();
    for atom in 1..atoms {
        if random.below(8) > 0 {
            bonds.push((
                random.below(atom),
                atom,
                bond_kinds[random.below(bond_kinds.len())],
            ));
        }
    }
    for _ in 0..extra {
        let (a, b) = (random.below(atoms), random.below(atoms));
        let exists = bonds
            .iter()
            .any(|&(x, y, _)| (x, y) == (a, b) || (x, y) == (b, a));
        if a != b && !exists {
            bonds.push((a, b, bond_kinds[random.below(bond_kinds.len())]));
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

/// The undecided comparison a refusal names: the target atom (else bond), then the query
/// item, with atoms before bonds and the lowest serials first.
type Offence = ((u8, u32, u32), Item, Item);

/// The brute-force answer. `None` means the query itself is refused; `Some(Err(offence))`
/// means a mapping rests on an undecided comparison; otherwise the mappings as target indices.
fn brute_force(
    query: &MoleculeGraph,
    target: &MoleculeGraph,
) -> Option<Result<Vec<Vec<u32>>, Offence>> {
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
        query_tests.push((bond_id, bond.begin, bond.end, test));
    }
    let count = query_atoms.len();
    let mut found = Vec::new();
    let mut offence: Option<Offence> = None;
    let mut assigned = vec![0_usize; count];
    loop {
        // Advance a counter over target^count; reject repeats and mismatches.
        let distinct = (0..count).all(|i| (0..i).all(|j| assigned[i] != assigned[j]));
        if distinct {
            let mut ok = true;
            let mut offences: Vec<Offence> = Vec::new();
            for (q, &(qid, qatom)) in query_atoms.iter().enumerate() {
                let (tid, tatom) = target_atoms[assigned[q]];
                match (qatom.kind.element(), tatom.kind.element()) {
                    (_, None) => {
                        if !matches!(qatom.kind, AtomKind::Query(QueryAtom::Any)) {
                            offences.push((
                                (0, tid.serial(), qid.serial()),
                                Item::Atom(tid),
                                Item::Atom(qid),
                            ));
                        }
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
                for (query_bond, a, b, test) in &query_tests {
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
                            Reading::Undecided => offences.push((
                                (1, tid.serial(), query_bond.serial()),
                                Item::Bond(tid),
                                Item::Bond(*query_bond),
                            )),
                            reading => ok &= kinds.contains(&reading),
                        }
                    }
                }
            }
            if ok {
                if let Some(lowest) = offences.iter().min_by_key(|o| o.0) {
                    offence = offence.into_iter().chain([*lowest]).min_by_key(|o| o.0);
                }
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
                return Some(if let Some(offence) = offence {
                    Err(offence)
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
                Some(Err((_, at, compared))),
                Err(MatchError::Unsupported {
                    side: Side::Target,
                    at: Some(named),
                    query: Some(named_query),
                    ..
                }),
            ) => {
                assert_eq!((named, named_query), (at, compared), "seed {seed}");
                refused += 1;
            }
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

#[test]
fn mapping_lists_are_ordered_by_numeric_id_not_by_text() {
    // Twelve atoms: a10, a11 and a12 sort after a9 numerically but before a2 as text.
    let path = |length: usize| {
        mol(
            &vec!["C"; length],
            &(0..length - 1).map(|i| (i, i + 1, "-")).collect::<Vec<_>>(),
        )
    };
    let matches = substructure::find(&path(12), &path(12)).unwrap();
    assert_eq!(matches.mappings.len(), 2);
    for mapping in &matches.mappings {
        let atoms: Vec<u32> = mapping.atoms.iter().map(|pair| pair.0.serial()).collect();
        let bonds: Vec<u32> = mapping.bonds.iter().map(|pair| pair.0.serial()).collect();
        assert_eq!(atoms, (1..=12).collect::<Vec<_>>());
        assert_eq!(bonds, (1..=11).collect::<Vec<_>>());
    }
}

#[test]
fn an_any_bond_matches_unknown_and_query_bonds_and_a_list_never_matches_a_dative_one() {
    let any = mol(&["C", "C"], &[(0, 1, "~")]);
    for kind in ["?", "-=", ">", "-", ":"] {
        // A lone aromatic bond is outside any ring: refused as a target only if compared.
        if kind == ":" {
            continue;
        }
        let target = mol(&["C", "C"], &[(0, 1, kind)]);
        assert_eq!(found(&any, &target).len(), 2, "any bond matches {kind:?}");
    }
    let dative = mol(&["C", "C"], &[(0, 1, ">")]);
    assert!(found(&mol(&["C", "C"], &[(0, 1, "-=#:")]), &dative).is_empty());
}

#[test]
fn the_search_cost_grows_with_the_target_bonds_not_with_pairs_of_atoms() {
    // A two-atom query in a 2,000-atom target: the first atom is tried on each carbon, and
    // the second only on that carbon's neighbors. A scan of all pairs would be four million.
    let target = grid(40, 50);
    let bonds = target.bonds().len();
    let ethane = mol(&["C", "C"], &[(0, 1, "-")]);
    let result = substructure::find(&ethane, &target);
    assert_eq!(
        result,
        Err(MatchError::MappingLimit),
        "2 x 3,910 placements"
    );
    let query = mol(&["C", "N"], &[(0, 1, "-")]);
    // Nitrogen is absent, so the prefilter ends it; with nitrogen present once, a handful of tries.
    assert_eq!(substructure::find(&query, &target).unwrap().steps, 0);
    let mut atoms = vec!["C"; 2_000];
    atoms[1_999] = "N";
    let spec: Vec<(usize, usize, &str)> = (0..1_999).map(|i| (i, i + 1, "-")).collect();
    let chain = mol(&atoms, &spec);
    let found = substructure::find(&query, &chain).unwrap();
    assert_eq!(found.mappings.len(), 1);
    assert!(
        found.steps <= 2_000 + 2,
        "{} steps for one nitrogen",
        found.steps
    );
    assert!(bonds > 3_000, "the grid is a document-sized target");
}

#[test]
fn seeded_larger_random_graphs_give_verified_mappings_or_a_refusal_never_a_panic() {
    let (mut verified, mut refusals) = (0, 0);
    for seed in 10_000..10_400_u64 {
        let mut random = Random::new(seed);
        // Simple atoms and bonds, so that most pairs can be answered; the first fuzz covers the rest.
        let atoms = ["C", "C", "C", "N", "O", "*", "A", "[C,N]"];
        let bonds = ["-", "-", "-", "=", "~", "-="];
        let (query_atoms, query_extra) = (1 + random.below(5), random.below(2));
        let query = random_graph_of(&mut random, query_atoms, query_extra, &atoms, &bonds);
        let (target_atoms, target_extra) = (8 + random.below(30), random.below(4));
        let target = random_graph_of(
            &mut random,
            target_atoms,
            target_extra,
            &["C", "C", "C", "N", "O"],
            &["-", "-", "-", "="],
        );
        match substructure::find(&query, &target) {
            Ok(matches) => {
                verify(&query, &target, &matches);
                assert!(matches.steps <= MAX_SEARCH_STEPS);
                verified += 1;
            }
            Err(error) => {
                assert!(
                    matches!(
                        error,
                        MatchError::Unsupported { .. }
                            | MatchError::StepLimit
                            | MatchError::MappingLimit
                    ),
                    "seed {seed}: {error:?}"
                );
                refusals += 1;
            }
        }
    }
    assert!(
        verified > 100 && refusals > 20,
        "{verified} verified, {refusals} refused"
    );
}

#[test]
fn exactly_the_mapping_limit_is_an_answer_and_one_more_is_a_limit() {
    let carbon = mol(&["C"], &[]);
    let at_limit = substructure::find(&carbon, &mol(&vec!["C"; MAX_MAPPINGS], &[])).unwrap();
    assert_eq!(at_limit.mappings.len(), MAX_MAPPINGS);
    assert_eq!(
        substructure::find(&carbon, &mol(&vec!["C"; MAX_MAPPINGS + 1], &[])),
        Err(MatchError::MappingLimit)
    );
}

#[test]
fn a_mapping_is_ordered_by_id_whatever_order_the_graph_lists_its_atoms_in() {
    // The same ethanol, listed with its atoms and bonds in descending ID order.
    let atom_of = |symbol: &str| Atom::from(Element::from_symbol(symbol).unwrap());
    let bond_of = |begin: u32, end: u32| {
        chemifree_domain::document::Bond::new(id(begin), id(end), BondOrder::Single)
    };
    let backwards = MoleculeGraph::from_parts(
        vec![
            (id(3), atom_of("O")),
            (id(2), atom_of("C")),
            (id(1), atom_of("C")),
        ],
        vec![(bond_id(2), bond_of(2, 3)), (bond_id(1), bond_of(1, 2))],
    );
    let query = mol(&["C", "O"], &[(0, 1, "-")]);
    let matches = substructure::find(&query, &backwards).unwrap();
    assert_eq!(matches.mappings.len(), 1);
    assert_eq!(
        matches.mappings[0].atoms,
        vec![(id(1), id(2)), (id(2), id(3))]
    );
    // The query listed backwards gives the same mapping, ordered by query atom ID.
    let reversed_query = MoleculeGraph::from_parts(
        vec![(id(2), atom_of("O")), (id(1), atom_of("C"))],
        vec![(bond_id(1), bond_of(1, 2))],
    );
    let again = substructure::find(&reversed_query, &backwards).unwrap();
    assert_eq!(again.mappings, matches.mappings);
}

#[test]
fn an_atom_with_too_few_bonds_is_never_tried() {
    // A query atom with three bonds has no candidate in a path, whose atoms have at most two.
    let star = mol(
        &["C", "C", "C", "C"],
        &[(0, 1, "-"), (0, 2, "-"), (0, 3, "-")],
    );
    let path = mol(
        &vec!["C"; 100],
        &(0..99).map(|i| (i, i + 1, "-")).collect::<Vec<_>>(),
    );
    let matches = substructure::find(&star, &path).unwrap();
    assert!(matches.mappings.is_empty());
    assert_eq!(
        matches.steps, 0,
        "the bond count rules the atom out before any step"
    );
}

/// A graph with the given IDs, listed in the given order, so that the order of the lists (the
/// order the search visits atoms in) can change while the IDs stay.
fn listed(
    atoms: &[(u32, &str)],
    bonds: &[(u32, u32, u32, &str)],
    descending: bool,
) -> MoleculeGraph {
    let mut atom_rows: Vec<_> = atoms.iter().map(|&(n, spec)| (id(n), atom(spec))).collect();
    let mut bond_rows: Vec<_> = bonds
        .iter()
        .map(|&(n, a, b, kind)| {
            (
                bond_id(n),
                chemifree_domain::document::Bond::new(id(a), id(b), order(kind)),
            )
        })
        .collect();
    if descending {
        atom_rows.reverse();
        bond_rows.reverse();
    }
    MoleculeGraph::from_parts(atom_rows, bond_rows)
}

#[test]
fn the_refusal_names_the_lowest_undecided_item_whatever_order_the_search_visits_atoms_in() {
    // Two undecided bonds (a list of kinds, b1, and an unknown bond, b2) and a decided bond
    // (b3): the single query bond has decided and undecided mappings. The refusal must not
    // depend on which a search reaches first, so it names the lowest bond, b1, and its reason.
    let atoms = [(1, "C"), (2, "C"), (3, "C"), (4, "C"), (5, "C"), (6, "C")];
    let query = mol(&["C", "C"], &[(0, 1, "-")]);
    for descending in [false, true] {
        let bonds = [(1, 1, 2, "-="), (2, 3, 4, "?"), (3, 5, 6, "-")];
        let expected = MatchError::Unsupported {
            side: Side::Target,
            reason: "query_bond",
            at: Some(Item::Bond(bond_id(1))),
            query: Some(Item::Bond(bond_id(1))),
        };
        assert_eq!(
            refusal(&query, &listed(&atoms, &bonds, descending)),
            expected
        );
        // The same bonds numbered the other way round: now the unknown bond is the lowest.
        let swapped = [(1, 3, 4, "?"), (2, 1, 2, "-="), (3, 5, 6, "-")];
        assert_eq!(
            refusal(&query, &listed(&atoms, &swapped, descending)),
            MatchError::Unsupported {
                side: Side::Target,
                reason: "unknown_bond",
                at: Some(Item::Bond(bond_id(1))),
                query: Some(Item::Bond(bond_id(1))),
            }
        );
    }
}

#[test]
fn an_undecided_atom_is_named_before_an_undecided_bond_and_lower_ids_first() {
    // The query C-C finds C(a1)-Q(a2) (undecided atom a2) and C(a3)-C(a4) over a list bond
    // (undecided bond b2); the atom is named.
    let atoms = [(1, "C"), (2, "Q"), (3, "C"), (4, "C")];
    let bonds = [(1, 1, 2, "-"), (2, 3, 4, "-=")];
    let query = mol(&["C", "C"], &[(0, 1, "-")]);
    for descending in [false, true] {
        match refusal(&query, &listed(&atoms, &bonds, descending)) {
            MatchError::Unsupported {
                side: Side::Target,
                reason,
                at,
                ..
            } => assert_eq!((reason, at), ("target_query_atom", Some(Item::Atom(id(2))))),
            other => panic!("{other:?}"),
        }
    }
}

#[test]
fn a_refusal_that_is_known_wins_over_the_mapping_limit_however_the_atoms_are_ordered() {
    let carbon = mol(&["C"], &[]);
    // 999 carbons and one query atom: 1,000 mappings, one of them undecided: a refusal.
    let mut atoms = vec!["C"; MAX_MAPPINGS - 1];
    atoms.push("Q");
    assert!(matches!(
        refusal(&carbon, &mol(&atoms, &[])),
        MatchError::Unsupported {
            reason: "target_query_atom",
            ..
        }
    ));
    // One more atom: 1,001 mappings, the last of which may be the undecided one. The mapping
    // that crosses the limit is read before the limit is reported, so wherever the query atom
    // lies the refusal wins. (Reverting the order of the limit check and the read of the 1,001st
    // mapping makes the query-atom-last case a limit.)
    for position in [0, MAPPING_LIMIT_MIDDLE, MAX_MAPPINGS] {
        let mut atoms = vec!["C"; MAX_MAPPINGS];
        atoms.insert(position, "Q");
        assert!(
            matches!(
                refusal(&carbon, &mol(&atoms, &[])),
                MatchError::Unsupported {
                    side: Side::Target,
                    reason: "target_query_atom",
                    ..
                }
            ),
            "query atom at index {position}"
        );
    }
    // With nothing undecided the 1,001st mapping is the limit.
    assert_eq!(
        refusal(&carbon, &mol(&vec!["C"; MAX_MAPPINGS + 1], &[])),
        MatchError::MappingLimit
    );
}

/// Where in the 1,001 atoms a query atom sits for the middle case of the test above.
const MAPPING_LIMIT_MIDDLE: usize = 500;

#[test]
fn a_known_refusal_wins_over_the_step_limit_but_which_one_is_known_depends_on_the_visit_order() {
    // A grid of carbons is bipartite, so a 15-ring cannot map into it and proving that takes
    // more than the step limit (see an_exhausted_step_budget_is_a_limit). Next to it, a 15-ring
    // with one query atom has 30 mappings, all undecided.
    let grid = grid(14, 14);
    let mut query_ring_atoms = vec![atom("Q")];
    query_ring_atoms.extend((1..15).map(|_| atom("C")));
    let ring_bonds: Vec<_> = (0..15).map(|i| (i, (i + 1) % 15, "-")).collect();
    let query_ring = graph_of(query_ring_atoms, &ring_bonds);
    let joined = |ring_first: bool| {
        let mut atoms: Vec<(AtomId, Atom)> = Vec::new();
        let mut bonds = Vec::new();
        let (grid_atoms, grid_bonds) = (
            grid.atoms().map(|(_, a)| a.clone()).collect::<Vec<_>>(),
            grid.bonds().map(|(_, b)| b.clone()).collect::<Vec<_>>(),
        );
        let ring_atoms: Vec<Atom> = query_ring.atoms().map(|(_, a)| a.clone()).collect();
        let ring_bond_rows: Vec<_> = query_ring.bonds().map(|(_, b)| b.clone()).collect();
        // Both parts keep their own relative numbering; `offset` places a part after the other.
        let (first, second) = if ring_first {
            (
                (ring_atoms.clone(), ring_bond_rows.clone()),
                (grid_atoms, grid_bonds),
            )
        } else {
            ((grid_atoms, grid_bonds), (ring_atoms, ring_bond_rows))
        };
        let mut offset = 0_u32;
        for (part_atoms, part_bonds) in [first, second] {
            let count = u32::try_from(part_atoms.len()).unwrap();
            for (i, a) in part_atoms.into_iter().enumerate() {
                atoms.push((id(offset + u32::try_from(i).unwrap() + 1), a));
            }
            for mut bond in part_bonds {
                bond.begin = id(offset + bond.begin.serial());
                bond.end = id(offset + bond.end.serial());
                bonds.push((bond_id(u32::try_from(bonds.len()).unwrap() + 1), bond));
            }
            offset += count;
        }
        MoleculeGraph::from_parts(atoms, bonds)
    };
    let ring_query = cycle(15, "-");
    // The ring is visited first: the refusal is known long before the step limit.
    let refused = refusal(&ring_query, &joined(true));
    assert!(
        matches!(
            refused,
            MatchError::Unsupported {
                side: Side::Target,
                reason: "target_query_atom",
                at: Some(Item::Atom(_)),
                ..
            }
        ),
        "{refused:?}"
    );
    // The grid is visited first: the step limit ends the search before the ring is reached, so
    // no refusal is known yet. That a limit stops the search early is why the named item (and
    // whether there is one) can depend on the order atoms were drawn in.
    assert_eq!(refusal(&ring_query, &joined(false)), MatchError::StepLimit);
}

#[test]
fn unbonded_query_atoms_are_charged_for_assignments_tried_not_for_candidates_scanned() {
    // Five unbonded nitrogens in a target with five of them among 2,000 atoms: every ordered
    // choice is 5 + 20 + 60 + 120 + 120 assignments, and used atoms are not charged again.
    let mut atoms = vec!["C"; 1_995];
    atoms.extend(["N"; 5]);
    let target = mol(&atoms, &[]);
    let five = mol(&["N"; 5], &[]);
    let matches = substructure::find(&five, &target).unwrap();
    assert_eq!(matches.mappings.len(), 120);
    assert_eq!(matches.steps, 325);
    // Six cannot map into five.
    let six = mol(&["N"; 6], &[]);
    let none = substructure::find(&six, &target).unwrap();
    assert!(none.mappings.is_empty());
    // Seven nitrogens: 5,040 ordered choices of six, which is the mapping limit, not the step
    // limit; so are three unbonded carbons in 2,000 atoms.
    let mut more = vec!["C"; 1_993];
    more.extend(["N"; 7]);
    assert_eq!(
        substructure::find(&six, &mol(&more, &[])),
        Err(MatchError::MappingLimit)
    );
    assert_eq!(
        substructure::find(&mol(&["C"; 3], &[]), &mol(&vec!["C"; 2_000], &[])),
        Err(MatchError::MappingLimit)
    );
}

#[test]
fn a_hub_with_many_used_or_filtered_neighbors_does_not_spend_steps() {
    // A carbon with 400 chlorine neighbors and one oxygen. The query C-O has one candidate for
    // each atom; the hub's other 400 neighbors are scanned but not charged.
    let mut atoms = vec!["C"];
    atoms.extend(vec!["Cl"; 400]);
    atoms.push("O");
    let bonds: Vec<_> = (1..atoms.len()).map(|i| (0, i, "-")).collect();
    let hub = mol(&atoms, &bonds);
    let matches = substructure::find(&mol(&["C", "O"], &[(0, 1, "-")]), &hub).unwrap();
    assert_eq!(matches.mappings.len(), 1);
    assert_eq!(matches.steps, 2);
    // A query chlorine on a carbon: the carbon (one candidate) once, then each chlorine once;
    // the oxygen, filtered by the element, is not charged.
    let all = substructure::find(&mol(&["Cl", "C"], &[(0, 1, "-")]), &hub).unwrap();
    assert_eq!(all.mappings.len(), 400);
    assert_eq!(all.steps, 1 + 400);
}

#[test]
fn a_drawn_single_in_an_aromatic_ring_is_aromatic_only_as_part_of_a_complete_kekule_pattern() {
    // Alternating singles and doubles that cover every ring atom once: aromatic, so a single
    // query bond does not match them; as a query the same drawing is aromatic.
    let single = mol(&["C", "C"], &[(0, 1, "-")]);
    assert!(found(&single, &kekule_benzene()).is_empty());
    assert_eq!(found(&aromatic_benzene(), &kekule_benzene()).len(), 12);
    assert_eq!(found(&kekule_benzene(), &kekule_benzene()).len(), 12);
    // A ring that mixes aromatic bonds with one single bond is not read: it is neither
    // normalised to aromatic nor kept as drawn. In the query:
    let mixed: Vec<_> = (0..6)
        .map(|i| (i, (i + 1) % 6, if i == 5 { "-" } else { ":" }))
        .collect();
    let mixed_ring = mol(&["C"; 6], &mixed);
    assert!(matches!(
        refusal(&mixed_ring, &aromatic_benzene()),
        MatchError::Unsupported {
            side: Side::Query,
            reason: "mixed_ring_bonds",
            ..
        }
    ));
    // ...and in the target, for a query that compares its bonds.
    assert!(matches!(
        refusal(&aromatic_benzene(), &mixed_ring),
        MatchError::Unsupported {
            side: Side::Target,
            reason: "mixed_ring_bonds",
            ..
        }
    ));
    // A query that compares none of its bonds is still answered.
    assert_eq!(found(&mol(&["C"], &[]), &mixed_ring).len(), 6);
}

#[test]
fn a_ring_the_model_calls_non_aromatic_keeps_its_drawn_kinds_in_query_and_target() {
    // Cyclohexa-1,3-diene: two double bonds and four single bonds, as drawn.
    let diene = mol(
        &["C"; 6],
        &[
            (0, 1, "="),
            (1, 2, "-"),
            (2, 3, "="),
            (3, 4, "-"),
            (4, 5, "-"),
            (5, 0, "-"),
        ],
    );
    assert_eq!(
        aromaticity::perceive(&diene).systems[0].classification,
        Classification::NonAromatic
    );
    assert_eq!(found(&mol(&["C", "C"], &[(0, 1, "=")]), &diene).len(), 4);
    assert_eq!(found(&mol(&["C", "C"], &[(0, 1, "-")]), &diene).len(), 8);
    assert!(
        found(
            &mol(&["C", "C"], &[(0, 1, "=:")]),
            &mol(&["C", "C"], &[(0, 1, "-")])
        )
        .is_empty()
    );
    // As a query the same ring needs its own drawn kinds in the target.
    assert_eq!(found(&diene, &diene).len(), 2);
    assert!(found(&diene, &kekule_benzene()).is_empty());
}

#[test]
fn a_dative_bond_in_a_ring_with_double_bonds_is_refused_not_read_as_drawn() {
    let ring = mol(
        &["C"; 6],
        &[
            (0, 1, "="),
            (1, 2, "-"),
            (2, 3, "="),
            (3, 4, "-"),
            (4, 5, "="),
            (5, 0, ">"),
        ],
    );
    assert!(matches!(
        refusal(&mol(&["C", "C"], &[(0, 1, "=")]), &ring),
        MatchError::Unsupported {
            side: Side::Target,
            reason: "unsupported_ring_bond",
            ..
        }
    ));
}

/// A graph of `atoms` listed with sequential IDs and bonds between their indices, without the
/// document limits (2,000 atoms), for targets larger than a document.
fn large(atoms: Vec<Atom>, bonds: &[(usize, usize)]) -> MoleculeGraph {
    let serial = |index: usize| u32::try_from(index + 1).unwrap();
    MoleculeGraph::from_parts(
        atoms
            .into_iter()
            .enumerate()
            .map(|(i, atom)| (id(serial(i)), atom))
            .collect(),
        bonds
            .iter()
            .enumerate()
            .map(|(i, &(a, b))| {
                (
                    bond_id(serial(i)),
                    chemifree_domain::document::Bond::new(
                        id(serial(a)),
                        id(serial(b)),
                        BondOrder::Single,
                    ),
                )
            })
            .collect(),
    )
}

#[test]
fn a_disconnected_query_in_a_large_target_ends_at_a_documented_limit_in_bounded_time() {
    // 20 sulfurs, 40 nitrogens each with 1,500 fluorine neighbors, and 50 carbons each with
    // one. The query: six unbonded sulfurs and a bonded N-C. Sulfur has the fewest candidates,
    // so every choice of six sulfurs is followed by the 40 nitrogens, whose 1,500 neighbors are
    // each looked at and none is a carbon. That is about 60,000 entries per choice and almost
    // no steps, so the scan limit ends the search long before the step limit would.
    let mut atoms: Vec<Atom> = (0..20).map(|_| atom("S")).collect();
    let mut bonds = Vec::new();
    for _ in 0..40 {
        let hub = atoms.len();
        atoms.push(atom("N"));
        for _ in 0..1_500 {
            bonds.push((hub, atoms.len()));
            atoms.push(atom("F"));
        }
    }
    for _ in 0..50 {
        let carbon = atoms.len();
        atoms.push(atom("C"));
        bonds.push((carbon, atoms.len()));
        atoms.push(atom("F"));
    }
    let target = large(atoms, &bonds);
    let mut query_atoms = vec!["S"; 6];
    query_atoms.extend(["N", "C"]);
    let query = mol(&query_atoms, &[(6, 7, "-")]);
    let started = std::time::Instant::now();
    assert_eq!(
        substructure::find(&query, &target),
        Err(MatchError::ScanLimit)
    );
    assert_eq!(MatchError::ScanLimit.code(), "match_limit");
    assert!(
        started.elapsed().as_secs() < 120,
        "took {:?}",
        started.elapsed()
    );
}

#[test]
fn a_disconnected_64_atom_query_in_a_large_target_is_bounded_by_the_mapping_limit() {
    let target = large(vec![atom("C"); 20_000], &[]);
    let query = mol(&vec!["C"; MAX_QUERY_ATOMS], &[]);
    let started = std::time::Instant::now();
    assert_eq!(
        substructure::find(&query, &target),
        Err(MatchError::MappingLimit)
    );
    assert!(
        started.elapsed().as_secs() < 60,
        "took {:?}",
        started.elapsed()
    );
}

#[test]
fn an_empty_answer_does_not_say_the_target_was_read() {
    // Kekulé tetralin: the aromaticity model cannot read its rings.
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
    // No assignment of a bromine exists, so the answer is empty and complete, and the rings
    // were never read (no step was taken).
    let none = substructure::find(&mol(&["Br"], &[]), &tetralin).unwrap();
    assert!(none.mappings.is_empty());
    assert_eq!(none.steps, 0);
    // A query that can be assigned compares the ring bonds and is refused, for the same target.
    assert!(matches!(
        refusal(&mol(&["C", "C"], &[(0, 1, "-")]), &tetralin),
        MatchError::Unsupported {
            side: Side::Target,
            reason: "fused_partially_aromatic",
            ..
        }
    ));
}

#[test]
fn a_query_bond_in_a_ring_with_a_pi_bond_is_refused_whatever_kind_of_query_bond_it_is() {
    // The aromaticity model reads no ring system with a query bond in it, and a ring system with
    // a double or aromatic bond cannot be taken as drawn: it is refused, naming the model's
    // reason, for an any bond and a list alike, in a Kekulé and an aromatic spelling.
    for kind in ["~", "-=", "-:"] {
        let kekule: Vec<_> = (0..6)
            .map(|i| {
                (
                    i,
                    (i + 1) % 6,
                    if i == 5 {
                        kind
                    } else if i % 2 == 0 {
                        "="
                    } else {
                        "-"
                    },
                )
            })
            .collect();
        let aromatic: Vec<_> = (0..6)
            .map(|i| (i, (i + 1) % 6, if i == 5 { kind } else { ":" }))
            .collect();
        for bonds in [kekule, aromatic] {
            let query = mol(&["C"; 6], &bonds);
            assert!(
                matches!(
                    refusal(&query, &aromatic_benzene()),
                    MatchError::Unsupported {
                        side: Side::Query,
                        reason: "unsupported_ring_bond",
                        ..
                    }
                ),
                "{kind} in {bonds:?}"
            );
        }
    }
    // The same ring bond in a ring with no double or aromatic bond is read as drawn.
    let all_single: Vec<_> = (0..6)
        .map(|i| (i, (i + 1) % 6, if i == 5 { "~" } else { "-" }))
        .collect();
    assert_eq!(
        found(&mol(&["C"; 6], &all_single), &cycle(6, "-")).len(),
        12
    );
}

#[test]
fn a_charge_or_isotope_on_a_query_atom_is_refused_not_ignored() {
    // A drawing cannot hold either (see Atom::validate), so only a graph built outside the
    // drawing model reaches this: the matcher would otherwise ignore the condition silently.
    let target = mol(&["C", "C"], &[(0, 1, "-")]);
    // The document model refuses to record either on a query atom, so the graph is assembled
    // from parts, as for a reader that has no document.
    let raw = |atoms: Vec<Atom>, bonds: &[(u32, u32)]| {
        MoleculeGraph::from_parts(
            atoms
                .into_iter()
                .enumerate()
                .map(|(i, a)| (id(u32::try_from(i).unwrap() + 1), a))
                .collect(),
            bonds
                .iter()
                .enumerate()
                .map(|(i, &(a, b))| {
                    (
                        bond_id(u32::try_from(i).unwrap() + 1),
                        chemifree_domain::document::Bond::new(id(a), id(b), BondOrder::Single),
                    )
                })
                .collect(),
        )
    };
    let with = |edit: fn(&mut Atom)| {
        let mut wildcard = atom("*");
        edit(&mut wildcard);
        raw(vec![atom("C"), wildcard], &[(1, 2)])
    };
    assert_eq!(
        refusal(&with(|a| a.charge = 1), &target),
        MatchError::Unsupported {
            side: Side::Query,
            reason: "query_atom_charge",
            at: Some(Item::Atom(id(2))),
            query: None,
        }
    );
    assert_eq!(
        refusal(&with(|a| a.isotope = Some(13)), &target),
        MatchError::Unsupported {
            side: Side::Query,
            reason: "query_atom_isotope",
            at: Some(Item::Atom(id(2))),
            query: None,
        }
    );
    // Every kind of query atom is held to it, a stated charge of zero is no charge, and an
    // element atom's charge and isotope remain conditions.
    for spec in ["A", "Q", "X", "[N,O]", "![N,O]"] {
        let mut query = atom(spec);
        query.charge = -1;
        assert!(
            matches!(
                refusal(&raw(vec![query], &[]), &target),
                MatchError::Unsupported {
                    side: Side::Query,
                    reason: "query_atom_charge",
                    ..
                }
            ),
            "{spec}"
        );
    }
    assert_eq!(found(&mol(&["*"], &[]), &target).len(), 2);
    assert_eq!(
        found(&mol(&["13C"], &[]), &mol(&["13C", "C"], &[])).len(),
        1
    );
}

#[test]
fn a_bond_in_an_aromatic_system_is_never_a_query_unknown_dative_or_triple_bond() {
    // The matcher reads a bond of a system classified aromatic as aromatic before it looks at
    // the bond's own kind. That order would be wrong for a query, unknown or dative bond; it
    // cannot be reached, because the aromaticity model never classifies as aromatic a system
    // that holds any bond kind but single, double or aromatic (`unsupported_ring_bond`), or an
    // atom that is a query (`query_atom`). Every such ring, in both Kekulé and aromatic
    // spelling, is therefore refused whole on whichever side it is on.
    let kekule = ["=", "-", "=", "-", "=", "-"];
    let aromatic = [":"; 6];
    for (spelling, kinds) in [("kekule", kekule), ("aromatic", aromatic)] {
        for odd in ["~", "-=", "?", ">", "#"] {
            for position in 0..6 {
                let mut drawn = kinds;
                drawn[position] = odd;
                let bonds: Vec<_> = (0..6).map(|i| (i, (i + 1) % 6, drawn[i])).collect();
                let ring = mol(&["C"; 6], &bonds);
                for system in aromaticity::perceive(&ring).systems {
                    assert_ne!(
                        system.classification,
                        Classification::Aromatic,
                        "{spelling} ring with {odd} at bond {position}"
                    );
                }
                // As a query it is refused for that reason; as a target a decided query
                // cannot map onto it.
                let one = mol(&["C", "C"], &[(0, 1, "=")]);
                for (side, outcome) in [
                    (Side::Query, substructure::find(&ring, &kekule_benzene())),
                    (Side::Target, substructure::find(&one, &ring)),
                ] {
                    let Err(MatchError::Unsupported { side: got, .. }) = outcome else {
                        panic!("{spelling} {odd} {position} on the {side:?} side: {outcome:?}");
                    };
                    assert_eq!(got, side, "{spelling} {odd} {position}");
                }
            }
        }
    }
    // A query atom in the ring is likewise never aromatic.
    for position in 0..6 {
        let mut atoms = ["C"; 6];
        atoms[position] = "*";
        let ring = mol(
            &atoms,
            &[
                (0, 1, "="),
                (1, 2, "-"),
                (2, 3, "="),
                (3, 4, "-"),
                (4, 5, "="),
                (5, 0, "-"),
            ],
        );
        for system in aromaticity::perceive(&ring).systems {
            assert_ne!(
                system.classification,
                Classification::Aromatic,
                "{position}"
            );
        }
    }
}
