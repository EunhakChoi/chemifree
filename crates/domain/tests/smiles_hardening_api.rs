//! Regression checks for the SMILES diagnostics and limits added after the first review round.

use chemifree_domain::document::{Atom, Command, Document, DocumentId, NewAtomPositions, Point};
use chemifree_domain::element::Element;
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::smiles::{self, WriteError, parse, parse_with_command_limit};

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
