//! Catalog-bound, immutable mass inputs. This module does not calculate masses.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Deserializer, de::DeserializeOwned};
use serde_json::{Number, Value, value::RawValue};

use crate::{decimal_input::DecimalInput, json, mass_sources::MassSources, phase6_profiles};

pub(crate) const FEATURE_PATH: &str = "feature-schema.json";
pub(crate) const RAW_TEXT_PATH: &str = "raw/CS Isotopes Table.txt";
pub(crate) const NORMALIZED_PATHS: [&str; 7] = [
    "parameters/elements.json",
    "parameters/isotopes.json",
    "parameters/default-monoisotopic-masses.json",
    "parameters/aliases.json",
    "parameters/mass-constants.json",
    FEATURE_PATH,
    "source-bounds.json",
];

/// Payload-free refusal while loading a single complete mass input generation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MassInputError {
    FileSet,
    InvalidShape,
    Inconsistent,
    CatalogMismatch,
}

impl std::fmt::Display for MassInputError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::FileSet => "invalid_file_set",
            Self::InvalidShape => "invalid_data_record",
            Self::Inconsistent => "inconsistent_dataset",
            Self::CatalogMismatch => "catalog_identity_mismatch",
        })
    }
}
impl std::error::Error for MassInputError {}

/// Check real metadata file bytes against the independently published catalog.
///
/// # Errors
/// Refuses a different dataset identity, metadata digest or oversized input.
pub fn verify_catalog(
    dataset_id: &str,
    dataset_version: &str,
    manifest: &[u8],
    registry: &[u8],
    profile: &[u8],
) -> Result<(), MassInputError> {
    let identity = phase6_profiles::CatalogIdentity {
        dataset_id: "chemdraw16-personal-local-atomic-isotope-masses-phase6",
        dataset_version: "sha256-set-49728f247c1c84f58cbf3532af3a96406e759221043f576817bae51677202440",
        registry_sha256: "36ea9a5c736d641aad97b60684993cf436028fe62672620894c55d31a0546428",
        profile_sha256: "9a00c9bdbe0094466c32b9ced092a32657721d62f14fda1c89713693ff41de81",
        manifest_file_sha256: "00e208487853d2f9a582ac861fb7fdf116d4e1c519b0c0e223cf2d945a82c55c",
    };
    if dataset_id != identity.dataset_id || dataset_version != identity.dataset_version {
        return Err(MassInputError::CatalogMismatch);
    }
    identity
        .check_file_bytes(manifest, registry, profile)
        .map_err(|_| MassInputError::CatalogMismatch)
}

/// Original decimal spelling and an independently decoded binary64 witness.
#[derive(PartialEq)]
pub struct DecimalValue {
    pub source: String,
    pub binary64_bits: u64,
    exact: DecimalInput,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DecimalWire {
    source_decimal: String,
    reference_f64: Number,
    reference_f64_bits_le: String,
}

impl<'de> Deserialize<'de> for DecimalValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = DecimalWire::deserialize(deserializer)?;
        let exact = DecimalInput::from_token(&raw.source_decimal)
            .ok_or_else(|| serde::de::Error::custom("invalid or unsupported decimal token"))?;
        let number = raw
            .source_decimal
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
            .ok_or_else(|| serde::de::Error::custom("invalid binary64 reference"))?;
        let pair =
            serde_json::json!({"value": raw.reference_f64, "bits_le": raw.reference_f64_bits_le});
        let pair = serde_json::to_vec(&pair).map_err(serde::de::Error::custom)?;
        let witness = phase6_profiles::decode_f64_literal(&pair)
            .map_err(|_| serde::de::Error::custom("invalid binary64 witness"))?;
        if number.to_bits() != witness.bits {
            return Err(serde::de::Error::custom(
                "decimal and binary64 witness differ",
            ));
        }
        Ok(Self {
            source: raw.source_decimal,
            binary64_bits: witness.bits,
            exact,
        })
    }
}

/// A unit label and finite binary64 operand, without any implied conversion.
#[derive(PartialEq)]
pub struct UnitValue {
    pub binary64_bits: u64,
    pub unit: String,
}

