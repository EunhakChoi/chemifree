use chemifree_application::{MAX_DATA_FILE_BYTES, json, phase6_profiles};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[path = "../src/exact_decimal.rs"]
mod exact_decimal;
#[path = "../src/mass_data.rs"]
mod mass_data;

use mass_data::{MassData, MassError, decode_set, verify_catalog};

const PATHS: [&str; 7] = [
    "parameters/elements.json",
    "parameters/isotopes.json",
    "parameters/default-monoisotopic-masses.json",
    "parameters/aliases.json",
    "parameters/mass-constants.json",
    "feature-schema.json",
    "source-bounds.json",
];

fn documents() -> Vec<Value> {
    [
        include_str!("data/mass-redesign/elements.json"),
        include_str!("data/mass-redesign/isotopes.json"),
        include_str!("data/mass-redesign/defaults.json"),
        include_str!("data/mass-redesign/aliases.json"),
        include_str!("data/mass-redesign/constants.json"),
        include_str!("data/mass-redesign/features.json"),
        include_str!("data/mass-redesign/bounds.json"),
    ]
    .into_iter()
    .map(|text| serde_json::from_str(text).unwrap())
    .collect()
}

fn decode(docs: &[Value]) -> Result<MassData, MassError> {
    let bytes: Vec<_> = docs
        .iter()
        .map(|doc| serde_json::to_vec(doc).unwrap())
        .collect();
    let files: Vec<_> = PATHS
        .into_iter()
        .zip(bytes.iter().map(Vec::as_slice))
        .collect();
    decode_set(&files)
}

fn decimal(source: &str) -> Value {
    let number = source.parse::<f64>().unwrap();
    let hex = number
        .to_le_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    json!({"source_decimal": source, "reference_f64": number, "reference_f64_bits_le": hex})
}

#[test]
fn typed_values_keep_order_units_and_static_selection_without_normalizing_sum() {
    let loaded = decode(&documents()).unwrap();
    assert_eq!(
        loaded.elements[0].natural_atomic_weight.source_decimal,
        "14.1250"
    );
    assert_eq!(
        loaded.isotopes[0].abundance_percent.source_decimal,
        "60.0001"
    );
    assert_eq!(
        loaded.isotopes[1].abundance_percent.source_decimal,
        "40.0000"
    );
    assert!(!loaded.isotopes[2].abundance_token_present);
    assert_eq!(loaded.selections[0].selected_isotope_row_ordinal, 0);
    assert_eq!(loaded.charge_coefficient.unit, "fixture-unit");
}

#[test]
fn exact_scientific_ranges_apply_even_when_binary64_loses_the_difference() {
    let mut positive_tiny = documents();
    positive_tiny[0]["elements"][0]["natural_atomic_weight"] = decimal("1e-99999999999999999999");
    assert!(decode(&positive_tiny).is_ok());
    for weight in ["0", "-1e-99999999999999999999", "-1.0"] {
        let mut docs = documents();
        docs[0]["elements"][0]["natural_atomic_weight"] = decimal(weight);
        assert_eq!(
            decode(&docs).err(),
            Some(MassError::Inconsistent),
            "weight {weight}"
        );
    }
    for (first, second) in [
        ("-1", "-2"),
        ("-1e-99999999999999999999", "-2e-99999999999999999999"),
        ("101", "40"),
        ("100.00000000000000000001", "40"),
    ] {
        let mut docs = documents();
        docs[1]["isotopes"][0]["abundance_percent"] = decimal(first);
        docs[1]["isotopes"][1]["abundance_percent"] = decimal(second);
        assert_eq!(
            decode(&docs).err(),
            Some(MassError::Inconsistent),
            "abundance {first}"
        );
    }
    let mut signed_zero = documents();
    signed_zero[1]["isotopes"][2]["abundance_percent"] = decimal("-0.0");
    assert!(decode(&signed_zero).is_ok());
}

