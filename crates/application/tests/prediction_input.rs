use chemifree_application::{prediction_input, prediction_wire};
use serde_json::{Value, json as json_value};

use prediction_input::{PredictionInputError, decode};

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

fn documents() -> Vec<Value> {
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
    .into_iter()
    .map(|text| serde_json::from_str(text).unwrap())
    .collect()
}

fn load(values: &[Value]) -> Result<prediction_input::PredictionInputs, PredictionInputError> {
    let bytes: Vec<_> = values
        .iter()
        .map(|value| serde_json::to_vec(value).unwrap())
        .collect();
    let files: Vec<_> = PATHS
        .into_iter()
        .zip(bytes.iter().map(Vec::as_slice))
        .collect();
    decode(&files)
}

#[test]
fn distinct_typed_vectors_and_separate_intercept_are_retained() {
    let data = load(&documents()).unwrap();
    assert_eq!(
        data.pka.models[0].rows[0].feature_id,
        "fictional-pka-feature"
    );
    assert_eq!(data.logs.model.rows[0].feature_id, "fictional-logs-feature");
    assert_eq!(data.logs.model.intercept_f64.bits, 0.0_f64.to_bits());
    assert_eq!(
        data.polarizability.table.rows[0].coefficient_f64.bits,
        0.5_f64.to_bits()
    );
    assert_eq!(data.features.peoe_element_tensor_shape, [1, 3, 4]);
}

#[test]
fn regression_rows_match_explicit_indices_and_feature_order() {
    let mut invalid = documents();
    invalid[5]["models"][0]["rows"][0]["index"] = 1.into();
    assert_eq!(
        load(&invalid).err(),
        Some(PredictionInputError::Inconsistent)
    );
    let mut invalid = documents();
    invalid[9]["regression_feature_orders"]["fictional-logs"][0] = "different".into();
    assert_eq!(
        load(&invalid).err(),
        Some(PredictionInputError::Inconsistent)
    );
    let mut invalid = documents();
    invalid[5]["models"][0]["feature_count"] = 2.into();
    assert_eq!(
        load(&invalid).err(),
        Some(PredictionInputError::Inconsistent)
    );
}

#[test]
fn declared_table_ids_refer_to_loaded_table_records() {
    let mut values = documents();
    values[9]["table_ids"] = json_value!(["fixture-polar"]);
    assert!(load(&values).is_ok());
    values[9]["table_ids"] = json_value!(["not-present"]);
    assert_eq!(
        load(&values).err(),
        Some(PredictionInputError::Inconsistent)
    );
}

#[test]
fn duplicate_loaded_table_ids_are_rejected_even_when_referenced_once() {
    let mut values = documents();
    values[3]["table"]["table_id"] = "fixture-polar".into();
    values[9]["table_ids"] = json_value!([
        "fixture-polar",
        "fixture-pi",
        "fixture-sigma",
        "fixture-cubic",
        "fixture-xlogp"
    ]);
    assert_eq!(
        load(&values).err(),
        Some(PredictionInputError::Inconsistent)
    );
}

#[test]
fn a_source_row_can_retain_multiple_distinct_literal_envelopes() {
    let mut values = documents();
    values[10]["table_bounds"]["literal-table"] = json_value!({
        "evidence_file": "fixture", "evidence_sha256": "0".repeat(64),
        "source_file_offset_start": 0, "source_file_offset_end_exclusive": 16,
        "binding_scope": "fixture", "row_count": 2,
        "literal_argument_envelopes": [
            {"row_ordinal": 1, "source_file_offset_start": 2, "source_file_offset_end_exclusive": 4},
            {"row_ordinal": 1, "source_file_offset_start": 6, "source_file_offset_end_exclusive": 8}
        ]
    });
    let loaded = load(&values).unwrap();
    let prediction_wire::TableBounds::Function(source) =
        &loaded.sources.table_bounds["literal-table"]
    else {
        panic!("expected a function envelope")
    };
    let spans = source.literal_argument_envelopes.as_ref().unwrap();
    assert_eq!(spans.len(), 2);
    assert_eq!(
        (spans[0].row_ordinal, spans[0].source_file_offset_start),
        (1, 2)
    );
    assert_eq!(
        (spans[1].row_ordinal, spans[1].source_file_offset_start),
        (1, 6)
    );
}

