use chemifree_application::mass_input::{MassInput, MassInputError, decode, verify_catalog};
use serde_json::{Value, json};

const PATHS: [&str; 7] = [
    "parameters/elements.json",
    "parameters/isotopes.json",
    "parameters/default-monoisotopic-masses.json",
    "parameters/aliases.json",
    "parameters/mass-constants.json",
    "feature-schema.json",
    "source-bounds.json",
];
const RAW: &[u8] =
    b"fictional element a\r\nfictional isotope a\r\nfictional isotope b\r\nfictional element b\r\n";

fn documents() -> Vec<Value> {
    [
        include_str!("data/mass-independent-v3/elements.json"),
        include_str!("data/mass-independent-v3/isotopes.json"),
        include_str!("data/mass-independent-v3/selection.json"),
        include_str!("data/mass-independent-v3/aliases.json"),
        include_str!("data/mass-independent-v3/constants.json"),
        include_str!("data/mass-independent-v3/features.json"),
        include_str!("data/mass-independent-v3/sources.json"),
    ]
    .into_iter()
    .map(|text| serde_json::from_str(text).unwrap())
    .collect()
}

fn load(documents: &[Value], raw: Option<&[u8]>) -> Result<MassInput, MassInputError> {
    let encoded: Vec<_> = documents
        .iter()
        .map(|value| serde_json::to_vec(value).unwrap())
        .collect();
    let mut files: Vec<_> = PATHS
        .into_iter()
        .zip(encoded.iter().map(Vec::as_slice))
        .collect();
    if let Some(bytes) = raw {
        files.push(("raw/CS Isotopes Table.txt", bytes));
    }
    decode(&files)
}

fn decimal(source: &str) -> Value {
    let number: f64 = source.parse().unwrap();
    let hex = number
        .to_le_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    json!({"source_decimal": source, "reference_f64": number, "reference_f64_bits_le": hex})
}

#[test]
fn complete_input_retains_source_tokens_units_order_and_empty_isotope_block() {
    let loaded = load(&documents(), Some(RAW)).unwrap();
    assert_eq!(loaded.elements.len(), 2);
    assert_eq!(loaded.elements[0].natural_atomic_weight.source, "7.2500");
    assert_eq!(loaded.elements[1].isotope_rows, 0);
    assert_eq!(loaded.isotopes[1].abundance_percent.source, "50.8751");
    assert_eq!(loaded.selections[0].selected_isotope_row_ordinal, 1);
    assert_eq!(loaded.signed_charge_mass.unit, "fixture-charge-unit");
    assert_eq!(
        loaded.signed_charge_mass.binary64_bits,
        (-0.25_f64).to_bits()
    );
    assert_eq!(loaded.aliases[0].symbol, "fixture-alias");
}

#[test]
fn every_declared_source_span_has_nonzero_length_and_stays_in_its_parent() {
    let mut results = Vec::new();
    let mut coefficient = documents();
    coefficient[6]["signed_charge_coefficient"]["bytes"] = 0.into();
    results.push(load(&coefficient, None).err());
    let mut overflow = documents();
    overflow[6]["signed_charge_coefficient"]["source_file_offset"] = u64::MAX.into();
    results.push(load(&overflow, None).err());
    let mut empty_line = documents();
    empty_line[6]["row_source_spans"][0]["source_bytes_including_newline"] = 0.into();
    results.push(load(&empty_line, None).err());
    let mut out_of_range = documents();
    out_of_range[6]["row_source_spans"][0]["row_ordinal"] = 900.into();
    results.push(load(&out_of_range, None).err());
    let mut duplicate_line = documents();
    duplicate_line[6]["row_source_spans"][0]["source_line_1based"] = 3.into();
    results.push(load(&duplicate_line, None).err());
    let mut overlap = documents();
    overlap[6]["row_source_spans"][0]["source_file_offset"] = 1.into();
    results.push(load(&overlap, None).err());
    assert_eq!(results, [Some(MassInputError::Inconsistent); 6]);
}

