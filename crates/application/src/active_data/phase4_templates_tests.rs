use super::prepared_tests::{Fixture, assert_binding_unavailable, fixture};
use super::{ActiveDataError, ProfileRecords};

const RAW: [&str; 5] = [
    "P4-BIOPOLYMER-TEMPLATE-CANDIDATES-001",
    "P4-BIOPOLYMER-TEMPLATE-CANDIDATES-003",
    "P4-BIOPOLYMER-TEMPLATE-CANDIDATES-005",
    "P4-BIOPOLYMER-TEMPLATE-CANDIDATES-007",
    "P4-BIOPOLYMER-TEMPLATE-CANDIDATES-009",
];
const NORMALIZED: [(&str, usize); 5] = [
    ("DERIVED-CTP-FRAMING-1", 3556),
    ("DERIVED-CTP-FRAMING-2", 2008),
    ("DERIVED-CTP-FRAMING-3", 5060),
    ("DERIVED-CTP-FRAMING-4", 4942),
    ("DERIVED-CTP-FRAMING-5", 4901),
];

fn templates_fixture(invalid_record: bool) -> Fixture {
    let mut entries = Vec::new();
    for (index, id) in RAW.into_iter().enumerate() {
        entries.push((id, id, format!("synthetic raw {index}").into_bytes()));
    }
    for (index, (id, count)) in NORMALIZED.into_iter().enumerate() {
        let row = if invalid_record && index == 0 {
            "{\"kind\":\"synthetic\",\"unexpected\":true}\n"
        } else {
            "{\"kind\":\"synthetic\"}\n"
        };
        let rows = if !invalid_record && index == 0 {
            let first = format!(
                r#"{{"kind":"synthetic","data":{{"index":{},"parent_index":null,"tag":"x","object_id":0,"header_offset":0,"header_bytes":1,"depth":0,"implicit_document_root":true,"property_indices":[],"child_indices":[],"end_marker_offset":1,"range_bytes":1}}}}"#,
                "9".repeat(400)
            );
            format!("{first}\n{}", row.repeat(count - 1))
        } else {
            row.repeat(count)
        };
        entries.push((id, id, rows.into_bytes()));
    }
    fixture("private-data-biopolymer-template-candidates-v1", entries)
}

#[test]
fn fixture_rights_are_checked_before_template_payload_is_read() {
    let test_fixture = templates_fixture(false);
    let error = match test_fixture.load_production() {
        Err(error) => error,
        Ok(_) => panic!("fixture authorization unexpectedly succeeded"),
    };
    assert_eq!(error, ActiveDataError::RightsUnverified);
    assert_eq!(test_fixture.reads(), 0);
}

#[test]
fn complete_framing_data_is_typed_without_a_sequence_engine() {
    let test_fixture = templates_fixture(false);
    let loaded = test_fixture.load().unwrap();
    let ProfileRecords::Phase4Templates(data) = loaded.records() else {
        panic!("wrong dataset type")
    };
    assert_eq!(data.tables.len(), 5);
    assert_eq!(
        data.tables
            .iter()
            .map(|table| table.rows.len())
            .sum::<usize>(),
        20467
    );
    assert_eq!(data.tables[0].rows[0].kind, "synthetic");
    assert_eq!(
        data.tables[0].rows[0]
            .data
            .as_ref()
            .unwrap()
            .index
            .to_string(),
        "9".repeat(400)
    );
    assert_eq!(data.ancillary.len(), 5);
    assert_binding_unavailable(&test_fixture, &loaded);
}

#[test]
fn absent_optional_raw_file_does_not_shift_other_file_identities() {
    let mut test_fixture = templates_fixture(false);
    test_fixture.mark_optional(RAW[0]);
    test_fixture.remove(RAW[0]);
    let loaded = test_fixture.load().unwrap();
    let ProfileRecords::Phase4Templates(data) = loaded.records() else {
        panic!("wrong dataset type")
    };
    assert_eq!(data.ancillary.len(), 4);
    assert_eq!(data.ancillary[0].file_id, RAW[1]);
    assert_eq!(data.ancillary[0].bytes, b"synthetic raw 1");
}

#[test]
fn missing_corrupt_and_semantically_invalid_files_are_not_published() {
    let mut missing = templates_fixture(false);
    missing.remove(NORMALIZED[0].0);
    assert!(matches!(
        missing.load(),
        Err(ActiveDataError::DataUnavailable)
    ));
    let mut corrupt = templates_fixture(false);
    corrupt.corrupt(NORMALIZED[0].0);
    assert!(matches!(
        corrupt.load(),
        Err(ActiveDataError::DataIdentityMismatch)
    ));
    let invalid = templates_fixture(true);
    assert!(matches!(
        invalid.load(),
        Err(ActiveDataError::ContentInvalid)
    ));
}

#[test]
fn normalized_files_are_required_and_catalog_roles_are_checked() {
    let mut optional = templates_fixture(false);
    optional.mark_optional(NORMALIZED[0].0);
    assert!(matches!(
        optional.load(),
        Err(ActiveDataError::ProfileUnsupported)
    ));
    let mut wrong_role = templates_fixture(false);
    wrong_role.set_role(NORMALIZED[0].0, "auxiliary_data");
    assert!(matches!(
        wrong_role.load(),
        Err(ActiveDataError::ProfileUnsupported)
    ));
    let mut missing_declaration = templates_fixture(false);
    missing_declaration.remove_declared(NORMALIZED[0].0);
    assert!(matches!(
        missing_declaration.load(),
        Err(ActiveDataError::ProfileUnsupported)
    ));
    let mut unexpected_feature = templates_fixture(false);
    unexpected_feature.set_feature_hash(Some(&"0".repeat(64)));
    assert!(matches!(
        unexpected_feature.load(),
        Err(ActiveDataError::ProfileUnsupported)
    ));
}
