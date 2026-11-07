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
                    Bond::new(id(b.begin), id(b.end), b.order.clone()),
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
                    Bond::new(id(a), id(b), order),
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
    sodium.charge = 2;
    refused(build(vec![sodium], vec![]), "element_or_charge");
    refused(build(vec![element("Fe")], vec![]), "element_or_charge");
    // Sodium(I) is in the valence model, so the graph model has an identity for it.
    let mut sodium = element("Na");
    sodium.charge = 1;
    assert_eq!(code(&build(vec![sodium], vec![])).unwrap(), "[Na+]");
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
    assert_eq!(refused("[C@H](F)Cl"), "stereo_neighbor_count");
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
    for text in ["[Fe]", "[CH2]", "[C]", "[CH]", "[N]"] {
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
    // A dot may follow `(` but `(` may not follow a dot.
    assert!(section.contains("`C(.C)C`") && section.contains("`C.(C)C` is `branch_without_atom`"));
    assert_eq!(accepted("C(.C)C").bonds.len(), 1);
    assert_eq!(refused("C.(C)C"), "branch_without_atom");
    assert_eq!(refused("C(.)C"), "dangling_dot");
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
    // A metal outside the valence model is written in brackets with the two consent-required
    // assumptions; the simple ions the model lists are not.
    let mut sodium = element("Na");
    sodium.charge = 1;
    let salt = OpenSmilesCodec
        .write(&build(vec![sodium], vec![]), SmilesStyle::Recorded)
        .unwrap();
    assert_eq!(salt.text, "[Na+]");
    assert!(
        salt.losses
            .iter()
            .all(|loss| loss.consent != chemifree_application::LossConsent::Required),
        "{:?}",
        salt.losses
    );
    let written = OpenSmilesCodec
        .write(&build(vec![element("Fe")], vec![]), SmilesStyle::Recorded)
        .unwrap();
    assert_eq!(written.text, "[Fe]");
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

#[test]
fn documented_hydrogen_state_claims_hold() {
    let section = contract_section();
    assert!(section.contains("is never written as a guess"));
    // A count the model determines is stated in recorded text and reported; canonical text is not.
    let mut ammonium = element("N");
    ammonium.charge = 1;
    let graph = build(vec![ammonium], vec![]);
    let recorded = OpenSmilesCodec
        .write(&graph, SmilesStyle::Recorded)
        .unwrap();
    assert!(
        recorded
            .losses
            .iter()
            .any(|loss| loss.feature == "hydrogen_state")
    );
    let canonical = OpenSmilesCodec
        .write(&graph, SmilesStyle::Canonical)
        .unwrap();
    assert!(canonical.losses.is_empty());
    // A count the model does not determine is refused, not guessed.
    let mut labelled = element("C");
    labelled.isotope = Some(13);
    let mut atoms = vec![labelled];
    atoms.extend((0..5).map(|_| element("C")));
    let bonds = (1..=5).map(|i| (0, i, BondOrder::Single)).collect();
    let error = OpenSmilesCodec
        .write(&build(atoms, bonds), SmilesStyle::Recorded)
        .unwrap_err();
    assert_eq!(error.code.as_str(), "hydrogens_not_writable");
}

#[test]
fn documented_hydrogen_count_placement_and_fault_claims_hold() {
    let section = contract_section();
    for claim in [
        "a second digit, as in `[CH10]`, is `invalid_hydrogen_count`",
        "atoms and the end points of lines, rectangles and arrows count",
        "decided by the lowest-numbered faulty atom",
        "`bond_endpoints_invalid`",
    ] {
        assert!(
            section.contains(claim),
            "the contract no longer says {claim:?}"
        );
    }
    assert_eq!(
        OpenSmilesCodec.read("[CH10]").unwrap_err().code.as_str(),
        "invalid_hydrogen_count"
    );
    assert_eq!(
        OpenSmilesCodec.read("[CH9]").unwrap().atoms[0]
            .atom
            .hydrogens,
        Hydrogens::Specified(9)
    );
    let to_itself = build(vec![element("C")], vec![(0, 0, BondOrder::Single)]);
    let error = OpenSmilesCodec
        .write(&to_itself, SmilesStyle::Recorded)
        .unwrap_err();
    assert_eq!(error.code.as_str(), "bond_endpoints_invalid");
}

/// The SMILES bullet of docs/HANDOFF.ko.md, from its first line to the next bullet.
fn handoff_smiles_bullet() -> String {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/HANDOFF.ko.md");
    let text = std::fs::read_to_string(path).unwrap();
    let start = text
        .find("- P21 진행(")
        .expect("HANDOFF.ko.md has a P21 SMILES bullet");
    let rest = &text[start..];
    let end = rest[2..].find("\n- ").map_or(rest.len(), |at| at + 2);
    rest[..end].to_owned()
}

#[test]
fn the_handoff_smiles_bullet_agrees_with_the_codec() {
    use chemifree_application::smiles::{SMILES_CANONICAL_MODEL, SMILES_MODEL};
    let bullet = handoff_smiles_bullet();
    // The limits it states, and the model ids it names.
    let limits = format!(
        "원자+결합 {}, 입력 {} KiB, 분기 깊이 {}",
        chemifree_domain::document::MAX_COMMANDS,
        chemifree_providers::MAX_INPUT_BYTES / 1024,
        chemifree_providers::MAX_BRANCH_DEPTH
    );
    assert!(
        bullet.contains(&limits),
        "the bullet no longer says {limits:?}"
    );
    for id in [SMILES_CANONICAL_MODEL, "chemifree-chemical-graph-2"] {
        assert!(bullet.contains(id), "{id}");
    }
    assert_ne!(SMILES_MODEL, SMILES_CANONICAL_MODEL);
    // Every error code and loss feature it names is one the codec has.
    for code in [
        "canonical_unsupported",
        "canonical_limit",
        "hydrogens_not_writable",
        "branch_without_atom",
        "invalid_hydrogen_count",
    ] {
        assert!(bullet.contains(&format!("`{code}`")), "{code}");
        assert!(
            SmilesCode::ALL.iter().any(|known| known.as_str() == code),
            "{code} is not a code of the codec"
        );
    }
    let accepted = |text: &str| {
        OpenSmilesCodec
            .read(text)
            .unwrap_or_else(|e| panic!("{text}: {e}"))
    };
    let refusal = |text: &str| OpenSmilesCodec.read(text).err().map(|e| e.code.as_str());
    // A dot may follow `(`, and `(` may not follow a dot.
    assert!(bullet.contains("`C(.C)C`") && bullet.contains("`C.(C)C`"));
    assert_eq!(accepted("C(.C)C").bonds.len(), 1);
    assert_eq!(refusal("C.(C)C"), Some("branch_without_atom"));
    // Two digits of hydrogens, and a directional bond on a ring-closure digit, are refused; a
    // directional bond beside a double bond is read.
    assert_eq!(refusal("[CH10]"), Some("invalid_hydrogen_count"));
    assert_eq!(
        refusal("C/1=C/CCCCCCC1"),
        Some("directional_bond_unsupported")
    );
    assert_eq!(accepted("F/C=C/F").stereo.double_bonds.len(), 1);
    // The simple ions of the valence model need no consent; a metal does, and so does an
    // unlisted charge. This is what makes `[Na+].[Cl-]` a plain salt.
    assert!(bullet.contains("`[Na+].[Cl-]`"));
    let salt = accepted("[Na+].[Cl-]");
    assert!(salt.losses.is_empty(), "{:?}", salt.losses);
    for text in ["[Fe]", "[Na+2]"] {
        assert!(
            accepted(text)
                .losses
                .iter()
                .any(|loss| loss.feature == "radical_state"),
            "{text}"
        );
        assert!(bullet.contains(&format!("`{text}`")), "{text} is named");
    }
    let graph = graph_of("[Na+].[Cl-]").unwrap();
    for style in [SmilesStyle::Recorded, SmilesStyle::Canonical] {
        let written = OpenSmilesCodec.write(&graph, style).unwrap();
        assert_eq!(written.text, "[Na+].[Cl-]", "{style:?}");
        assert!(
            written
                .losses
                .iter()
                .all(|loss| loss.consent != chemifree_application::LossConsent::Required)
        );
    }
    let fe = graph_of("[Fe]").unwrap();
    assert_eq!(
        OpenSmilesCodec
            .write(&fe, SmilesStyle::Canonical)
            .err()
            .map(|e| e.code),
        Some(SmilesCode::CanonicalUnsupported)
    );
    // The canonical refusal for more than 128 atoms is the limit, not an unsupported graph.
    let chain = build(
        (0..129).map(|_| element("C")).collect(),
        (1..129).map(|i| (i - 1, i, BondOrder::Single)).collect(),
    );
    assert_eq!(
        OpenSmilesCodec
            .write(&chain, SmilesStyle::Canonical)
            .err()
            .map(|e| e.code),
        Some(SmilesCode::CanonicalLimit)
    );
    // The two doublet radicals it names have a canonical text; `[CH2]` has none.
    for text in ["[CH3]", "[OH]"] {
        let graph = graph_of(text).unwrap();
        assert!(
            OpenSmilesCodec
                .write(&graph, SmilesStyle::Canonical)
                .is_ok(),
            "{text}"
        );
    }
    assert!(
        OpenSmilesCodec
            .write(&graph_of("[CH2]").unwrap(), SmilesStyle::Canonical)
            .is_err()
    );
    // The web client check and the browser smoke it cites exist.
    assert!(bullet.contains("smiles_panel"));
    let smoke = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../scripts/browser-smoke.mjs"
    ))
    .unwrap();
    assert!(smoke.contains("results.smiles_panel = true"));
    // Recovery belongs to the immutable attempt, independently of edited source
    // or a discarded preview. The cited browser checks compare actual requests
    // and entity counts, rather than merely showing an outcome-check button.
    for claim in [
        "immutable attempt",
        "Check SMILES import outcome",
        "request ID/body",
        "source를 바꿔도",
        "reload/reconcile",
    ] {
        assert!(bullet.contains(claim), "missing recovery claim: {claim}");
    }
    assert!(
        !bullet.contains("Retry the same import"),
        "the handoff must not describe the superseded preview-bound retry"
    );
    for check in [
        "assert.deepEqual(lostSmilesRequests[0], lostSmilesRequests[1])",
        "editing text retains the original attempt",
        "assert.deepEqual((await checkedImport).postDataJSON(), lostSmilesRequests[0]",
        "the retry was answered from the committed reply, not imported again",
        "Reload document to reconcile",
    ] {
        assert!(
            smoke.contains(check),
            "missing smoke recovery check: {check}"
        );
    }
    let clipboard = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../scripts/web-smiles-clipboard-check.mjs"
    ))
    .unwrap();
    for check in [
        "assert.deepEqual(accepted[0], accepted[1])",
        "assert.deepEqual(checked[2], accepted[0])",
        "assert.equal(commits.size, 1)",
        "explicit new consent is a new, distinct attempt",
    ] {
        assert!(
            clipboard.contains(check),
            "missing clipboard recovery check: {check}"
        );
    }
}

