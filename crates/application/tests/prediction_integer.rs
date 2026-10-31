use chemifree_application::{prediction_input, prediction_wire};

use prediction_input::{PredictionInputError, PredictionInputs, decode};

// Independent fixture shapes; none of these numbers is a scientific observation.
const PATHS: [&str; 11] = [
    "parameters/atomic-polarizability.json",
    "parameters/pi-charges.json",
    "parameters/sigma-charges.json",
    "parameters/residual-electronegativity.json",
    "parameters/xlogp.json",
    "parameters/pka.json",
    "parameters/logs.json",
    "parameters/accessibility.json",
    "parameters/calculator-controls.json",
    "feature-schema.json",
    "source-bounds.json",
];

fn documents() -> [String; 11] {
    [
        include_str!("data/prediction-independent/polarizability.json"),
        include_str!("data/prediction-independent/pi.json"),
        include_str!("data/prediction-independent/sigma.json"),
        include_str!("data/prediction-independent/residual.json"),
        include_str!("data/prediction-independent/xlogp.json"),
        include_str!("data/prediction-independent/pka.json"),
        include_str!("data/prediction-independent/logs.json"),
        include_str!("data/prediction-independent/accessibility.json"),
        include_str!("data/prediction-independent/controls.json"),
        include_str!("data/prediction-independent/features.json"),
        include_str!("data/prediction-independent/sources.json"),
    ]
    .map(str::to_owned)
}

fn load(input: &[String; 11]) -> Result<PredictionInputs, PredictionInputError> {
    let files: Vec<_> = PATHS
        .into_iter()
        .zip(input.iter().map(|text| text.as_bytes()))
        .collect();
    decode(&files)
}

