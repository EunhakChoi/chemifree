use chemifree_application::mass_profile::{MassError, decode_mass, verify_catalog_files};
use sha2::{Digest, Sha256};

// Invented values and symbols are shape fixtures, not chemical measurements.
const ELEMENTS: &str = include_str!("data/mass-synthetic/elements.json");
const ISOTOPES: &str = include_str!("data/mass-synthetic/isotopes.json");
const DEFAULTS: &str = include_str!("data/mass-synthetic/defaults.json");
const ALIASES: &str = include_str!("data/mass-synthetic/aliases.json");
const CONSTANTS: &str = include_str!("data/mass-synthetic/constants.json");
const FEATURES: &str = include_str!("data/mass-synthetic/features.json");
const BOUNDS: &str = include_str!("data/mass-synthetic/bounds.json");

fn files<'a>(
    elements: &'a str,
    isotopes: &'a str,
    defaults: &'a str,
    aliases: &'a str,
    constants: &'a str,
    features: &'a str,
    bounds: &'a str,
) -> [(&'static str, &'a [u8]); 7] {
    [
        ("parameters/elements.json", elements.as_bytes()),
        ("parameters/isotopes.json", isotopes.as_bytes()),
        (
            "parameters/default-monoisotopic-masses.json",
            defaults.as_bytes(),
        ),
        ("parameters/aliases.json", aliases.as_bytes()),
        ("parameters/mass-constants.json", constants.as_bytes()),
        ("feature-schema.json", features.as_bytes()),
        ("source-bounds.json", bounds.as_bytes()),
    ]
}

#[test]
fn complete_dataset_preserves_decimals_order_units_and_missing_abundance() {
    let data = decode_mass(&files(
        ELEMENTS, ISOTOPES, DEFAULTS, ALIASES, CONSTANTS, FEATURES, BOUNDS,
    ))
    .unwrap();
    assert_eq!(
        data.elements[1].natural_atomic_weight.source_decimal,
        "20.000"
    );
    assert_eq!(
        data.elements[1].natural_atomic_weight.reference_bits,
        20.0_f64.to_bits()
    );
    assert_eq!(data.isotopes[0].nickname.as_deref(), Some("q"));
    assert_eq!(data.isotopes[1].abundance_percent.source_decimal, "50.00");
    assert!(!data.isotopes[2].abundance_token_present);
    assert_eq!(data.defaults.rows[0].selected_isotope_row_ordinal, 0);
    assert!(
        data.defaults
            .selection_is_static_table_analysis_not_runtime_capture
    );
    assert_eq!(data.aliases[0].source_isotope_row_ordinal, 0);
    assert_eq!(data.charge_mass_coefficient.bits, (-0.25_f64).to_bits());
    assert_eq!(data.charge_mass_coefficient.unit, "fixture-unit");
}

#[test]
fn a_missing_duplicate_or_corrupt_sibling_never_returns_a_partial_dataset() {
    let valid = files(
        ELEMENTS, ISOTOPES, DEFAULTS, ALIASES, CONSTANTS, FEATURES, BOUNDS,
    );
    assert_eq!(decode_mass(&valid[..6]).err(), Some(MassError::FileSet));
    let mut duplicate = valid;
    duplicate[6] = duplicate[0];
    assert_eq!(decode_mass(&duplicate).err(), Some(MassError::FileSet));
    let corrupt = ISOTOPES.replace("0000000000002240", "0000000000002440");
    assert_eq!(
        decode_mass(&files(
            ELEMENTS, &corrupt, DEFAULTS, ALIASES, CONSTANTS, FEATURES, BOUNDS
        ))
        .err(),
        Some(MassError::Record)
    );
    let missing_nickname = ISOTOPES.replacen(",\"nickname\":null", "", 1);
    assert_eq!(
        decode_mass(&files(
            ELEMENTS,
            &missing_nickname,
            DEFAULTS,
            ALIASES,
            CONSTANTS,
            FEATURES,
            BOUNDS
        ))
        .err(),
        Some(MassError::Record)
    );
}

#[test]
fn matching_profile_names_do_not_replace_verified_catalog_bytes() {
    assert_eq!(
        verify_catalog_files(
            "chemdraw16-personal-local-atomic-isotope-masses-phase6",
            "sha256-set-49728f247c1c84f58cbf3532af3a96406e759221043f576817bae51677202440",
            b"synthetic manifest",
            b"synthetic registry",
            b"synthetic profile",
        ),
        Err(MassError::CatalogMismatch)
    );
}

