//! What docs/CONTRACT.md says about substructure matching, checked against the matcher: its
//! code table row by row, the reason and example tables against the shared fixtures, and the
//! numbers it quotes.

use chemifree_application::MATCH_CODES;
use chemifree_domain::aromaticity::{MAX_MATCH_STEPS, MAX_SYSTEM_ATOMS};
use chemifree_domain::document::{AtomId, BondId, MAX_ATOMS, MAX_BONDS};
use chemifree_domain::substructure::{
    Item, MAX_MAPPINGS, MAX_QUERY_ATOMS, MAX_SCANNED_CANDIDATES, MAX_SEARCH_STEPS, MatchError, Side,
};
use serde_json::Value;

fn contract_section() -> String {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/CONTRACT.md");
    let text = std::fs::read_to_string(path).unwrap();
    let start = text
        .find("\n## Substructure matching\n")
        .expect("CONTRACT.md has a Substructure matching section");
    let rest = &text[start + 1..];
    let end = rest[3..].find("\n## ").map_or(rest.len(), |at| at + 3);
    rest[..end].to_owned()
}

/// The data rows of the first table after the `### heading`, each cell trimmed of spaces and
/// backticks.
fn table(section: &str, heading: &str) -> Vec<Vec<String>> {
    let start = section
        .find(&format!("### {heading}"))
        .unwrap_or_else(|| panic!("no heading {heading}"));
    let mut rows = Vec::new();
    let mut in_table = false;
    for line in section[start..].lines().skip(1) {
        if line.starts_with('|') {
            in_table = true;
            rows.push(
                line.trim()
                    .trim_matches('|')
                    .split(" | ")
                    .map(|cell| {
                        cell.trim()
                            .trim_matches('|')
                            .trim()
                            .trim_matches('`')
                            .to_owned()
                    })
                    .collect::<Vec<_>>(),
            );
        } else if in_table {
            break;
        }
    }
    rows.into_iter().skip(2).collect()
}

fn fixtures() -> Vec<Value> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/query-fixtures.json"
    );
    let document: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    document["fixtures"].as_array().unwrap().clone()
}

fn fixture(name: &str) -> Value {
    fixtures()
        .into_iter()
        .find(|fixture| fixture["name"] == name)
        .unwrap_or_else(|| panic!("no fixture {name}"))
}

/// `2000000` as `2,000,000`, the way the contract writes it.
fn grouped(number: usize) -> String {
    let digits = number.to_string();
    let mut grouped = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    grouped
}

#[test]
fn the_code_table_is_the_operations_table_row_for_row() {
    let documented: Vec<(String, String, String)> =
        table(&contract_section(), "Substructure codes")
            .into_iter()
            .map(|row| {
                assert_eq!(row.len(), 3, "{row:?}");
                (row[0].clone(), row[1].clone(), row[2].clone())
            })
            .collect();
    let actual: Vec<(String, String, String)> = MATCH_CODES
        .iter()
        .map(|row| {
            let outcome = serde_json::to_value(row.outcome).unwrap();
            (
                row.code.to_owned(),
                outcome.as_str().unwrap().to_owned(),
                row.message.to_owned(),
            )
        })
        .collect();
    assert_eq!(documented, actual);
}

#[test]
fn every_error_the_matcher_returns_has_a_row_and_every_row_has_an_error() {
    // The match has no wildcard arm: a new error variant stops compiling here until it has a
    // code, and the assertion below until that code has a row.
    let a = AtomId::from_serial(1).unwrap();
    let b = BondId::from_serial(1).unwrap();
    let every = [
        MatchError::EmptyQuery,
        MatchError::QueryTooLarge { atoms: 65 },
        MatchError::Unsatisfiable(Item::Atom(a)),
        MatchError::Unsupported {
            side: Side::Query,
            reason: "x",
            at: Some(Item::Bond(b)),
            query: None,
        },
        MatchError::StepLimit,
        MatchError::ScanLimit,
        MatchError::MappingLimit,
    ];
    for error in every {
        match error {
            MatchError::EmptyQuery
            | MatchError::QueryTooLarge { .. }
            | MatchError::Unsatisfiable(_)
            | MatchError::Unsupported { .. }
            | MatchError::StepLimit
            | MatchError::ScanLimit
            | MatchError::MappingLimit => {}
        }
    }
    let produced: std::collections::BTreeSet<&str> = every.iter().map(|e| e.code()).collect();
    let rows: std::collections::BTreeSet<&str> = MATCH_CODES.iter().map(|row| row.code).collect();
    assert_eq!(produced, rows);
    assert_eq!(rows.len(), MATCH_CODES.len(), "codes are unique");
}

