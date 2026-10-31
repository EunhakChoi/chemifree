use super::prepared_tests::{Fixture, assert_binding_unavailable, fixture};
use super::{ActiveDataError, ProfileRecords, phase4_cmr};

const FILES: [(&str, &str, usize, usize); 4] = [
    (
        "DATA-RANGE-BB-CMR-1-raw_data",
        "DATA-RANGE-BB-CMR-1-normalized",
        100,
        4,
    ),
    (
        "DATA-RANGE-BB-CMR-2-raw_data",
        "DATA-RANGE-BB-CMR-2-normalized",
        100,
        4,
    ),
    (
        "DATA-RANGE-BB-CMR-3-raw_data",
        "DATA-RANGE-BB-CMR-3-normalized",
        62,
        4,
    ),
    (
        "DATA-RANGE-BB-CMR-4-raw_data",
        "DATA-RANGE-BB-CMR-4-normalized",
        62,
        5,
    ),
];

fn cmr_fixture(wrong_raw_witness: bool) -> Fixture {
    let mut entries = Vec::new();
    for (array, (raw_id, normalized_id, count, width)) in FILES.into_iter().enumerate() {
        let record = if width == 4 {
            vec![0, 1, 2, 3]
        } else {
            vec![0xe9, 0, 0, 0, 0]
        };
        let raw = record.repeat(count);
        let rows: String = (0..count).map(|index| {
            if width == 4 {
                let hex = if wrong_raw_witness && array == 0 && index == 0 { "04050607" } else { "00010203" };
                format!("{{\"storage_index_0based\":{index},\"raw_hex\":\"{hex}\",\"value\":0.1000000000000000000000000000001}}\n")
            } else {
                format!("{{\"storage_index_0based\":{index},\"raw_hex\":\"e900000000\",\"key_text_latin1\":\"é\"}}\n")
            }
        }).collect();
        entries.push((raw_id, raw_id, raw));
        entries.push((normalized_id, normalized_id, rows.into_bytes()));
    }
    let mut fixture = fixture("private-data-biobyte-cmr-static-data-v1", entries);
    fixture.set_role(FILES[3].0, "fragment_reference");
    fixture.set_role(FILES[3].1, "fragment_reference");
    fixture
}

#[test]
fn fixture_only_review_cannot_authorize_cmr_inputs() {
    let fixture = cmr_fixture(false);
    let error = match fixture.load_production() {
        Err(error) => error,
        Ok(_) => panic!("fixture-only authorization unexpectedly succeeded"),
    };
    assert_eq!(error, ActiveDataError::RightsUnverified);
    assert_eq!(fixture.reads(), 0);
}

#[test]
fn complete_cmr_arrays_are_typed_without_activating_a_model() {
    let fixture = cmr_fixture(false);
    let loaded = fixture.load().unwrap();
    let ProfileRecords::Phase4Cmr(data) = loaded.records() else {
        panic!("wrong dataset type")
    };
    assert_eq!(data.arrays.len(), 4);
    let phase4_cmr::CmrRows::Numeric(rows) = &data.arrays[0].rows else {
        panic!("wrong array type")
    };
    assert_eq!(rows[0].raw, [0, 1, 2, 3]);
    assert_eq!(rows[0].value.as_str(), "0.1000000000000000000000000000001");
    assert_binding_unavailable(&fixture, &loaded);
}

#[test]
fn missing_corrupt_or_inconsistent_members_never_publish_partial_data() {
    let mut missing = cmr_fixture(false);
    missing.remove(FILES[0].0);
    assert!(matches!(
        missing.load(),
        Err(ActiveDataError::DataUnavailable)
    ));
    let mut corrupt = cmr_fixture(false);
    corrupt.corrupt(FILES[0].1);
    assert!(matches!(
        corrupt.load(),
        Err(ActiveDataError::DataIdentityMismatch)
    ));
    let inconsistent = cmr_fixture(true);
    assert!(matches!(
        inconsistent.load(),
        Err(ActiveDataError::ContentInvalid)
    ));
}

#[test]
fn catalog_files_are_required_and_roles_are_not_interchangeable() {
    let mut optional = cmr_fixture(false);
    optional.mark_optional(FILES[0].0);
    assert!(matches!(
        optional.load(),
        Err(ActiveDataError::ProfileUnsupported)
    ));
    let mut wrong_role = cmr_fixture(false);
    wrong_role.set_role(FILES[3].1, "weights");
    assert!(matches!(
        wrong_role.load(),
        Err(ActiveDataError::ProfileUnsupported)
    ));
    let mut missing_declaration = cmr_fixture(false);
    missing_declaration.remove_declared(FILES[0].0);
    assert!(matches!(
        missing_declaration.load(),
        Err(ActiveDataError::ProfileUnsupported)
    ));
}