#[test]
fn exact_integer_spellings_work_across_nested_tables_and_fixed_arrays() {
    let mut input = documents();
    input[0] = input[0]
        .replace("\"key_u64\":0", "\"key_u64\":18446744073709551615.0")
        .replace("\"row_ordinal\":0", "\"row_ordinal\":-0.0")
        .replace(
            "\"value\":0.5,\"bits_le\":\"000000000000e03f\"",
            "\"value\":-0,\"bits_le\":\"0000000000000080\"",
        );
    input[1] = input[1]
        .replace("\"key_u64\":0", "\"key_u64\":0e0")
        .replace("\"row_ordinal\":0", "\"row_ordinal\":0.0");
    input[2] = input[2]
        .replace(
            "\"shape_per_element\":[3,4]",
            "\"shape_per_element\":[3.0,4e0]",
        )
        .replace("\"key_u64\":0", "\"key_u64\":0.0")
        .replace("\"atomic_number_u32\":1", "\"atomic_number_u32\":1e0")
        .replace("\"row_ordinal\":0", "\"row_ordinal\":0e0")
        .replace(
            "\"orbital_parameter_mode\":0",
            "\"orbital_parameter_mode\":-2147483648.0",
        )
        .replace("\"iteration_count\":0", "\"iteration_count\":0.0")
        .replace("\"iteration_start\":0", "\"iteration_start\":0e0");
    input[3] = input[3]
        .replace("\"environment_key_i32\":0", "\"environment_key_i32\":-0.0")
        .replace("\"row_ordinal\":0", "\"row_ordinal\":0e0");
    input[4] = input[4]
        .replacen("\"index\":0", "\"index\":0.0", 1)
        .replacen("\"row_ordinal\":0", "\"row_ordinal\":0e0", 1);
    input[5] = input[5]
        .replacen("\"feature_count\":6", "\"feature_count\":6.0", 1)
        .replacen("\"index\":0", "\"index\":0e0", 1);
    input[6] = input[6]
        .replace("\"feature_count\":11", "\"feature_count\":11e0")
        .replacen("\"index\":0", "\"index\":0.0", 1);
    input[7] = input[7]
        .replacen("\"atomic_number_u32\":0", "\"atomic_number_u32\":-0.0", 1)
        .replacen("\"graph_distance_u32\":0", "\"graph_distance_u32\":0e0", 1);
    input[8] = input[8]
        .replace(
            "\"sphere_default_flavor_i32\":0",
            "\"sphere_default_flavor_i32\":-2147483648.0",
        )
        .replace(
            "\"orbital_parameter_mode\":0",
            "\"orbital_parameter_mode\":-2147483648.0",
        )
        .replace("\"iteration_count\":0", "\"iteration_count\":0.0")
        .replace("\"iteration_start\":0", "\"iteration_start\":0e0");
    input[9] = input[9]
        .replace("\"version\":1", "\"version\":1.0")
        .replace("\"row_count\":105", "\"row_count\":105e0")
        .replace(
            "\"peoe_element_tensor_shape\":[1,3,4]",
            "\"peoe_element_tensor_shape\":[1e0,3.0,4e0]",
        );
    input[10] = input[10]
        .replace("\"bytes\":128", "\"bytes\":128.0")
        .replace(
            "\"source_file_offset_start\":0",
            "\"source_file_offset_start\":0e0",
        )
        .replace(
            "\"source_file_offset_end_exclusive\":1",
            "\"source_file_offset_end_exclusive\":1.0",
        );
    for (index, token) in [
        (0, "\"key_u64\":18446744073709551615.0"),
        (2, "\"shape_per_element\":[3.0,4e0]"),
        (2, "\"atomic_number_u32\":1e0"),
        (2, "\"orbital_parameter_mode\":-2147483648.0"),
        (5, "\"feature_count\":6.0"),
        (8, "\"sphere_default_flavor_i32\":-2147483648.0"),
        (9, "\"peoe_element_tensor_shape\":[1e0,3.0,4e0]"),
        (10, "\"bytes\":128.0"),
    ] {
        assert!(
            input[index].contains(token),
            "missing fixture token {token}"
        );
    }
    let data = load(&input).unwrap();
    assert_eq!(data.polarizability.table.rows[0].key_u64, u64::MAX);
    assert_eq!(
        data.polarizability.table.rows[0].coefficient_f64.bits,
        (-0.0_f64).to_bits()
    );
    assert_eq!(data.controls.sphere_default_flavor_i32, i32::MIN);
    assert_eq!(data.features.peoe_element_tensor_shape, [1, 3, 4]);
    let mut fraction = input.clone();
    fraction[5] = fraction[5].replacen(
        "\"feature_count\":6.0",
        "\"feature_count\":6.0000000000000000001",
        1,
    );
    assert_eq!(load(&fraction).err(), Some(PredictionInputError::Shape));
    let mut overflow = input;
    overflow[0] = overflow[0].replace("18446744073709551615.0", "18446744073709551616.0");
    assert_eq!(load(&overflow).err(), Some(PredictionInputError::Shape));
}

#[test]
fn fixed_shapes_and_optional_source_integers_remain_strict() {
    let source = documents();
    for shape in ["[1,3]", "[1,3,4,5]", "[1,3.0001,4]"] {
        let mut input = source.clone();
        input[9] = input[9].replace(
            "\"peoe_element_tensor_shape\":[1,3,4]",
            &format!("\"peoe_element_tensor_shape\":{shape}"),
        );
        assert!(input[9].contains(shape));
        assert_eq!(load(&input).err(), Some(PredictionInputError::Shape));
    }
    let mut null_index = source.clone();
    null_index[10] = null_index[10].replace(
        "\"regression_cell_bounds\":{}",
        &format!("\"regression_cell_bounds\":{{\"test\":[{{\"index\":null,\"source_file_offset\":0,\"bytes\":1,\"sha256\":\"{}\"}}]}}", "0".repeat(64)),
    );
    assert!(null_index[10].contains("\"index\":null"));
    assert_eq!(load(&null_index).err(), Some(PredictionInputError::Shape));

    let mut duplicate = source;
    duplicate[9] = duplicate[9].replace("\"version\":1", "\"version\":1,\"ver\\u0073ion\":1");
    assert!(duplicate[9].contains("\\u0073"));
    assert_eq!(load(&duplicate).err(), Some(PredictionInputError::Shape));
}