#[test]
fn selection_metadata_and_references_are_checked_across_documents() {
    let mut wrong = documents();
    wrong[3]["aliases"][0]["source_isotope_row_ordinal"] = 2.into();
    assert_eq!(decode(&wrong).err(), Some(MassError::Inconsistent));
    let mut wrong = documents();
    wrong[2]["rows"][0]["mass"] = wrong[1]["isotopes"][1]["mass"].clone();
    assert_eq!(decode(&wrong).err(), Some(MassError::Inconsistent));
    let mut wrong = documents();
    wrong[2]["rows"][0]["selected_isotope_row_ordinal"] = 1.into();
    wrong[2]["rows"][0]["mass_number_u32"] = 15.into();
    wrong[2]["rows"][0]["mass"] = wrong[1]["isotopes"][1]["mass"].clone();
    assert_eq!(decode(&wrong).err(), Some(MassError::Inconsistent));
    let mut wrong = documents();
    wrong[5]["native_selected_input_secondary"]["value"] = 1.into();
    assert_eq!(decode(&wrong).err(), Some(MassError::Inconsistent));
    let mut wrong = documents();
    wrong[4]["signed_charge_mass_coefficient"]["value"] = 0.125.into();
    wrong[4]["signed_charge_mass_coefficient"]["bits_le"] = "000000000000c03f".into();
    assert_eq!(decode(&wrong).err(), Some(MassError::Inconsistent));
}

#[test]
fn static_selection_keeps_the_first_tied_row_and_checks_reference_mass_number() {
    let mut docs = documents();
    docs[1]["isotopes"][0]["abundance_percent"] = decimal("20");
    docs[1]["isotopes"][1]["abundance_percent"] = decimal("20.00");
    assert!(decode(&docs).is_ok());
    docs[2]["rows"][0]["selected_isotope_row_ordinal"] = 1.into();
    docs[2]["rows"][0]["mass_number_u32"] = 15.into();
    docs[2]["rows"][0]["mass"] = docs[1]["isotopes"][1]["mass"].clone();
    assert_eq!(decode(&docs).err(), Some(MassError::Inconsistent));

    let mut below = documents();
    below[1]["isotopes"][0]["mass"] = decimal("13.5");
    below[2]["rows"][0]["mass"] = below[1]["isotopes"][0]["mass"].clone();
    assert!(decode(&below).is_ok());
    let mut upper = documents();
    upper[1]["isotopes"][0]["mass"] = decimal("14.5");
    upper[2]["rows"][0]["mass"] = upper[1]["isotopes"][0]["mass"].clone();
    assert_eq!(decode(&upper).err(), Some(MassError::Inconsistent));
}

#[test]
fn isotope_mass_number_is_unique_within_each_element() {
    let mut duplicate = documents();
    duplicate[1]["isotopes"][1]["mass_number_u32"] = 14.into();
    duplicate[1]["isotopes"][1]["mass"] = decimal("14.25");
    assert_eq!(decode(&duplicate).err(), Some(MassError::Inconsistent));

    let mut other_element = documents();
    other_element[1]["isotopes"][2]["mass_number_u32"] = 14.into();
    other_element[1]["isotopes"][2]["mass"] = decimal("14.0");
    other_element[2]["rows"][1]["mass_number_u32"] = 14.into();
    other_element[2]["rows"][1]["mass"] = other_element[1]["isotopes"][2]["mass"].clone();
    assert!(decode(&other_element).is_ok());
}

#[test]
fn names_cannot_point_to_conflicting_element_or_isotope_identities() {
    let mut elements = documents();
    elements[0]["elements"][1]["symbol"] = "X".into();
    let element_result = decode(&elements).err();

    let mut aliases = documents();
    let mut conflicting = aliases[3]["aliases"][0].clone();
    conflicting["mass_number_u32"] = 15.into();
    conflicting["source_isotope_row_ordinal"] = 1.into();
    aliases[3]["aliases"]
        .as_array_mut()
        .unwrap()
        .push(conflicting);
    aliases[5]["alias_rows"] = 2.into();
    let alias_result = decode(&aliases).err();

    let mut collision = documents();
    collision[3]["aliases"][0]["symbol"] = "Y".into();
    let collision_result = decode(&collision).err();

    let mut repeated = documents();
    let same = repeated[3]["aliases"][0].clone();
    repeated[3]["aliases"].as_array_mut().unwrap().push(same);
    repeated[5]["alias_rows"] = 2.into();
    assert_eq!(decode(&repeated).unwrap().aliases.len(), 2);
    assert_eq!(
        [element_result, alias_result, collision_result],
        [Some(MassError::Inconsistent); 3]
    );
}

