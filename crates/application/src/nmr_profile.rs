//! Typed decoding for the declarative NMR coupling parameter profile.
//!
//! These are inert inputs. Loading them does not implement an NMR engine.

use std::collections::HashSet;

use num_bigint::{BigInt, BigUint};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Number;

use crate::{json, phase6_profiles};
pub use phase6_profiles::F64Operand;

pub(crate) const LOOKUP_PATH: &str = "parameters/substituent-lookup.json";
pub(crate) const AMPLITUDES_PATH: &str = "parameters/coupling-amplitudes.json";
pub(crate) const WIDTHS_PATH: &str = "parameters/width-configuration.json";
pub(crate) const FEATURES_PATH: &str = "feature-schema.json";
pub(crate) const BOUNDS_PATH: &str = "source-bounds.json";
pub(crate) const REQUIRED_FILES: [&str; 5] = [
    LOOKUP_PATH,
    AMPLITUDES_PATH,
    WIDTHS_PATH,
    FEATURES_PATH,
    BOUNDS_PATH,
];

/// Payload-free reason an NMR dataset could not be decoded.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NmrError {
    FileSet,
    Record,
    Inconsistent,
    CatalogMismatch,
}

impl std::fmt::Display for NmrError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::FileSet => "invalid_file_set",
            Self::Record => "invalid_data_record",
            Self::Inconsistent => "inconsistent_dataset",
            Self::CatalogMismatch => "catalog_identity_mismatch",
        })
    }
}

impl std::error::Error for NmrError {}

pub(crate) fn catalog_identity() -> phase6_profiles::CatalogIdentity<'static> {
    phase6_profiles::CatalogIdentity {
        dataset_id: "chemdraw16-personal-local-nmr-coupling-parameters-phase6",
        dataset_version: "sha256-set-69fa98dcb2582340daf209db3c3214313ffbe623aadf5cc1ff51289d090a3f7d",
        registry_sha256: "b3e7d4c784cf8b8382d85420f4007ad697203331f7134ee2a717ca6a4b41ae52",
        profile_sha256: "df9b2490ea1d8fac16213f6a25d239614b73cc03e7859e876a479f815885002f",
        manifest_file_sha256: "1a2ba98fef1447473baf5460d11510eadf396b02e532bade40edb61eb430a64a",
    }
}

/// Verify the three actual metadata files and dataset identity against the public catalog.
///
/// # Errors
/// Returns `CatalogMismatch` for any identity, file size or digest mismatch.
pub fn verify_catalog_files(
    dataset_id: &str,
    dataset_version: &str,
    manifest: &[u8],
    registry: &[u8],
    profile: &[u8],
) -> Result<(), NmrError> {
    let identity = catalog_identity();
    if dataset_id != identity.dataset_id || dataset_version != identity.dataset_version {
        return Err(NmrError::CatalogMismatch);
    }
    identity
        .check_file_bytes(manifest, registry, profile)
        .map_err(|_| NmrError::CatalogMismatch)
}

/// An atomic-number lookup operand; index zero may be a declared sentinel.
#[derive(PartialEq)]
pub struct LookupRow {
    pub atomic_number: u32,
    pub parameter: F64Operand,
}

/// Ordered lookup rows and their declared unit.
#[derive(PartialEq)]
pub struct LookupTable {
    pub table_id: String,
    pub unit: String,
    pub rows: Vec<LookupRow>,
}

/// A coefficient, its identifier, and its independently declared unit and category.
#[derive(PartialEq)]
pub struct Amplitude {
    pub parameter_id: String,
    pub coefficient: F64Operand,
    pub unit: String,
    pub parameter_category: String,
    pub unit_status: String,
}

/// Ordered amplitude operands, without an implemented coupling calculation.
#[derive(PartialEq)]
pub struct Amplitudes {
    pub parameter_set_id: String,
    pub parameters: Vec<Amplitude>,
}

/// A configured width, including its unit and convention label.
#[derive(PartialEq)]
pub struct Width {
    pub parameter_id: String,
    pub value: F64Operand,
    pub unit: String,
    pub parameter_category: String,
    pub convention_label: String,
}

/// Ordered width configuration; it is not an independently trained model.
#[derive(PartialEq)]
pub struct Widths {
    pub profile_id: String,
    pub parameters: Vec<Width>,
    pub trained_model_claimed: bool,
}

/// Declared source key representations.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct KeyTypes {
    pub atomic_number_u32: String,
    pub parameter_id: String,
}