#[test]
fn every_documented_reason_is_the_refusal_of_its_fixture() {
    let rows = table(&contract_section(), "Substructure refusal reasons");
    assert!(rows.len() >= 12, "{} rows", rows.len());
    for row in &rows {
        assert_eq!(row.len(), 3, "{row:?}");
        let fixture = fixture(&row[2]);
        let refused = &fixture["refused"];
        assert_eq!(refused["code"], "match_unsupported", "{row:?}");
        assert_eq!(refused["reason"], row[0].as_str(), "{row:?}");
        assert_eq!(refused["side"], row[1].as_str(), "{row:?}");
    }
    // And no refusal in the fixtures is missing from the table.
    let documented: std::collections::BTreeSet<(String, String)> = rows
        .iter()
        .map(|row| (row[0].clone(), row[1].clone()))
        .collect();
    for fixture in fixtures() {
        if let Some(refused) = fixture.get("refused") {
            let key = (
                refused["reason"].as_str().unwrap().to_owned(),
                refused["side"].as_str().unwrap().to_owned(),
            );
            assert!(
                documented.contains(&key),
                "{} is refused for {key:?}, which is not documented",
                fixture["name"]
            );
        }
    }
}

#[test]
fn every_documented_example_has_the_documented_result() {
    let rows = table(&contract_section(), "Substructure examples");
    assert!(rows.len() >= 12, "{} rows", rows.len());
    for row in rows {
        assert_eq!(row.len(), 2, "{row:?}");
        let fixture = fixture(&row[0]);
        let result = if fixture.get("refused").is_some() {
            "refused".to_owned()
        } else {
            match fixture["expect"]["mappings"].as_array().unwrap().len() {
                1 => "1 mapping".to_owned(),
                count => format!("{count} mappings"),
            }
        };
        assert_eq!(row[1], result, "{}", row[0]);
    }
}

#[test]
fn the_numbers_the_contract_quotes_are_the_limits() {
    let section = contract_section();
    let limits = format!(
        "`{{ query_atoms: {MAX_QUERY_ATOMS}, search_steps: {MAX_SEARCH_STEPS}, scanned_candidates: {MAX_SCANNED_CANDIDATES}, mappings: {MAX_MAPPINGS} }}`"
    );
    assert!(section.contains(&limits), "{limits}");
    assert!(section.contains(&format!("at most {MAX_QUERY_ATOMS} atoms")));
    assert!(section.contains(&format!(
        "at most {} assignments and examines at most {} candidate entries",
        grouped(MAX_SEARCH_STEPS),
        grouped(MAX_SCANNED_CANDIDATES)
    )));
    assert!(section.contains("any of the last three is `match_limit`"));
    assert!(section.contains(&format!("at most {} mappings", grouped(MAX_MAPPINGS))));
    assert!(section.contains(&format!(
        "finding the {}st is the limit",
        grouped(MAX_MAPPINGS + 1)
    )));
    assert!(section.contains("(2,000 atoms)"), "the document maximum");
}

#[test]
fn the_operation_table_lists_the_operation() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/CONTRACT.md");
    let text = std::fs::read_to_string(path).unwrap();
    assert!(text.contains("| `chem.substructure` | `{ query_document_id, target_document_id }` |"));
}

fn repository_file(relative: &str) -> String {
    let path = format!("{}/../../{relative}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path}: {error}"))
}

#[test]
fn the_rdkit_comparison_is_described_as_a_manual_check_everywhere() {
    let section = contract_section();
    assert!(section.contains(
        "`scripts/verify-query-rdkit.py`, which is run by hand and is not part of `npm run check`"
    ));
    assert!(section.contains("no equivalence is claimed beyond the fixtures"));
    assert!(
        !section.contains("agrees with an RDKit"),
        "the old claim is gone"
    );
    assert!(repository_file("README.md").contains("compared, by hand, with an RDKit 2022.03.2 Python (no equivalence is claimed beyond those fixtures)"));
    let handoff = repository_file("docs/HANDOFF.ko.md");
    assert!(handoff.contains(
        "수동 실행이며 `npm run check`와 CI 밖이고 fixture 범위를 넘는 동등성은 주장하지 않는다"
    ));
    assert!(repository_file("docs/TRACEABILITY.ko.md").contains(
        "수동 실행이며 `npm run check` 밖이고 fixture 범위를 넘는 동등성을 주장하지 않는다"
    ));
    assert!(
        std::path::Path::new(&format!(
            "{}/../../scripts/verify-query-rdkit.py",
            env!("CARGO_MANIFEST_DIR")
        ))
        .exists()
    );
    // The handoff quotes how many fixtures there are.
    assert!(handoff.contains(&format!("공유 fixture {}개", fixtures().len())));
}