/// The "Stereo read" and "SMILES double-bond direction" sections name exactly the stereo refusal
/// codes the codec has: a code the text cites exists, and a stereo, CXSMILES or directional-bond
/// code of the codec is cited.
#[test]
fn the_stereo_read_sections_cite_every_stereo_code_and_only_real_ones() {
    let section = contract_section();
    let subsection = |heading: &str| {
        let start = section
            .find(heading)
            .unwrap_or_else(|| panic!("the SMILES section has {heading}"));
        let rest = &section[start..];
        let end = rest[3..].find("\n### ").map_or(rest.len(), |at| at + 3);
        rest[..end].to_owned()
    };
    let stereo = [
        subsection("### Stereo read (P29c)"),
        subsection("### SMILES double-bond direction (P29c2a)"),
    ]
    .join("\n");
    let stereo_like = |code: &str| {
        code.starts_with("stereo_")
            || code.starts_with("cxsmiles_")
            || code.starts_with("directional_bond_")
    };
    for code in SmilesCode::ALL.iter().map(|code| code.as_str()) {
        if stereo_like(code) && !code.starts_with("stereo_group") {
            assert!(
                stereo.contains(&format!("`{code}`")),
                "the sections do not cite `{code}`"
            );
        }
    }
    let known: Vec<&str> = SmilesCode::ALL.iter().map(|code| code.as_str()).collect();
    for token in stereo.split('`').skip(1).step_by(2) {
        if stereo_like(token) && !token.starts_with("stereo_group") && !token.contains([' ', ':']) {
            assert!(
                known.contains(&token),
                "`{token}` is cited but is not a code of the codec"
            );
        }
    }
}