impl<'de> Deserialize<'de> for UnitValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = Value::deserialize(deserializer)?;
        let bytes = serde_json::to_vec(&raw).map_err(serde::de::Error::custom)?;
        let decoded = phase6_profiles::decode_f64_with_unit(&bytes)
            .map_err(|_| serde::de::Error::custom("invalid unit operand"))?;
        Ok(Self {
            binary64_bits: decoded.value.bits,
            unit: decoded.unit,
        })
    }
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ElementRow {
    pub element_row_ordinal: u64,
    pub atomic_number_u32: u32,
    pub symbol: String,
    pub natural_atomic_weight: DecimalValue,
    pub permitted_valences_u32: Vec<u32>,
    pub first_isotope_row_ordinal: u64,
    pub isotope_rows: u64,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct IsotopeRow {
    pub isotope_row_ordinal: u64,
    pub element_row_ordinal: u64,
    pub atomic_number_u32: u32,
    pub mass_number_u32: u32,
    pub mass: DecimalValue,
    pub abundance_percent: DecimalValue,
    pub abundance_token_present: bool,
    #[serde(deserialize_with = "required_nullable")]
    pub nickname: Option<String>,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct StaticSelection {
    pub element_row_ordinal: u64,
    pub atomic_number_u32: u32,
    pub selected_isotope_row_ordinal: u64,
    pub mass_number_u32: u32,
    pub mass: DecimalValue,
    pub all_abundances_zero: bool,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AliasRow {
    pub symbol: String,
    pub atomic_number_u32: u32,
    pub mass_number_u32: u32,
    pub source_isotope_row_ordinal: u64,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SecondaryInput {
    pub value: Number,
    pub unit: String,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MassFeatures {
    pub schema_id: String,
    pub source_decimal_tokens_authoritative: bool,
    pub reference_f64_decode_is_independent_not_original_runtime_capture: bool,
    pub element_rows: u64,
    pub isotope_rows: u64,
    pub alias_rows: u64,
    pub element_key_type: String,
    pub isotope_key_type: String,
    pub row_order: String,
    pub mass_unit: String,
    pub natural_atomic_weight_unit: String,
    pub abundance_unit: String,
    pub native_selected_input_secondary: SecondaryInput,
    pub source_mass_number_field_explicit_in_text: bool,
    pub mass_number_values_independently_checked_against_source_constructor_contract: bool,
    pub original_parser_or_selection_code_included: bool,
    pub original_naming_grammar_or_module_dispatch_included: bool,
}

/// A complete input generation; no Debug implementation can print its payload by accident.
#[derive(PartialEq)]
pub struct MassInput {
    pub elements: Vec<ElementRow>,
    pub isotopes: Vec<IsotopeRow>,
    pub selections: Vec<StaticSelection>,
    pub aliases: Vec<AliasRow>,
    pub signed_charge_mass: UnitValue,
    pub features: MassFeatures,
    pub sources: MassSources,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ElementsDocument {
    elements: Vec<ElementRow>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IsotopesDocument {
    isotopes: Vec<IsotopeRow>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectionDocument {
    selection_is_static_table_analysis_not_runtime_capture: bool,
    rows: Vec<StaticSelection>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AliasesDocument {
    aliases: Vec<AliasRow>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConstantsDocument {
    signed_charge_mass_coefficient: UnitValue,
}

fn required_nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

fn read_document<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, MassInputError> {
    if bytes.len() as u64 > crate::MAX_DATA_FILE_BYTES || bytes.starts_with(b"\xef\xbb\xbf") {
        return Err(MassInputError::InvalidShape);
    }
    let value = json::parse_integer_zero_strict(bytes).map_err(|_| MassInputError::InvalidShape)?;
    serde_json::from_value(value).map_err(|_| MassInputError::InvalidShape)
}

// Deserialize only this value as a raw JSON token after strict duplicate-key checking.
// A binary64 parser alone cannot distinguish exact zero from a nonzero underflow.
#[derive(Deserialize)]
struct RawFeature {
    native_selected_input_secondary: RawSecondary,
}
#[derive(Deserialize)]
struct RawSecondary {
    value: Box<RawValue>,
}

fn check_secondary_input(bytes: &[u8]) -> Result<(), MassInputError> {
    let raw: RawFeature =
        serde_json::from_slice(bytes).map_err(|_| MassInputError::InvalidShape)?;
    let number = DecimalInput::from_token(raw.native_selected_input_secondary.value.get())
        .ok_or(MassInputError::InvalidShape)?;
    let zero = DecimalInput::from_token("0").ok_or(MassInputError::InvalidShape)?;
    if number != zero {
        return Err(MassInputError::Inconsistent);
    }
    Ok(())
}

/// Decode the complete named file set into a validated immutable generation.
///
/// # Errors
/// Refuses malformed files, contradictory row identities, ranges or source witnesses.
pub fn decode(files: &[(&str, &[u8])]) -> Result<MassInput, MassInputError> {
    let mut file_map = BTreeMap::new();
    for (name, bytes) in files {
        if (!NORMALIZED_PATHS.contains(name) && *name != RAW_TEXT_PATH)
            || file_map.insert(*name, *bytes).is_some()
        {
            return Err(MassInputError::FileSet);
        }
    }
    let get = |name| file_map.get(name).copied().ok_or(MassInputError::FileSet);
    let elements: ElementsDocument = read_document(get(NORMALIZED_PATHS[0])?)?;
    let isotopes: IsotopesDocument = read_document(get(NORMALIZED_PATHS[1])?)?;
    let selected: SelectionDocument = read_document(get(NORMALIZED_PATHS[2])?)?;
    let aliases: AliasesDocument = read_document(get(NORMALIZED_PATHS[3])?)?;
    let constants: ConstantsDocument = read_document(get(NORMALIZED_PATHS[4])?)?;
    let feature_bytes = get(FEATURE_PATH)?;
    let features = read_document(feature_bytes)?;
    check_secondary_input(feature_bytes)?;
    let sources: MassSources = read_document(get(NORMALIZED_PATHS[6])?)?;
    if !selected.selection_is_static_table_analysis_not_runtime_capture {
        return Err(MassInputError::Inconsistent);
    }
    let result = MassInput {
        elements: elements.elements,
        isotopes: isotopes.isotopes,
        selections: selected.rows,
        aliases: aliases.aliases,
        signed_charge_mass: constants.signed_charge_mass_coefficient,
        features,
        sources,
    };
    result.validate()?;
    if let Some(raw) = file_map.get(RAW_TEXT_PATH) {
        result
            .sources
            .verify_text(raw)
            .map_err(|_| MassInputError::Inconsistent)?;
    }
    Ok(result)
}

impl MassInput {
    fn validate(&self) -> Result<(), MassInputError> {
        let meta = &self.features;
        let strings = [
            &meta.schema_id,
            &meta.mass_unit,
            &meta.natural_atomic_weight_unit,
            &meta.abundance_unit,
            &meta.native_selected_input_secondary.unit,
            &self.signed_charge_mass.unit,
        ];
        if self.elements.is_empty()
            || meta.element_rows != self.elements.len() as u64
            || meta.isotope_rows != self.isotopes.len() as u64
            || meta.alias_rows != self.aliases.len() as u64
            || meta.element_key_type != "u32"
            || meta.isotope_key_type != "u32"
            || meta.row_order != "source"
            || strings.iter().any(|value| value.trim().is_empty())
            || !meta.source_decimal_tokens_authoritative
            || !meta.reference_f64_decode_is_independent_not_original_runtime_capture
            || meta.source_mass_number_field_explicit_in_text
            || !meta.mass_number_values_independently_checked_against_source_constructor_contract
            || meta.original_parser_or_selection_code_included
            || meta.original_naming_grammar_or_module_dispatch_included
            || meta.native_selected_input_secondary.value.as_f64() != Some(0.0)
            || f64::from_bits(self.signed_charge_mass.binary64_bits) >= 0.0
        {
            return Err(MassInputError::Inconsistent);
        }
        let zero = DecimalInput::from_token("0").ok_or(MassInputError::InvalidShape)?;
        let hundred = DecimalInput::from_token("100").ok_or(MassInputError::InvalidShape)?;
        let mut atomic_numbers = HashSet::new();
        let mut element_symbols = HashSet::new();
        let mut selection = self.selections.iter();
        let mut cursor = 0_usize;
        for (index, element) in self.elements.iter().enumerate() {
            let count =
                usize::try_from(element.isotope_rows).map_err(|_| MassInputError::Inconsistent)?;
            let end = cursor
                .checked_add(count)
                .filter(|end| *end <= self.isotopes.len())
                .ok_or(MassInputError::Inconsistent)?;
            if element.element_row_ordinal != index as u64
                || element.first_isotope_row_ordinal != cursor as u64
                || element.atomic_number_u32 == 0
                || element.symbol.trim().is_empty()
                || !atomic_numbers.insert(element.atomic_number_u32)
                || !element_symbols.insert(element.symbol.as_str())
                || element.natural_atomic_weight.exact <= zero
            {
                return Err(MassInputError::Inconsistent);
            }
            let mut numbers = HashSet::new();
            let mut best: Option<&IsotopeRow> = None;
            let mut all_zero = true;
            for (within, isotope) in self.isotopes[cursor..end].iter().enumerate() {
                let nominal = f64::from(isotope.mass_number_u32);
                let witness = f64::from_bits(isotope.mass.binary64_bits);
                if isotope.isotope_row_ordinal != (cursor + within) as u64
                    || isotope.element_row_ordinal != index as u64
                    || isotope.atomic_number_u32 != element.atomic_number_u32
                    || isotope.mass_number_u32 == 0
                    || !numbers.insert(isotope.mass_number_u32)
                    || isotope.mass.exact <= zero
                    || isotope.abundance_percent.exact < zero
                    || isotope.abundance_percent.exact > hundred
                    || (!isotope.abundance_token_present && isotope.abundance_percent.exact != zero)
                    || witness < nominal - 0.5
                    || witness >= nominal + 0.5
                {
                    return Err(MassInputError::Inconsistent);
                }
                all_zero &= isotope.abundance_percent.exact == zero;
                if best.is_none_or(|current| {
                    isotope.abundance_percent.exact > current.abundance_percent.exact
                }) {
                    best = Some(isotope);
                }
            }
            if let Some(best) = best {
                let row = selection.next().ok_or(MassInputError::Inconsistent)?;
                if row.element_row_ordinal != index as u64
                    || row.atomic_number_u32 != best.atomic_number_u32
                    || row.selected_isotope_row_ordinal != best.isotope_row_ordinal
                    || row.mass_number_u32 != best.mass_number_u32
                    || row.mass != best.mass
                    || row.all_abundances_zero != all_zero
                {
                    return Err(MassInputError::Inconsistent);
                }
            }
            cursor = end;
        }
        if cursor != self.isotopes.len() || selection.next().is_some() {
            return Err(MassInputError::Inconsistent);
        }
        let mut aliases = HashMap::new();
        for alias in &self.aliases {
            let index = usize::try_from(alias.source_isotope_row_ordinal)
                .map_err(|_| MassInputError::Inconsistent)?;
            let isotope = self
                .isotopes
                .get(index)
                .ok_or(MassInputError::Inconsistent)?;
            if alias.symbol.trim().is_empty()
                || element_symbols.contains(alias.symbol.as_str())
                || isotope.atomic_number_u32 != alias.atomic_number_u32
                || isotope.mass_number_u32 != alias.mass_number_u32
                || aliases
                    .insert(alias.symbol.as_str(), index)
                    .is_some_and(|old| old != index)
            {
                return Err(MassInputError::Inconsistent);
            }
        }
        self.sources
            .validate(self.elements.len(), self.isotopes.len())
            .map_err(|_| MassInputError::Inconsistent)
    }
}
