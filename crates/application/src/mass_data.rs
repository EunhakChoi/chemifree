//! Typed, inert operands for a selected mass dataset; no mass calculation is performed.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Deserializer, de::DeserializeOwned};
use serde_json::{Number, Value};
use sha2::{Digest, Sha256};

use crate::{exact_decimal::DecimalOrder, json, phase6_profiles};

pub(crate) const FEATURES: &str = "feature-schema.json";
pub(crate) const RAW: &str = "raw/CS Isotopes Table.txt";
pub(crate) const FILES: [&str; 7] = [
    "parameters/elements.json",
    "parameters/isotopes.json",
    "parameters/default-monoisotopic-masses.json",
    "parameters/aliases.json",
    "parameters/mass-constants.json",
    FEATURES,
    "source-bounds.json",
];

/// Payload-free reason an input set cannot be admitted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MassError {
    FileSet,
    Record,
    Inconsistent,
    CatalogMismatch,
}

impl std::fmt::Display for MassError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::FileSet => "invalid_file_set",
            Self::Record => "invalid_data_record",
            Self::Inconsistent => "inconsistent_dataset",
            Self::CatalogMismatch => "catalog_identity_mismatch",
        })
    }
}
impl std::error::Error for MassError {}

/// Verify actual metadata bytes against the public frozen catalog, not dataset claims.
///
/// # Errors
/// Returns `CatalogMismatch` when the dataset identity or any metadata digest differs.
pub fn verify_catalog(
    id: &str,
    version: &str,
    manifest: &[u8],
    registry: &[u8],
    profile: &[u8],
) -> Result<(), MassError> {
    let expected = phase6_profiles::CatalogIdentity {
        dataset_id: "chemdraw16-personal-local-atomic-isotope-masses-phase6",
        dataset_version: "sha256-set-49728f247c1c84f58cbf3532af3a96406e759221043f576817bae51677202440",
        registry_sha256: "36ea9a5c736d641aad97b60684993cf436028fe62672620894c55d31a0546428",
        profile_sha256: "9a00c9bdbe0094466c32b9ced092a32657721d62f14fda1c89713693ff41de81",
        manifest_file_sha256: "00e208487853d2f9a582ac861fb7fdf116d4e1c519b0c0e223cf2d945a82c55c",
    };
    if id != expected.dataset_id || version != expected.dataset_version {
        return Err(MassError::CatalogMismatch);
    }
    expected
        .check_file_bytes(manifest, registry, profile)
        .map_err(|_| MassError::CatalogMismatch)
}

/// Source spelling and separately checked independent binary64 bits.
#[derive(PartialEq)]
pub struct DecimalValue {
    pub source_decimal: String,
    pub reference_bits: u64,
    order: DecimalOrder,
}

impl<'de> Deserialize<'de> for DecimalValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = Value::deserialize(deserializer)?;
        let bytes = serde_json::to_vec(&wire).map_err(serde::de::Error::custom)?;
        let reference = phase6_profiles::decode_decimal_reference(&bytes)
            .map_err(|_| serde::de::Error::custom("invalid decimal reference"))?;
        let order = DecimalOrder::parse(&reference.source_decimal)
            .ok_or_else(|| serde::de::Error::custom("decimal exponent exceeds supported bounds"))?;
        Ok(Self {
            source_decimal: reference.source_decimal,
            reference_bits: reference.reference.bits,
            order,
        })
    }
}

/// A finite binary64 scalar and its explicitly declared unit.
#[derive(PartialEq)]
pub struct UnitValue {
    pub bits: u64,
    pub unit: String,
}

