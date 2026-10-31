use chemifree_domain::document::BondOrder;
use chemifree_domain::smiles::{WriteError, parse, write};
use chemifree_domain::states::{Hydrogens, Radical};

#[test]
fn branches_disconnected_ions_and_bracket_atom_states() {
    // 2-propanol and labeled methyl anion/ammonium follow OpenSMILES atom/branch syntax.
    let branched = parse("CC(O)C").unwrap();
    assert_eq!(
        branched
            .atoms
            .iter()
            .map(|a| a.atom.kind.element().unwrap().symbol())
            .collect::<Vec<_>>(),
        ["C", "C", "O", "C"]
    );
    assert_eq!(
        branched
            .bonds
            .iter()
            .map(|b| (b.begin, b.end))
            .collect::<Vec<_>>(),
        [(0, 1), (1, 2), (1, 3)]
    );
    let ions = parse("[13CH3-].[NH4+]").unwrap();
    assert_eq!(ions.bonds.len(), 0);
    assert_eq!(
        (
            ions.atoms[0].atom.isotope,
            ions.atoms[0].atom.hydrogens,
            ions.atoms[0].atom.charge
        ),
        (Some(13), Hydrogens::Specified(3), -1)
    );
    assert_eq!(
        (ions.atoms[1].atom.hydrogens, ions.atoms[1].atom.charge),
        (Hydrogens::Specified(4), 1)
    );
}

#[test]
fn dot_branches_restore_the_parent_without_connecting_the_detached_fragment() {
    // OpenSMILES permits dot chains within branches as well as at top level.
    for (input, expected) in [("C(.O)C", vec![(0, 2)]), ("C(C.O)N", vec![(0, 1), (0, 3)])] {
        let graph = parse(input).unwrap();
        assert_eq!(
            graph
                .bonds
                .iter()
                .map(|bond| (bond.begin, bond.end))
                .collect::<Vec<_>>(),
            expected,
            "{input}"
        );
    }
    for input in ["C(.)", "C(..O)", "C(C.)"] {
        assert!(parse(input).is_err(), "{input}");
    }
}

#[test]
fn writer_reuses_ring_labels_after_their_closures() {
    // One hundred small rings do not need one hundred simultaneously open labels.
    for input in [vec!["C1CC1"; 100].join("."), vec!["C1CC1"; 100].join("")] {
        let graph = parse(&input).unwrap();
        let order: Vec<_> = (0..graph.atoms.len()).collect();
        let output = write(&graph, &order).unwrap();
        let decoded = parse(&output).unwrap();
        assert_eq!(
            (decoded.atoms.len(), decoded.bonds.len()),
            (graph.atoms.len(), graph.bonds.len())
        );
    }
    // The central atom closes one ring and immediately opens another with the same label.
    let shared_atom = parse("C1CC11CC1").unwrap();
    let order: Vec<_> = (0..shared_atom.atoms.len()).collect();
    assert_eq!(write(&shared_atom, &order).unwrap(), "C1CC11CC1");
}

#[test]
fn bracket_atoms_do_not_silently_erase_possible_radical_electrons() {
    // RDKit 2022.03.2 independently identifies one radical electron in [CH3] and [OH].
    // This syntax alone does not specify all spin states, so keep uncertain state explicit.
    for input in ["[CH3]", "[OH]", "[O]"] {
        assert_eq!(
            parse(input).unwrap().atoms[0].atom.radical,
            Radical::Unknown,
            "{input}"
        );
    }
    for input in ["[CH4]", "[13CH3-]", "[NH4+]"] {
        assert_eq!(
            parse(input).unwrap().atoms[0].atom.radical,
            Radical::None,
            "{input}"
        );
    }
}

#[test]
fn rings_aromatic_bonds_and_explicit_double_bonds_are_distinct() {
    let benzene = parse("c1ccccc1").unwrap();
    assert_eq!((benzene.atoms.len(), benzene.bonds.len()), (6, 6));
    assert!(benzene.atoms.iter().all(|a| a.aromatic));
    assert!(benzene.bonds.iter().all(|b| b.order == BondOrder::Aromatic));
    let alternate = parse("C1=CC=CC=C1").unwrap();
    assert_eq!(
        alternate
            .bonds
            .iter()
            .filter(|b| b.order == BondOrder::Double)
            .count(),
        3
    );
    let two_digit = parse("C%12CCCCC%12").unwrap();
    assert_eq!((two_digit.atoms.len(), two_digit.bonds.len()), (6, 6));
    let pyrrole = parse("[nH]1cccc1").unwrap();
    assert_eq!(pyrrole.atoms[0].atom.hydrogens, Hydrogens::Specified(1));
}

