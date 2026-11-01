//! Regression checks for the bounded SMILES grammar, valence-model agreement and writer.

use chemifree_domain::document::{
    Atom, AtomRef, BondOrder, Command, Document, DocumentId, NewAtomPositions, Point,
};
use chemifree_domain::element::Element;
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::smiles::{self, parse};
use chemifree_domain::states::Radical;

fn molecule(atoms: Vec<Atom>, edges: &[(usize, usize, BondOrder)]) -> MoleculeGraph {
    let mut document = Document::new(DocumentId::parse("smiles-hardening").unwrap(), "t").unwrap();
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
    document.apply(&commands).unwrap();
    MoleculeGraph::from_document(&document)
}

fn element(symbol: &str) -> Atom {
    Atom::from(Element::from_symbol(symbol).unwrap())
}

/// The graph of a SMILES string as comparable plain data.
fn shape(input: &str) -> (Vec<String>, Vec<(usize, usize, BondOrder)>) {
    let graph = parse(input).unwrap_or_else(|e| panic!("{input}: {e:?}"));
    (
        graph
            .atoms
            .iter()
            .map(|a| a.atom.kind.element().unwrap().symbol().to_owned())
            .collect(),
        graph
            .bonds
            .iter()
            .map(|b| (b.begin, b.end, b.order.clone()))
            .collect(),
    )
}

fn canonical(input: &str) -> String {
    let source = parse(input).unwrap_or_else(|e| panic!("{input}: {e:?}"));
    let atoms = source.atoms.iter().map(|a| a.atom.clone()).collect();
    let edges: Vec<_> = source
        .bonds
        .iter()
        .map(|b| (b.begin, b.end, b.order.clone()))
        .collect();
    smiles::write_canonical(&molecule(atoms, &edges))
        .unwrap_or_else(|e| panic!("{input}: canonical refused {e:?}"))
}

#[test]
fn canonical_text_is_a_fixpoint_and_ignores_spelling() {
    for input in [
        "C1CC1",
        "CC1(C)CC1",
        "C1CCC2(CC1)CCCC2",
        "C1CCCCC1",
        "c1ccccc1",
        "c1ccccc1c1ccccc1",
        "[nH+]1ccccc1",
        "[13CH3-].[NH4+]",
        "[2H]O[2H]",
        "CC(O)C.C1CC1",
    ] {
        let once = canonical(input);
        assert_eq!(canonical(&once), once, "fixpoint failed for {input}");
    }
    for (first, second) in [
        ("[CH2]1CC1", "C1CC1"),
        ("C1CC1(C)C", "CC1(C)CC1"),
        ("c1ccccc1-c1ccccc1", "c1ccccc1c1ccccc1"),
    ] {
        assert_eq!(canonical(first), canonical(second), "{first} vs {second}");
    }
}

#[test]
fn unspecified_bond_between_separate_aromatic_systems_is_single() {
    assert_eq!(shape("c1ccccc1c1ccccc1"), shape("c1ccccc1-c1ccccc1"));
    let (_, bonds) = shape("c1ccccc1c1ccccc1");
    assert_eq!(
        bonds
            .iter()
            .filter(|(_, _, order)| *order == BondOrder::Single)
            .count(),
        1
    );
    // A bond inside one fused system stays aromatic.
    assert!(
        shape("c1ccc2ccccc2c1")
            .1
            .iter()
            .all(|(_, _, order)| *order == BondOrder::Aromatic)
    );
    // An explicit aromatic symbol outside a ring is still refused.
    assert!(parse("c1ccccc1:c1ccccc1").is_err());
}

#[test]
fn ring_digits_after_a_closed_branch_are_a_positioned_syntax_error() {
    for (input, start) in [("C(C)1CC1", 4), ("C(C)(C)1CC1", 7)] {
        let error = parse(input).unwrap_err();
        assert_eq!(
            (error.code, error.start, error.end),
            ("ring_after_branch", start, start + 1),
            "{input}"
        );
    }
    assert!(parse("C1(C)CC1").is_ok());
    assert!(parse("C(C)C1CC1").is_ok());
}

