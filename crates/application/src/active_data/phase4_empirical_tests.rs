use super::prepared_tests::{Fixture, assert_binding_unavailable, fixture};
use super::{ActiveDataError, ProfileRecords};

const RAW: [&str; 12] = [
    "P4-CHEMPROP-EMPIRICAL-001",
    "P4-CHEMPROP-EMPIRICAL-002",
    "P4-CHEMPROP-EMPIRICAL-003",
    "P4-CHEMPROP-EMPIRICAL-004",
    "P4-CHEMPROP-EMPIRICAL-005",
    "P4-CHEMPROP-EMPIRICAL-006",
    "P4-CHEMPROP-EMPIRICAL-007",
    "P4-CHEMPROP-EMPIRICAL-008",
    "P4-CHEMPROP-EMPIRICAL-009",
    "P4-CHEMPROP-EMPIRICAL-010",
    "P4-CHEMPROP-EMPIRICAL-011",
    "P4-CHEMPROP-EMPIRICAL-012",
];
const NORMALIZED: [(&str, usize); 5] = [
    ("DERIVED-CHEMPROP-EMPIRICAL-1", 2754),
    ("DERIVED-CHEMPROP-EMPIRICAL-2", 422),
    ("DERIVED-CHEMPROP-EMPIRICAL-3", 23),
    ("DERIVED-CHEMPROP-EMPIRICAL-4", 32077),
    ("DERIVED-CHEMPROP-EMPIRICAL-5", 8926),
];
const MEASUREMENT: &str = r#"{"molecule_id":7,"DAT_field_id":2,"source_offset":0,"flag_byte":255,"raw_record_hex":"A0ff","primary":{"ieee754_f32_le_hex":"0100803f","value":1.000000059604644775390626,"finite":true}}"#;
const GRAPH: &str = r#"{"molecule_id":7,"atomic_numbers":[6,8],"header_H_count":0,"bonds":[{"a_1based":1,"b_1based":2,"source_bond_code":1}],"unresolved_indexed_annotations":{"unknown_decimal":0.1000000000000000000000001},"header_raw_hex":"AA00","COS_row_raw_hex":"bb"}"#;

fn empirical_fixture(invalid: bool) -> Fixture {
    let mut entries = Vec::new();
    for (index, id) in RAW.into_iter().enumerate() {
        entries.push((id, id, format!("synthetic raw {index}").into_bytes()));
    }
    for (id, count) in NORMALIZED {
        let row = if id == NORMALIZED[3].0 && invalid {
            MEASUREMENT.replace("0100803f", "0000803f")
        } else if id == NORMALIZED[3].0 {
            MEASUREMENT.to_owned()
        } else if id == NORMALIZED[4].0 {
            GRAPH.to_owned()
        } else {
            r#"{"ordinal":0,"text":"synthetic"}"#.to_owned()
        };
        entries.push((id, id, format!("{row}\n").repeat(count).into_bytes()));
    }
    fixture("private-data-chemprop-empirical-v1", entries)
}

#[test]
fn fixture_rights_are_checked_before_empirical_payload_is_read() {
    let test_fixture = empirical_fixture(false);
    let error = match test_fixture.load_production() {
        Err(error) => error,
        Ok(_) => panic!("fixture authorization unexpectedly succeeded"),
    };
    assert_eq!(error, ActiveDataError::RightsUnverified);
    assert_eq!(test_fixture.reads(), 0);
}

#[test]
fn complete_empirical_rows_are_typed_without_a_property_engine() {
    let test_fixture = empirical_fixture(false);
    let loaded = test_fixture.load().unwrap();
    let ProfileRecords::Phase4Empirical(data) = loaded.records() else {
        panic!("wrong dataset type")
    };
    assert_eq!(data.measurements.len(), 32077);
    assert_eq!(data.graphs.len(), 8926);
    assert_eq!(
        data.measurements[0].primary.as_ref().unwrap().bits(),
        0x3f80_0001
    );
    assert_eq!(data.graphs[0].bonds[0].b_1based, 2);
    assert_binding_unavailable(&test_fixture, &loaded);
}

#[test]
fn absent_optional_raw_does_not_shift_file_identities() {
    let mut test_fixture = empirical_fixture(false);
    test_fixture.mark_optional(RAW[0]);
    test_fixture.remove(RAW[0]);
    let loaded = test_fixture.load().unwrap();
    let ProfileRecords::Phase4Empirical(data) = loaded.records() else {
        panic!("wrong dataset type")
    };
    assert_eq!(data.ancillary.len(), 11);
    assert_eq!(data.ancillary[0].file_id, RAW[1]);
    assert_eq!(data.ancillary[0].bytes, b"synthetic raw 1");
}

#[test]
fn missing_corrupt_or_invalid_inputs_do_not_publish_data() {
    let mut missing = empirical_fixture(false);
    missing.remove(NORMALIZED[0].0);
    assert!(matches!(
        missing.load(),
        Err(ActiveDataError::DataUnavailable)
    ));
    let mut corrupt = empirical_fixture(false);
    corrupt.corrupt(NORMALIZED[0].0);
    assert!(matches!(
        corrupt.load(),
        Err(ActiveDataError::DataIdentityMismatch)
    ));
    let invalid = empirical_fixture(true);
    assert!(matches!(
        invalid.load(),
        Err(ActiveDataError::ContentInvalid)
    ));
}

#[test]
fn normalized_files_are_required_and_catalog_roles_are_checked() {
    let mut optional = empirical_fixture(false);
    optional.mark_optional(NORMALIZED[0].0);
    assert!(matches!(
        optional.load(),
        Err(ActiveDataError::ProfileUnsupported)
    ));
    let mut wrong_role = empirical_fixture(false);
    wrong_role.set_role(NORMALIZED[0].0, "weights");
    assert!(matches!(
        wrong_role.load(),
        Err(ActiveDataError::ProfileUnsupported)
    ));
    let mut wrong_class = empirical_fixture(false);
    wrong_class.set_class(NORMALIZED[0].0, "model_parameters");
    assert!(matches!(
        wrong_class.load(),
        Err(ActiveDataError::ProfileUnsupported)
    ));
    let mut missing_declaration = empirical_fixture(false);
    missing_declaration.remove_declared(NORMALIZED[0].0);
    assert!(matches!(
        missing_declaration.load(),
        Err(ActiveDataError::ProfileUnsupported)
    ));
    let mut unexpected_feature = empirical_fixture(false);
    unexpected_feature.set_feature_hash(Some(&"0".repeat(64)));
    assert!(matches!(
        unexpected_feature.load(),
        Err(ActiveDataError::ProfileUnsupported)
    ));
}