#[test]
fn exact_ranges_do_not_round_negative_or_above_hundred_into_valid_values() {
    let mut tiny = documents();
    tiny[0]["elements"][0]["natural_atomic_weight"] = decimal("1e-99999999999999999999");
    assert!(load(&tiny, None).is_ok());
    for invalid in ["0", "-1e-99999999999999999999", "-0.5"] {
        let mut input = documents();
        input[0]["elements"][0]["natural_atomic_weight"] = decimal(invalid);
        assert_eq!(load(&input, None).err(), Some(MassInputError::Inconsistent));
    }
    for invalid in [
        "-1e-99999999999999999999",
        "-0.1",
        "100.00000000000000000001",
        "101",
    ] {
        let mut input = documents();
        input[1]["isotopes"][0]["abundance_percent"] = decimal(invalid);
        if !invalid.starts_with('-') {
            // Keep the static selection correct for an above-range first isotope, so the
            // range check itself must reject the input.
            input[2]["rows"][0]["selected_isotope_row_ordinal"] = 0.into();
            input[2]["rows"][0]["mass_number_u32"] = 7.into();
            input[2]["rows"][0]["mass"] = decimal("7.125");
        }
        assert_eq!(load(&input, None).err(), Some(MassInputError::Inconsistent));
    }
    let mut zero = documents();
    zero[1]["isotopes"][0]["abundance_percent"] = decimal("-0.0");
    assert!(load(&zero, None).is_ok());
}

#[test]
fn duplicate_keys_conflicting_names_and_wrong_selection_are_rejected() {
    let mut duplicate = documents();
    duplicate[1]["isotopes"][1]["mass_number_u32"] = 7.into();
    duplicate[1]["isotopes"][1]["mass"] = decimal("7.25");
    duplicate[2]["rows"][0]["mass_number_u32"] = 7.into();
    duplicate[2]["rows"][0]["mass"] = decimal("7.25");
    let mut wrong_alias = documents();
    wrong_alias[3]["aliases"][0]["symbol"] = "J".into();
    let mut blank_symbol = documents();
    blank_symbol[0]["elements"][0]["symbol"] = "  ".into();
    let mut blank_alias = documents();
    blank_alias[3]["aliases"][0]["symbol"] = "".into();
    let mut selection = documents();
    selection[2]["rows"][0]["selected_isotope_row_ordinal"] = 0.into();
    selection[2]["rows"][0]["mass_number_u32"] = 7.into();
    selection[2]["rows"][0]["mass"] = decimal("7.125");
    for invalid in [
        &duplicate,
        &wrong_alias,
        &blank_symbol,
        &blank_alias,
        &selection,
    ] {
        assert_eq!(
            load(invalid, None).err(),
            Some(MassInputError::Inconsistent)
        );
    }
}

#[test]
fn features_must_agree_with_typed_key_order_and_nonempty_units() {
    for (field, replacement) in [
        ("element_key_type", "i32"),
        ("isotope_key_type", "u64"),
        ("row_order", "sorted"),
        ("schema_id", ""),
        ("abundance_unit", " "),
    ] {
        let mut input = documents();
        input[5][field] = replacement.into();
        assert_eq!(
            load(&input, None).err(),
            Some(MassInputError::Inconsistent),
            "{field}"
        );
    }
}

#[test]
fn exact_zero_metadata_rejects_nonzero_underflow_without_changing_signed_zero() {
    let docs = documents();
    let mut encoded: Vec<_> = docs
        .iter()
        .map(|value| serde_json::to_vec(value).unwrap())
        .collect();
    let feature = String::from_utf8(encoded[5].clone()).unwrap();
    for (number, accepted) in [
        ("1e-99999", false),
        ("-1e-99999", false),
        ("-0.0", true),
        ("0e99", true),
    ] {
        encoded[5] = feature
            .replacen("\"value\":0", &format!("\"value\":{number}"), 1)
            .into_bytes();
        let files: Vec<_> = PATHS
            .into_iter()
            .zip(encoded.iter().map(Vec::as_slice))
            .collect();
        assert_eq!(decode(&files).is_ok(), accepted, "{number}");
    }
}

