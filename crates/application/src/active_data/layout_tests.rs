use super::prepared_tests::{Fixture, assert_binding_unavailable, fixture};
use super::{ActiveDataError, ProfileRecords};
use crate::layout_data_binding::LayoutReader;
use serde_json::json;

const PATH: &str = "independent/layout.jsonl";
const LARGE_ID: &str = "legacy-drawing-ring-large-seed-operands";
const LARGE_VERSION: &str =
    "sha256-set-e0313b4ee9f29cd9283a2947b72366d342320e751b340a39690588a3ffaf1993";

fn row(id: &str, size: usize) -> String {
    let zeros = vec!["0000000000000000"; size];
    let lengths = vec!["000000000000f03f"; size];
    json!({
        "operand_id": id, "ring_size": size,
        "calibration_reference_radians_f64le": "0000000000000080",
        "constructor_heading_radians_f64le": "0000000000000000",
        "winding_mode": 1, "joint_policy_flag": false,
        "turn_degrees_f64le": zeros, "observed_heading_radians_f64le": zeros,
        "edge_length_multipliers_f64le": lengths
    })
    .to_string()
}

fn large_fixture() -> Fixture {
    let bytes = format!(
        "{}\n{}",
        row("LARGE-CONTOUR-EXACT-002", 20),
        row("LARGE-CONTOUR-EXACT-001", 20)
    )
    .into_bytes();
    let mut source = fixture(
        "independent-layout-fixture",
        vec![("synthetic-layout", PATH, bytes)],
    );
    source.set_identity(LARGE_ID, LARGE_VERSION);
    source
}

fn fixed_fixture() -> Fixture {
    let rows = (0..158)
        .map(|index| row(&format!("synthetic-{index}"), 3))
        .collect::<Vec<_>>();
    fixture(
        "independent-layout-fixture",
        vec![("synthetic-layout", PATH, rows.join("\n").into_bytes())],
    )
}

#[test]
fn fixture_rights_do_not_authorize_personal_layout_data() {
    let source = large_fixture();
    assert!(matches!(
        source.load_layout_production(LayoutReader::LargeTwentySeed),
        Err(ActiveDataError::RightsUnverified)
    ));
    assert_eq!(source.reads(), 0);
}

#[test]
fn large_seed_role_publishes_only_complete_typed_rows_without_a_model() {
    let source = large_fixture();
    let data = source.load_layout(LayoutReader::LargeTwentySeed).unwrap();
    let ProfileRecords::LayoutContours(input) = data.records() else {
        panic!("wrong record type")
    };
    assert_eq!(input.role, LayoutReader::LargeTwentySeed);
    assert_eq!(input.contours.len(), 2);
    assert_eq!(input.contours[0].operand_id, "LARGE-CONTOUR-EXACT-002");
    assert_eq!(
        input.contours[0].calibration_reference_bits,
        (-0.0_f64).to_bits()
    );
    assert_binding_unavailable(&source, &data);
}

#[test]
fn fixed_role_requires_all_158_rows_and_never_publishes_corrupt_input() {
    let mut source = fixed_fixture();
    let data = source.load_layout(LayoutReader::FixedContours).unwrap();
    let ProfileRecords::LayoutContours(input) = data.records() else {
        panic!("wrong record type")
    };
    assert_eq!(input.role, LayoutReader::FixedContours);
    assert_eq!(input.contours.len(), 158);
    assert_binding_unavailable(&source, &data);
    source.corrupt(PATH);
    assert!(matches!(
        source.load_layout(LayoutReader::FixedContours),
        Err(ActiveDataError::DataIdentityMismatch)
    ));
    source.remove(PATH);
    assert!(matches!(
        source.load_layout(LayoutReader::FixedContours),
        Err(ActiveDataError::DataUnavailable)
    ));
}

#[test]
fn large_role_requires_published_dataset_identity_before_payload_read() {
    let mut wrong_id = large_fixture();
    wrong_id.set_identity("unrelated", LARGE_VERSION);
    assert!(matches!(
        wrong_id.load_layout(LayoutReader::LargeTwentySeed),
        Err(ActiveDataError::ProfileUnsupported)
    ));
    assert_eq!(wrong_id.reads(), 0);
    let mut wrong_version = large_fixture();
    wrong_version.set_identity(LARGE_ID, "unrelated-version");
    assert!(matches!(
        wrong_version.load_layout(LayoutReader::LargeTwentySeed),
        Err(ActiveDataError::ProfileUnsupported)
    ));
    assert_eq!(wrong_version.reads(), 0);
}

#[test]
fn selected_role_rejects_wrong_ids_missing_required_files_and_feature_binding() {
    let bytes = format!(
        "{}\n{}",
        row("LARGE-CONTOUR-EXACT-001", 20),
        row("synthetic-other", 20)
    )
    .into_bytes();
    let mut wrong_id = fixture(
        "independent-layout-fixture",
        vec![("synthetic-layout", PATH, bytes)],
    );
    wrong_id.set_identity(LARGE_ID, LARGE_VERSION);
    assert!(matches!(
        wrong_id.load_layout(LayoutReader::LargeTwentySeed),
        Err(ActiveDataError::ContentInvalid)
    ));
    let mut optional = large_fixture();
    optional.mark_optional(PATH);
    assert!(matches!(
        optional.load_layout(LayoutReader::LargeTwentySeed),
        Err(ActiveDataError::ProfileUnsupported)
    ));
    let mut feature = large_fixture();
    feature.set_feature_hash(Some(&"0".repeat(64)));
    assert!(matches!(
        feature.load_layout(LayoutReader::LargeTwentySeed),
        Err(ActiveDataError::ProfileUnsupported)
    ));
}
