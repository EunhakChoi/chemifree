//! Regression checks for the SMILES diagnostics and limits added after the first review round.

use chemifree_domain::document::{
    Atom, AtomId, AtomRef, BondOrder, Command, Document, DocumentId, NewAtomPositions, Point,
};
use chemifree_domain::element::Element;
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::smiles::{self, WriteError, parse, parse_with_command_limit};

/// Builds a graph in transactions small enough for the document's command limit.
fn molecule(atoms: Vec<Atom>, edges: &[(usize, usize, BondOrder)]) -> MoleculeGraph {
    let mut document = Document::new(DocumentId::parse("smiles-api").unwrap(), "t").unwrap();
    for chunk in atoms.chunks(400) {
        let commands: Vec<_> = chunk
            .iter()
            .map(|atom| Command::AddAtom {
                atom: atom.clone(),
                positions: NewAtomPositions::Single(Point::new(0.0, 0.0).unwrap()),
                bond_to: None,
            })
            .collect();
        document.apply(&commands).unwrap();
    }
    let id = |index: usize| AtomRef::Id(AtomId::from_serial(index as u32 + 1).unwrap());
    for chunk in edges.chunks(400) {
        let commands: Vec<_> = chunk
            .iter()
            .map(|(a, b, order)| Command::AddBond {
                begin: id(*a),
                end: id(*b),
                order: order.clone(),
            })
            .collect();
        document.apply(&commands).unwrap();
    }
    MoleculeGraph::from_document(&document)
}

fn element(symbol: &str) -> Atom {
    Atom::from(Element::from_symbol(symbol).unwrap())
}

#[test]
fn ignored_text_after_whitespace_is_reported_not_dropped() {
    let graph = parse("CCO ethanol").unwrap();
    assert_eq!(graph.ignored, Some((3, 11)));
    // Only whitespace after the SMILES leaves nothing ignored.
    for input in ["CCO", "CCO\n", "CCO \t\r\n"] {
        assert_eq!(parse(input).unwrap().ignored, None, "{input:?}");
    }
}

#[test]
fn aromatic_refusal_names_the_failing_system_and_its_reason() {
    let input = "c1ccccc1C.c1ccc2[nH]ccc2c1";
    let error = parse(input).unwrap_err();
    assert_eq!(error.code, "aromaticity_outside_model");
    assert_eq!(error.reason, Some("fused_system_outside_model"));
    // The span covers the indole atoms (not the benzene) up to its last atom; the final
    // ring digit is not part of an atom.
    assert_eq!((error.start, error.end), (10, input.len() - 1));
    let error = parse("C1=CC=CC=C1.c1ccc1").unwrap_err();
    assert_eq!(error.reason, Some("electron_count"));
    assert_eq!((error.start, error.end), (12, 17));
}

#[test]
fn command_limit_stops_parsing_before_the_whole_input_is_read() {
    let error = parse_with_command_limit("CCCCCCCCCC", 5).unwrap_err();
    assert_eq!(error.code, "command_limit");
    assert!(error.start < 6, "stopped at {}", error.start);
    assert!(parse_with_command_limit("CCC", 5).is_ok());
}

#[test]
fn recorded_writer_refuses_a_bracket_atom_whose_hydrogens_are_undetermined() {
    let mut sodium = Atom::from(Element::from_symbol("Na").unwrap());
    sodium.charge = 1;
    let mut document = Document::new(DocumentId::parse("undetermined").unwrap(), "t").unwrap();
    document
        .apply(&[Command::AddAtom {
            atom: sodium,
            positions: NewAtomPositions::Single(Point::new(0.0, 0.0).unwrap()),
            bond_to: None,
        }])
        .unwrap();
    assert_eq!(
        smiles::write_recorded(&MoleculeGraph::from_document(&document)),
        Err(WriteError::HydrogensUndetermined)
    );
}

