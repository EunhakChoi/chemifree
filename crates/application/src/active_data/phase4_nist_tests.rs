use super::prepared_tests::{Fixture, assert_binding_unavailable, fixture};
use super::{ActiveDataError, ProfileRecords, phase4_nist};
use serde_json::json;

const PATH: &str = "synthetic.json";

fn nist_fixture(invalid: bool) -> Fixture {
    let source = json!({"path":"synthetic","bytes":1,"sha256":"fixture"});
    let reported = json!({"raw":"1.0","value_decimal":"1.0","uncertainty_digits":"1","absolute_uncertainty_decimal":"0.1","estimated_marker_present":false});
    let value = json!({
        "record_id":"synthetic","source":source,"source_url":"inert:example","version":"fixture",
        "version_basis":source,"source_evaluation_epochs":{"masses":"fixture","isotopic_compositions":"fixture","standard_atomic_weights":"fixture"},
        "distribution_basis":{"classification":"fixture","classification_source":source,"terms":"fixture","terms_source":source,"terms_url":"inert:terms"},
        "citation":"fixture","normalization":{"quantities":"fixture","decimal_policy":"fixture","estimated_marker":"fixture","bracketed_singleton":"fixture","missing_policy":"fixture","symbols":"fixture","changes":"fixture"},
        "element_count":1,"isotope_count":if invalid {2} else {1},
        "elements":[{"atomic_number":1,"symbol_as_served":"H","standard_atomic_weight":{"kind":"range","raw":null,"lower_decimal":"1.0","upper_decimal":"1.1","standard_atomic_weight_value":null},"notes_raw":null}],
        "isotopes":[{"atomic_number":1,"element_symbol":"H","isotope_symbol_as_printed":null,"mass_number":1,"relative_atomic_mass":reported,"isotopic_composition":null,"source_preformatted_line":1}],
        "reported_composition_sums_decimal":{"synthetic":"marker"},"limits":["synthetic"]
    });
    let bytes = serde_json::to_string(&value)
        .unwrap()
        .replace("\"bytes\":1", &format!("\"bytes\":-{}", "9".repeat(400)))
        .replace(
            "\"synthetic\":\"marker\"",
            "\"synthetic\":0.1000000000000000000000001",
        )
        .into_bytes();
    fixture(
        "private-independent-nist-comp-4.1-snapshot-2026-10-24",
        vec![(phase4_nist::FILE_ID, PATH, bytes)],
    )
}

#[test]
fn fixture_rights_are_checked_before_snapshot_payload_is_read() {
    let test_fixture = nist_fixture(false);
    let error = match test_fixture.load_production() {
        Err(error) => error,
        Ok(_) => panic!("fixture authorization unexpectedly succeeded"),
    };
    assert_eq!(error, ActiveDataError::RightsUnverified);
    assert_eq!(test_fixture.reads(), 0);
}
#[test]
fn complete_snapshot_is_typed_without_a_mass_engine() {
    let test_fixture = nist_fixture(false);
    let loaded = test_fixture.load().unwrap();
    let ProfileRecords::Phase4Nist(data) = loaded.records() else {
        panic!("wrong dataset type")
    };
    assert_eq!(data.elements.len(), 1);
    assert_eq!(data.isotopes.len(), 1);
    assert_eq!(data.isotopes[0].relative_atomic_mass.value_decimal, "1.0");
    assert_eq!(
        data.source.bytes.to_string(),
        format!("-{}", "9".repeat(400))
    );
    assert_eq!(
        data.reported_composition_sums_decimal.as_str(),
        r#"{"synthetic":0.1000000000000000000000001}"#
    );
    assert_binding_unavailable(&test_fixture, &loaded);
}
#[test]
fn missing_corrupt_or_invalid_inputs_do_not_publish_data() {
    let mut missing = nist_fixture(false);
    missing.remove(PATH);
    assert!(matches!(
        missing.load(),
        Err(ActiveDataError::DataUnavailable)
    ));
    let mut corrupt = nist_fixture(false);
    corrupt.corrupt(PATH);
    assert!(matches!(
        corrupt.load(),
        Err(ActiveDataError::DataIdentityMismatch)
    ));
    let invalid = nist_fixture(true);
    assert!(matches!(
        invalid.load(),
        Err(ActiveDataError::ContentInvalid)
    ));
}
#[test]
fn the_catalog_file_is_required_with_its_role_and_no_feature_binding() {
    let mut optional = nist_fixture(false);
    optional.mark_optional(PATH);
    assert!(matches!(
        optional.load(),
        Err(ActiveDataError::ProfileUnsupported)
    ));
    let mut wrong_role = nist_fixture(false);
    wrong_role.set_role(PATH, "weights");
    assert!(matches!(
        wrong_role.load(),
        Err(ActiveDataError::ProfileUnsupported)
    ));
    let mut wrong_class = nist_fixture(false);
    wrong_class.set_class(PATH, "model_parameters");
    assert!(matches!(
        wrong_class.load(),
        Err(ActiveDataError::ProfileUnsupported)
    ));
    assert_eq!(wrong_class.reads(), 0);
    let mut unexpected_feature = nist_fixture(false);
    unexpected_feature.set_feature_hash(Some(&"0".repeat(64)));
    assert!(matches!(
        unexpected_feature.load(),
        Err(ActiveDataError::ProfileUnsupported)
    ));
}
