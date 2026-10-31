use super::prepared_tests::{Fixture, assert_binding_unavailable, fixture};
use super::{ActiveDataError, ProfileRecords};

fn nmr_fixture(invalid_first_row: bool) -> Fixture {
    // Public catalog counts; the repeated lexical rows and opaque bytes are independently invented.
    const COUNTS: [usize; 19] = [
        207, 1007, 935, 1483, 1438, 7600, 875, 79160, 3835, 141291, 9505, 47889, 14957, 62, 18534,
        1967, 3298, 3476, 3473,
    ];
    let mut owned: Vec<(String, Vec<u8>)> = Vec::new();
    for (offset, count) in COUNTS.into_iter().enumerate() {
        let index = offset + 1;
        owned.push((
            format!("P4-NMR-CARRIERS-{index:03}"),
            b"independent raw fixture".to_vec(),
        ));
        owned.push((
            format!("P4-NMR-CARRIERS-{index:03}-decoded"),
            b"independent decoded fixture".to_vec(),
        ));
        let row = if matches!(index, 6 | 8 | 10 | 12 | 14) {
            "{\"line\":1,\"kind\":\"fixture\",\"tokens\":[],\"key_lexeme\":\"x\"}\n"
        } else if invalid_first_row && index == 1 {
            "{\"line\":0,\"kind\":\"fixture\"}\n"
        } else {
            "{\"line\":1,\"kind\":\"fixture\"}\n"
        };
        let mut normalized = row.repeat(count);
        if index == 1 && !invalid_first_row {
            normalized =
                normalized.replacen("\"line\":1", &format!("\"line\":{}", "9".repeat(400)), 1);
        }
        owned.push((
            format!("P4-NMR-CARRIERS-{index:03}-normalized"),
            normalized.into_bytes(),
        ));
    }
    let entries = owned
        .iter()
        .map(|(id, bytes)| (id.as_str(), id.as_str(), bytes.clone()))
        .collect();
    let mut fixture = fixture("private-data-nmr-carriers-v1", entries);
    for index in 1..=19 {
        fixture.set_role(
            &format!("P4-NMR-CARRIERS-{index:03}-normalized"),
            "contribution_table",
        );
    }
    fixture
}

#[test]
fn fixture_only_review_does_not_authorize_nmr_carriers() {
    let fixture = nmr_fixture(false);
    let error = match fixture.load_production() {
        Err(error) => error,
        Ok(_) => panic!("fixture-only authorization unexpectedly succeeded"),
    };
    assert_eq!(error, ActiveDataError::RightsUnverified);
    assert_eq!(fixture.reads(), 0);
}

#[test]
fn a_complete_generation_is_typed_and_does_not_enable_nmr() {
    let fixture = nmr_fixture(false);
    let loaded = fixture.load().unwrap();
    let ProfileRecords::Phase4Nmr(data) = loaded.records() else {
        panic!("wrong dataset type")
    };
    assert_eq!(data.tables.len(), 19);
    let super::phase4_nmr::CarrierRow::Commented(first) = &data.tables[0].rows[0] else {
        panic!("wrong carrier shape")
    };
    assert_eq!(first.line.to_string(), "9".repeat(400));
    assert_eq!(
        data.tables
            .iter()
            .map(|table| table.rows.len())
            .sum::<usize>(),
        340_992
    );
    assert_eq!(data.ancillary.len(), 38);
    assert_binding_unavailable(&fixture, &loaded);
}

#[test]
fn unavailable_or_invalid_files_do_not_publish_partial_data() {
    let mut missing = nmr_fixture(false);
    missing.remove("P4-NMR-CARRIERS-001-normalized");
    assert!(matches!(
        missing.load(),
        Err(ActiveDataError::DataUnavailable)
    ));
    let mut corrupt = nmr_fixture(false);
    corrupt.corrupt("P4-NMR-CARRIERS-001-normalized");
    assert!(matches!(
        corrupt.load(),
        Err(ActiveDataError::DataIdentityMismatch)
    ));
    let invalid = nmr_fixture(true);
    assert!(matches!(
        invalid.load(),
        Err(ActiveDataError::ContentInvalid)
    ));
}

#[test]
fn an_absent_optional_ancillary_file_is_not_a_missing_normalized_table() {
    let mut fixture = nmr_fixture(false);
    fixture.mark_optional("P4-NMR-CARRIERS-001");
    fixture.remove("P4-NMR-CARRIERS-001");
    let loaded = fixture.load().unwrap();
    let ProfileRecords::Phase4Nmr(data) = loaded.records() else {
        panic!("wrong dataset type")
    };
    assert_eq!(data.tables.len(), 19);
    assert_eq!(data.ancillary.len(), 37);
}

#[test]
fn normalized_declarations_are_required_and_have_the_catalog_role() {
    let mut optional = nmr_fixture(false);
    optional.mark_optional("P4-NMR-CARRIERS-001-normalized");
    assert!(matches!(
        optional.load(),
        Err(ActiveDataError::ProfileUnsupported)
    ));
    let mut wrong_role = nmr_fixture(false);
    wrong_role.set_role("P4-NMR-CARRIERS-001-normalized", "auxiliary_data");
    assert!(matches!(
        wrong_role.load(),
        Err(ActiveDataError::ProfileUnsupported)
    ));

    let mut missing_declaration = nmr_fixture(false);
    missing_declaration.remove_declared("P4-NMR-CARRIERS-001-decoded");
    assert!(matches!(
        missing_declaration.load(),
        Err(ActiveDataError::ProfileUnsupported)
    ));
}