#[test]
fn large_inert_metadata_and_spans_are_preserved_and_compared_exactly() {
    let maximum = format!("1{}", "0".repeat(400));
    let half = format!("1{}", "0".repeat(200));
    let offset = "9".repeat(399);
    let end = format!("1{}", "0".repeat(399));
    let digest = "0".repeat(64);
    let mut input = documents();
    input[0] = input[0].replace("\"row_ordinal\":0", &format!("\"row_ordinal\":{offset}"));
    input[4] = input[4]
        .replacen("\"index\":0", &format!("\"index\":{offset}"), 1)
        .replacen("\"row_ordinal\":0", &format!("\"row_ordinal\":{offset}"), 1);
    input[9] = input[9]
        .replace("\"version\":1", &format!("\"version\":{maximum}"))
        .replace("\"row_count\":105", &format!("\"row_count\":{maximum}"))
        .replace(
            "\"atom_type\":{\"start\":0,\"end_inclusive\":89}",
            &format!("\"atom_type\":{{\"start\":0,\"end_inclusive\":{maximum}}}"),
        );
    let table_bounds = format!(
        r#"{{"byte-table":{{"source_file_offset_start":0,"bytes":{maximum},"sha256":"{digest}","row_width_bytes":{half},"row_count":{half},"binding_scope":"fixture"}},"fn-table":{{"evidence_file":"fixture","evidence_sha256":"{digest}","source_file_offset_start":{offset},"source_file_offset_end_exclusive":{end},"binding_scope":"fixture","row_count":{maximum},"zero_shift_default_source_file_offset":{offset},"literal_argument_envelopes":[{{"row_ordinal":{offset},"source_file_offset_start":{offset},"source_file_offset_end_exclusive":{end}}}]}}}}"#
    );
    input[10] = input[10]
        .replace("\"bytes\":128", &format!("\"bytes\":{maximum}"))
        .replace(
            "\"table_bounds\":{}",
            &format!("\"table_bounds\":{table_bounds}"),
        );
    for token in [
        format!("\"version\":{maximum}"),
        format!("\"row_count\":{maximum}"),
        format!("\"end_inclusive\":{maximum}"),
    ] {
        assert!(input[9].contains(&token));
    }
    assert!(input[10].contains("\"byte-table\""));
    assert!(input[10].contains("\"fn-table\""));
    let data = load(&input).unwrap();
    assert_eq!(data.features.version.to_string(), maximum);
    assert_eq!(data.features.row_count.to_string(), maximum);
    assert_eq!(
        data.polarizability.table.rows[0].row_ordinal.to_string(),
        offset
    );
    assert_eq!(data.xlogp.table.rows[0].index.to_string(), offset);
    assert_eq!(data.sources.source.bytes.to_string(), maximum);
    let prediction_wire::TableBounds::Function(span) = &data.sources.table_bounds["fn-table"]
    else {
        panic!("wrong source variant")
    };
    assert_eq!(span.source_file_offset_start.to_string(), offset);
    assert_eq!(
        span.literal_argument_envelopes.as_ref().unwrap()[0]
            .row_ordinal
            .to_string(),
        offset
    );

    let mut product_overflow = input.clone();
    product_overflow[10] = product_overflow[10].replace(
        &format!("\"row_width_bytes\":{half}"),
        &format!("\"row_width_bytes\":{half}1"),
    );
    assert_eq!(
        load(&product_overflow).err(),
        Some(PredictionInputError::Inconsistent)
    );
    let mut outside = input.clone();
    outside[10] = outside[10].replace(
        &format!("\"zero_shift_default_source_file_offset\":{offset}"),
        &format!("\"zero_shift_default_source_file_offset\":{end}"),
    );
    assert_eq!(
        load(&outside).err(),
        Some(PredictionInputError::Inconsistent)
    );
    let mut duplicate = input.clone();
    duplicate[4] = duplicate[4].replacen("\"index\":1,", &format!("\"index\":{offset}.0,"), 1);
    assert_eq!(
        load(&duplicate).err(),
        Some(PredictionInputError::Inconsistent)
    );
    let mut overlong = input;
    overlong[9] = overlong[9].replace(
        &format!("\"version\":{maximum}"),
        &format!("\"version\":{}", "9".repeat(4097)),
    );
    assert_eq!(load(&overlong).err(), Some(PredictionInputError::Shape));
}
