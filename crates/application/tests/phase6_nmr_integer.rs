use chemifree_application::nmr_profile::{NmrError, decode_nmr};

// Independent synthetic shapes: these values are not scientific observations.
const LOOKUP: &str = include_str!("data/nmr-synthetic/lookup.json");
const AMPLITUDES: &str = include_str!("data/nmr-synthetic/amplitudes.json");
const WIDTHS: &str = include_str!("data/nmr-synthetic/widths.json");
const FEATURES: &str = include_str!("data/nmr-synthetic/feature-schema.json");
const BOUNDS: &str = include_str!("data/nmr-synthetic/source-bounds.json");

fn files<'a>(lookup: &'a str, features: &'a str, bounds: &'a str) -> [(&'static str, &'a [u8]); 5] {
    [
        ("parameters/substituent-lookup.json", lookup.as_bytes()),
        ("parameters/coupling-amplitudes.json", AMPLITUDES.as_bytes()),
        ("parameters/width-configuration.json", WIDTHS.as_bytes()),
        ("feature-schema.json", features.as_bytes()),
        ("source-bounds.json", bounds.as_bytes()),
    ]
}

#[test]
fn exact_decimal_integers_preserve_float_witnesses_and_array_counts() {
    let lookup = LOOKUP
        .replace("\"atomic_number_u32\":0", "\"atomic_number_u32\":-0.0")
        .replace("\"atomic_number_u32\":8", "\"atomic_number_u32\":8e0");
    let features = FEATURES
        .replace("\"version\":1", "\"version\":1.0")
        .replace("\"minimum\":0", "\"minimum\":-0e4")
        .replace("\"exclusive_maximum\":9", "\"exclusive_maximum\":9.0")
        .replace("\"lookup_rows\":2", "\"lookup_rows\":2e0")
        .replace("\"empirical_scalars\":1", "\"empirical_scalars\":1.0")
        .replace("\"configuration_widths\":1", "\"configuration_widths\":1e0");
    let bounds = BOUNDS
        .replace("\"source_bytes\":64", "\"source_bytes\":64.0")
        .replace(
            "\"source_file_offset_start\":4",
            "\"source_file_offset_start\":4e0",
        )
        .replace("\"source_bytes\":8", "\"source_bytes\":8.0");
    for token in ["\"atomic_number_u32\":-0.0", "\"atomic_number_u32\":8e0"] {
        assert!(lookup.contains(token));
    }
    for token in [
        "\"version\":1.0",
        "\"minimum\":-0e4",
        "\"exclusive_maximum\":9.0",
        "\"lookup_rows\":2e0",
        "\"empirical_scalars\":1.0",
        "\"configuration_widths\":1e0",
    ] {
        assert!(features.contains(token));
    }
    for token in [
        "\"source_bytes\":64.0",
        "\"source_file_offset_start\":4e0",
        "\"source_bytes\":8.0",
    ] {
        assert!(bounds.contains(token));
    }
    let data = decode_nmr(&files(&lookup, &features, &bounds)).unwrap();
    assert_eq!(data.lookup.rows[0].atomic_number, 0);
    assert_eq!(data.lookup.rows[1].atomic_number, 8);
    assert_eq!(data.lookup.rows[0].parameter.bits, (-0.0_f64).to_bits());
    assert_eq!(data.features.counts.lookup_rows, 2);
    assert_eq!(
        data.source_bounds.numeric_source_bounds[0]
            .source_bytes
            .to_string(),
        "8"
    );

    let near_integer = lookup.replace("8e0", "8.0000000000000000001");
    assert_eq!(
        decode_nmr(&files(&near_integer, &features, &bounds)).err(),
        Some(NmrError::Record)
    );
    let fractional_count = features.replace(
        "\"lookup_rows\":2e0",
        "\"lookup_rows\":2.0000000000000000001",
    );
    assert_eq!(
        decode_nmr(&files(&lookup, &fractional_count, &bounds)).err(),
        Some(NmrError::Record)
    );
}