/// Declared numeric and bit-witness representations.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct F64Type {
    pub bits_le: String,
    pub value: String,
}

/// Numeric type metadata as named by the source schema.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct NumericType {
    #[serde(rename = "F64")]
    pub f64_type: F64Type,
}

/// Bounds and sentinel status for the atomic-number lookup.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct IndexBounds {
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub minimum: BigInt,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub exclusive_maximum: BigUint,
    pub zero_is_sentinel_key: bool,
}

/// Counts declared for the three separate operand families.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Counts {
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub lookup_rows: u64,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub empirical_scalars: u64,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub configuration_widths: u64,
}

/// Feature identity and units remain data; they do not authorize a model execution.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FeatureMetadata {
    pub schema_id: String,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub version: BigUint,
    pub key_types: KeyTypes,
    pub numeric_type: NumericType,
    pub lookup_index_bounds: IndexBounds,
    pub lookup_scale: String,
    pub zero_and_negative_values: String,
    pub counts: Counts,
    pub parameter_ids: Vec<String>,
    pub units_are_per_parameter: bool,
    pub original_factory_dispatch_rules_included: bool,
    pub original_formula_or_instruction_sequence_included: bool,
}

/// A bounded source span retained for provenance only.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SourceSpan {
    pub row_id: String,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub source_file_offset_start: BigUint,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub source_bytes: BigUint,
    pub source_encoding: String,
    pub source_payload_sha256: String,
}

/// Source offsets and hashes are inert lineage, never callable or dispatch entries.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SourceBounds {
    pub record_id: String,
    pub source_file_name: String,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub source_bytes: BigUint,
    pub source_sha256: String,
    pub numeric_source_bounds: Vec<SourceSpan>,
    pub inert_provenance_only: bool,
    pub original_code_or_control_flow_included: bool,
    pub original_native_calls_included: bool,
    pub source_bounds_are_not_dispatch_entries: bool,
}

/// A complete NMR operand set, without an engine or inferred chemical results.
#[derive(PartialEq)]
pub struct NmrData {
    pub lookup: LookupTable,
    pub amplitudes: Amplitudes,
    pub widths: Widths,
    pub features: FeatureMetadata,
    pub source_bounds: SourceBounds,
}

/// Decodes the catalog-selected five-file set after caller authorization and hash checks.
///
/// # Errors
/// Rejects an incomplete set, malformed values, inconsistent identities or bounds.
pub fn decode_nmr(files: &[(&str, &[u8])]) -> Result<NmrData, NmrError> {
    if files.len() != REQUIRED_FILES.len() {
        return Err(NmrError::FileSet);
    }
    let file = |path| {
        let mut matching = files.iter().filter(|(name, _)| *name == path);
        let bytes = matching.next().ok_or(NmrError::FileSet)?.1;
        if matching.next().is_some() {
            return Err(NmrError::FileSet);
        }
        Ok(bytes)
    };
    let lookup: LookupWire = decode(file(LOOKUP_PATH)?)?;
    let amplitudes: AmplitudesWire = decode(file(AMPLITUDES_PATH)?)?;
    let widths: WidthsWire = decode(file(WIDTHS_PATH)?)?;
    let features: FeatureMetadata = decode(file(FEATURES_PATH)?)?;
    let source_bounds: SourceBounds = decode(file(BOUNDS_PATH)?)?;

    let lookup = LookupTable {
        table_id: lookup.table_id,
        unit: lookup.unit,
        rows: lookup
            .rows
            .into_iter()
            .map(|row| {
                Ok(LookupRow {
                    atomic_number: row.atomic_number_u32,
                    parameter: row.parameter_f64.decode()?,
                })
            })
            .collect::<Result<_, NmrError>>()?,
    };
    let amplitudes = Amplitudes {
        parameter_set_id: amplitudes.parameter_set_id,
        parameters: amplitudes
            .parameters
            .into_iter()
            .map(|row| {
                Ok(Amplitude {
                    parameter_id: row.parameter_id,
                    coefficient: row.coefficient_f64.decode()?,
                    unit: row.unit,
                    parameter_category: row.parameter_category,
                    unit_status: row.unit_status,
                })
            })
            .collect::<Result<_, NmrError>>()?,
    };
    let widths = Widths {
        profile_id: widths.profile_id,
        parameters: widths
            .parameters
            .into_iter()
            .map(|row| {
                Ok(Width {
                    parameter_id: row.parameter_id,
                    value: row.value_f64.decode()?,
                    unit: row.unit,
                    parameter_category: row.parameter_category,
                    convention_label: row.convention_label,
                })
            })
            .collect::<Result<_, NmrError>>()?,
        trained_model_claimed: widths.trained_model_claimed,
    };
    let result = NmrData {
        lookup,
        amplitudes,
        widths,
        features,
        source_bounds,
    };
    validate(&result)?;
    Ok(result)
}

fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, NmrError> {
    if bytes.len() as u64 > crate::MAX_DATA_FILE_BYTES || bytes.starts_with(b"\xef\xbb\xbf") {
        return Err(NmrError::Record);
    }
    json::validate_raw_strict(bytes).map_err(|_| NmrError::Record)?;
    serde_json::from_slice(bytes).map_err(|_| NmrError::Record)
}

fn validate(data: &NmrData) -> Result<(), NmrError> {
    let features = &data.features;
    let lower = &features.lookup_index_bounds.minimum;
    let upper = BigInt::from(features.lookup_index_bounds.exclusive_maximum.clone());
    if features.counts.lookup_rows != data.lookup.rows.len() as u64
        || features.counts.empirical_scalars != data.amplitudes.parameters.len() as u64
        || features.counts.configuration_widths != data.widths.parameters.len() as u64
        || lower > &upper
        || data.widths.trained_model_claimed
        || features.original_factory_dispatch_rules_included
        || features.original_formula_or_instruction_sequence_included
    {
        return Err(NmrError::Inconsistent);
    }
    let allowed: HashSet<_> = features.parameter_ids.iter().collect();
    if allowed.len() != features.parameter_ids.len() {
        return Err(NmrError::Inconsistent);
    }
    let mut keys = HashSet::new();
    for row in &data.lookup.rows {
        let key = BigInt::from(row.atomic_number);
        if !keys.insert(row.atomic_number) || &key < lower || key >= upper {
            return Err(NmrError::Inconsistent);
        }
    }
    let mut amplitude_ids = HashSet::new();
    for row in &data.amplitudes.parameters {
        if !allowed.contains(&row.parameter_id) || !amplitude_ids.insert(&row.parameter_id) {
            return Err(NmrError::Inconsistent);
        }
    }
    let mut width_ids = HashSet::new();
    for row in &data.widths.parameters {
        if !allowed.contains(&row.parameter_id) || !width_ids.insert(&row.parameter_id) {
            return Err(NmrError::Inconsistent);
        }
    }
    // Every advertised parameter must resolve to at least one of the two files.
    if amplitude_ids.union(&width_ids).count() != allowed.len() {
        return Err(NmrError::Inconsistent);
    }
    let bounds = &data.source_bounds;
    if !is_hash(&bounds.source_sha256)
        || !bounds.inert_provenance_only
        || bounds.original_code_or_control_flow_included
        || bounds.original_native_calls_included
        || !bounds.source_bounds_are_not_dispatch_entries
    {
        return Err(NmrError::Inconsistent);
    }
    let mut span_ids = HashSet::new();
    for span in &bounds.numeric_source_bounds {
        if !span_ids.insert(&span.row_id)
            || !is_hash(&span.source_payload_sha256)
            || &span.source_file_offset_start + &span.source_bytes > bounds.source_bytes
        {
            return Err(NmrError::Inconsistent);
        }
    }
    Ok(())
}

fn is_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FloatWire {
    value: Number,
    bits_le: String,
}

impl FloatWire {
    fn decode(&self) -> Result<F64Operand, NmrError> {
        phase6_profiles::decode_f64_pair(&self.value, &self.bits_le).map_err(|_| NmrError::Record)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LookupWire {
    table_id: String,
    unit: String,
    rows: Vec<LookupRowWire>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LookupRowWire {
    #[serde(deserialize_with = "json::deserialize_integer")]
    atomic_number_u32: u32,
    parameter_f64: FloatWire,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AmplitudesWire {
    parameter_set_id: String,
    parameters: Vec<AmplitudeWire>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AmplitudeWire {
    parameter_id: String,
    coefficient_f64: FloatWire,
    unit: String,
    parameter_category: String,
    unit_status: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WidthsWire {
    profile_id: String,
    parameters: Vec<WidthWire>,
    trained_model_claimed: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WidthWire {
    parameter_id: String,
    value_f64: FloatWire,
    unit: String,
    parameter_category: String,
    convention_label: String,
}
