use chemifree_application::nmr_profile::{NmrError, decode_nmr, verify_catalog_files};

// These are independent shape examples. None of their values are scientific observations.
const LOOKUP: &str = include_str!("data/nmr-synthetic/lookup.json");
const AMPLITUDES: &str = include_str!("data/nmr-synthetic/amplitudes.json");
const WIDTHS: &str = include_str!("data/nmr-synthetic/widths.json");
const FEATURES: &str = include_str!("data/nmr-synthetic/feature-schema.json");
const BOUNDS: &str = include_str!("data/nmr-synthetic/source-bounds.json");

fn files<'a>(
    lookup: &'a str,
    amplitudes: &'a str,
    widths: &'a str,
    features: &'a str,
    bounds: &'a str,
) -> [(&'static str, &'a [u8]); 5] {
    [
        ("parameters/substituent-lookup.json", lookup.as_bytes()),
        ("parameters/coupling-amplitudes.json", amplitudes.as_bytes()),
        ("parameters/width-configuration.json", widths.as_bytes()),
        ("feature-schema.json", features.as_bytes()),
        ("source-bounds.json", bounds.as_bytes()),
    ]
}

#[test]
fn complete_synthetic_data_preserves_bits_units_and_separate_parameter_families() {
    let decoded = decode_nmr(&files(LOOKUP, AMPLITUDES, WIDTHS, FEATURES, BOUNDS)).unwrap();
    assert_eq!(decoded.lookup.rows.len(), 2);
    assert_eq!(decoded.lookup.rows[0].atomic_number, 0);
    assert_eq!(decoded.lookup.rows[0].parameter.bits, (-0.0_f64).to_bits());
    assert_eq!(decoded.lookup.rows[1].parameter.bits, 0.5_f64.to_bits());
    assert_eq!(decoded.lookup.unit, "fixture-unit");
    assert_eq!(
        decoded.amplitudes.parameters[0].coefficient.bits,
        (-0.25_f64).to_bits()
    );
    assert_eq!(decoded.amplitudes.parameters[0].unit, "fixture-a");
    assert_eq!(decoded.widths.parameters[0].value.bits, 1.25_f64.to_bits());
    assert_eq!(decoded.widths.parameters[0].unit, "fixture-w");
    assert!(!decoded.widths.trained_model_claimed);
    assert_eq!(decoded.features.counts.lookup_rows, 2);
    assert_eq!(
        decoded.source_bounds.numeric_source_bounds[0].source_bytes,
        8
    );
}

#[test]
fn integer_negative_zero_is_an_integer_zero_not_a_decimal_token() {
    let zero = LOOKUP.replace("\"atomic_number_u32\":0", "\"atomic_number_u32\":-0");
    let decoded = decode_nmr(&files(&zero, AMPLITUDES, WIDTHS, FEATURES, BOUNDS)).unwrap();
    assert_eq!(decoded.lookup.rows[0].atomic_number, 0);
    let fractional_zero = LOOKUP.replace("\"atomic_number_u32\":0", "\"atomic_number_u32\":-0.0");
    assert!(matches!(
        decode_nmr(&files(
            &fractional_zero,
            AMPLITUDES,
            WIDTHS,
            FEATURES,
            BOUNDS
        )),
        Err(NmrError::Record)
    ));
}

#[test]
fn no_partial_dataset_is_returned_when_a_file_is_missing_or_duplicated() {
    let valid = files(LOOKUP, AMPLITUDES, WIDTHS, FEATURES, BOUNDS);
    assert!(matches!(decode_nmr(&valid[..4]), Err(NmrError::FileSet)));
    let duplicate = [valid[0], valid[1], valid[2], valid[3], valid[3]];
    assert!(matches!(decode_nmr(&duplicate), Err(NmrError::FileSet)));
}

#[test]
fn matching_names_without_the_frozen_metadata_bytes_cannot_claim_the_profile() {
    assert_eq!(
        verify_catalog_files(
            "chemdraw16-personal-local-nmr-coupling-parameters-phase6",
            "sha256-set-69fa98dcb2582340daf209db3c3214313ffbe623aadf5cc1ff51289d090a3f7d",
            b"synthetic manifest",
            b"synthetic registry",
            b"synthetic profile",
        ),
        Err(NmrError::CatalogMismatch)
    );
}