#[test]
fn large_declared_bounds_remain_exact_and_are_checked_without_overflow() {
    let negative_minimum = format!("-{}", "9".repeat(400));
    let maximum = format!("1{}", "0".repeat(400));
    let offset = "9".repeat(399);
    let version = "8".repeat(400);
    let features = FEATURES
        .replace("\"version\":1", &format!("\"version\":{version}"))
        .replace("\"minimum\":0", &format!("\"minimum\":{negative_minimum}"))
        .replace(
            "\"exclusive_maximum\":9",
            &format!("\"exclusive_maximum\":{maximum}"),
        );
    let bounds = BOUNDS
        .replace(
            "\"source_bytes\":64",
            &format!("\"source_bytes\":{maximum}"),
        )
        .replace(
            "\"source_file_offset_start\":4",
            &format!("\"source_file_offset_start\":{offset}"),
        );
    assert!(features.contains(&format!("\"version\":{version}")));
    assert!(features.contains(&format!("\"minimum\":{negative_minimum}")));
    assert!(features.contains(&format!("\"exclusive_maximum\":{maximum}")));
    assert!(bounds.contains(&format!("\"source_bytes\":{maximum}")));
    assert!(bounds.contains(&format!("\"source_file_offset_start\":{offset}")));
    let data = decode_nmr(&files(LOOKUP, &features, &bounds)).unwrap();
    assert_eq!(data.features.version.to_string(), version);
    assert_eq!(
        data.features.lookup_index_bounds.minimum.to_string(),
        negative_minimum
    );
    assert_eq!(
        data.features
            .lookup_index_bounds
            .exclusive_maximum
            .to_string(),
        maximum
    );
    assert_eq!(
        data.source_bounds.numeric_source_bounds[0]
            .source_file_offset_start
            .to_string(),
        offset
    );
    assert_eq!(data.lookup.rows[0].parameter.bits, (-0.0_f64).to_bits());

    let outside = bounds.replace(
        &format!("\"source_file_offset_start\":{offset}"),
        &format!("\"source_file_offset_start\":{maximum}"),
    );
    assert_eq!(
        decode_nmr(&files(LOOKUP, &features, &outside)).err(),
        Some(NmrError::Inconsistent)
    );
    let reversed = features.replace(
        &format!("\"minimum\":{negative_minimum}"),
        &format!("\"minimum\":{maximum}1"),
    );
    assert_eq!(
        decode_nmr(&files(LOOKUP, &reversed, &bounds)).err(),
        Some(NmrError::Inconsistent)
    );
}

#[test]
fn signed_bounds_limit_lookup_and_overlong_numbers_are_refused() {
    let positive_minimum = FEATURES.replace("\"minimum\":0", "\"minimum\":1.0");
    assert_eq!(
        decode_nmr(&files(LOOKUP, &positive_minimum, BOUNDS)).err(),
        Some(NmrError::Inconsistent)
    );
    let exclusive = FEATURES.replace("\"exclusive_maximum\":9", "\"exclusive_maximum\":8e0");
    assert_eq!(
        decode_nmr(&files(LOOKUP, &exclusive, BOUNDS)).err(),
        Some(NmrError::Inconsistent)
    );
    let overlong = FEATURES.replace(
        "\"version\":1",
        &format!("\"version\":{}", "9".repeat(4097)),
    );
    assert_eq!(
        decode_nmr(&files(LOOKUP, &overlong, BOUNDS)).err(),
        Some(NmrError::Record)
    );
    let overlong_signed = FEATURES.replace(
        "\"minimum\":0",
        &format!("\"minimum\":-{}", "9".repeat(4097)),
    );
    assert_eq!(
        decode_nmr(&files(LOOKUP, &overlong_signed, BOUNDS)).err(),
        Some(NmrError::Record)
    );
}

#[test]
fn lookup_key_respects_u32_boundary_even_when_declared_bounds_are_larger() {
    let lookup = LOOKUP.replace(
        "\"atomic_number_u32\":8",
        "\"atomic_number_u32\":4294967295.0",
    );
    let features = FEATURES.replace(
        "\"exclusive_maximum\":9",
        "\"exclusive_maximum\":4294967296e0",
    );
    assert!(lookup.contains("\"atomic_number_u32\":4294967295.0"));
    assert!(features.contains("\"exclusive_maximum\":4294967296e0"));
    let data = decode_nmr(&files(&lookup, &features, BOUNDS)).unwrap();
    assert_eq!(data.lookup.rows[1].atomic_number, u32::MAX);
    let overflow = lookup.replace("4294967295.0", "4294967296.0");
    assert_eq!(
        decode_nmr(&files(&overflow, &features, BOUNDS)).err(),
        Some(NmrError::Record)
    );
}