#[test]
fn supplied_raw_text_and_lines_are_checked_without_executing_or_parsing_them() {
    let mut input = documents();
    input[6]["row_source_spans"][0]["source_line_sha256"] = "0".repeat(64).into();
    assert!(load(&input, None).is_ok());
    assert_eq!(
        load(&input, Some(RAW)).err(),
        Some(MassInputError::Inconsistent)
    );
    let mut changed = RAW.to_vec();
    changed[0] ^= 1;
    assert_eq!(
        load(&documents(), Some(&changed)).err(),
        Some(MassInputError::Inconsistent)
    );
}

#[test]
fn catalog_labels_alone_do_not_authorize_an_invented_file_set() {
    assert_eq!(
        verify_catalog(
            "chemdraw16-personal-local-atomic-isotope-masses-phase6",
            "sha256-set-49728f247c1c84f58cbf3532af3a96406e759221043f576817bae51677202440",
            b"independent-manifest",
            b"independent-registry",
            b"independent-profile"
        ),
        Err(MassInputError::CatalogMismatch)
    );
}

#[test]
fn selection_uses_the_first_row_on_ties_and_when_all_abundances_are_zero() {
    for abundance in ["0", "25"] {
        let mut input = documents();
        input[1]["isotopes"][0]["abundance_percent"] = decimal(abundance);
        input[1]["isotopes"][1]["abundance_percent"] = decimal(abundance);
        input[2]["rows"][0]["selected_isotope_row_ordinal"] = 0.into();
        input[2]["rows"][0]["mass_number_u32"] = 7.into();
        input[2]["rows"][0]["mass"] = decimal("7.125");
        input[2]["rows"][0]["all_abundances_zero"] = (abundance == "0").into();
        assert!(load(&input, None).is_ok());
        input[2]["rows"][0]["selected_isotope_row_ordinal"] = 1.into();
        input[2]["rows"][0]["mass_number_u32"] = 8.into();
        input[2]["rows"][0]["mass"] = decimal("8.25");
        assert_eq!(load(&input, None).err(), Some(MassInputError::Inconsistent));
    }
}

#[test]
fn the_same_mass_number_is_allowed_for_different_elements() {
    let mut input = documents();
    input[0]["elements"][1]["isotope_rows"] = 1.into();
    input[1]["isotopes"].as_array_mut().unwrap().push(json!({
        "isotope_row_ordinal":2,"element_row_ordinal":1,"atomic_number_u32":47,
        "mass_number_u32":7,"mass":decimal("7.25"),"abundance_percent":decimal("0"),
        "abundance_token_present":false,"nickname":null
    }));
    input[2]["rows"].as_array_mut().unwrap().push(json!({
        "element_row_ordinal":1,"atomic_number_u32":47,"selected_isotope_row_ordinal":2,
        "mass_number_u32":7,"mass":decimal("7.25"),"all_abundances_zero":true
    }));
    input[5]["isotope_rows"] = 3.into();
    assert!(load(&input, None).is_ok());
    input[2]["rows"].as_array_mut().unwrap().reverse();
    assert_eq!(load(&input, None).err(), Some(MassInputError::Inconsistent));
}

#[test]
fn source_decimal_and_reference_bits_must_agree_even_for_zero_sign() {
    let mut input = documents();
    input[1]["isotopes"][0]["abundance_percent"] = decimal("-0.0");
    input[1]["isotopes"][0]["abundance_percent"]["reference_f64_bits_le"] =
        "0000000000000000".into();
    assert_eq!(load(&input, None).err(), Some(MassInputError::InvalidShape));
    let mut missing = documents();
    missing[1]["isotopes"][1]
        .as_object_mut()
        .unwrap()
        .remove("nickname");
    assert_eq!(
        load(&missing, None).err(),
        Some(MassInputError::InvalidShape)
    );
}

#[test]
fn source_line_numbers_cannot_claim_impossible_byte_offsets() {
    let mut input = documents();
    let mut first = input[6]["row_source_spans"][3].clone();
    first["source_file_offset"] = 1.into();
    first["source_bytes_including_newline"] = 1.into();
    input[6]["row_source_spans"] = json!([first.clone()]);
    let first_result = load(&input, None).err();
    first["source_line_1based"] = 100.into();
    input[6]["row_source_spans"] = json!([first]);
    assert_eq!(
        [first_result, load(&input, None).err()],
        [Some(MassInputError::Inconsistent); 2]
    );
}