#[test]
fn cross_file_counts_indices_and_lineage_bounds_are_checked() {
    let count = FEATURES.replace("\"isotope_rows\":3", "\"isotope_rows\":4");
    assert_eq!(
        decode_mass(&files(
            ELEMENTS, ISOTOPES, DEFAULTS, ALIASES, CONSTANTS, &count, BOUNDS
        ))
        .err(),
        Some(MassError::Inconsistent)
    );
    let wrong_element =
        ISOTOPES.replacen("\"atomic_number_u32\":42", "\"atomic_number_u32\":77", 1);
    assert_eq!(
        decode_mass(&files(
            ELEMENTS,
            &wrong_element,
            DEFAULTS,
            ALIASES,
            CONSTANTS,
            FEATURES,
            BOUNDS
        ))
        .err(),
        Some(MassError::Inconsistent)
    );
    let overlap = ELEMENTS.replace(
        "\"first_isotope_row_ordinal\":2",
        "\"first_isotope_row_ordinal\":1",
    );
    assert_eq!(
        decode_mass(&files(
            &overlap, ISOTOPES, DEFAULTS, ALIASES, CONSTANTS, FEATURES, BOUNDS
        ))
        .err(),
        Some(MassError::Inconsistent)
    );
    let wrong_alias = ALIASES.replace(
        "\"source_isotope_row_ordinal\":0",
        "\"source_isotope_row_ordinal\":2",
    );
    assert_eq!(
        decode_mass(&files(
            ELEMENTS,
            ISOTOPES,
            DEFAULTS,
            &wrong_alias,
            CONSTANTS,
            FEATURES,
            BOUNDS
        ))
        .err(),
        Some(MassError::Inconsistent)
    );
    let wrong_default = DEFAULTS.replacen(
        "\"selected_isotope_row_ordinal\":0",
        "\"selected_isotope_row_ordinal\":1",
        1,
    );
    assert_eq!(
        decode_mass(&files(
            ELEMENTS,
            ISOTOPES,
            &wrong_default,
            ALIASES,
            CONSTANTS,
            FEATURES,
            BOUNDS
        ))
        .err(),
        Some(MassError::Inconsistent)
    );
    let overflow = BOUNDS.replace(
        "\"source_file_offset\":0",
        "\"source_file_offset\":18446744073709551615",
    );
    assert_eq!(
        decode_mass(&files(
            ELEMENTS, ISOTOPES, DEFAULTS, ALIASES, CONSTANTS, FEATURES, &overflow
        ))
        .err(),
        Some(MassError::Inconsistent)
    );
}

#[test]
fn lineage_rows_must_identify_existing_unique_element_or_isotope_records() {
    let original: serde_json::Value = serde_json::from_str(BOUNDS).unwrap();
    let mut outcomes = Vec::new();
    for (kind, ordinal) in [
        ("element", 1_u64),
        ("isotope", 2),
        ("element", 2),
        ("isotope", 3),
        ("unknown", 0),
    ] {
        let mut bounds = original.clone();
        bounds["row_source_spans"][0]["kind"] = kind.into();
        bounds["row_source_spans"][0]["row_ordinal"] = ordinal.into();
        let bounds = serde_json::to_string(&bounds).unwrap();
        outcomes.push(
            decode_mass(&files(
                ELEMENTS, ISOTOPES, DEFAULTS, ALIASES, CONSTANTS, FEATURES, &bounds,
            ))
            .err(),
        );
    }
    assert_eq!(
        outcomes,
        [
            None,
            None,
            Some(MassError::Inconsistent),
            Some(MassError::Inconsistent),
            Some(MassError::Record)
        ]
    );
}

#[test]
fn lineage_does_not_assign_multiple_source_lines_to_one_record() {
    let original: serde_json::Value = serde_json::from_str(BOUNDS).unwrap();
    let mut duplicate = original;
    let span = duplicate["row_source_spans"][0].clone();
    duplicate["row_source_spans"]
        .as_array_mut()
        .unwrap()
        .push(span);
    let duplicate = serde_json::to_string(&duplicate).unwrap();
    assert_eq!(
        decode_mass(&files(
            ELEMENTS, ISOTOPES, DEFAULTS, ALIASES, CONSTANTS, FEATURES, &duplicate,
        ))
        .err(),
        Some(MassError::Inconsistent)
    );
}

