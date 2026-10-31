//! Typed wire shapes for inert prediction operands.
//! Generated from the public functional schema, independently of private data.

use num_bigint::BigUint;
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use serde_json::value::RawValue;
use std::collections::BTreeMap;

use crate::json;

// An optional field may be absent, but the public wire schema does not permit null.
fn present_value<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

/// Finite binary64 operand; the declared original little-endian bits are retained.
#[derive(PartialEq)]
pub struct Scalar {
    pub bits: u64,
}

impl<'de> Deserialize<'de> for Scalar {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        let bytes = serde_json::to_vec(&value).map_err(serde::de::Error::custom)?;
        let decoded = crate::phase6_profiles::decode_f64_literal(&bytes)
            .map_err(|_| serde::de::Error::custom("invalid binary64 witness"))?;
        Ok(Self { bits: decoded.bits })
    }
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PolarizabilityPayload {
    pub table: PolarizabilityTable,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PolarizabilityTable {
    pub key_type: String,
    pub coefficient_type: String,
    pub row_order: String,
    pub table_id: String,
    pub unit: String,
    pub rows: Vec<LabelledKeyedF64Row>,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LabelledKeyedF64Row {
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub key_u64: u64,
    pub coefficient_f64: Scalar,
    pub label: String,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub row_ordinal: BigUint,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PiParametersPayload {
    pub tables: Vec<PiParameterTable>,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PiParameterTable {
    pub key_type: String,
    pub coefficient_type: String,
    pub row_order: String,
    pub table_id: String,
    pub unit: String,
    pub rows: Vec<FamilyKeyedF64Row>,
    pub numeric_controls: PiNumericControls,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FamilyKeyedF64Row {
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub key_u64: u64,
    pub coefficient_f64: Scalar,
    pub parameter_family: String,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub row_ordinal: BigUint,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PiNumericControls {
    pub sigma_response: Scalar,
    pub negative_neighbor_response: Scalar,
    pub charge_scale: Scalar,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SigmaParametersPayload {
    pub tables: Vec<SigmaTable>,
    pub default_controls: PeoeNumericControls,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DampingParameterTable {
    pub key_type: String,
    pub coefficient_type: String,
    pub row_order: String,
    pub table_id: String,
    pub unit: String,
    pub rows: Vec<KeyedF64Row>,
    pub atomic_descriptor_shifts: DescriptorShifts,
    pub numeric_controls: PeoeNumericControls,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct KeyedF64Row {
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub key_u64: u64,
    pub coefficient_f64: Scalar,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub row_ordinal: BigUint,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DescriptorShifts {
    pub default_for_atomic_numbers_1_through_102: Scalar,
    pub overrides: BTreeMap<String, Scalar>,
    pub source_storage_type: String,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PeoeNumericControls {
    pub base_damping: Scalar,
    pub charge_scale: Scalar,
    pub hydrogen_hardness: Scalar,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub iteration_count: BigUint,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub iteration_start: BigUint,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub orbital_parameter_mode: i32,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct OrbitalParameterTable {
    pub key_type: String,
    pub coefficient_type: String,
    pub row_order: String,
    pub table_id: String,
    pub unit: String,
    pub rows: Vec<LabelledKeyedF64Row>,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ElementCubicTable {
    pub key_type: String,
    pub coefficient_type: String,
    #[serde(deserialize_with = "json::deserialize_integer_array")]
    pub shape_per_element: [u64; 2],
    pub coefficient_order: String,
    pub rows: Vec<ElementCubicRow>,
    pub table_id: String,
    pub axis_0: [String; 3],
    pub axis_1: [String; 4],
    pub input_unit: String,
    pub output_unit: String,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ElementCubicRow {
    pub element: String,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub atomic_number_u32: u32,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub row_ordinal: BigUint,
    pub cubic_coefficients: [[Scalar; 4]; 3],
}

#[derive(PartialEq)]
pub enum SigmaTable {
    Damping(DampingParameterTable),
    Orbital(OrbitalParameterTable),
    ElementCubic(ElementCubicTable),
}

impl<'de> Deserialize<'de> for SigmaTable {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = Box::<RawValue>::deserialize(deserializer)?;
        if let Ok(table) = serde_json::from_str(raw.get()) {
            return Ok(Self::Damping(table));
        }
        if let Ok(table) = serde_json::from_str(raw.get()) {
            return Ok(Self::Orbital(table));
        }
        serde_json::from_str(raw.get())
            .map(Self::ElementCubic)
            .map_err(|_| serde::de::Error::custom("invalid sigma table"))
    }
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ResidualEnPayload {
    pub table: ResidualEnTable,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ResidualEnTable {
    pub coefficient_type: String,
    pub rows: Vec<ResidualEnRow>,
    pub table_id: String,
    pub coefficient_order: [String; 3],
    pub key_type: String,
    pub unit_metadata: ResidualUnits,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ResidualEnRow {
    pub property: String,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub environment_key_i32: i32,
    pub quadratic_coefficients_a_b_c: [Scalar; 3],
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub row_ordinal: BigUint,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ResidualUnits {
    pub native_calculator: String,
    #[serde(rename = "installed_PropertyInfos_ini")]
    pub installed_property_infos_ini: String,
    pub resolution: String,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct XlogpPayload {
    pub table: XlogpTable,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct XlogpTable {
    pub coefficient_type: String,
    pub row_order: String,
    pub rows: Vec<XlogpRow>,
    pub table_id: String,
    pub unit: String,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct XlogpRow {
    pub kind: String,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub index: BigUint,
    pub coefficient_f64: Scalar,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub row_ordinal: BigUint,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PkaPayload {
    pub models: Vec<PkaModel>,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PkaModel {
    pub model_id: String,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub feature_count: u64,
    pub coefficient_type: String,
    pub output_unit: String,
    pub rows: Vec<RegressionRow>,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RegressionRow {
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub index: u64,
    pub feature_id: String,
    pub coefficient_f64: Scalar,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LogsPayload {
    pub model: LogsModel,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LogsModel {
    pub model_id: String,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub feature_count: u64,
    pub coefficient_type: String,
    pub output_unit: String,
    pub rows: Vec<RegressionRow>,
    pub intercept_f64: Scalar,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AccessibilityPayload {
    pub coefficient_type: String,
    pub radius_unit: String,
    pub radii: Vec<RadiusRow>,
    pub distance_denominators: Vec<DistanceDenominatorRow>,
    pub fallback_radius_f64: Scalar,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RadiusRow {
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub atomic_number_u32: u32,
    pub symbol: String,
    pub radius_f64: Scalar,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DistanceDenominatorRow {
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub graph_distance_u32: u32,
    pub denominator_f64: Scalar,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CalculatorControlsPayload {
    pub scalar_type: String,
    pub sigma_default_parameterization: String,
    pub pi_default_parameterization: String,
    pub total_default_parameterization: String,
    #[serde(rename = "residual_EN_increment_f64")]
    pub residual_en_increment_f64: Scalar,
    #[serde(rename = "residual_EN_scaling_f64")]
    pub residual_en_scaling_f64: Scalar,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub sphere_default_flavor_i32: i32,
    pub sphere_default_property_label: String,
    pub effective_polarizability_damping_f64: Scalar,
    pub peoe_default_numeric_controls: PeoeNumericControls,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PredictionFeatureMetadata {
    pub schema_id: String,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub version: BigUint,
    pub numeric_encoding: NumericEncoding,
    pub table_ids: Vec<String>,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub row_count: BigUint,
    pub regression_feature_orders: BTreeMap<String, Vec<String>>,
    pub xlogp_axes: XlogpAxes,
    #[serde(deserialize_with = "json::deserialize_integer_array")]
    pub peoe_element_tensor_shape: [u64; 3],
    pub units: BTreeMap<String, String>,
    pub row_order_is_data_identity_not_code_dispatch: bool,
    pub original_atom_type_rules_or_pass_sequence_included: bool,
    pub original_code_or_cfg_included: bool,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct NumericEncoding {
    #[serde(rename = "F64")]
    pub f64: FloatEncoding,
    #[serde(rename = "U64")]
    pub u64: String,
    #[serde(rename = "I32")]
    pub i32: String,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FloatEncoding {
    pub bits_le: String,
    pub value: String,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct XlogpAxes {
    pub atom_type: IndexRange,
    pub correction: IndexRange,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct IndexRange {
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub start: BigUint,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub end_inclusive: BigUint,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PredictionSourceBounds {
    pub record_id: String,
    pub source: SourceFileIdentity,
    pub inert_lineage_only: bool,
    pub original_code_or_control_flow_included: bool,
    pub static_literal_profile: EvidencePathDigest,
    pub parameter_bindings: EvidencePathDigest,
    pub numeric_qa: EvidencePathDigest,
    pub table_bounds: BTreeMap<String, TableBounds>,
    pub regression_cell_bounds: BTreeMap<String, Vec<RegressionCellEnvelope>>,
    pub offsets_are_not_dispatch_entries: bool,
    pub accessibility: AccessibilityLineage,
    pub control_parameter_records: Vec<FunctionEnvelope>,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SourceFileIdentity {
    pub file_name: String,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub bytes: BigUint,
    pub sha256: String,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EvidencePathDigest {
    pub path: String,
    pub sha256: String,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TableFunctionEnvelope {
    pub evidence_file: String,
    pub evidence_sha256: String,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub source_file_offset_start: BigUint,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub source_file_offset_end_exclusive: BigUint,
    pub binding_scope: String,
    #[serde(default, deserialize_with = "present_value")]
    pub literal_argument_envelopes: Option<Vec<LiteralSourceSpan>>,
    #[serde(default, deserialize_with = "json::deserialize_optional_integer")]
    pub row_count: Option<BigUint>,
    #[serde(default, deserialize_with = "json::deserialize_optional_integer")]
    pub zero_shift_default_source_file_offset: Option<BigUint>,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LiteralSourceSpan {
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub row_ordinal: BigUint,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub source_file_offset_start: BigUint,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub source_file_offset_end_exclusive: BigUint,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TableByteEnvelope {
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub source_file_offset_start: BigUint,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub bytes: BigUint,
    pub sha256: String,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub row_width_bytes: BigUint,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub row_count: BigUint,
    pub binding_scope: String,
}

#[derive(PartialEq)]
pub enum TableBounds {
    Function(TableFunctionEnvelope),
    Bytes(TableByteEnvelope),
}

impl<'de> Deserialize<'de> for TableBounds {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = Box::<RawValue>::deserialize(deserializer)?;
        if let Ok(span) = serde_json::from_str(raw.get()) {
            return Ok(Self::Function(span));
        }
        serde_json::from_str(raw.get())
            .map(Self::Bytes)
            .map_err(|_| serde::de::Error::custom("invalid table bounds"))
    }
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RegressionCellEnvelope {
    #[serde(default, deserialize_with = "json::deserialize_optional_integer")]
    pub index: Option<BigUint>,
    #[serde(default, deserialize_with = "present_value")]
    pub role: Option<String>,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub source_file_offset: BigUint,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub bytes: BigUint,
    pub sha256: String,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AccessibilityLineage {
    pub constructor: FunctionEnvelope,
    pub literal_radius_cells: Vec<RadiusSourceSpan>,
    pub chlorine_and_denominators: String,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FunctionEnvelope {
    pub evidence_file: String,
    pub evidence_sha256: String,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub source_file_offset_start: BigUint,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub source_file_offset_end_exclusive: BigUint,
    pub binding_scope: String,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RadiusSourceSpan {
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub atomic_number_u32: u32,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub source_file_offset: BigUint,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub bytes: BigUint,
    pub sha256: String,
}