#[test]
fn missing_required_prediction_operands_are_rejected() {
    let values = documents();
    assert!(load(&values).is_ok());

    let mut pka = values.clone();
    pka[5]["models"][0]["rows"].as_array_mut().unwrap().pop();
    pka[5]["models"][0]["feature_count"] = 5.into();
    pka[9]["regression_feature_orders"]["fictional-pka"]
        .as_array_mut()
        .unwrap()
        .pop();
    assert_eq!(load(&pka).err(), Some(PredictionInputError::Inconsistent));

    let mut logs = values.clone();
    logs[6]["model"]["rows"].as_array_mut().unwrap().pop();
    logs[6]["model"]["feature_count"] = 10.into();
    logs[9]["regression_feature_orders"]["fictional-logs"]
        .as_array_mut()
        .unwrap()
        .pop();
    assert_eq!(load(&logs).err(), Some(PredictionInputError::Inconsistent));

    let mut nonzero_intercept = values.clone();
    nonzero_intercept[6]["model"]["intercept_f64"] =
        json_value!({"value":0.5,"bits_le":"000000000000e03f"});
    assert_eq!(
        load(&nonzero_intercept).err(),
        Some(PredictionInputError::Inconsistent)
    );

    let mut xlogp = values.clone();
    xlogp[4]["table"]["rows"].as_array_mut().unwrap().pop();
    assert_eq!(load(&xlogp).err(), Some(PredictionInputError::Inconsistent));

    let mut distance = values.clone();
    distance[7]["distance_denominators"]
        .as_array_mut()
        .unwrap()
        .remove(3);
    assert_eq!(
        load(&distance).err(),
        Some(PredictionInputError::Inconsistent)
    );

    let mut tensor = values;
    tensor[2]["tables"].as_array_mut().unwrap().pop();
    tensor[9]["table_ids"] = json_value!([
        "fixture-polar",
        "fixture-pi",
        "fixture-sigma",
        "fixture-residual",
        "fixture-xlogp"
    ]);
    assert_eq!(
        load(&tensor).err(),
        Some(PredictionInputError::Inconsistent)
    );
}

#[test]
fn pka_family_shapes_cannot_be_repartitioned_while_preserving_the_total() {
    let mut values = documents();
    let mut extra = values[5]["models"][0]["rows"][0].clone();
    extra["index"] = 6.into();
    extra["feature_id"] = "fictional-extra".into();
    values[5]["models"][0]["rows"]
        .as_array_mut()
        .unwrap()
        .push(extra);
    values[5]["models"][0]["feature_count"] = 7.into();
    values[9]["regression_feature_orders"]["fictional-pka"]
        .as_array_mut()
        .unwrap()
        .push("fictional-extra".into());
    values[5]["models"][1]["rows"].as_array_mut().unwrap().pop();
    values[5]["models"][1]["feature_count"] = 5.into();
    values[9]["regression_feature_orders"]["fictional-pka-1"]
        .as_array_mut()
        .unwrap()
        .pop();
    assert_eq!(
        load(&values).err(),
        Some(PredictionInputError::Inconsistent)
    );
}