#[test]
fn static_view_and_provenance_cannot_claim_executable_behavior() {
    let claimed_runtime = DEFAULTS.replace(
        "\"selection_is_static_table_analysis_not_runtime_capture\":true",
        "\"selection_is_static_table_analysis_not_runtime_capture\":false",
    );
    assert_eq!(
        decode_mass(&files(
            ELEMENTS,
            ISOTOPES,
            &claimed_runtime,
            ALIASES,
            CONSTANTS,
            FEATURES,
            BOUNDS
        ))
        .err(),
        Some(MassError::Inconsistent)
    );
    let dispatch = BOUNDS.replace(
        "\"source_offsets_are_not_native_dispatch\":true",
        "\"source_offsets_are_not_native_dispatch\":false",
    );
    assert_eq!(
        decode_mass(&files(
            ELEMENTS, ISOTOPES, DEFAULTS, ALIASES, CONSTANTS, FEATURES, &dispatch
        ))
        .err(),
        Some(MassError::Inconsistent)
    );
}

#[test]
fn static_selection_uses_exact_abundance_and_keeps_the_first_on_a_tie() {
    let later_tie = DEFAULTS
        .replacen("\"selected_isotope_row_ordinal\":0", "\"selected_isotope_row_ordinal\":1", 1)
        .replacen("\"mass_number_u32\":9", "\"mass_number_u32\":10", 1)
        .replacen("\"source_decimal\":\"9.0\",\"reference_f64\":9.0,\"reference_f64_bits_le\":\"0000000000002240\"", "\"source_decimal\":\"10.0\",\"reference_f64\":10.0,\"reference_f64_bits_le\":\"0000000000002440\"", 1);
    assert_eq!(
        decode_mass(&files(
            ELEMENTS, ISOTOPES, &later_tie, ALIASES, CONSTANTS, FEATURES, BOUNDS
        ))
        .err(),
        Some(MassError::Inconsistent)
    );

    // Both decimal abundances underflow to binary64 zero but are still distinct source values.
    let tiny = ISOTOPES
        .replacen("\"source_decimal\":\"50.0\",\"reference_f64\":50.0,\"reference_f64_bits_le\":\"0000000000004940\"", "\"source_decimal\":\"1e-99999999999999999999\",\"reference_f64\":0.0,\"reference_f64_bits_le\":\"0000000000000000\"", 1)
        .replacen("\"source_decimal\":\"50.00\",\"reference_f64\":50.0,\"reference_f64_bits_le\":\"0000000000004940\"", "\"source_decimal\":\"2e-99999999999999999999\",\"reference_f64\":0.0,\"reference_f64_bits_le\":\"0000000000000000\"", 1);
    assert_eq!(
        decode_mass(&files(
            ELEMENTS, &tiny, DEFAULTS, ALIASES, CONSTANTS, FEATURES, BOUNDS
        ))
        .err(),
        Some(MassError::Inconsistent)
    );
    assert!(
        decode_mass(&files(
            ELEMENTS, &tiny, &later_tie, ALIASES, CONSTANTS, FEATURES, BOUNDS
        ))
        .is_ok()
    );
}

#[test]
fn absent_abundance_tokens_mean_zero() {
    let absent_nonzero = ISOTOPES.replace(
        "\"source_decimal\":\"0\",\"reference_f64\":0.0,\"reference_f64_bits_le\":\"0000000000000000\"",
        "\"source_decimal\":\"50.0\",\"reference_f64\":50.0,\"reference_f64_bits_le\":\"0000000000004940\"",
    );
    let not_zero = DEFAULTS.replace(
        "\"all_abundances_zero\":true",
        "\"all_abundances_zero\":false",
    );
    assert_eq!(
        decode_mass(&files(
            ELEMENTS,
            &absent_nonzero,
            &not_zero,
            ALIASES,
            CONSTANTS,
            FEATURES,
            BOUNDS
        ))
        .err(),
        Some(MassError::Inconsistent)
    );
}

