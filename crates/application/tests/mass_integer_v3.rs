use chemifree_application::mass_input::{MassInput, MassInputError, decode};

// These are independently authored fixture bytes, not scientific observations.
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

fn documents() -> [String; 7] {
    [
        include_str!("data/mass-independent-v3/elements.json"),
        include_str!("data/mass-independent-v3/isotopes.json"),
        include_str!("data/mass-independent-v3/selection.json"),
        include_str!("data/mass-independent-v3/aliases.json"),
        include_str!("data/mass-independent-v3/constants.json"),
        include_str!("data/mass-independent-v3/features.json"),
        include_str!("data/mass-independent-v3/sources.json"),
    ]
    .map(str::to_owned)
}

fn load(documents: &[String; 7], raw: Option<&[u8]>) -> Result<MassInput, MassInputError> {
    let mut files: Vec<_> = PATHS
        .into_iter()
        .zip(documents.iter().map(|text| text.as_bytes()))
        .collect();
    if let Some(bytes) = raw {
        files.push(("raw/CS Isotopes Table.txt", bytes));
    }
    decode(&files)
}

#[test]
fn exact_decimal_integers_preserve_references_and_optional_raw_verification() {
    let mut input = documents();
    input[0] = input[0]
        .replacen(
            "\"element_row_ordinal\":0",
            "\"element_row_ordinal\":-0.0",
            1,
        )
        .replacen("\"atomic_number_u32\":31", "\"atomic_number_u32\":31e0", 1)
        .replace(
            "\"permitted_valences_u32\":[2,3]",
            "\"permitted_valences_u32\":[2.0,3e0]",
        )
        .replacen(
            "\"first_isotope_row_ordinal\":0",
            "\"first_isotope_row_ordinal\":0.0",
            1,
        )
        .replacen("\"isotope_rows\":2", "\"isotope_rows\":2e0", 1);
    input[1] = input[1]
        .replacen(
            "\"isotope_row_ordinal\":0",
            "\"isotope_row_ordinal\":0e0",
            1,
        )
        .replacen(
            "\"element_row_ordinal\":0",
            "\"element_row_ordinal\":0.0",
            1,
        )
        .replacen("\"atomic_number_u32\":31", "\"atomic_number_u32\":31.0", 1)
        .replacen("\"mass_number_u32\":7", "\"mass_number_u32\":7e0", 1);
    input[2] = input[2]
        .replace("\"element_row_ordinal\":0", "\"element_row_ordinal\":0e0")
        .replace("\"atomic_number_u32\":31", "\"atomic_number_u32\":31.0")
        .replace(
            "\"selected_isotope_row_ordinal\":1",
            "\"selected_isotope_row_ordinal\":1.0",
        )
        .replace("\"mass_number_u32\":8", "\"mass_number_u32\":8e0");
    input[3] = input[3]
        .replace("\"atomic_number_u32\":31", "\"atomic_number_u32\":31e0")
        .replace("\"mass_number_u32\":7", "\"mass_number_u32\":7.0")
        .replace(
            "\"source_isotope_row_ordinal\":0",
            "\"source_isotope_row_ordinal\":0.0",
        );
    input[5] = input[5]
        .replace("\"element_rows\":2", "\"element_rows\":2.0")
        .replace("\"isotope_rows\":2", "\"isotope_rows\":2e0")
        .replace("\"alias_rows\":1", "\"alias_rows\":1.0")
        .replace("\"value\":0", "\"value\":-0");
    input[6] = input[6]
        .replace("\"bytes\":84", "\"bytes\":84.0")
        .replace("\"bytes\":128", "\"bytes\":128e0")
        .replace("\"row_ordinal\":1", "\"row_ordinal\":1.0")
        .replace("\"source_line_1based\":1", "\"source_line_1based\":1e0")
        .replace(
            "\"source_bytes_including_newline\":21",
            "\"source_bytes_including_newline\":21.0",
        )
        .replace("\"source_file_offset\":8", "\"source_file_offset\":8e0");
    for (index, token) in [
        (0, "\"permitted_valences_u32\":[2.0,3e0]"),
        (1, "\"mass_number_u32\":7e0"),
        (2, "\"selected_isotope_row_ordinal\":1.0"),
        (3, "\"source_isotope_row_ordinal\":0.0"),
        (5, "\"isotope_rows\":2e0"),
        (6, "\"source_file_offset\":8e0"),
    ] {
        assert!(input[index].contains(token));
    }
    let data = load(&input, Some(RAW)).unwrap();
    assert_eq!(data.elements[0].permitted_valences_u32, [2, 3]);
    assert_eq!(data.selections[0].selected_isotope_row_ordinal, 1);
    assert_eq!(
        data.sources
            .signed_charge_coefficient
            .source_file_offset
            .to_string(),
        "8"
    );
    assert_eq!(
        data.features
            .native_selected_input_secondary
            .value
            .as_f64()
            .unwrap()
            .to_bits(),
        (-0.0_f64).to_bits()
    );
    let mut fractional = input.clone();
    fractional[0] = fractional[0].replace(
        "\"atomic_number_u32\":31e0",
        "\"atomic_number_u32\":31.00000000000000000001",
    );
    assert_eq!(
        load(&fractional, Some(RAW)).err(),
        Some(MassInputError::InvalidShape)
    );
}