#[test]
fn malformed_values_witnesses_and_unknown_fields_are_rejected() {
    let bad_witness = LOOKUP.replace("000000000000e03f", "000000000000f03f");
    assert!(matches!(
        decode_nmr(&files(&bad_witness, AMPLITUDES, WIDTHS, FEATURES, BOUNDS)),
        Err(NmrError::Record)
    ));
    let duplicate_key = LOOKUP.replace(
        "\"unit\":\"fixture-unit\"",
        "\"unit\":\"fixture-unit\",\"unit\":\"other\"",
    );
    assert!(matches!(
        decode_nmr(&files(&duplicate_key, AMPLITUDES, WIDTHS, FEATURES, BOUNDS)),
        Err(NmrError::Record)
    ));
    let unknown = WIDTHS.replace(
        "\"trained_model_claimed\":false",
        "\"trained_model_claimed\":false,\"instruction\":1",
    );
    assert!(matches!(
        decode_nmr(&files(LOOKUP, AMPLITUDES, &unknown, FEATURES, BOUNDS)),
        Err(NmrError::Record)
    ));
    let nonfinite = LOOKUP.replace("000000000000e03f", "000000000000f07f");
    assert!(matches!(
        decode_nmr(&files(&nonfinite, AMPLITUDES, WIDTHS, FEATURES, BOUNDS)),
        Err(NmrError::Record)
    ));
    let uppercase = LOOKUP.replace("000000000000e03f", "000000000000E03F");
    assert!(matches!(
        decode_nmr(&files(&uppercase, AMPLITUDES, WIDTHS, FEATURES, BOUNDS)),
        Err(NmrError::Record)
    ));
    let missing_unit = AMPLITUDES.replace(",\"unit\":\"fixture-a\"", "");
    assert!(matches!(
        decode_nmr(&files(LOOKUP, &missing_unit, WIDTHS, FEATURES, BOUNDS)),
        Err(NmrError::Record)
    ));
}

#[test]
fn declared_counts_bounds_and_references_are_checked_before_exposing_data() {
    let count = FEATURES.replace("\"lookup_rows\":2", "\"lookup_rows\":3");
    assert!(matches!(
        decode_nmr(&files(LOOKUP, AMPLITUDES, WIDTHS, &count, BOUNDS)),
        Err(NmrError::Inconsistent)
    ));
    let out_of_range = FEATURES.replace("\"exclusive_maximum\":9", "\"exclusive_maximum\":8");
    assert!(matches!(
        decode_nmr(&files(LOOKUP, AMPLITUDES, WIDTHS, &out_of_range, BOUNDS)),
        Err(NmrError::Inconsistent)
    ));
    let empty_lookup = r#"{"table_id":"toy-table","unit":"fixture-unit","rows":[]}"#;
    let reversed_bounds = FEATURES
        .replace("\"lookup_rows\":2", "\"lookup_rows\":0")
        .replace("\"minimum\":0", "\"minimum\":10");
    assert!(matches!(
        decode_nmr(&files(
            empty_lookup,
            AMPLITUDES,
            WIDTHS,
            &reversed_bounds,
            BOUNDS
        )),
        Err(NmrError::Inconsistent)
    ));
    let missing_id = FEATURES.replace("\"toy-amp\",\"toy-width\"", "\"toy-amp\"");
    assert!(matches!(
        decode_nmr(&files(LOOKUP, AMPLITUDES, WIDTHS, &missing_id, BOUNDS)),
        Err(NmrError::Inconsistent)
    ));
    let unbacked_id = FEATURES.replace(
        "\"toy-amp\",\"toy-width\"",
        "\"toy-amp\",\"toy-width\",\"not-in-any-parameter-file\"",
    );
    assert!(matches!(
        decode_nmr(&files(LOOKUP, AMPLITUDES, WIDTHS, &unbacked_id, BOUNDS)),
        Err(NmrError::Inconsistent)
    ));
    let outside = BOUNDS.replace(
        "\"source_file_offset_start\":4",
        "\"source_file_offset_start\":60",
    );
    assert!(matches!(
        decode_nmr(&files(LOOKUP, AMPLITUDES, WIDTHS, FEATURES, &outside)),
        Err(NmrError::Inconsistent)
    ));
    let overflow = BOUNDS.replace(
        "\"source_file_offset_start\":4",
        "\"source_file_offset_start\":18446744073709551615",
    );
    assert!(matches!(
        decode_nmr(&files(LOOKUP, AMPLITUDES, WIDTHS, FEATURES, &overflow)),
        Err(NmrError::Inconsistent)
    ));
    let duplicates = LOOKUP.replace("\"atomic_number_u32\":8", "\"atomic_number_u32\":0");
    assert!(matches!(
        decode_nmr(&files(&duplicates, AMPLITUDES, WIDTHS, FEATURES, BOUNDS)),
        Err(NmrError::Inconsistent)
    ));
    let model_claim = WIDTHS.replace(
        "\"trained_model_claimed\":false",
        "\"trained_model_claimed\":true",
    );
    assert!(matches!(
        decode_nmr(&files(LOOKUP, AMPLITUDES, &model_claim, FEATURES, BOUNDS)),
        Err(NmrError::Inconsistent)
    ));
    let executable_claim = BOUNDS.replace(
        "\"original_native_calls_included\":false",
        "\"original_native_calls_included\":true",
    );
    assert!(matches!(
        decode_nmr(&files(
            LOOKUP,
            AMPLITUDES,
            WIDTHS,
            FEATURES,
            &executable_claim
        )),
        Err(NmrError::Inconsistent)
    ));
}
