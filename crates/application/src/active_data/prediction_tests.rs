use super::prepared_tests::{Fixture, assert_binding_unavailable, fixture};
use super::{ActiveDataError, ProfileRecords};

fn prediction_fixture() -> Fixture {
    prediction_fixture_with(None)
}

fn prediction_fixture_with(replacement: Option<(&str, &[u8])>) -> Fixture {
    let files: [(&str, &str, &[u8]); 11] = [
        (
            "polarizability",
            "parameters/atomic-polarizability.json",
            include_bytes!("../../tests/data/prediction-independent/polarizability.json"),
        ),
        (
            "pi",
            "parameters/pi-charges.json",
            include_bytes!("../../tests/data/prediction-independent/pi.json"),
        ),
        (
            "sigma",
            "parameters/sigma-charges.json",
            include_bytes!("../../tests/data/prediction-independent/sigma.json"),
        ),
        (
            "residual",
            "parameters/residual-electronegativity.json",
            include_bytes!("../../tests/data/prediction-independent/residual.json"),
        ),
        (
            "xlogp",
            "parameters/xlogp.json",
            include_bytes!("../../tests/data/prediction-independent/xlogp.json"),
        ),
        (
            "pka",
            "parameters/pka.json",
            include_bytes!("../../tests/data/prediction-independent/pka.json"),
        ),
        (
            "logs",
            "parameters/logs.json",
            include_bytes!("../../tests/data/prediction-independent/logs.json"),
        ),
        (
            "accessibility",
            "parameters/accessibility.json",
            include_bytes!("../../tests/data/prediction-independent/accessibility.json"),
        ),
        (
            "controls",
            "parameters/calculator-controls.json",
            include_bytes!("../../tests/data/prediction-independent/controls.json"),
        ),
        (
            "features",
            "feature-schema.json",
            include_bytes!("../../tests/data/prediction-independent/features.json"),
        ),
        (
            "sources",
            "source-bounds.json",
            include_bytes!("../../tests/data/prediction-independent/sources.json"),
        ),
    ];
    let mut fixture = fixture(
        "personal-local-prediction-numeric-profile-v1",
        files
            .into_iter()
            .map(|(id, path, bytes)| {
                let content = replacement
                    .filter(|(replaced_path, _)| *replaced_path == path)
                    .map_or(bytes, |(_, content)| content);
                (id, path, content.to_vec())
            })
            .collect(),
    );
    fixture.bind_feature("feature-schema.json");
    fixture
}

#[test]
fn fixture_only_review_cannot_authorize_prediction_inputs() {
    let fixture = prediction_fixture();
    let error = match fixture.load_production() {
        Err(error) => error,
        Ok(_) => panic!("fixture-only authorization unexpectedly succeeded"),
    };
    assert_eq!(error, ActiveDataError::RightsUnverified);
    assert_eq!(fixture.reads(), 0);
}

#[test]
fn complete_prediction_inputs_are_typed_and_do_not_enable_a_model() {
    let fixture = prediction_fixture();
    let mut dataset = fixture.load().unwrap();
    let ProfileRecords::PredictionInputs(records) = dataset.records() else {
        panic!("wrong dataset type")
    };
    assert_eq!(
        records.pka.models[0].rows[0].feature_id,
        "fictional-pka-feature"
    );
    assert_eq!(records.logs.model.intercept_f64.bits, 0.0_f64.to_bits());
    assert_eq!(fixture.reads(), 11);
    assert_binding_unavailable(&fixture, &dataset);
    // Keep the profile guard independently testable if the general feature gate evolves.
    dataset.feature_schema_sha256 = None;
    assert_binding_unavailable(&fixture, &dataset);
}

#[test]
fn registered_prediction_metadata_preserves_large_exact_integer_values() {
    let original = include_str!("../../tests/data/prediction-independent/features.json");
    let large = format!("1{}", "0".repeat(400));
    let features = original.replace("\"version\":1", &format!("\"version\":{large}.0"));
    assert_ne!(features, original);
    let fixture = prediction_fixture_with(Some(("feature-schema.json", features.as_bytes())));
    let dataset = fixture.load().unwrap();
    let ProfileRecords::PredictionInputs(records) = dataset.records() else {
        panic!("wrong dataset type")
    };
    assert_eq!(records.features.version.to_string(), large);
    assert_binding_unavailable(&fixture, &dataset);
}

#[test]
fn missing_or_corrupt_members_never_expose_a_partial_dataset() {
    let mut missing = prediction_fixture();
    missing.remove("parameters/pka.json");
    assert!(matches!(
        missing.load(),
        Err(ActiveDataError::DataUnavailable)
    ));

    let mut corrupt = prediction_fixture();
    corrupt.corrupt("parameters/pka.json");
    assert!(matches!(
        corrupt.load(),
        Err(ActiveDataError::DataIdentityMismatch)
    ));

    let invalid = prediction_fixture_with(Some(("parameters/pka.json", b"{}")));
    assert!(matches!(
        invalid.load(),
        Err(ActiveDataError::ContentInvalid)
    ));
}

#[test]
fn feature_identity_and_every_required_file_are_mandatory() {
    let mut wrong_feature = prediction_fixture();
    wrong_feature.set_feature_hash(Some(&"0".repeat(64)));
    assert!(matches!(
        wrong_feature.load(),
        Err(ActiveDataError::DataIdentityMismatch)
    ));

    let mut missing_feature = prediction_fixture();
    missing_feature.set_feature_hash(None);
    assert!(matches!(
        missing_feature.load(),
        Err(ActiveDataError::DataIdentityMismatch)
    ));

    let mut optional = prediction_fixture();
    optional.mark_optional("parameters/accessibility.json");
    assert!(matches!(
        optional.load(),
        Err(ActiveDataError::ProfileUnsupported)
    ));
}