#[test]
fn duplicated_keys_defaults_and_numeric_witnesses_are_rejected() {
    let mut invalid = documents();
    let duplicate = invalid[0]["table"]["rows"][0].clone();
    invalid[0]["table"]["rows"]
        .as_array_mut()
        .unwrap()
        .push(duplicate);
    assert_eq!(
        load(&invalid).err(),
        Some(PredictionInputError::Inconsistent)
    );
    let mut invalid = documents();
    invalid[8]["peoe_default_numeric_controls"]["iteration_count"] = 2.into();
    assert_eq!(
        load(&invalid).err(),
        Some(PredictionInputError::Inconsistent)
    );
    let mut invalid = documents();
    invalid[6]["model"]["intercept_f64"]["bits_le"] = "000000000000e03f".into();
    assert_eq!(load(&invalid).err(), Some(PredictionInputError::Shape));
}

#[test]
fn inert_source_spans_cannot_be_empty_or_outside_the_declared_source() {
    let mut invalid = documents();
    invalid[10]["regression_cell_bounds"]["fixture"] =
        json_value!([{"index":0,"source_file_offset":1,"bytes":0,"sha256":"0".repeat(64)}]);
    assert_eq!(
        load(&invalid).err(),
        Some(PredictionInputError::Inconsistent)
    );
    let mut invalid = documents();
    invalid[10]["accessibility"]["constructor"]["source_file_offset_end_exclusive"] = 129.into();
    assert_eq!(
        load(&invalid).err(),
        Some(PredictionInputError::Inconsistent)
    );
    let mut invalid = documents();
    invalid[10]["original_code_or_control_flow_included"] = true.into();
    assert_eq!(
        load(&invalid).err(),
        Some(PredictionInputError::Inconsistent)
    );
}

#[test]
fn source_table_counts_and_literal_row_indices_fit_their_declared_bounds() {
    let mut values = documents();
    values[10]["table_bounds"]["byte-table"] = json_value!({
        "source_file_offset_start": 4, "bytes": 16, "sha256": "0".repeat(64),
        "row_width_bytes": 8, "row_count": 2, "binding_scope": "fixture"
    });
    values[10]["table_bounds"]["literal-table"] = json_value!({
        "evidence_file": "fixture", "evidence_sha256": "0".repeat(64),
        "source_file_offset_start": 0, "source_file_offset_end_exclusive": 16,
        "binding_scope": "fixture", "row_count": 2,
        "literal_argument_envelopes": [{"row_ordinal": 1,
            "source_file_offset_start": 2, "source_file_offset_end_exclusive": 4}]
    });
    assert!(load(&values).is_ok());

    let mut short = values.clone();
    short[10]["table_bounds"]["byte-table"]["bytes"] = 15.into();
    assert_eq!(load(&short).err(), Some(PredictionInputError::Inconsistent));

    let mut overflow = values.clone();
    overflow[10]["table_bounds"]["byte-table"]["row_width_bytes"] = u64::MAX.into();
    assert_eq!(
        load(&overflow).err(),
        Some(PredictionInputError::Inconsistent)
    );

    let mut outside = values;
    outside[10]["table_bounds"]["literal-table"]["literal_argument_envelopes"][0]["row_ordinal"] =
        2.into();
    assert_eq!(
        load(&outside).err(),
        Some(PredictionInputError::Inconsistent)
    );
}

#[test]
fn optional_nonnullable_fields_do_not_accept_null() {
    let mut invalid = documents();
    invalid[10]["regression_cell_bounds"]["fixture"] =
        json_value!([{"index":null,"source_file_offset":1,"bytes":1,"sha256":"0".repeat(64)}]);
    assert_eq!(load(&invalid).err(), Some(PredictionInputError::Shape));
}

#[test]
fn catalog_metadata_requires_exact_bytes_and_identity() {
    assert_eq!(
        prediction_input::verify_catalog(
            "chemdraw16-personal-local-prediction-parameters-phase6",
            "sha256-set-c8fc1873da505cd087bb21de16637e94e8e60ff10f96e4d433884fa3f9f86895",
            b"independent manifest",
            b"independent registry",
            b"independent profile"
        ),
        Err(PredictionInputError::CatalogMismatch)
    );
}