#[test]
fn source_rows_cannot_reference_missing_or_duplicate_records() {
    let mut docs = documents();
    docs[6]["row_source_spans"][0]["row_ordinal"] = 99.into();
    assert_eq!(decode(&docs).err(), Some(MassError::Inconsistent));
    let mut docs = documents();
    docs[6]["row_source_spans"][0]["kind"] = "unknown".into();
    assert!(decode(&docs).is_err());
    let mut docs = documents();
    let duplicate = docs[6]["row_source_spans"][0].clone();
    docs[6]["row_source_spans"]
        .as_array_mut()
        .unwrap()
        .push(duplicate);
    assert_eq!(decode(&docs).err(), Some(MassError::Inconsistent));
}

#[test]
fn closed_shapes_require_nullable_fields_and_reject_extra_fields() {
    let mut missing = documents();
    missing[1]["isotopes"][2]
        .as_object_mut()
        .unwrap()
        .remove("nickname");
    assert_eq!(decode(&missing).err(), Some(MassError::Record));
    let mut extra = documents();
    extra[0]["elements"][0]["unexpected"] = 1.into();
    assert_eq!(decode(&extra).err(), Some(MassError::Record));
}

#[test]
fn raw_file_and_line_digests_are_checked_only_when_source_is_supplied() {
    let raw = b"row\nentry\nspare\n";
    let mut docs = documents();
    docs[6]["text_source"]["bytes"] = raw.len().into();
    docs[6]["text_source"]["sha256"] = format!("{:x}", Sha256::digest(raw)).into();
    docs[6]["row_source_spans"][0]["source_line_sha256"] =
        format!("{:x}", Sha256::digest(&raw[..4])).into();
    docs[6]["row_source_spans"][1]["source_bytes_including_newline"] = 6.into();
    docs[6]["row_source_spans"][1]["source_line_sha256"] =
        format!("{:x}", Sha256::digest(&raw[4..10])).into();
    let encoded: Vec<_> = docs
        .iter()
        .map(|value| serde_json::to_vec(value).unwrap())
        .collect();
    let mut files: Vec<_> = PATHS
        .into_iter()
        .zip(encoded.iter().map(Vec::as_slice))
        .collect();
    files.push(("raw/CS Isotopes Table.txt", raw));
    assert!(decode_set(&files).is_ok());
    // A changed unreferenced line keeps the per-line witnesses intact.
    files[7].1 = b"row\nentry\nspace\n";
    assert_eq!(decode_set(&files).err(), Some(MassError::Inconsistent));

    let mut bad_line = docs.clone();
    bad_line[6]["row_source_spans"][0]["source_line_sha256"] = "0".repeat(64).into();
    let bad_encoded: Vec<_> = bad_line
        .iter()
        .map(|value| serde_json::to_vec(value).unwrap())
        .collect();
    let mut bad_files: Vec<_> = PATHS
        .into_iter()
        .zip(bad_encoded.iter().map(Vec::as_slice))
        .collect();
    bad_files.push(("raw/CS Isotopes Table.txt", raw));
    assert_eq!(decode_set(&bad_files).err(), Some(MassError::Inconsistent));
}

#[test]
fn catalog_labels_do_not_replace_exact_metadata_bytes() {
    assert_eq!(
        verify_catalog(
            "chemdraw16-personal-local-atomic-isotope-masses-phase6",
            "sha256-set-49728f247c1c84f58cbf3532af3a96406e759221043f576817bae51677202440",
            b"invented manifest",
            b"invented registry",
            b"invented profile"
        ),
        Err(MassError::CatalogMismatch)
    );
}