#[test]
fn present_abundances_cannot_be_negative_even_when_binary64_underflows() {
    let variants = [
        (
            "-1.0",
            "-1.0",
            "000000000000f0bf",
            "-2.0",
            "-2.0",
            "00000000000000c0",
        ),
        (
            "-1e-99999999999999999999",
            "-0.0",
            "0000000000000080",
            "-2e-99999999999999999999",
            "-0.0",
            "0000000000000080",
        ),
    ];
    let mut outcomes = Vec::new();
    for (first, first_reference, first_bits, second, second_reference, second_bits) in variants {
        let isotopes = ISOTOPES
            .replacen(
                "\"source_decimal\":\"50.0\",\"reference_f64\":50.0,\"reference_f64_bits_le\":\"0000000000004940\"",
                &format!("\"source_decimal\":\"{first}\",\"reference_f64\":{first_reference},\"reference_f64_bits_le\":\"{first_bits}\""),
                1,
            )
            .replacen(
                "\"source_decimal\":\"50.00\",\"reference_f64\":50.0,\"reference_f64_bits_le\":\"0000000000004940\"",
                &format!("\"source_decimal\":\"{second}\",\"reference_f64\":{second_reference},\"reference_f64_bits_le\":\"{second_bits}\""),
                1,
            );
        outcomes.push(
            decode_mass(&files(
                ELEMENTS, &isotopes, DEFAULTS, ALIASES, CONSTANTS, FEATURES, BOUNDS,
            ))
            .err(),
        );
    }
    assert_eq!(
        outcomes,
        [Some(MassError::Inconsistent), Some(MassError::Inconsistent)]
    );
}

#[test]
fn negative_zero_abundance_remains_a_valid_zero() {
    let isotopes = ISOTOPES
        .replacen(
            "\"source_decimal\":\"50.0\",\"reference_f64\":50.0,\"reference_f64_bits_le\":\"0000000000004940\"",
            "\"source_decimal\":\"-0.0\",\"reference_f64\":-0.0,\"reference_f64_bits_le\":\"0000000000000080\"",
            1,
        )
        .replacen(
            "\"source_decimal\":\"50.00\",\"reference_f64\":50.0,\"reference_f64_bits_le\":\"0000000000004940\"",
            "\"source_decimal\":\"0.0\",\"reference_f64\":0.0,\"reference_f64_bits_le\":\"0000000000000000\"",
            1,
        );
    let defaults = DEFAULTS.replacen(
        "\"all_abundances_zero\":false",
        "\"all_abundances_zero\":true",
        1,
    );
    assert!(
        decode_mass(&files(
            ELEMENTS, &isotopes, &defaults, ALIASES, CONSTANTS, FEATURES, BOUNDS
        ))
        .is_ok()
    );
}

#[test]
fn isotope_masses_must_be_positive() {
    let negative_isotope = ISOTOPES.replacen(
        "\"source_decimal\":\"9.0\",\"reference_f64\":9.0,\"reference_f64_bits_le\":\"0000000000002240\"",
        "\"source_decimal\":\"-9.0\",\"reference_f64\":-9.0,\"reference_f64_bits_le\":\"00000000000022c0\"", 1,
    );
    let negative_default = DEFAULTS.replacen(
        "\"source_decimal\":\"9.0\",\"reference_f64\":9.0,\"reference_f64_bits_le\":\"0000000000002240\"",
        "\"source_decimal\":\"-9.0\",\"reference_f64\":-9.0,\"reference_f64_bits_le\":\"00000000000022c0\"", 1,
    );
    assert_eq!(
        decode_mass(&files(
            ELEMENTS,
            &negative_isotope,
            &negative_default,
            ALIASES,
            CONSTANTS,
            FEATURES,
            BOUNDS
        ))
        .err(),
        Some(MassError::Inconsistent)
    );
}

#[test]
fn nominal_mass_number_matches_the_independent_binary64_reference() {
    let base_isotopes: serde_json::Value = serde_json::from_str(ISOTOPES).unwrap();
    let base_defaults: serde_json::Value = serde_json::from_str(DEFAULTS).unwrap();
    let mut outcomes = Vec::new();
    for (source, number, bits) in [
        ("8.5", 8.5, "0000000000002140"),
        ("8.49999999999999999999", 8.5, "0000000000002140"),
        ("9.5", 9.5, "0000000000002340"),
        ("9.49999999999999999999", 9.5, "0000000000002340"),
    ] {
        let mut isotopes = base_isotopes.clone();
        let mut defaults = base_defaults.clone();
        let mass = serde_json::json!({"source_decimal": source, "reference_f64": number, "reference_f64_bits_le": bits});
        isotopes["isotopes"][0]["mass"] = mass.clone();
        defaults["rows"][0]["mass"] = mass;
        let isotopes = serde_json::to_string(&isotopes).unwrap();
        let defaults = serde_json::to_string(&defaults).unwrap();
        outcomes.push(
            decode_mass(&files(
                ELEMENTS, &isotopes, &defaults, ALIASES, CONSTANTS, FEATURES, BOUNDS,
            ))
            .err(),
        );
    }
    assert_eq!(
        outcomes,
        [
            None,
            None,
            Some(MassError::Inconsistent),
            Some(MassError::Inconsistent)
        ]
    );
}

