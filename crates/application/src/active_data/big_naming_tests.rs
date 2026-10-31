use super::prepared_tests::{assert_binding_unavailable, fixture};
use super::{ActiveDataError, ProfileRecords};
use crate::naming_patterns::PatternRows;

const QUERY_PATH: &str = "normalized/query-graph-patterns.jsonl";
const KIND_PATH: &str = "normalized/kind-labels.jsonl";
const SUBTYPE_PATH: &str = "normalized/subtype-labels.jsonl";
const QUERY: &str = include_str!("../../tests/data/naming-patterns-synthetic/query.jsonl");
const KIND: &str = include_str!("../../tests/data/naming-labels-synthetic/kind.jsonl");
const SUBTYPE: &str = include_str!("../../tests/data/naming-labels-synthetic/subtype.jsonl");

#[test]
fn large_pattern_provenance_is_published_only_after_complete_validation() {
    let large = "9".repeat(400);
    let query = QUERY.replacen(
        "\"source_file_offset\":4",
        &format!("\"source_file_offset\":{large}"),
        1,
    );
    let mut source = fixture(
        "private-naming-query-patterns-v1",
        vec![("query", QUERY_PATH, query.into_bytes())],
    );
    assert!(matches!(
        source.load_production(),
        Err(ActiveDataError::RightsUnverified)
    ));
    assert_eq!(source.reads(), 0);
    let dataset = source.load().unwrap();
    let ProfileRecords::NamingPatterns(patterns) = dataset.records() else {
        panic!("wrong record type")
    };
    let PatternRows::Query(rows) = patterns.as_ref() else {
        panic!("wrong pattern type")
    };
    assert_eq!(rows[0].source.offset.to_string(), large);
    assert_binding_unavailable(&source, &dataset);
    source.corrupt(QUERY_PATH);
    assert!(matches!(
        source.load(),
        Err(ActiveDataError::DataIdentityMismatch)
    ));
    source.remove(QUERY_PATH);
    assert!(matches!(
        source.load(),
        Err(ActiveDataError::DataUnavailable)
    ));
}

#[test]
fn large_label_ordinal_survives_an_authorized_immutable_generation() {
    let large = "9".repeat(400);
    let subtype = SUBTYPE.replace(
        "\"source_ordinal_0based\":0",
        &format!("\"source_ordinal_0based\":{large}"),
    );
    let mut source = fixture(
        "private-naming-enum-labels-v1",
        vec![
            ("kind", KIND_PATH, KIND.as_bytes().to_vec()),
            ("subtype", SUBTYPE_PATH, subtype.into_bytes()),
        ],
    );
    let dataset = source.load().unwrap();
    let ProfileRecords::NamingLabels(labels) = dataset.records() else {
        panic!("wrong record type")
    };
    assert_eq!(labels.subtype[0].source_ordinal.to_string(), large);
    assert_eq!(dataset.feature_schema_sha256(), None);
    source.corrupt(SUBTYPE_PATH);
    assert!(matches!(
        source.load(),
        Err(ActiveDataError::DataIdentityMismatch)
    ));
}
