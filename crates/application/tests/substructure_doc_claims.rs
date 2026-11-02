//! What docs/CONTRACT.md says about substructure matching, checked against the matcher: its
//! code table row by row, the reason and example tables against the shared fixtures, and the
//! numbers it quotes.

use chemifree_application::MATCH_CODES;
use chemifree_domain::document::{AtomId, BondId};
use chemifree_domain::substructure::{
    Item, MAX_MAPPINGS, MAX_QUERY_ATOMS, MAX_SEARCH_STEPS, MatchError, Side,
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
        if index > 0 && (digits.len() - index) % 3 == 0 {
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
        MatchError::MappingLimit,
    ];
    for error in every {
        match error {
            MatchError::EmptyQuery
            | MatchError::QueryTooLarge { .. }
            | MatchError::Unsatisfiable(_)
            | MatchError::Unsupported { .. }
            | MatchError::StepLimit
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
        "`{{ query_atoms: {MAX_QUERY_ATOMS}, search_steps: {MAX_SEARCH_STEPS}, mappings: {MAX_MAPPINGS} }}`"
    );
    assert!(section.contains(&limits), "{limits}");
    assert!(section.contains(&format!("at most {MAX_QUERY_ATOMS} atoms")));
    assert!(section.contains(&format!(
        "at most {} candidate assignments",
        grouped(MAX_SEARCH_STEPS)
    )));
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