#[test]
fn regression_feature_ids_are_unique_even_when_indices_differ() {
    let mut invalid = documents();
    invalid[5]["models"][0]["rows"][1]["feature_id"] = "fictional-pka-feature".into();
    invalid[9]["regression_feature_orders"]["fictional-pka"][1] = "fictional-pka-feature".into();
    assert_eq!(
        load(&invalid).err(),
        Some(PredictionInputError::Inconsistent)
    );
}

#[test]
fn cubic_tensor_shape_and_orbital_rows_keep_distinct_family_contexts() {
    let mut data = documents();
    let mut orbital = data[0]["table"].clone();
    orbital["table_id"] = "fixture-orbital".into();
    let scalar = json_value!({"value":-0.0,"bits_le":"0000000000000080"});
    let cubic = json_value!({
        "key_type":"fixture-key", "coefficient_type":"f64", "shape_per_element":[3,4],
        "coefficient_order":"fixture-order", "table_id":"fixture-cubic", "axis_0":["a","b","c"],
        "axis_1":["0","1","2","3"], "input_unit":"fixture-in", "output_unit":"fixture-out",
        "rows":[{"element":"fixture", "atomic_number_u32":1,"row_ordinal":0,
            "cubic_coefficients":[[scalar.clone(),scalar.clone(),scalar.clone(),scalar.clone()],
                                  [scalar.clone(),scalar.clone(),scalar.clone(),scalar.clone()],
                                  [scalar.clone(),scalar.clone(),scalar.clone(),scalar.clone()]]}]
    });
    data[2]["tables"][1] = cubic;
    data[2]["tables"].as_array_mut().unwrap().insert(1, orbital);
    let decoded = load(&data).unwrap();
    assert_eq!(decoded.sigma.tables.len(), 3);
    let prediction_wire::SigmaTable::ElementCubic(table) = &decoded.sigma.tables[2] else {
        panic!("cubic operands are a distinct variant");
    };
    assert_eq!(
        table.rows[0].cubic_coefficients[2][3].bits,
        (-0.0_f64).to_bits()
    );
    data[2]["tables"][2]["shape_per_element"] = json_value!([4, 3]);
    assert_eq!(load(&data).err(), Some(PredictionInputError::Inconsistent));
    data[2]["tables"][2]["shape_per_element"] = json_value!([3, 4]);
    data[2]["tables"][2]["rows"][0]["cubic_coefficients"][0]
        .as_array_mut()
        .unwrap()
        .pop();
    assert_eq!(load(&data).err(), Some(PredictionInputError::Shape));
}

#[test]
fn accessibility_preserves_raw_radii_and_rejects_used_zero_denominators() {
    let mut raw = documents();
    raw[7]["radii"][0]["radius_f64"] = json_value!({"value":-0.5,"bits_le":"000000000000e0bf"});
    raw[7]["fallback_radius_f64"] = json_value!({"value":-1.0,"bits_le":"000000000000f0bf"});
    let loaded = load(&raw).unwrap();
    assert_eq!(
        loaded.accessibility.radii[0].radius_f64.bits,
        (-0.5_f64).to_bits()
    );
    assert_eq!(
        loaded.accessibility.fallback_radius_f64.bits,
        (-1.0_f64).to_bits()
    );
    let mut used_zero = documents();
    used_zero[7]["distance_denominators"][1]["denominator_f64"] =
        json_value!({"value":0.0,"bits_le":"0000000000000000"});
    assert_eq!(
        load(&used_zero).err(),
        Some(PredictionInputError::Inconsistent)
    );
    let mut unused_sentinel = documents();
    unused_sentinel[7]["radii"][0]["radius_f64"] =
        json_value!({"value":0.0,"bits_le":"0000000000000000"});
    unused_sentinel[7]["distance_denominators"][0]["denominator_f64"] =
        json_value!({"value":0.0,"bits_le":"0000000000000000"});
    assert!(load(&unused_sentinel).is_ok());
}