#[test]
fn aromatic_bracket_atoms_keep_closed_shell_pyrrole_and_benzene() {
    for input in ["[nH]1cccc1", "[cH]1ccccc1"] {
        let graph = parse(input).unwrap();
        assert_eq!(graph.atoms[0].atom.radical, Radical::None, "{input}");
        let order: Vec<_> = (0..graph.atoms.len()).collect();
        let output = write(&graph, &order).unwrap();
        assert_eq!(parse(&output).unwrap().atoms.len(), graph.atoms.len());
    }
}

#[test]
fn aromatic_syntax_requires_a_complete_supported_assignment() {
    for input in ["c", "c1ccc1", "[se]1cccc1"] {
        let error = parse(input).unwrap_err();
        assert_eq!(error.code, "aromaticity_outside_model", "{input}");
        assert!(input.get(error.start..error.end).is_some());
    }
    for input in ["n1ccccc1", "o1cccc1", "c1ccccc1-c1ccccc1"] {
        assert!(parse(input).is_ok(), "{input}");
    }
    let mut orphan = parse("C").unwrap();
    orphan.atoms[0].aromatic = true;
    assert_eq!(
        write(&orphan, &[0]),
        Err(WriteError::UnsupportedAromaticity)
    );
}

#[test]
fn aromatic_bond_requires_aromatic_endpoints() {
    for input in ["C:C", "C:c", "c:C"] {
        let error = parse(input).unwrap_err();
        assert_eq!(error.code, "invalid_aromatic_bond", "{input}");
        assert!(input.get(error.start..error.end).is_some());
    }
    let mut graph = parse("CC").unwrap();
    graph.bonds[0].order = BondOrder::Aromatic;
    assert_eq!(write(&graph, &[0, 1]), Err(WriteError::UnsupportedBond));
}

#[test]
fn malformed_syntax_names_its_input_span_and_never_returns_partial_graph() {
    for (input, code, start, end) in [
        ("C1CC", "unclosed_ring", 1, 2),
        ("C(=O", "unclosed_branch", 1, 4),
        ("C=", "trailing_bond", 1, 2),
        ("C..O", "unexpected_dot", 2, 3),
        ("C%1", "invalid_ring_number", 1, 3),
        ("C/C", "stereo_unsupported", 1, 2),
        ("C*", "query_unsupported", 1, 2),
        ("Cé", "expected_element", 1, 3),
        ("[N+0]", "invalid_charge", 0, 4),
    ] {
        let error = parse(input).unwrap_err();
        assert_eq!(
            (error.code, error.start, error.end),
            (code, start, end),
            "{input}"
        );
    }
}

#[test]
fn syntax_spans_cover_complete_utf8_characters() {
    for (input, start, end) in [("[Cé]", 2, 4), ("C%1é", 1, 5)] {
        let error = parse(input).unwrap_err();
        assert_eq!((error.start, error.end), (start, end), "{input}");
        assert!(
            input.get(error.start..error.end).is_some(),
            "span must be sliceable UTF-8"
        );
    }
}

#[test]
fn generic_writer_emits_independently_interpretable_branches_rings_and_ions() {
    // These outputs are OpenSMILES forms with known graph meaning, rather than only a
    // self-roundtrip claim. Canonical ordering is provided separately by the graph model.
    for input in ["CC(O)C", "c1ccccc1", "[13CH3-].[NH4+]", "C1=CC=CC=C1"] {
        let graph = parse(input).unwrap();
        let order: Vec<_> = (0..graph.atoms.len()).collect();
        assert_eq!(write(&graph, &order).unwrap(), input);
    }
    let mut unsupported = parse("CC").unwrap();
    unsupported.bonds[0].order = BondOrder::Dative;
    assert_eq!(
        write(&unsupported, &[0, 1]),
        Err(WriteError::UnsupportedBond)
    );
    assert_eq!(
        write(&parse("CC").unwrap(), &[0, 0]),
        Err(WriteError::InvalidGraph)
    );
}
