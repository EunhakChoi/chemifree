//! What docs/CONTRACT.md says about SMILES, checked against the codec: its error-code table row
//! by row, every example row, and the lists of what is refused and what is accepted.

use chemifree_application::smiles::{SmilesCode, SmilesCodec, SmilesStyle};
use chemifree_domain::document::{Atom, AtomId, Bond, BondId, BondOrder};
use chemifree_domain::element::Element;
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::states::{AtomKind, Hydrogens, QueryAtom};
use chemifree_providers::OpenSmilesCodec;

fn contract_section() -> String {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/CONTRACT.md");
    let text = std::fs::read_to_string(path).unwrap();
    let start = text
        .find("\n## SMILES\n")
        .expect("CONTRACT.md has a SMILES section");
    let rest = &text[start + 1..];
    let end = rest[3..].find("\n## ").map_or(rest.len(), |at| at + 3);
    rest[..end].to_owned()
}

/// The rows of the first table after the `### heading`, header and rule rows excluded, each cell
/// trimmed. A cell's backticks are kept.
fn table(section: &str, heading: &str) -> Vec<Vec<String>> {
    let start = section
        .find(&format!("### {heading}"))
        .unwrap_or_else(|| panic!("no heading {heading}"));
    let mut rows = Vec::new();
    let mut in_table = false;
    for line in section[start..].lines().skip(1) {
        if line.starts_with('|') {
            in_table = true;
            let cells: Vec<String> = line
                .trim()
                .trim_matches('|')
                .split(" | ")
                .map(|cell| cell.trim().trim_matches('|').trim().to_owned())
                .collect();
            rows.push(cells);
        } else if in_table {
            // The first line that is not a row ends the table.
            break;
        }
    }
    // The header row and the rule under it are not data.
    rows.into_iter().skip(2).collect()
}

fn unquote(cell: &str) -> &str {
    cell.trim().trim_matches('`')
}

fn graph_of(text: &str) -> Option<MoleculeGraph> {
    let read = OpenSmilesCodec.read(text).ok()?;
    let id = |i: usize| AtomId::from_serial(u32::try_from(i + 1).unwrap()).unwrap();
    Some(MoleculeGraph::from_parts(
        read.atoms
            .iter()
            .enumerate()
            .map(|(i, a)| (id(i), a.atom.clone()))
            .collect(),
        read.bonds
            .iter()
            .enumerate()
            .map(|(i, b)| {
                (
                    BondId::from_serial(u32::try_from(i + 1).unwrap()).unwrap(),
                    Bond {
                        begin: id(b.begin),
                        end: id(b.end),
                        order: b.order.clone(),
                    },
                )
            })
            .collect(),
    ))
}

#[test]
fn the_error_code_table_is_the_codecs_table_row_for_row() {
    let section = contract_section();
    let rows = table(&section, "Error codes");
    let documented: Vec<(String, String, String)> = rows
        .iter()
        .map(|row| {
            assert_eq!(row.len(), 3, "{row:?}");
            (unquote(&row[0]).to_owned(), row[1].clone(), row[2].clone())
        })
        .collect();
    let actual: Vec<(String, String, String)> = SmilesCode::ALL
        .iter()
        .map(|code| {
            (
                code.as_str().to_owned(),
                code.class().as_str().to_owned(),
                code.message().to_owned(),
            )
        })
        .collect();
    assert_eq!(
        documented.len(),
        actual.len(),
        "the table lists every code once"
    );
    for (documented, actual) in documented.iter().zip(&actual) {
        assert_eq!(documented, actual);
    }
    assert!(documented.iter().any(|(code, _, _)| code == "empty_input"));
}

#[test]
fn every_reading_refusal_example_is_refused_with_its_code_and_offset() {
    let section = contract_section();
    let rows = table(&section, "Examples: reading refusals");
    assert!(rows.len() >= 15, "{} rows", rows.len());
    for row in rows {
        let input = unquote(&row[0]);
        let expected_code = unquote(&row[1]);
        let expected_offset: usize = row[2].parse().unwrap();
        let error = OpenSmilesCodec
            .read(input)
            .expect_err(&format!("{input:?} should be refused"));
        assert_eq!(error.code.as_str(), expected_code, "{input:?}");
        assert_eq!(error.span.unwrap().offset, expected_offset, "{input:?}");
    }
}

#[test]
fn every_writing_example_gives_the_documented_text_or_refusal() {
    let section = contract_section();
    let rows = table(&section, "Examples: writing");
    assert!(rows.len() >= 10, "{} writing examples", rows.len());
    for cells in rows {
        assert_eq!(cells.len(), 3, "{cells:?}");
        let input = unquote(&cells[0]);
        let graph = graph_of(input).unwrap_or_else(|| panic!("{input:?} is not readable"));
        for (style, cell) in [
            (SmilesStyle::Recorded, &cells[1]),
            (SmilesStyle::Canonical, &cells[2]),
        ] {
            let found = match OpenSmilesCodec.write(&graph, style) {
                Ok(written) => format!("`{}`", written.text),
                Err(error) => format!("refused: `{}`", error.code.as_str()),
            };
            assert_eq!(&found, cell, "{input:?} {style:?}");
        }
    }
}