#[test]
fn whitespace_ends_the_smiles() {
    for (input, atoms) in [
        ("CC(=O)O ", 4),
        ("CCO\n", 3),
        ("CCO\t", 3),
        ("CCO ethanol", 3),
        ("CCO \r\n", 3),
    ] {
        let graph = parse(input).unwrap_or_else(|e| panic!("{input:?}: {e:?}"));
        assert_eq!(graph.atoms.len(), atoms, "{input:?}");
    }
    assert_eq!(parse("C1CC ring").unwrap_err().code, "unclosed_ring");
}

#[test]
fn the_earliest_opened_unclosed_ring_is_reported() {
    let error = parse("C%100").unwrap_err();
    assert_eq!(
        (error.code, error.start, error.end),
        ("unclosed_ring", 1, 4)
    );
    let error = parse("C2CC1").unwrap_err();
    assert_eq!(
        (error.code, error.start, error.end),
        ("unclosed_ring", 1, 2)
    );
}

#[test]
fn bracket_radical_state_follows_the_shared_valence_model() {
    // Inside chemifree-valence-2 and closed shell.
    for input in ["[CH4]", "[NH4+]", "[OH-]", "[Cl-]", "[13CH3-]"] {
        assert_eq!(
            parse(input).unwrap().atoms[0].atom.radical,
            Radical::None,
            "{input}"
        );
    }
    // Open shell in the model, or an element or charge the model does not cover.
    for input in [
        "[CH3]",
        "[Na+]",
        "[K+]",
        "[Mg+2]",
        "[Fe+2]",
        "[Zn]",
        "C[Si](C)(C)C",
    ] {
        let graph = parse(input).unwrap();
        assert!(
            graph
                .atoms
                .iter()
                .any(|a| a.atom.radical == Radical::Unknown),
            "{input}"
        );
    }
    let graph = parse("C[P+](C)(C)C").unwrap();
    assert_eq!(graph.atoms[1].atom.radical, Radical::Unknown);
    assert_eq!(graph.atoms[0].atom.radical, Radical::None);
}

#[test]
fn recorded_writer_states_hydrogens_the_valence_model_determines() {
    let write = |atoms: Vec<Atom>, edges: &[(usize, usize, BondOrder)]| {
        smiles::write_recorded(&molecule(atoms, edges)).unwrap()
    };
    let mut methoxide_oxygen = element("O");
    methoxide_oxygen.charge = -1;
    assert_eq!(
        write(
            vec![element("C"), methoxide_oxygen],
            &[(0, 1, BondOrder::Single)]
        ),
        "C[O-]"
    );
    let mut ammonium = element("N");
    ammonium.charge = 1;
    assert_eq!(write(vec![ammonium], &[]), "[NH4+]");
    let mut carbon_13 = element("C");
    carbon_13.isotope = Some(13);
    assert_eq!(write(vec![carbon_13], &[]), "[13CH4]");
}

#[test]
fn recorded_writer_resolves_a_charged_aromatic_atom_through_its_kekule_form() {
    let mut cation = element("N");
    cation.charge = 1;
    let mut atoms = vec![cation];
    atoms.extend((0..5).map(|_| element("C")));
    atoms.push(element("C"));
    let mut edges: Vec<_> = (0..6)
        .map(|i| (i, (i + 1) % 6, BondOrder::Aromatic))
        .collect();
    edges.push((0, 6, BondOrder::Single));
    let text = smiles::write_recorded(&molecule(atoms, &edges)).unwrap();
    assert_eq!(text, "[n+]1(ccccc1)C");
    assert_eq!(shape(&text).1.len(), 7);
}

#[test]
fn leading_whitespace_is_a_positioned_error_not_a_panic() {
    for (input, end) in [(" C", 1), (" ", 1), ("\nC", 1), ("\t\t", 2), (" \r\nC", 3)] {
        let error = parse(input).unwrap_err();
        assert_eq!(
            (error.code, error.start, error.end),
            ("leading_whitespace", 0, end),
            "{input:?}"
        );
    }
}

