use super::prepared_tests::{Fixture, assert_binding_unavailable, fixture};
use super::{ActiveDataError, ProfileRecords};

const LEXICON: &[u8] = include_bytes!("../../tests/data/naming-lexicon-synthetic/lexicon.jsonl");
const PRIMARY: &[u8] = include_bytes!("../../tests/data/naming-lexicon-synthetic/primary.jsonl");
const SECONDARY: &[u8] =
    include_bytes!("../../tests/data/naming-lexicon-synthetic/secondary.jsonl");

fn independent_lexicon() -> Fixture {
    fixture(
        "private-naming-lexicon-typed-v1",
        vec![
            (
                "lexicon",
                "normalized/lexicon-entries.jsonl",
                LEXICON.to_vec(),
            ),
            (
                "primary",
                "normalized/text-map-primary.jsonl",
                PRIMARY.to_vec(),
            ),
            (
                "secondary",
                "normalized/text-map-secondary.jsonl",
                SECONDARY.to_vec(),
            ),
        ],
    )
}

#[test]
fn a_fixture_review_cannot_authorize_lexicon_data_in_production() {
    let fixture = independent_lexicon();
    let error = match fixture.load_production() {
        Err(error) => error,
        Ok(_) => panic!("fixture-only authorization unexpectedly succeeded"),
    };
    assert_eq!(error, ActiveDataError::RightsUnverified);
    assert_eq!(fixture.reads(), 0);
}

#[test]
fn normalized_files_must_be_marked_required_in_the_manifest() {
    let mut fixture = independent_lexicon();
    fixture.mark_optional("normalized/text-map-primary.jsonl");
    assert!(matches!(
        fixture.load(),
        Err(ActiveDataError::ProfileUnsupported)
    ));
    assert_eq!(fixture.reads(), 0);
}

#[test]
fn complete_lexicon_loads_atomically_without_enabling_a_model() {
    let mut fixture = independent_lexicon();
    let dataset = fixture.load().unwrap();
    let ProfileRecords::NamingLexicon(records) = dataset.records() else {
        panic!("wrong dataset type");
    };
    assert_eq!(records.entries.len(), 2);
    assert_eq!(records.entries[0].token.bytes, [0xe9]);
    assert_eq!(records.primary.len(), 2);
    assert_eq!(fixture.reads(), 3);
    assert_binding_unavailable(&fixture, &dataset);

    fixture.corrupt("normalized/text-map-primary.jsonl");
    assert!(matches!(
        fixture.load(),
        Err(ActiveDataError::DataIdentityMismatch)
    ));
    let mut missing = independent_lexicon();
    missing.remove("normalized/lexicon-entries.jsonl");
    assert!(matches!(
        missing.load(),
        Err(ActiveDataError::DataUnavailable)
    ));
}

#[test]
fn large_lexicon_provenance_survives_the_complete_registration() {
    let large = "9".repeat(400);
    let entries = String::from_utf8(LEXICON.to_vec()).unwrap().replacen(
        "\"source_file_offset\":0",
        &format!("\"source_file_offset\":{large}"),
        1,
    );
    let fixture = fixture(
        "private-naming-lexicon-typed-v1",
        vec![
            (
                "lexicon",
                "normalized/lexicon-entries.jsonl",
                entries.into_bytes(),
            ),
            (
                "primary",
                "normalized/text-map-primary.jsonl",
                PRIMARY.to_vec(),
            ),
            (
                "secondary",
                "normalized/text-map-secondary.jsonl",
                SECONDARY.to_vec(),
            ),
        ],
    );
    let dataset = fixture.load().unwrap();
    let ProfileRecords::NamingLexicon(records) = dataset.records() else {
        panic!("wrong record type")
    };
    assert_eq!(records.entries[0].source.file_offset.to_string(), large);
    assert_binding_unavailable(&fixture, &dataset);
}

#[test]
fn optional_lexicon_source_may_be_absent_but_is_checked_when_present() {
    let mut fixture = independent_lexicon();
    let raw_path = "raw/NameToStructure.dat";
    let raw = b"0123456789";
    fixture.add_missing_optional("raw", raw_path, raw);
    assert!(fixture.load().is_ok());
    fixture.insert(raw_path, raw);
    assert!(fixture.load().is_ok());
    fixture.corrupt(raw_path);
    assert!(matches!(
        fixture.load(),
        Err(ActiveDataError::DataIdentityMismatch)
    ));
}

#[test]
fn authorized_but_inconsistent_graph_records_are_rejected() {
    let invalid = String::from_utf8(LEXICON.to_vec()).unwrap().replacen(
        "\"end_atom_0based\":1",
        "\"end_atom_0based\":2",
        1,
    );
    let fixture = fixture(
        "private-naming-lexicon-typed-v1",
        vec![
            (
                "lexicon",
                "normalized/lexicon-entries.jsonl",
                invalid.into_bytes(),
            ),
            (
                "primary",
                "normalized/text-map-primary.jsonl",
                PRIMARY.to_vec(),
            ),
            (
                "secondary",
                "normalized/text-map-secondary.jsonl",
                SECONDARY.to_vec(),
            ),
        ],
    );
    assert!(matches!(
        fixture.load(),
        Err(ActiveDataError::ContentInvalid)
    ));
}