#[test]
fn mass_metadata_and_charge_coefficient_match_the_profile_contract() {
    let positive_charge = CONSTANTS
        .replace("-0.25", "0.25")
        .replace("000000000000d0bf", "000000000000d03f");
    let nonzero_secondary = FEATURES.replace("\"value\":0.0", "\"value\":1.0");
    let explicit_mass_number = FEATURES.replace(
        "\"source_mass_number_field_explicit_in_text\":false",
        "\"source_mass_number_field_explicit_in_text\":true",
    );
    let unchecked_mass_number = FEATURES.replace(
        "\"mass_number_values_independently_checked_against_source_constructor_contract\":true",
        "\"mass_number_values_independently_checked_against_source_constructor_contract\":false",
    );
    let outcomes = [
        decode_mass(&files(
            ELEMENTS,
            ISOTOPES,
            DEFAULTS,
            ALIASES,
            &positive_charge,
            FEATURES,
            BOUNDS,
        ))
        .err(),
        decode_mass(&files(
            ELEMENTS,
            ISOTOPES,
            DEFAULTS,
            ALIASES,
            CONSTANTS,
            &nonzero_secondary,
            BOUNDS,
        ))
        .err(),
        decode_mass(&files(
            ELEMENTS,
            ISOTOPES,
            DEFAULTS,
            ALIASES,
            CONSTANTS,
            &explicit_mass_number,
            BOUNDS,
        ))
        .err(),
        decode_mass(&files(
            ELEMENTS,
            ISOTOPES,
            DEFAULTS,
            ALIASES,
            CONSTANTS,
            &unchecked_mass_number,
            BOUNDS,
        ))
        .err(),
    ];
    assert_eq!(outcomes, [Some(MassError::Inconsistent); 4]);
}

#[test]
fn optional_raw_source_is_verified_as_inert_lineage() {
    let raw = b"synthetic\nrest\n";
    let mut bounds: serde_json::Value = serde_json::from_str(BOUNDS).unwrap();
    bounds["text_source"]["bytes"] = raw.len().into();
    bounds["text_source"]["sha256"] = format!("{:x}", Sha256::digest(raw)).into();
    bounds["row_source_spans"][0]["source_bytes_including_newline"] = 10.into();
    bounds["row_source_spans"][0]["source_line_sha256"] =
        format!("{:x}", Sha256::digest(&raw[..10])).into();
    let bounds = serde_json::to_string(&bounds).unwrap();
    let mut files = files(
        ELEMENTS, ISOTOPES, DEFAULTS, ALIASES, CONSTANTS, FEATURES, &bounds,
    )
    .to_vec();
    files.push(("raw/CS Isotopes Table.txt", raw));
    assert!(decode_mass(&files).is_ok());

    let mut wrong_row: serde_json::Value = serde_json::from_str(&bounds).unwrap();
    wrong_row["row_source_spans"][0]["row_ordinal"] = 2.into();
    let wrong_row = serde_json::to_string(&wrong_row).unwrap();
    files[6].1 = wrong_row.as_bytes();
    assert_eq!(decode_mass(&files).err(), Some(MassError::Inconsistent));

    let mut wrong_line: serde_json::Value = serde_json::from_str(&bounds).unwrap();
    wrong_line["row_source_spans"][0]["source_line_1based"] = 2.into();
    let wrong_line = serde_json::to_string(&wrong_line).unwrap();
    files[6].1 = wrong_line.as_bytes();
    assert_eq!(decode_mass(&files).err(), Some(MassError::Inconsistent));

    let mut wrong_digest: serde_json::Value = serde_json::from_str(&bounds).unwrap();
    wrong_digest["row_source_spans"][0]["source_line_sha256"] = "0".repeat(64).into();
    let wrong_digest = serde_json::to_string(&wrong_digest).unwrap();
    files[6].1 = wrong_digest.as_bytes();
    assert_eq!(decode_mass(&files).err(), Some(MassError::Inconsistent));
    files[6].1 = bounds.as_bytes();

    let changed = b"synthetix\nrest\n";
    files[7].1 = changed;
    assert_eq!(decode_mass(&files).err(), Some(MassError::Inconsistent));
    files[7].0 = "raw/other.txt";
    assert_eq!(decode_mass(&files).err(), Some(MassError::FileSet));
}
