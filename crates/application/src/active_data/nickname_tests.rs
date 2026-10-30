use super::prepared_tests::{Fixture, assert_binding_unavailable, fixture};
use super::{ActiveDataError, ProfileRecords, hash};
use serde_json::Value;

const GRAPHS: &str = include_str!("../../tests/data/nickname-synthetic/graphs.jsonl");
const ALPHABET: &[u8] = include_bytes!("../../tests/data/nickname-synthetic/alphabet.jsonl");
const RAW_Q: &[u8] = br#"<CDXML><fragment id="10"><n id="1" NodeType="Element">first</n>tail<n id="2"/><b id="3" B="1" E="2" Order="1"/></fragment></CDXML>"#;
const RAW_WITH_ENTITY: &[u8] = br#"<!DOCTYPE CDXML [<!ENTITY word "first">]><CDXML><fragment id="10"><n id="1" NodeType="Element">&word;</n>tail<n id="2"/><b id="3" B="1" E="2" Order="1"/></fragment></CDXML>"#;
const GRAPH_PATH: &str = "normalized/nickname-graphs.jsonl";
const ALPHABET_PATH: &str = "normalized/sequence-alphabets.jsonl";
const RAW_PATH: &str = "raw/nicknames/Q.cdxml";

fn graphs_with_raw_q(raw: &[u8]) -> Vec<u8> {
    let mut rows: Vec<Value> = GRAPHS
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    rows[0]["source_sha256"] = hash(raw).into();
    rows[0]["source_bytes"] = raw.len().into();
    rows.into_iter()
        .map(|row| format!("{row}\n"))
        .collect::<String>()
        .into_bytes()
}

fn nickname_fixture(graphs: Vec<u8>) -> Fixture {
    fixture(
        "private-sequence-nickname-graphs-v1",
        vec![
            ("graphs", GRAPH_PATH, graphs),
            ("alphabet", ALPHABET_PATH, ALPHABET.to_vec()),
        ],
    )
}

#[test]
fn fixture_only_review_cannot_authorize_nickname_data() {
    let fixture = nickname_fixture(GRAPHS.as_bytes().to_vec());
    let error = match fixture.load_production() {
        Err(error) => error,
        Ok(_) => panic!("fixture-only authorization unexpectedly succeeded"),
    };
    assert_eq!(error, ActiveDataError::RightsUnverified);
    assert_eq!(fixture.reads(), 0);
}

#[test]
fn complete_nickname_data_is_atomic_and_cannot_enable_a_model() {
    let mut fixture = nickname_fixture(GRAPHS.as_bytes().to_vec());
    let dataset = fixture.load().unwrap();
    let ProfileRecords::NicknameData(records) = dataset.records() else {
        panic!("wrong dataset type")
    };
    assert_eq!(records.graphs.len(), 2);
    assert_eq!(records.alphabets[0].nickname_names, ["Q", "R", "Q"]);
    assert_binding_unavailable(&fixture, &dataset);
    fixture.corrupt(ALPHABET_PATH);
    assert!(matches!(
        fixture.load(),
        Err(ActiveDataError::DataIdentityMismatch)
    ));
    let mut missing = nickname_fixture(GRAPHS.as_bytes().to_vec());
    missing.remove(GRAPH_PATH);
    assert!(matches!(
        missing.load(),
        Err(ActiveDataError::DataUnavailable)
    ));
}

#[test]
fn optional_raw_nickname_sources_may_be_a_verified_subset() {
    let mut fixture = nickname_fixture(graphs_with_raw_q(RAW_Q));
    fixture.add_missing_optional("raw-q", RAW_PATH, RAW_Q);
    assert!(fixture.load().is_ok());
    fixture.insert(RAW_PATH, RAW_Q);
    assert!(fixture.load().is_ok());
    fixture.corrupt(RAW_PATH);
    assert!(matches!(
        fixture.load(),
        Err(ActiveDataError::DataIdentityMismatch)
    ));
}

#[test]
fn unsupported_entity_declarations_are_reported_without_enabling_a_dataset() {
    let mut fixture = nickname_fixture(graphs_with_raw_q(RAW_WITH_ENTITY));
    fixture.add_missing_optional("raw-q", RAW_PATH, RAW_WITH_ENTITY);
    fixture.insert(RAW_PATH, RAW_WITH_ENTITY);
    assert!(matches!(
        fixture.load(),
        Err(ActiveDataError::ProfileUnsupported)
    ));
}

#[test]
fn missing_required_declaration_and_bad_graph_references_are_refused() {
    let mut fixture = nickname_fixture(GRAPHS.as_bytes().to_vec());
    fixture.mark_optional(ALPHABET_PATH);
    assert!(matches!(
        fixture.load(),
        Err(ActiveDataError::ProfileUnsupported)
    ));
    let bad = GRAPHS.replacen("\"E\":\"2\"", "\"E\":\"99\"", 1);
    let fixture = nickname_fixture(bad.into_bytes());
    assert!(matches!(
        fixture.load(),
        Err(ActiveDataError::ContentInvalid)
    ));
}
