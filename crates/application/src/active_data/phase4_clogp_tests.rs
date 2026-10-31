use super::prepared_tests::{Fixture, assert_binding_unavailable, fixture};
use super::{ActiveDataError, ProfileRecords, phase4_clogp};

const RAW_DECLARATIVE: [&str; 3] = [
    "P4-CLOGP-CARRIERS-001",
    "P4-CLOGP-CARRIERS-014",
    "P4-CLOGP-CARRIERS-016",
];
const RAW_MODEL: [&str; 6] = [
    "P4-CLOGP-CARRIERS-002",
    "P4-CLOGP-CARRIERS-003",
    "P4-CLOGP-CARRIERS-004",
    "P4-CLOGP-CARRIERS-005",
    "P4-CLOGP-CARRIERS-009",
    "P4-CLOGP-CARRIERS-010",
];
const NORMALIZED: [(&str, usize); 10] = [
    ("DERIVED-CLOGP-CARRIERS-1", 11),
    ("DERIVED-CLOGP-CARRIERS-2", 32),
    ("DERIVED-CLOGP-CARRIERS-3", 26),
    ("DERIVED-CLOGP-CARRIERS-4", 34),
    ("DERIVED-CLOGP-CARRIERS-5", 14),
    ("DERIVED-CLOGP-CARRIERS-6", 141),
    ("DERIVED-CLOGP-CARRIERS-7", 10026),
    ("DERIVED-CLOGP-CARRIERS-8", 12),
    ("DERIVED-CLOGP-CARRIERS-9", 5),
    ("DERIVED-BIOBYTE-ACTIVE-1686", 1686),
];
const TEXT: &str = r#"{"line":1,"text":"synthetic"}"#;
const TAGGED: &str = r#"{"line":1,"tag":"synthetic","fields":["a","b"]}"#;
const PARAMETER: &str = r#"{"ordinal":0,"data_role":"synthetic","count":0,"element_bytes":0,"raw_payload_hex":"00","storage_kind":"synthetic","values":[42,{"ieee754_f32_le_hex":"0100803f","value":1.000000059604644775390626,"finite":true}]}"#;
const ACTIVE: &str = r#"{"fragment_storage_index_0based":0,"key_latin1":"é","key_hex":"00","value_slot_1based":1,"environment_raw_hex":"aa","environment_trimmed_latin1":"é","FVALUE_f32_le_hex":"00000080","FVALUE":-0.0,"quality_integer":0,"zwitter_category":0}"#;

fn clogp_fixture(invalid: bool) -> Fixture {
    let mut entries = Vec::new();
    for (index, id) in RAW_DECLARATIVE.into_iter().chain(RAW_MODEL).enumerate() {
        entries.push((id, id, format!("synthetic raw {index}").into_bytes()));
    }
    for (position, (id, count)) in NORMALIZED.into_iter().enumerate() {
        let row = match position {
            0 | 1 | 7 | 8 => TEXT.to_owned(),
            2 if invalid => PARAMETER.replace("0100803f", "0000803f"),
            2 => PARAMETER.to_owned(),
            3..=6 => TAGGED.to_owned(),
            9 => ACTIVE.to_owned(),
            _ => unreachable!(),
        };
        entries.push((id, id, format!("{row}\n").repeat(count).into_bytes()));
    }
    let mut test_fixture = fixture("private-data-clogp-carriers-v1", entries);
    for id in RAW_MODEL {
        test_fixture.set_class(id, "model_parameters");
    }
    for (id, _) in NORMALIZED {
        test_fixture.set_class(id, "model_parameters");
        test_fixture.set_role(id, "fragment_reference");
    }
    test_fixture
}