#[test]
fn zero_charge_is_accepted_per_the_opensmiles_charge_grammar() {
    // charge ::= '-' DIGIT? DIGIT | '+' DIGIT? DIGIT | ... so `+0` and `-0` are legal.
    for input in ["[N+0]", "[Fe-0]", "[CH4+0]"] {
        let graph = parse(input).unwrap_or_else(|e| panic!("{input}: {e:?}"));
        assert_eq!(graph.atoms[0].atom.charge, 0, "{input}");
    }
    assert_eq!(parse("[N+00]").unwrap().atoms[0].atom.charge, 0);
}

#[test]
fn a_ring_digit_directly_after_an_open_parenthesis_names_the_missing_atom() {
    let error = parse("C(1)CC1").unwrap_err();
    assert_eq!(
        (error.code, error.start, error.end),
        ("ring_without_atom", 2, 3)
    );
}

#[test]
fn unsupported_aromatic_system_is_reported_before_atom_limitations() {
    let mut cation = element("N");
    cation.charge = 1;
    let atoms = vec![cation, element("C"), element("C"), element("C")];
    let edges: Vec<_> = (0..4)
        .map(|i| (i, (i + 1) % 4, BondOrder::Aromatic))
        .collect();
    assert_eq!(
        smiles::write_recorded(&molecule(atoms, &edges)),
        Err(smiles::WriteError::UnsupportedAromaticity)
    );
}

#[test]
fn kekule_written_pyrrole_and_imidazole_match_their_aromatic_spellings() {
    for (kekule, aromatic) in [
        ("N1C=CC=C1", "[nH]1cccc1"),
        ("C1=CNC=C1", "[nH]1cccc1"),
        ("[NH]1C=CC=C1", "c1cc[nH]c1"),
        ("N1C=CN=C1", "c1c[nH]cn1"),
        ("C1=CN=CN1", "c1c[nH]cn1"),
    ] {
        assert_eq!(
            canonical(kekule),
            canonical(aromatic),
            "{kekule} vs {aromatic}"
        );
    }
    // A pyridine-type N written aromatic without hydrogens is unchanged.
    assert_eq!(canonical("N1=CC=CC=C1"), canonical("n1ccccc1"));
}

#[test]
fn partly_saturated_fused_systems_are_refused_not_classified_by_their_spelling() {
    use chemifree_domain::aromaticity::{self, Classification};
    let classes = |input: &str| {
        let source = parse(input).unwrap();
        let atoms = source.atoms.iter().map(|a| a.atom.clone()).collect();
        let edges: Vec<_> = source
            .bonds
            .iter()
            .map(|b| (b.begin, b.end, b.order.clone()))
            .collect();
        aromaticity::perceive(&molecule(atoms, &edges))
            .systems
            .iter()
            .map(|s| s.classification)
            .collect::<Vec<_>>()
    };
    for input in [
        "C1CC2=CC=CC=C2C1",
        "C1CC2=C(C1)C=CC=C2",
        "C1CCC2=CC=CC=C2C1",
        "C1=CC=C2C(=C1)CC1=CC=CC=C12",
        "CC1(C)CC2=CC=CC=C12",
        "C1=CC2CCC1C2",
    ] {
        assert_eq!(classes(input), [Classification::Unsupported], "{input}");
        let source = parse(input).unwrap();
        let atoms = source.atoms.iter().map(|a| a.atom.clone()).collect();
        let edges: Vec<_> = source
            .bonds
            .iter()
            .map(|b| (b.begin, b.end, b.order.clone()))
            .collect();
        assert!(
            smiles::write_canonical(&molecule(atoms, &edges)).is_err(),
            "{input} must be refused, not given a spelling-dependent string"
        );
    }
    // Fused or bridged systems with only single ring bonds stay non-aromatic.
    for input in ["C1CC2CCC1C2", "C1CCC2CCCCC2C1", "C1CC12CC2"] {
        assert!(
            classes(input)
                .iter()
                .all(|class| *class == Classification::NonAromatic),
            "{input}"
        );
        assert!(!canonical(input).is_empty());
    }
}
