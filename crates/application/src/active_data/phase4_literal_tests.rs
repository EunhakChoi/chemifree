use super::prepared_tests::{Fixture, assert_binding_unavailable, fixture};
use super::{ActiveDataError, ProfileRecords, phase4_literals};
use num_bigint::BigInt;
use serde_json::json;

fn metadata_fixture() -> Fixture {
    fixture(
        "private-data-molnet-metadata-v1",
        vec![
            (
                "P4-MOLNET-METADATA-002",
                "fixture/two.txt",
                b"fictional metadata two".to_vec(),
            ),
            (
                "P4-MOLNET-METADATA-001",
                "fixture/one.txt",
                b"fictional metadata one".to_vec(),
            ),
        ],
    )
}

fn numeric_fixture() -> Fixture {
    let files =
        phase4_literals::NUMERIC_FILES
            .iter()
            .map(|&(id, count)| {
                let rows: String = (0..count).map(|index| format!("{}\n", json!({
            "preferred_data_va": "independent-fixture", "source_file_offset": index,
            "source_section": "fixture", "ieee754_f64_le_hex": "0000000000000000",
            "value": 0, "semantic_role": "fixture"
        }))).collect();
                (id, id, rows.into_bytes())
            })
            .collect();
    fixture("private-data-molnet-supporting-numeric-literals-v1", files)
}

fn real48_fixture(invalid_rational: bool) -> Fixture {
    let files = phase4_literals::REAL48_FILES.iter().map(|&(id, count)| {
        let thermodynamic = id == phase4_literals::REAL48_FILES[2].0;
        let rows: String = (0..count).map(|index| {
            let mut row = json!({
                "storage_index": index, "key_hex": "e9", "key_latin1": "é",
                "real48_columns": [{
                    "column_byte_offset": 0, "raw_real48_le_hex": "010203040506",
                    "exact_rational_numerator": "-7",
                    "exact_rational_denominator": if invalid_rational && index == 0 { "0" } else { "13" }
                }]
            });
            if thermodynamic {
                row["secondary_key_latin1"] = "fixture".into();
            } else {
                row["aux_word"] = 0.into();
                row["flag_byte"] = 0.into();
            }
            format!("{row}\n")
        }).collect();
        (id, id, rows.into_bytes())
    }).collect();
    fixture("private-data-chemprop-typed-parameter-tables-v1", files)
}

#[test]
fn fixture_only_review_cannot_authorize_phase4_metadata() {
    let fixture = metadata_fixture();
    let error = match fixture.load_production() {
        Err(error) => error,
        Ok(_) => panic!("fixture-only authorization unexpectedly succeeded"),
    };
    assert_eq!(error, ActiveDataError::RightsUnverified);
    assert_eq!(fixture.reads(), 0);
}

#[test]
fn metadata_and_numeric_profiles_are_atomic_and_never_enable_models() {
    let metadata = metadata_fixture();
    let loaded = metadata.load().unwrap();
    let ProfileRecords::Phase4Literals(records) = loaded.records() else {
        panic!("wrong dataset type")
    };
    let phase4_literals::Records::Metadata(files) = records.as_ref() else {
        panic!("wrong profile")
    };
    assert_eq!(files[0].bytes, b"fictional metadata one");
    assert_binding_unavailable(&metadata, &loaded);

    let numeric = numeric_fixture();
    let loaded = numeric.load().unwrap();
    let ProfileRecords::Phase4Literals(records) = loaded.records() else {
        panic!("wrong dataset type")
    };
    let phase4_literals::Records::Numeric(files) = records.as_ref() else {
        panic!("wrong profile")
    };
    assert_eq!(files.len(), 6);
    assert_eq!(files[0].rows.len(), 13);
    assert_eq!(files[0].rows[0].bits, 0.0_f64.to_bits());
    assert_binding_unavailable(&numeric, &loaded);
}

#[test]
fn real48_raw_bytes_and_exact_rationals_are_retained_separately() {
    let fixture = real48_fixture(false);
    let loaded = fixture.load().unwrap();
    let ProfileRecords::Phase4Literals(records) = loaded.records() else {
        panic!("wrong dataset type")
    };
    let phase4_literals::Records::ChempropTables(files) = records.as_ref() else {
        panic!("wrong profile")
    };
    assert_eq!(
        files.iter().map(|file| file.rows.len()).collect::<Vec<_>>(),
        [243, 304, 87]
    );
    let column = &files[0].rows[0].columns[0];
    assert_eq!(column.raw, [1, 2, 3, 4, 5, 6]);
    assert_eq!(column.numerator, BigInt::from(-7));
    assert_eq!(column.denominator, BigInt::from(13));
    assert_binding_unavailable(&fixture, &loaded);
}

#[test]
fn missing_corrupt_or_invalid_members_cannot_publish_a_partial_profile() {
    let mut missing = metadata_fixture();
    missing.remove("fixture/one.txt");
    assert!(matches!(
        missing.load(),
        Err(ActiveDataError::DataUnavailable)
    ));
    let mut corrupt = numeric_fixture();
    corrupt.corrupt(phase4_literals::NUMERIC_FILES[0].0);
    assert!(matches!(
        corrupt.load(),
        Err(ActiveDataError::DataIdentityMismatch)
    ));
    let invalid = real48_fixture(true);
    assert!(matches!(
        invalid.load(),
        Err(ActiveDataError::ContentInvalid)
    ));
}

#[test]
fn every_catalog_file_is_required_and_must_have_its_declared_role() {
    let mut optional = metadata_fixture();
    optional.mark_optional("fixture/one.txt");
    assert!(matches!(
        optional.load(),
        Err(ActiveDataError::ProfileUnsupported)
    ));
    let mut wrong_role = numeric_fixture();
    wrong_role.set_role(phase4_literals::NUMERIC_FILES[0].0, "weights");
    assert!(matches!(
        wrong_role.load(),
        Err(ActiveDataError::ProfileUnsupported)
    ));
    let mut wrong_role = real48_fixture(false);
    wrong_role.set_role(phase4_literals::REAL48_FILES[0].0, "auxiliary_data");
    assert!(matches!(
        wrong_role.load(),
        Err(ActiveDataError::ProfileUnsupported)
    ));
}