#[test]
fn fixture_rights_are_checked_before_fragment_payload_is_read() {
    let test_fixture = clogp_fixture(false);
    let error = match test_fixture.load_production() {
        Err(error) => error,
        Ok(_) => panic!("fixture authorization unexpectedly succeeded"),
    };
    assert_eq!(error, ActiveDataError::RightsUnverified);
    assert_eq!(test_fixture.reads(), 0);
}
#[test]
fn complete_carriers_are_typed_without_a_prediction_engine() {
    let test_fixture = clogp_fixture(false);
    let loaded = test_fixture.load().unwrap();
    let ProfileRecords::Phase4Clogp(data) = loaded.records() else {
        panic!("wrong dataset type")
    };
    assert_eq!(data.tables.len(), 10);
    let phase4_clogp::CarrierRows::Active(rows) = &data.tables[9].rows else {
        panic!("wrong table")
    };
    assert_eq!(rows.len(), 1686);
    assert_eq!(rows[0].fvalue.bits(), 0x8000_0000);
    assert_binding_unavailable(&test_fixture, &loaded);
}
#[test]
fn absent_optional_raw_does_not_shift_other_file_identities() {
    let mut test_fixture = clogp_fixture(false);
    test_fixture.mark_optional(RAW_DECLARATIVE[0]);
    test_fixture.remove(RAW_DECLARATIVE[0]);
    let loaded = test_fixture.load().unwrap();
    let ProfileRecords::Phase4Clogp(data) = loaded.records() else {
        panic!("wrong dataset type")
    };
    assert_eq!(data.ancillary.len(), 8);
    assert_eq!(data.ancillary[0].file_id, RAW_DECLARATIVE[1]);
    assert_eq!(data.ancillary[0].bytes, b"synthetic raw 1");
}
#[test]
fn missing_corrupt_or_invalid_inputs_do_not_publish_data() {
    let mut missing = clogp_fixture(false);
    missing.remove(NORMALIZED[0].0);
    assert!(matches!(
        missing.load(),
        Err(ActiveDataError::DataUnavailable)
    ));
    let mut corrupt = clogp_fixture(false);
    corrupt.corrupt(NORMALIZED[0].0);
    assert!(matches!(
        corrupt.load(),
        Err(ActiveDataError::DataIdentityMismatch)
    ));
    let invalid = clogp_fixture(true);
    assert!(matches!(
        invalid.load(),
        Err(ActiveDataError::ContentInvalid)
    ));
}
#[test]
fn normalized_files_are_required_and_catalog_roles_and_classes_are_checked() {
    let mut optional = clogp_fixture(false);
    optional.mark_optional(NORMALIZED[0].0);
    assert!(matches!(
        optional.load(),
        Err(ActiveDataError::ProfileUnsupported)
    ));
    let mut wrong_role = clogp_fixture(false);
    wrong_role.set_role(NORMALIZED[0].0, "auxiliary_data");
    assert!(matches!(
        wrong_role.load(),
        Err(ActiveDataError::ProfileUnsupported)
    ));
    let mut wrong_raw_role = clogp_fixture(false);
    wrong_raw_role.set_role(RAW_MODEL[0], "weights");
    assert!(matches!(
        wrong_raw_role.load(),
        Err(ActiveDataError::ProfileUnsupported)
    ));
    let mut wrong_class = clogp_fixture(false);
    wrong_class.set_class(RAW_MODEL[0], "declarative_data");
    assert!(matches!(
        wrong_class.load(),
        Err(ActiveDataError::ProfileUnsupported)
    ));
    let mut wrong_raw_class = clogp_fixture(false);
    wrong_raw_class.set_class(RAW_DECLARATIVE[0], "model_parameters");
    assert!(matches!(
        wrong_raw_class.load(),
        Err(ActiveDataError::ProfileUnsupported)
    ));
    let mut wrong_normalized_class = clogp_fixture(false);
    wrong_normalized_class.set_class(NORMALIZED[0].0, "declarative_data");
    assert!(matches!(
        wrong_normalized_class.load(),
        Err(ActiveDataError::ProfileUnsupported)
    ));
    let mut missing_declaration = clogp_fixture(false);
    missing_declaration.remove_declared(NORMALIZED[0].0);
    assert!(matches!(
        missing_declaration.load(),
        Err(ActiveDataError::ProfileUnsupported)
    ));
    let mut unexpected_feature = clogp_fixture(false);
    unexpected_feature.set_feature_hash(Some(&"0".repeat(64)));
    assert!(matches!(
        unexpected_feature.load(),
        Err(ActiveDataError::ProfileUnsupported)
    ));
}