#[test]
fn every_syntax_error_code_is_reachable_and_has_a_message() {
    let cases: &[(&str, &str)] = &[
        ("empty_input", ""),
        ("leading_whitespace", " C"),
        ("input_limit", &"C".repeat(smiles::MAX_SMILES_BYTES + 1)),
        ("atom_limit", &"C".repeat(smiles::MAX_ATOMS + 1)),
        (
            "branch_limit",
            &format!("C{}", "(C".repeat(smiles::MAX_BRANCH_DEPTH + 1)),
        ),
        ("expected_element", "C!"),
        ("expected_atom", "C=(C)C"),
        ("unknown_element", "[Xx]"),
        ("brackets_required", "H"),
        ("invalid_bracket_atom", "[C"),
        ("isotope_out_of_range", "[1000C]"),
        ("hydrogens_out_of_range", "[CH17]"),
        ("hydrogen_count_on_hydrogen", "[HH]"),
        ("charge_out_of_range", "[C+9]"),
        ("stereo_unsupported", "C/C"),
        ("query_unsupported", "*"),
        ("invalid_aromatic_bond", "C:C"),
        ("aromaticity_outside_model", "c1ccc1"),
        ("invalid_ring_number", "C%1"),
        ("ring_without_atom", "1C"),
        ("ring_bond_conflict", "C=1CC-1"),
        ("unclosed_ring", "C1CC"),
        ("ring_after_branch", "C(C)1CC1"),
        ("unexpected_branch_end", "C)"),
        ("branch_without_atom", "(C)C"),
        ("empty_or_incomplete_branch", "C()C"),
        ("unclosed_branch", "C(C"),
        ("unexpected_dot", ".C"),
        ("trailing_dot", "C."),
        ("unexpected_bond", "=C"),
        ("trailing_bond", "C="),
        ("duplicate_or_self_bond", "C1C1"),
    ];
    let mut covered = std::collections::BTreeSet::new();
    for &(code, input) in cases {
        let error = parse(input).unwrap_err();
        assert_eq!(error.code, code, "{input:?}");
        covered.insert(code);
    }
    for (code, message) in smiles::SYNTAX_ERRORS {
        assert_eq!(smiles::syntax_message(code), *message);
        assert!(!message.is_empty());
        assert!(
            // `command_limit` is exercised by `parse_with_command_limit` in the API tests; the
            // bond limit needs about 2,000 atoms with ring closures on all of them.
            covered.contains(code) || *code == "bond_limit" || *code == "command_limit",
            "{code} has a message but no input that reaches it"
        );
    }
    let listed: std::collections::BTreeSet<_> = smiles::SYNTAX_ERRORS
        .iter()
        .map(|(code, _)| *code)
        .collect();
    assert_eq!(listed.len(), smiles::SYNTAX_ERRORS.len(), "duplicate code");
    for code in &covered {
        assert!(
            listed.contains(code),
            "{code} is emitted but has no message"
        );
    }
}

fn spine_with_leaves(spine: usize) -> MoleculeGraph {
    let mut edges: Vec<_> = (0..spine - 1)
        .map(|i| (i, i + 1, BondOrder::Single))
        .collect();
    edges.extend((0..spine).map(|i| (i, spine + i, BondOrder::Single)));
    molecule(vec![element("C"); spine * 2], &edges)
}

#[test]
fn writer_refuses_nesting_its_own_reader_would_refuse() {
    // A spine of n atoms, each with a leaf, nests n - 1 branches.
    let limit = smiles::MAX_BRANCH_DEPTH + 1;
    let text = smiles::write_recorded(&spine_with_leaves(limit)).unwrap();
    assert!(parse(&text).is_ok(), "128 nested branches must read back");
    assert_eq!(
        smiles::write_recorded(&spine_with_leaves(limit + 1)),
        Err(smiles::WriteError::BranchLimit)
    );
    assert!(matches!(
        smiles::write_canonical(&spine_with_leaves(limit + 1)),
        Ok(_)
            | Err(smiles::CanonicalError::Write(
                smiles::WriteError::BranchLimit
            ))
            | Err(smiles::CanonicalError::Identity(_))
    ));
}
