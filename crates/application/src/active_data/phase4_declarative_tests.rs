use super::prepared_tests::{Fixture, assert_binding_unavailable, fixture};
use super::{ActiveDataError, ProfileRecords};

const RAW: [&str; 2] = ["P4-NAMING-DECLARATIVE-001", "P4-NAMING-DECLARATIVE-002"];
const NORMALIZED: [&str; 3] = [
    "DERIVED-NAMING-DECLARATIVE-1",
    "DERIVED-NAMING-DECLARATIVE-2",
    "DERIVED-FDA-VALIDATED-159",
];

fn declarative_fixture(invalid_record: bool) -> Fixture {
    let text = if invalid_record {
        "{\"line\":1,\"text\":\"synthetic\",\"unexpected\":true}\n"
    } else {
        "{\"line\":1,\"text\":\"synthetic\"}\n"
    };
    let source = "{\"ordinal\":0,\"attributes\":{\"name\":\"example\",\"smiles\":\"C\"}}\n";
    let report = "{\"source_ordinal\":0,\"source_attributes\":{\"name\":\"example\",\"smiles\":\"C\"},\"independent_parse_ok\":false,\"canonical_isomeric_smiles\":\"\",\"formal_charge\":-1,\"formula\":\"\",\"atoms\":0,\"bonds\":0}\n";
    let mut test_fixture = fixture(
        "private-data-naming-declarative-v1",
        vec![
            (RAW[0], RAW[0], b"synthetic raw one".to_vec()),
            (RAW[1], RAW[1], b"synthetic raw two".to_vec()),
            (NORMALIZED[0], NORMALIZED[0], text.repeat(127).into_bytes()),
            (
                NORMALIZED[1],
                NORMALIZED[1],
                source.repeat(159).into_bytes(),
            ),
            (
                NORMALIZED[2],
                NORMALIZED[2],
                report
                    .repeat(159)
                    .replacen(
                        "\"formal_charge\":-1",
                        &format!("\"formal_charge\":-{}", "9".repeat(400)),
                        1,
                    )
                    .into_bytes(),
            ),
        ],
    );
    for id in NORMALIZED {
        test_fixture.set_role(id, "dictionary_entries");
    }
    test_fixture
}

#[test]
fn fixture_rights_are_checked_before_declarative_payload_is_read() {
    let test_fixture = declarative_fixture(false);
    let error = match test_fixture.load_production() {
        Err(error) => error,
        Ok(_) => panic!("fixture authorization unexpectedly succeeded"),
    };
    assert_eq!(error, ActiveDataError::RightsUnverified);
    assert_eq!(test_fixture.reads(), 0);
}

#[test]
fn complete_inputs_are_typed_without_enabling_a_naming_engine() {
    let test_fixture = declarative_fixture(false);
    let loaded = test_fixture.load().unwrap();
    let ProfileRecords::Phase4Declarative(data) = loaded.records() else {
        panic!("wrong dataset type")
    };
    assert_eq!(data.text.len(), 127);
    assert_eq!(data.source.len(), 159);
    assert_eq!(data.reports.len(), 159);
    assert!(!data.reports[0].independent_parse_ok);
    assert_eq!(
        data.reports[0].formal_charge.to_string(),
        format!("-{}", "9".repeat(400))
    );
    assert_eq!(data.ancillary.len(), 2);
    assert_binding_unavailable(&test_fixture, &loaded);
}

#[test]
fn absent_optional_raw_file_does_not_shift_the_remaining_file_identity() {
    let mut test_fixture = declarative_fixture(false);
    test_fixture.mark_optional(RAW[0]);
    test_fixture.remove(RAW[0]);
    let loaded = test_fixture.load().unwrap();
    let ProfileRecords::Phase4Declarative(data) = loaded.records() else {
        panic!("wrong dataset type")
    };
    assert_eq!(data.ancillary.len(), 1);
    assert_eq!(data.ancillary[0].file_id, RAW[1]);
    assert_eq!(data.ancillary[0].bytes, b"synthetic raw two");
}

#[test]
fn missing_corrupt_and_semantically_invalid_files_are_not_published() {
    let mut missing = declarative_fixture(false);
    missing.remove(NORMALIZED[0]);
    assert!(matches!(
        missing.load(),
        Err(ActiveDataError::DataUnavailable)
    ));
    let mut corrupt = declarative_fixture(false);
    corrupt.corrupt(NORMALIZED[0]);
    assert!(matches!(
        corrupt.load(),
        Err(ActiveDataError::DataIdentityMismatch)
    ));
    let invalid = declarative_fixture(true);
    assert!(matches!(
        invalid.load(),
        Err(ActiveDataError::ContentInvalid)
    ));
}

#[test]
fn normalized_files_are_required_and_catalog_roles_are_checked() {
    let mut optional = declarative_fixture(false);
    optional.mark_optional(NORMALIZED[0]);
    assert!(matches!(
        optional.load(),
        Err(ActiveDataError::ProfileUnsupported)
    ));
    let mut wrong_role = declarative_fixture(false);
    wrong_role.set_role(NORMALIZED[0], "auxiliary_data");
    assert!(matches!(
        wrong_role.load(),
        Err(ActiveDataError::ProfileUnsupported)
    ));
    let mut missing_declaration = declarative_fixture(false);
    missing_declaration.remove_declared(NORMALIZED[0]);
    assert!(matches!(
        missing_declaration.load(),
        Err(ActiveDataError::ProfileUnsupported)
    ));
    let mut unexpected_feature = declarative_fixture(false);
    unexpected_feature.set_feature_hash(Some(&"0".repeat(64)));
    assert!(matches!(
        unexpected_feature.load(),
        Err(ActiveDataError::ProfileUnsupported)
    ));
}