impl<'de> Deserialize<'de> for UnitValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = Value::deserialize(deserializer)?;
        let bytes = serde_json::to_vec(&wire).map_err(serde::de::Error::custom)?;
        let value = phase6_profiles::decode_f64_with_unit(&bytes)
            .map_err(|_| serde::de::Error::custom("invalid unit value"))?;
        Ok(Self {
            bits: value.value.bits,
            unit: value.unit,
        })
    }
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Element {
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
pub struct Isotope {
    pub isotope_row_ordinal: u64,
    pub element_row_ordinal: u64,
    pub atomic_number_u32: u32,
    pub mass_number_u32: u32,
    pub mass: DecimalValue,
    pub abundance_percent: DecimalValue,
    pub abundance_token_present: bool,
    #[serde(deserialize_with = "required_option")]
    pub nickname: Option<String>,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub element_row_ordinal: u64,
    pub atomic_number_u32: u32,
    pub selected_isotope_row_ordinal: u64,
    pub mass_number_u32: u32,
    pub mass: DecimalValue,
    pub all_abundances_zero: bool,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Alias {
    pub symbol: String,
    pub atomic_number_u32: u32,
    pub mass_number_u32: u32,
    pub source_isotope_row_ordinal: u64,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Secondary {
    pub value: Number,
    pub unit: String,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Features {
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
    pub native_selected_input_secondary: Secondary,
    pub source_mass_number_field_explicit_in_text: bool,
    pub mass_number_values_independently_checked_against_source_constructor_contract: bool,
    pub original_parser_or_selection_code_included: bool,
    pub original_naming_grammar_or_module_dispatch_included: bool,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SourceIdentity {
    pub file_name: String,
    pub bytes: u64,
    pub sha256: String,
}

/// Currently supported source-row kinds. Other spellings remain unsupported.
#[derive(Clone, Copy, Deserialize, Eq, Hash, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RowKind {
    Element,
    Isotope,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RowSpan {
    pub kind: RowKind,
    pub row_ordinal: u64,
    pub source_line_1based: u64,
    pub source_file_offset: u64,
    pub source_bytes_including_newline: u64,
    pub source_line_sha256: String,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub evidence_path: String,
    pub evidence_sha256: String,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ByteSpan {
    pub source_file_offset: u64,
    pub bytes: u64,
    pub sha256: String,
}

/// Inert lineage; paths and offsets are never opened as executable material.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub binding_id: String,
    pub inert_source_lineage_only: bool,
    pub text_source: SourceIdentity,
    pub installed_equivalent_copy_sha256: String,
    pub row_source_spans: Vec<RowSpan>,
    pub consumer_source: SourceIdentity,
    pub consumer_evidence: Vec<Evidence>,
    pub signed_charge_coefficient: ByteSpan,
    pub source_offsets_are_not_native_dispatch: bool,
    #[serde(rename = "original_source_or_CFG_included")]
    pub original_source_or_cfg_included: bool,
}

/// A complete checked generation. It is not an executable chemistry model.
#[derive(PartialEq)]
pub struct MassData {
    pub elements: Vec<Element>,
    pub isotopes: Vec<Isotope>,
    pub selections: Vec<Selection>,
    pub aliases: Vec<Alias>,
    pub charge_coefficient: UnitValue,
    pub features: Features,
    pub provenance: Provenance,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ElementsFile {
    elements: Vec<Element>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IsotopesFile {
    isotopes: Vec<Isotope>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectionsFile {
    selection_is_static_table_analysis_not_runtime_capture: bool,
    rows: Vec<Selection>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AliasesFile {
    aliases: Vec<Alias>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConstantsFile {
    signed_charge_mass_coefficient: UnitValue,
}

/// Decode authorized, hashed files as one atomic generation.
///
/// # Errors
/// Rejects incomplete sets, malformed fields and inconsistent operands or provenance.
pub fn decode_set(files: &[(&str, &[u8])]) -> Result<MassData, MassError> {
    let mut by_path = BTreeMap::new();
    for (path, bytes) in files {
        if (!FILES.contains(path) && *path != RAW) || by_path.insert(*path, *bytes).is_some() {
            return Err(MassError::FileSet);
        }
    }
    if FILES.iter().any(|path| !by_path.contains_key(path)) {
        return Err(MassError::FileSet);
    }
    let file = |path| by_path.get(path).copied().ok_or(MassError::FileSet);
    let elements: ElementsFile = read_json(file(FILES[0])?)?;
    let isotopes: IsotopesFile = read_json(file(FILES[1])?)?;
    let selections: SelectionsFile = read_json(file(FILES[2])?)?;
    let aliases: AliasesFile = read_json(file(FILES[3])?)?;
    let constants: ConstantsFile = read_json(file(FILES[4])?)?;
    let features = read_json(file(FEATURES)?)?;
    let provenance = read_json(file(FILES[6])?)?;
    if !selections.selection_is_static_table_analysis_not_runtime_capture {
        return Err(MassError::Inconsistent);
    }
    let data = MassData {
        elements: elements.elements,
        isotopes: isotopes.isotopes,
        selections: selections.rows,
        aliases: aliases.aliases,
        charge_coefficient: constants.signed_charge_mass_coefficient,
        features,
        provenance,
    };
    validate(&data)?;
    if let Some(raw) = by_path.get(RAW) {
        verify_source_lines(&data.provenance, raw)?;
    }
    Ok(data)
}

fn required_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

fn read_json<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, MassError> {
    if bytes.len() as u64 > crate::MAX_DATA_FILE_BYTES || bytes.starts_with(b"\xef\xbb\xbf") {
        return Err(MassError::Record);
    }
    let value = json::parse_integer_zero_strict(bytes).map_err(|_| MassError::Record)?;
    serde_json::from_value(value).map_err(|_| MassError::Record)
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn validate(data: &MassData) -> Result<(), MassError> {
    use std::cmp::Ordering;
    let meta = &data.features;
    if meta.element_rows != data.elements.len() as u64
        || meta.isotope_rows != data.isotopes.len() as u64
        || meta.alias_rows != data.aliases.len() as u64
        || !meta.source_decimal_tokens_authoritative
        || !meta.reference_f64_decode_is_independent_not_original_runtime_capture
        || meta.source_mass_number_field_explicit_in_text
        || !meta.mass_number_values_independently_checked_against_source_constructor_contract
        || meta.native_selected_input_secondary.value.as_f64() != Some(0.0)
        || meta.original_parser_or_selection_code_included
        || meta.original_naming_grammar_or_module_dispatch_included
        || f64::from_bits(data.charge_coefficient.bits) >= 0.0
    {
        return Err(MassError::Inconsistent);
    }
    let zero = DecimalOrder::parse("0").ok_or(MassError::Record)?;
    let hundred = DecimalOrder::parse("100").ok_or(MassError::Record)?;
    let mut ids = HashSet::new();
    let mut element_symbols = HashSet::new();
    let mut selections = HashMap::new();
    for selection in &data.selections {
        if selections
            .insert(selection.element_row_ordinal, selection)
            .is_some()
        {
            return Err(MassError::Inconsistent);
        }
    }
    let mut position = 0_usize;
    for (ordinal, element) in data.elements.iter().enumerate() {
        let count = usize::try_from(element.isotope_rows).map_err(|_| MassError::Inconsistent)?;
        let end = position
            .checked_add(count)
            .filter(|end| *end <= data.isotopes.len())
            .ok_or(MassError::Inconsistent)?;
        if element.element_row_ordinal != ordinal as u64
            || element.first_isotope_row_ordinal != position as u64
            || !ids.insert(element.atomic_number_u32)
            || !element_symbols.insert(element.symbol.as_str())
            || element.natural_atomic_weight.order <= zero
        {
            return Err(MassError::Inconsistent);
        }
        let mut best: Option<&Isotope> = None;
        let mut all_zero = true;
        let mut mass_numbers = HashSet::new();
        for (index, isotope) in data.isotopes[position..end].iter().enumerate() {
            let abundance = &isotope.abundance_percent.order;
            let reference = f64::from_bits(isotope.mass.reference_bits);
            let nominal = f64::from(isotope.mass_number_u32);
            if isotope.isotope_row_ordinal != (position + index) as u64
                || isotope.element_row_ordinal != ordinal as u64
                || isotope.atomic_number_u32 != element.atomic_number_u32
                || isotope.mass.order <= zero
                || abundance < &zero
                || abundance > &hundred
                || !isotope.abundance_token_present && abundance != &zero
                || isotope.mass_number_u32 == 0
                || !mass_numbers.insert(isotope.mass_number_u32)
                || reference < nominal - 0.5
                || reference >= nominal + 0.5
            {
                return Err(MassError::Inconsistent);
            }
            all_zero &= abundance == &zero;
            if best.is_none_or(|current| {
                abundance.cmp(&current.abundance_percent.order) == Ordering::Greater
            }) {
                best = Some(isotope);
            }
        }
        match (best, selections.remove(&(ordinal as u64))) {
            (None, None) => {}
            (Some(chosen), Some(selection))
                if selection.selected_isotope_row_ordinal == chosen.isotope_row_ordinal
                    && selection.atomic_number_u32 == chosen.atomic_number_u32
                    && selection.mass_number_u32 == chosen.mass_number_u32
                    && selection.mass == chosen.mass
                    && selection.all_abundances_zero == all_zero => {}
            _ => return Err(MassError::Inconsistent),
        }
        position = end;
    }
    if position != data.isotopes.len() || !selections.is_empty() {
        return Err(MassError::Inconsistent);
    }
    let mut alias_targets = HashMap::new();
    for alias in &data.aliases {
        let row = usize::try_from(alias.source_isotope_row_ordinal)
            .ok()
            .and_then(|index| data.isotopes.get(index))
            .ok_or(MassError::Inconsistent)?;
        if row.atomic_number_u32 != alias.atomic_number_u32
            || row.mass_number_u32 != alias.mass_number_u32
            || element_symbols.contains(alias.symbol.as_str())
            || alias_targets
                .insert(&alias.symbol, alias.source_isotope_row_ordinal)
                .is_some_and(|previous| previous != alias.source_isotope_row_ordinal)
        {
            return Err(MassError::Inconsistent);
        }
    }
    validate_provenance(data)
}

fn validate_provenance(data: &MassData) -> Result<(), MassError> {
    let source = &data.provenance;
    let coefficient = &source.signed_charge_coefficient;
    if !source.inert_source_lineage_only
        || !source.source_offsets_are_not_native_dispatch
        || source.original_source_or_cfg_included
        || !valid_digest(&source.text_source.sha256)
        || !valid_digest(&source.consumer_source.sha256)
        || !valid_digest(&source.installed_equivalent_copy_sha256)
        || !valid_digest(&coefficient.sha256)
        || coefficient
            .source_file_offset
            .checked_add(coefficient.bytes)
            .is_none_or(|end| end > source.consumer_source.bytes)
        || source
            .consumer_evidence
            .iter()
            .any(|item| !valid_digest(&item.evidence_sha256))
    {
        return Err(MassError::Inconsistent);
    }
    let mut targets = HashSet::new();
    for span in &source.row_source_spans {
        let row = usize::try_from(span.row_ordinal).ok();
        let exists = match span.kind {
            RowKind::Element => row.is_some_and(|index| index < data.elements.len()),
            RowKind::Isotope => row.is_some_and(|index| index < data.isotopes.len()),
        };
        if !exists
            || !targets.insert((span.kind, span.row_ordinal))
            || span.source_line_1based == 0
            || !valid_digest(&span.source_line_sha256)
            || span
                .source_file_offset
                .checked_add(span.source_bytes_including_newline)
                .is_none_or(|end| end > source.text_source.bytes)
        {
            return Err(MassError::Inconsistent);
        }
    }
    Ok(())
}

fn verify_source_lines(source: &Provenance, raw: &[u8]) -> Result<(), MassError> {
    if raw.len() as u64 > crate::MAX_DATA_FILE_BYTES
        || raw.len() as u64 != source.text_source.bytes
        || format!("{:x}", Sha256::digest(raw)) != source.text_source.sha256
    {
        return Err(MassError::Inconsistent);
    }
    let wanted: HashSet<_> = source
        .row_source_spans
        .iter()
        .map(|span| span.source_line_1based)
        .collect();
    let mut found = HashMap::new();
    let mut offset = 0_u64;
    for (index, line) in raw.split_inclusive(|byte| *byte == b'\n').enumerate() {
        let number = index as u64 + 1;
        if wanted.contains(&number) {
            found.insert(
                number,
                (
                    offset,
                    line.len() as u64,
                    format!("{:x}", Sha256::digest(line)),
                ),
            );
        }
        offset += line.len() as u64;
    }
    for span in &source.row_source_spans {
        let Some((offset, length, digest)) = found.get(&span.source_line_1based) else {
            return Err(MassError::Inconsistent);
        };
        if *offset != span.source_file_offset
            || *length != span.source_bytes_including_newline
            || digest != &span.source_line_sha256
        {
            return Err(MassError::Inconsistent);
        }
    }
    Ok(())
}