#[test]
fn large_declared_source_ranges_are_exact_and_do_not_claim_file_verification() {
    let maximum = format!("1{}", "0".repeat(400));
    let offset = "9".repeat(399);
    let mut input = documents();
    input[6] = input[6]
        .replace(
            "\"file_name\":\"fictional-consumer\",\"bytes\":128",
            &format!("\"file_name\":\"fictional-consumer\",\"bytes\":{maximum}"),
        )
        .replace(
            "\"signed_charge_coefficient\":{\"source_file_offset\":8",
            &format!("\"signed_charge_coefficient\":{{\"source_file_offset\":{offset}"),
        );
    assert!(input[6].contains(&format!("\"bytes\":{maximum}")));
    assert!(input[6].contains(&format!("\"source_file_offset\":{offset}")));
    let data = load(&input, Some(RAW)).unwrap();
    assert_eq!(data.sources.consumer_source.bytes.to_string(), maximum);
    assert_eq!(
        data.sources
            .signed_charge_coefficient
            .source_file_offset
            .to_string(),
        offset
    );
    let mut outside = input.clone();
    outside[6] = outside[6].replace(
        &format!("\"signed_charge_coefficient\":{{\"source_file_offset\":{offset}"),
        &format!("\"signed_charge_coefficient\":{{\"source_file_offset\":{maximum}"),
    );
    assert_eq!(
        load(&outside, Some(RAW)).err(),
        Some(MassInputError::Inconsistent)
    );
    let mut declared = input.clone();
    declared[6] = declared[6].replace(
        "\"file_name\":\"independent-raw.txt\",\"bytes\":84",
        &format!("\"file_name\":\"independent-raw.txt\",\"bytes\":{maximum}"),
    );
    assert!(load(&declared, None).is_ok());
    assert_eq!(
        load(&declared, Some(RAW)).err(),
        Some(MassInputError::Inconsistent)
    );
    let mut overlong = input;
    overlong[6] = overlong[6].replace(
        &format!("\"bytes\":{maximum}"),
        &format!("\"bytes\":{}", "9".repeat(4097)),
    );
    assert_eq!(
        load(&overlong, Some(RAW)).err(),
        Some(MassInputError::InvalidShape)
    );
}

#[test]
fn large_declared_line_positions_keep_order_and_physical_lower_bounds() {
    let maximum = format!("1{}", "0".repeat(400));
    let line = "9".repeat(399);
    let too_early = format!("{}7", "9".repeat(398));
    let mut input = documents();
    input[6] = input[6]
        .replace(
            "\"file_name\":\"independent-raw.txt\",\"bytes\":84",
            &format!("\"file_name\":\"independent-raw.txt\",\"bytes\":{maximum}"),
        )
        .replacen(
            "\"source_line_1based\":4",
            &format!("\"source_line_1based\":{line}"),
            1,
        )
        .replacen(
            "\"source_file_offset\":63",
            &format!("\"source_file_offset\":{line}"),
            1,
        );
    assert!(input[6].contains(&format!("\"source_line_1based\":{line}")));
    assert!(input[6].contains(&format!("\"source_file_offset\":{line}")));
    let data = load(&input, None).unwrap();
    assert_eq!(
        data.sources.row_source_spans[0]
            .source_line_1based
            .to_string(),
        line
    );
    assert_eq!(
        data.sources.row_source_spans[0]
            .source_file_offset
            .to_string(),
        line
    );
    let mut invalid = input.clone();
    invalid[6] = invalid[6].replacen(
        &format!("\"source_file_offset\":{line}"),
        &format!("\"source_file_offset\":{too_early}"),
        1,
    );
    assert_eq!(
        load(&invalid, None).err(),
        Some(MassInputError::Inconsistent)
    );
    assert_eq!(
        load(&input, Some(RAW)).err(),
        Some(MassInputError::Inconsistent)
    );
}

#[test]
fn source_line_overlap_is_rejected_even_when_each_span_is_individually_valid() {
    let mut input = documents();
    input[6] = input[6].replacen("\"source_file_offset\":63", "\"source_file_offset\":50", 1);
    assert!(input[6].contains("\"source_file_offset\":50"));
    // The fourth source line remains inside the 84-byte declared file, starts after
    // its minimum possible line offset and overlaps only the preceding line's span.
    assert_eq!(load(&input, None).err(), Some(MassInputError::Inconsistent));
}