fn element(symbol: &str) -> Atom {
    Atom::new(AtomKind::Element(Element::from_symbol(symbol).unwrap()))
}

fn build(atoms: Vec<Atom>, bonds: Vec<(usize, usize, BondOrder)>) -> MoleculeGraph {
    let id = |i: usize| AtomId::from_serial(u32::try_from(i + 1).unwrap()).unwrap();
    MoleculeGraph::from_parts(
        atoms
            .into_iter()
            .enumerate()
            .map(|(i, a)| (id(i), a))
            .collect(),
        bonds
            .into_iter()
            .enumerate()
            .map(|(i, (a, b, order))| {
                (
                    BondId::from_serial(u32::try_from(i + 1).unwrap()).unwrap(),
                    Bond {
                        begin: id(a),
                        end: id(b),
                        order,
                    },
                )
            })
            .collect(),
    )
}

/// The documented list of what the canonical form has no text for, one graph for each item, and
/// the documented exceptions that do have one.
#[test]
fn the_documented_canonical_refusals_match_the_codec_exactly() {
    let section = contract_section();
    for documented in [
        "atoms outside the valence model",
        "a radical the model cannot state",
        "unknown hydrogens",
        "query atoms",
        "dative or unknown bonds",
        "an aromatic system the aromaticity model cannot class",
        "More than 128 atoms, or a search beyond its work limit, is `canonical_limit`",
        "Doublet radicals such as `[CH3]` and `[OH]` have a canonical form",
    ] {
        assert!(
            section.contains(documented),
            "the contract no longer says {documented:?}"
        );
    }
    let code = |graph: &MoleculeGraph| {
        OpenSmilesCodec
            .write(graph, SmilesStyle::Canonical)
            .map(|written| written.text)
            .map_err(|error| (error.code.as_str(), error.reason))
    };
    let refused = |graph: MoleculeGraph, reason: &str| {
        let (found, found_reason) = code(&graph).expect_err("the canonical form refuses this");
        assert_eq!(found, "canonical_unsupported");
        assert_eq!(found_reason.as_deref(), Some(reason));
    };
    let mut sodium = element("Na");
    sodium.charge = 1;
    refused(build(vec![sodium], vec![]), "element_or_charge");
    refused(build(vec![element("Fe")], vec![]), "element_or_charge");
    refused(graph_of("[CH2]").unwrap(), "radical_state");
    let mut unknown = element("C");
    unknown.hydrogens = Hydrogens::Unknown;
    refused(build(vec![unknown], vec![]), "unknown_hydrogens");
    let mut query = element("C");
    query.kind = AtomKind::Query(QueryAtom::Any);
    refused(build(vec![query], vec![]), "query_atom");
    refused(
        build(
            vec![element("C"), element("O")],
            vec![(0, 1, BondOrder::Dative)],
        ),
        "non_integral_or_unknown_bond",
    );
    refused(
        build(
            vec![element("C"), element("O")],
            vec![(0, 1, BondOrder::Unknown)],
        ),
        "non_integral_or_unknown_bond",
    );
    let five: Vec<_> = (0..5)
        .map(|i| (i, (i + 1) % 5, BondOrder::Aromatic))
        .collect();
    let (found, reason) = code(&build((0..5).map(|_| element("C")).collect(), five)).unwrap_err();
    assert_eq!(found, "canonical_unsupported");
    assert!(reason.is_some());
    let chain = build(
        (0..129).map(|_| element("C")).collect(),
        (1..129).map(|i| (i - 1, i, BondOrder::Single)).collect(),
    );
    assert_eq!(code(&chain).unwrap_err().0, "canonical_limit");
    // The exceptions have a text.
    for (input, text) in [("[CH3]", "[CH3]"), ("[OH]", "[OH]")] {
        assert_eq!(code(&graph_of(input).unwrap()).unwrap(), text);
    }
    // A drawn singlet or triplet radical is not writable at all.
    let mut carbene = element("C");
    carbene.radical = chemifree_domain::states::Radical::Triplet;
    let error = OpenSmilesCodec
        .write(&build(vec![carbene], vec![]), SmilesStyle::Canonical)
        .unwrap_err();
    assert_eq!(error.code.as_str(), "radical_not_writable");
}

