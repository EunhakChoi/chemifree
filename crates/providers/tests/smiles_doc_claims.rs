//! Every outcome docs/CONTRACT.md states for a SMILES, every error code and every limit it
//! names is checked here against the code, so the document cannot say what the code does not do.

mod smiles_common;

use chemifree_application::smiles_codec::SmilesStyle;
use chemifree_providers::smiles::{Code, MAX_BRANCH_DEPTH, MAX_COMMANDS, MAX_INPUT_BYTES, MAX_OPEN_RINGS};
use smiles_common::{CODEC, graph, read, write};

const CONTRACT: &str = include_str!("../../../docs/CONTRACT.md");
const HANDOFF: &str = include_str!("../../../docs/HANDOFF.ko.md");

fn claims() -> Vec<(String, String, String)> {
    let start = CONTRACT.find("<!-- smiles-claims:start -->").expect("claims table start");
    let end = CONTRACT.find("<!-- smiles-claims:end -->").expect("claims table end");
    CONTRACT[start..end]
        .lines()
        .filter(|line| line.starts_with("| `"))
        .map(|line| {
            let cells: Vec<&str> = line.trim_matches('|').split(" | ").map(str::trim).collect();
            assert_eq!(cells.len(), 3, "{line}");
            (
                cells[0].trim_matches('`').to_owned(),
                cells[1].trim_matches('`').to_owned(),
                cells[2].to_owned(),
            )
        })
        .collect()
}

/// "`text`" is a canonical text; "-" means there is none to check; anything else is
/// "code (reason)" or "code".
fn canonical_outcome(input: &str) -> String {
    let Ok(source) = read(input) else { return "-".into() };
    match write(&graph(&source), SmilesStyle::Canonical) {
        Ok(written) => format!("`{}`", written.text),
        Err(error) => match error.detail {
            Some(reason) => format!("{} ({reason})", error.code),
            None => error.code.to_owned(),
        },
    }
}

fn read_outcome(input: &str) -> String {
    match read(input) {
        Ok(_) => "ok".into(),
        Err(error) => match error.detail {
            Some(reason) => format!("{} ({reason})", error.code),
            None => error.code.to_owned(),
        },
    }
}

#[test]
fn every_documented_example_behaves_as_documented() {
    let rows = claims();
    assert!(rows.len() >= 30, "the table documents the accepted and refused cases: {}", rows.len());
    for (input, read_claim, canonical_claim) in rows {
        let input = input.replace("\\n", "\n").replace("\\t", "\t");
        assert_eq!(read_outcome(&input), read_claim.trim_matches('`'), "reading {input:?}");
        assert_eq!(canonical_outcome(&input), canonical_claim, "canonical form of {input:?}");
    }
}

#[test]
fn every_error_code_is_documented_and_no_unknown_code_is() {
    for code in Code::ALL {
        assert!(CONTRACT.contains(&format!("`{}`", code.as_str())), "CONTRACT.md omits `{}`", code.as_str());
    }
    let from = CONTRACT.find("## SMILES").expect("a SMILES section");
    let to = CONTRACT[from + 3..].find("\n## ").map_or(CONTRACT.len(), |at| from + 3 + at);
    let section = &CONTRACT[from..to];
    let known: Vec<&str> = Code::ALL.iter().map(|code| code.as_str()).collect();
    for token in section.split('`').skip(1).step_by(2) {
        let looks_like_code = token.ends_with("_unsupported")
            || token.ends_with("_not_writable")
            || token.ends_with("_not_representable")
            || token.ends_with("_limit");
        if looks_like_code && !token.contains(' ') && token != "canonical_unsupported" {
            assert!(
                known.contains(&token) || ["identity_unsupported", "operation_not_implemented", "unsupported_format"].contains(&token),
                "CONTRACT.md names `{token}`, which is not a SMILES code"
            );
        }
    }
}

#[test]
fn documented_limits_are_the_codec_constants() {
    for (name, value) in [
        ("MAX_INPUT_BYTES", MAX_INPUT_BYTES),
        ("MAX_COMMANDS", MAX_COMMANDS),
        ("MAX_BRANCH_DEPTH", MAX_BRANCH_DEPTH),
        ("MAX_OPEN_RINGS", MAX_OPEN_RINGS),
    ] {
        assert!(CONTRACT.contains(&format!("{name} = {value}")), "CONTRACT.md must state {name} = {value}");
    }
    let limits = chemifree_application::smiles_codec::SmilesCodec::limits(&CODEC);
    assert_eq!(limits.canonical_max_atoms, chemifree_domain::identity::MAX_ATOMS);
    assert!(CONTRACT.contains(&format!("canonical form numbers at most {} atoms", limits.canonical_max_atoms)));
}

#[test]
fn the_documents_state_the_same_limits_of_the_feature() {
    for (name, text) in [("CONTRACT.md", CONTRACT), ("HANDOFF.ko.md", HANDOFF)] {
        for needed in ["chemifree-smiles-canonical-1", "chemifree-chemical-graph-2"] {
            assert!(text.contains(needed), "{name} omits {needed}");
        }
    }
    for needed in ["Stereochemistry", "InChI", "reaction SMILES", "not compatible with any toolkit"] {
        assert!(CONTRACT.contains(needed), "CONTRACT.md omits {needed}");
    }
    for needed in ["입체", "InChI", "반응 SMILES", "도구"] {
        assert!(HANDOFF.contains(needed), "HANDOFF.ko.md omits {needed}");
    }
    // The old statements that SMILES does not exist must be gone.
    assert!(!HANDOFF.contains("SMILES는 없다"));
    assert!(!CONTRACT.contains("SMILES is not implemented"));
}
