use chemifree_application::{DataProfile, decode_profile};
use serde_json::json;

#[test]
fn valid_layout_json_lines_need_not_end_with_a_newline() {
    let row = |id: &str| {
        let zeros = vec!["0000000000000000"; 20];
        let lengths = vec!["000000000000f03f"; 20];
        json!({
            "operand_id":id, "ring_size":20,
            "calibration_reference_radians_f64le":"0000000000000000",
            "constructor_heading_radians_f64le":"0000000000000000",
            "winding_mode":1, "joint_policy_flag":false,
            "turn_degrees_f64le":zeros, "observed_heading_radians_f64le":zeros,
            "edge_length_multipliers_f64le":lengths
        })
        .to_string()
    };
    let input = format!(
        "{}\n{}",
        row("LARGE-CONTOUR-EXACT-001"),
        row("LARGE-CONTOUR-EXACT-002")
    );
    assert!(decode_profile(DataProfile::LargeRingSeeds, input.as_bytes()).is_ok());
}