#[test]
fn the_unbonded_example_in_the_contract_is_what_the_matcher_does() {
    use chemifree_domain::document::{
        Atom, Command, Document, DocumentId, NewAtomPositions, Point,
    };
    use chemifree_domain::element::Element;
    use chemifree_domain::graph::MoleculeGraph;
    use chemifree_domain::substructure;
    let graph = |symbols: &[&str]| {
        let mut document = Document::new(DocumentId::parse("claims").unwrap(), "claims").unwrap();
        let commands: Vec<Command> = symbols
            .iter()
            .map(|symbol| Command::AddAtom {
                atom: Atom::from(Element::from_symbol(symbol).unwrap()),
                positions: NewAtomPositions::Single(Point::new(0.0, 0.0).unwrap()),
                bond_to: None,
            })
            .collect();
        for chunk in commands.chunks(500) {
            document.apply(chunk).unwrap();
        }
        MoleculeGraph::from_document(&document)
    };
    let mut target = vec!["C"; 1_995];
    target.extend(["N"; 5]);
    let found = substructure::find(&graph(&["N"; 5]), &graph(&target)).unwrap();
    assert_eq!((found.mappings.len(), found.steps), (120, 325));
    assert!(contract_section().contains("(120 mappings, 325 steps)"));
    assert_eq!(
        substructure::find(&graph(&["C"; 3]), &graph(&vec!["C"; 2_000])),
        Err(MatchError::MappingLimit)
    );
}

#[test]
fn the_contract_states_what_an_empty_answer_and_a_query_bond_in_a_ring_mean() {
    let section = contract_section();
    // An empty answer does not say the target was read: the same unreadable target gets a
    // refusal for one query and a complete empty answer for another.
    assert!(section.contains("It does not say the target could be read in full"));
    let refused = fixture("benzene-kekule-in-tetralin");
    let empty = fixture("bromine-not-in-tetralin");
    assert_eq!(refused["target"], empty["target"], "the same target");
    assert_eq!(refused["refused"]["reason"], "fused_partially_aromatic");
    assert_eq!(empty["expect"]["mappings"].as_array().unwrap().len(), 0);
    // A query bond in a ring system with a pi bond is refused with the model's reason.
    assert!(section.contains("This blocks the most common use of a wildcard ring bond"));
    assert!(section.contains(
        "It is a limit of the aromaticity model, and the matcher has no workaround for it."
    ));
    for name in [
        "query-bond-in-conjugated-ring-of-query",
        "wildcard-bond-in-aromatic-ring-query",
    ] {
        let refused = &fixture(name)["refused"];
        assert_eq!(
            (refused["side"].as_str(), refused["reason"].as_str()),
            (Some("query"), Some("unsupported_ring_bond")),
            "{name}"
        );
    }
}

#[test]
fn the_contract_states_what_the_refusal_precedence_and_the_perception_cost_are() {
    let section = contract_section();
    // A known refusal wins over a limit: the old precedence is gone, the new one is stated,
    // and so is what it costs (which assignments a limit reaches depends on the drawing order).
    assert!(
        !section.contains("A limit takes precedence over a refusal"),
        "the old precedence is gone"
    );
    assert!(section.contains("**A refusal that is known wins over a limit.**"));
    assert!(section.contains("never `match_limit`"));
    assert!(
        section.contains(
            "the assignment that would be the 1,001st is read before the limit is reported"
        )
    );
    assert!(section.contains("only a search that runs to its end names that one"));
    assert!(
        section
            .contains("When the search runs to its end, neither the order the atoms were drawn in")
    );
    // Perceiving the drawings is outside the counted limits, and the caps that bound it are
    // the code's.
    assert!(section.contains("**Perceiving the drawings is not counted in these limits.**"));
    for quoted in [
        format!(
            "at most {} atoms and {} bonds",
            grouped(MAX_ATOMS),
            grouped(MAX_BONDS)
        ),
        format!("more than {MAX_SYSTEM_ATOMS} atoms is not read (`system_limit`"),
        format!(
            "fixed budget of {} steps for each such system (`search_limit`",
            grouped(MAX_MATCH_STEPS)
        ),
    ] {
        assert!(section.contains(&quoted), "{quoted}");
    }
    // A lookup failure names no revision; a query atom with a charge or isotope is refused.
    assert!(section.contains("has `document_revision: null`"));
    for reason in ["query_atom_charge", "query_atom_isotope"] {
        assert!(section.contains(&format!("`{reason}`")), "{reason}");
    }
}

#[test]
fn the_rdkit_comparison_says_how_and_when_to_run_it() {
    let section = contract_section();
    assert!(section.contains(
        "`python -I scripts/verify-query-rdkit.py target/debug/chemifree`) whenever a fixture, the matcher's rules or the aromaticity model change"
    ));
    assert!(
        repository_file("README.md")
            .contains("whenever a fixture, the matcher's rules or the aromaticity model change")
    );
}
