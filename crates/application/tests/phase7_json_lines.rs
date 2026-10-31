use chemifree_application::{DataProfile, decode_profile};

#[test]
fn rank_json_lines_accept_a_final_record_without_a_newline() {
    let ranks = (1..=22)
        .map(|n| format!("{{\"atomic_number_u32\":{n},\"source_rank_u32\":{n}}}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(decode_profile(DataProfile::RingRank, ranks.as_bytes()).is_ok());
}

#[test]
fn valence_json_lines_accept_a_final_record_without_a_newline() {
    let valences = (0..=109)
        .map(|n| format!("{{\"atomic_number_u32\":{n},\"valence_reference_u32\":{n}}}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(decode_profile(DataProfile::ValenceReference, valences.as_bytes()).is_ok());
}

#[test]
fn endpoint_json_lines_accept_a_final_record_without_a_newline() {
    let endpoint = b"{\"atomic_number_u32\":1,\"source_rank_u32\":0}";
    assert!(decode_profile(DataProfile::ElementEndpoint, endpoint).is_ok());
}