/// Each sentence of the contract that says what is accepted or what is refused, with an input
/// that proves it.
#[test]
fn documented_reading_claims_hold() {
    let section = contract_section();
    let accepted = |input: &str| {
        OpenSmilesCodec
            .read(input)
            .unwrap_or_else(|e| panic!("{input}: {e}"))
    };
    let refused = |input: &str| OpenSmilesCodec.read(input).unwrap_err().code.as_str();
    // "so `[002H]` is `[2H]`"
    assert!(section.contains("`[002H]` is `[2H]`"));
    assert_eq!(accepted("[002H]").atoms[0].atom.isotope, Some(2));
    // "`+0` and `-0` are charge 0"
    assert!(section.contains("`+0` and `-0` are charge 0"));
    assert_eq!(accepted("[C+0]").atoms[0].atom.charge, 0);
    assert_eq!(accepted("[C-0]").atoms[0].atom.charge, 0);
    // "biphenyl `c1ccccc1c1ccccc1` joins two aromatic rings by a single bond"
    assert!(
        section.contains("biphenyl `c1ccccc1c1ccccc1` joins two aromatic rings by a single bond")
    );
    let biphenyl = accepted("c1ccccc1c1ccccc1");
    assert_eq!(
        biphenyl
            .bonds
            .iter()
            .filter(|b| b.order == BondOrder::Single)
            .count(),
        1
    );
    // "`$` is refused as `quadruple_bond_unsupported`", "`*` ... `wildcard_unsupported`"
    assert_eq!(refused("C$C"), "quadruple_bond_unsupported");
    assert_eq!(refused("*"), "wildcard_unsupported");
    assert_eq!(refused("[C@H](F)Cl"), "stereo_unsupported");
    assert_eq!(refused("[0C]"), "isotope_zero");
    assert_eq!(refused("[1000C]"), "isotope_out_of_range");
    // "Ending": text after whitespace is reported, leading whitespace and empty text are codes.
    assert!(accepted("CCO ethanol").ignored_text.is_some());
    assert_eq!(refused(" CCO"), "leading_whitespace");
    assert_eq!(refused(""), "empty_input");
    // "Nesting up to 64 deep", "16 KiB", "500 atoms plus bonds"
    for fact in [
        "Nesting up to 64 deep",
        "16 KiB of input",
        "at most 500 atoms plus bonds",
    ] {
        assert!(section.contains(fact), "{fact}");
    }
    assert_eq!(chemifree_providers::MAX_BRANCH_DEPTH, 64);
    assert_eq!(chemifree_providers::MAX_INPUT_BYTES, 16 * 1024);
    assert_eq!(chemifree_domain::document::MAX_COMMANDS, 500);
    // The bracket atoms named as having no single radical state.
    for text in ["[Na+]", "[Fe]", "[CH2]", "[C]", "[CH]", "[N]"] {
        assert!(
            section.contains(&format!("`{text}`")),
            "{text} is named in the contract"
        );
        let read = accepted(text);
        assert!(
            read.losses
                .iter()
                .any(|loss| loss.feature == "radical_state"),
            "{text}"
        );
    }
    // "Ring labels `0`-`9` and `%00`-`%99`", "may be reused ... including at the atom that closes them"
    assert_eq!(accepted("C%99CCCCC%99").bonds.len(), 6);
    assert_eq!(accepted("C1CC11CC1").bonds.len(), 6);
    assert_eq!(refused("C%(123)C"), "ring_label_extended_unsupported");
}

#[test]
fn documented_writing_claims_hold() {
    let section = contract_section();
    let write = |input: &str, style: SmilesStyle| {
        OpenSmilesCodec
            .write(&graph_of(input).unwrap(), style)
            .map(|written| written.text)
    };
    // Kekulé stays Kekulé in the recorded writer and becomes aromatic in the canonical one.
    assert!(section.contains("Kekulé bonds stay Kekulé and aromatic bonds stay aromatic"));
    assert_eq!(
        write("C1=CC=CC=C1", SmilesStyle::Recorded).unwrap(),
        "C1=CC=CC=C1"
    );
    assert_eq!(
        write("C1=CC=CC=C1", SmilesStyle::Canonical).unwrap(),
        "c1ccccc1"
    );
    // An ion or a metal is written in brackets with the two consent-required assumptions.
    let mut sodium = element("Na");
    sodium.charge = 1;
    let written = OpenSmilesCodec
        .write(&build(vec![sodium], vec![]), SmilesStyle::Recorded)
        .unwrap();
    assert_eq!(written.text, "[Na+]");
    let required: Vec<&str> = written
        .losses
        .iter()
        .filter(|loss| loss.consent == chemifree_application::LossConsent::Required)
        .map(|loss| loss.feature.as_str())
        .collect();
    assert_eq!(required, ["hydrogen_assumption", "radical_assumption"]);
    // Folded hydrogens are listed apart from the atom order.
    let methane = OpenSmilesCodec
        .write(&graph_of("C[H]").unwrap(), SmilesStyle::Canonical)
        .unwrap();
    assert_eq!(
        (
            methane.text.as_str(),
            methane.atom_order.len(),
            methane.folded_hydrogens.len()
        ),
        ("C", 1, 1)
    );
}
