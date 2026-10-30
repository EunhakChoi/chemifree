//! Declarative mass-profile inputs. Decoding them does not compute a mass.

use std::{cmp::Ordering, collections::HashSet};

use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Number, Value};
use sha2::{Digest, Sha256};

use crate::{json, phase6_profiles};

#[path = "mass_profile/decimal_order.rs"]
mod decimal_order;
use decimal_order::DecimalKey;

/// Payload-free reason the complete file set could not be decoded.
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

pub(crate) const FEATURES_PATH: &str = "feature-schema.json";
pub(crate) const REQUIRED_FILES: [&str; 7] = [
    "parameters/elements.json",
    "parameters/isotopes.json",
    "parameters/default-monoisotopic-masses.json",
    "parameters/aliases.json",
    "parameters/mass-constants.json",
    FEATURES_PATH,
    "source-bounds.json",
];
pub(crate) const RAW_PATH: &str = "raw/CS Isotopes Table.txt";

/// Compare the actual metadata bytes with the identities in the public catalog.
///
/// # Errors
/// Returns `CatalogMismatch` if an identity or a file digest differs.
pub fn verify_catalog_files(
    dataset_id: &str,
    dataset_version: &str,
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
    if dataset_id != expected.dataset_id || dataset_version != expected.dataset_version {
        return Err(MassError::CatalogMismatch);
    }
    expected
        .check_file_bytes(manifest, registry, profile)
        .map_err(|_| MassError::CatalogMismatch)
}

/// An authoritative source token and the separately declared binary64 witness.
#[derive(PartialEq)]
pub struct DecimalOperand {
    pub source_decimal: String,
    pub reference_bits: u64,
}

/// One ordered element record and the span of its isotope records.
#[derive(PartialEq)]
pub struct Element {
    pub element_row_ordinal: u64,
    pub atomic_number_u32: u32,
    pub symbol: String,
    pub natural_atomic_weight: DecimalOperand,
    pub permitted_valences_u32: Vec<u32>,
    pub first_isotope_row_ordinal: u64,
    pub isotope_rows: u64,
}

/// One ordered isotope record, retaining absence of an abundance token.
#[derive(PartialEq)]
pub struct Isotope {
    pub isotope_row_ordinal: u64,
    pub element_row_ordinal: u64,
    pub atomic_number_u32: u32,
    pub mass_number_u32: u32,
    pub mass: DecimalOperand,
    pub abundance_percent: DecimalOperand,
    pub abundance_token_present: bool,
    pub nickname: Option<String>,
}

/// A static selection view, not a capture of a running engine.
#[derive(PartialEq)]
pub struct DefaultMassRow {
    pub element_row_ordinal: u64,
    pub atomic_number_u32: u32,
    pub selected_isotope_row_ordinal: u64,
    pub mass_number_u32: u32,
    pub mass: DecimalOperand,
    pub all_abundances_zero: bool,
}

#[derive(PartialEq)]
pub struct DefaultMasses {
    pub selection_is_static_table_analysis_not_runtime_capture: bool,
    pub rows: Vec<DefaultMassRow>,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Alias {
    pub symbol: String,
    pub atomic_number_u32: u32,
    pub mass_number_u32: u32,
    pub source_isotope_row_ordinal: u64,
}

#[derive(PartialEq)]
pub struct UnitOperand {
    pub bits: u64,
    pub unit: String,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SecondaryMetadata {
    pub value: Number,
    pub unit: String,
}

/// Feature identity and units, separate from any future computational engine.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FeatureMetadata {
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
    pub native_selected_input_secondary: SecondaryMetadata,
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

/// Kinds for which the reader can resolve a source line to a decoded row.
#[derive(Clone, Copy, Deserialize, Eq, Hash, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum LineKind {
    Element,
    Isotope,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LineSpan {
    pub kind: LineKind,
    pub row_ordinal: u64,
    pub source_line_1based: u64,
    pub source_file_offset: u64,
    pub source_bytes_including_newline: u64,
    pub source_line_sha256: String,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EvidenceIdentity {
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

/// Inert provenance; its paths and offsets are never opened or executed.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SourceBounds {
    pub binding_id: String,
    pub inert_source_lineage_only: bool,
    pub text_source: SourceIdentity,
    pub installed_equivalent_copy_sha256: String,
    pub row_source_spans: Vec<LineSpan>,
    pub consumer_source: SourceIdentity,
    pub consumer_evidence: Vec<EvidenceIdentity>,
    pub signed_charge_coefficient: ByteSpan,
    pub source_offsets_are_not_native_dispatch: bool,
    #[serde(rename = "original_source_or_CFG_included")]
    pub original_source_or_cfg_included: bool,
}

/// A complete immutable collection of inert operands and provenance.
#[derive(PartialEq)]
pub struct MassData {
    pub elements: Vec<Element>,
    pub isotopes: Vec<Isotope>,
    pub defaults: DefaultMasses,
    pub aliases: Vec<Alias>,
    pub charge_mass_coefficient: UnitOperand,
    pub features: FeatureMetadata,
    pub source_bounds: SourceBounds,
}

/// Decode the seven required files and an optional raw source as one value.
///
/// The caller must authorize and verify the bytes before calling this function.
///
/// # Errors
/// Rejects incomplete files, invalid numeric witnesses, or inconsistent references.
pub fn decode_mass(files: &[(&str, &[u8])]) -> Result<MassData, MassError> {
    if !matches!(files.len(), 7 | 8) {
        return Err(MassError::FileSet);
    }
    let raw = if files.len() == 8 {
        Some(
            files
                .iter()
                .find(|(name, _)| *name == RAW_PATH)
                .ok_or(MassError::FileSet)?
                .1,
        )
    } else {
        None
    };
    let file = |path| {
        let mut matching = files.iter().filter(|(name, _)| *name == path);
        let bytes = matching.next().ok_or(MassError::FileSet)?.1;
        if matching.next().is_some() {
            return Err(MassError::FileSet);
        }
        Ok(bytes)
    };
    let elements: ElementsWire = decode(file(REQUIRED_FILES[0])?)?;
    let isotopes: IsotopesWire = decode(file(REQUIRED_FILES[1])?)?;
    let defaults: DefaultsWire = decode(file(REQUIRED_FILES[2])?)?;
    let aliases: AliasesWire = decode(file(REQUIRED_FILES[3])?)?;
    let constants: ConstantsWire = decode(file(REQUIRED_FILES[4])?)?;
    let features = decode(file(REQUIRED_FILES[5])?)?;
    let source_bounds = decode(file(REQUIRED_FILES[6])?)?;
    let charge_bytes = serde_json::to_vec(&constants.signed_charge_mass_coefficient)
        .map_err(|_| MassError::Record)?;
    let charge =
        phase6_profiles::decode_f64_with_unit(&charge_bytes).map_err(|_| MassError::Record)?;
    let data = MassData {
        elements: elements
            .elements
            .into_iter()
            .map(ElementWire::convert)
            .collect::<Result<_, _>>()?,
        isotopes: isotopes
            .isotopes
            .into_iter()
            .map(IsotopeWire::convert)
            .collect::<Result<_, _>>()?,
        defaults: DefaultMasses {
            selection_is_static_table_analysis_not_runtime_capture: defaults
                .selection_is_static_table_analysis_not_runtime_capture,
            rows: defaults
                .rows
                .into_iter()
                .map(DefaultWire::convert)
                .collect::<Result<_, _>>()?,
        },
        aliases: aliases.aliases,
        charge_mass_coefficient: UnitOperand {
            bits: charge.value.bits,
            unit: charge.unit,
        },
        features,
        source_bounds,
    };
    validate(&data)?;
    if let Some(raw) = raw {
        validate_raw_source(&data.source_bounds, raw)?;
    }
    Ok(data)
}

fn validate_raw_source(bounds: &SourceBounds, raw: &[u8]) -> Result<(), MassError> {
    if raw.len() as u64 > crate::MAX_DATA_FILE_BYTES
        || raw.len() as u64 != bounds.text_source.bytes
        || format!("{:x}", Sha256::digest(raw)) != bounds.text_source.sha256
    {
        return Err(MassError::Inconsistent);
    }
    let mut spans: Vec<_> = bounds.row_source_spans.iter().collect();
    spans.sort_unstable_by_key(|span| span.source_line_1based);
    let mut number = 1_u64;
    let mut start = 0_usize;
    let mut previous_line: Option<(u64, usize, String)> = None;
    for span in spans {
        while number < span.source_line_1based {
            if start >= raw.len() {
                return Err(MassError::Inconsistent);
            }
            start += raw[start..]
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(raw.len() - start, |offset| offset + 1);
            number += 1;
        }
        if start >= raw.len() {
            return Err(MassError::Inconsistent);
        }
        if previous_line
            .as_ref()
            .is_none_or(|(previous, _, _)| *previous != number)
        {
            let end = start
                + raw[start..]
                    .iter()
                    .position(|byte| *byte == b'\n')
                    .map_or(raw.len() - start, |offset| offset + 1);
            previous_line = Some((
                number,
                end,
                format!("{:x}", Sha256::digest(&raw[start..end])),
            ));
        }
        let (_, end, digest) = previous_line.as_ref().ok_or(MassError::Inconsistent)?;
        if span.source_file_offset != start as u64
            || span.source_bytes_including_newline != (*end - start) as u64
            || digest != &span.source_line_sha256
        {
            return Err(MassError::Inconsistent);
        }
    }
    Ok(())
}

fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, MassError> {
    if bytes.len() as u64 > crate::MAX_DATA_FILE_BYTES || bytes.starts_with(b"\xef\xbb\xbf") {
        return Err(MassError::Record);
    }
    let value = json::parse_integer_zero_strict(bytes).map_err(|_| MassError::Record)?;
    serde_json::from_value(value).map_err(|_| MassError::Record)
}

fn decimal(value: Value) -> Result<DecimalOperand, MassError> {
    let bytes = serde_json::to_vec(&value).map_err(|_| MassError::Record)?;
    let value = phase6_profiles::decode_decimal_reference(&bytes).map_err(|_| MassError::Record)?;
    Ok(DecimalOperand {
        source_decimal: value.source_decimal,
        reference_bits: value.reference.bits,
    })
}

fn is_zero_decimal(value: &str) -> bool {
    value
        .split(['e', 'E'])
        .next()
        .is_some_and(|part| !part.bytes().any(|b| (b'1'..=b'9').contains(&b)))
}

fn is_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn validate(data: &MassData) -> Result<(), MassError> {
    let meta = &data.features;
    if meta.element_rows != data.elements.len() as u64
        || meta.isotope_rows != data.isotopes.len() as u64
        || meta.alias_rows != data.aliases.len() as u64
        || !meta.source_decimal_tokens_authoritative
        || !meta.reference_f64_decode_is_independent_not_original_runtime_capture
        || meta.source_mass_number_field_explicit_in_text
        || !meta.mass_number_values_independently_checked_against_source_constructor_contract
        || meta.native_selected_input_secondary.value.as_f64() != Some(0.0)
        || f64::from_bits(data.charge_mass_coefficient.bits) >= 0.0
        || meta.original_parser_or_selection_code_included
        || meta.original_naming_grammar_or_module_dispatch_included
        || !data
            .defaults
            .selection_is_static_table_analysis_not_runtime_capture
    {
        return Err(MassError::Inconsistent);
    }
    let mut atomic_numbers = HashSet::new();
    let zero = DecimalKey::new("0");
    let mut start = 0usize;
    for (index, element) in data.elements.iter().enumerate() {
        let count = usize::try_from(element.isotope_rows).map_err(|_| MassError::Inconsistent)?;
        let end = start
            .checked_add(count)
            .filter(|end| *end <= data.isotopes.len())
            .ok_or(MassError::Inconsistent)?;
        if element.element_row_ordinal != index as u64
            || element.first_isotope_row_ordinal != start as u64
            || !atomic_numbers.insert(element.atomic_number_u32)
        {
            return Err(MassError::Inconsistent);
        }
        for (ordinal, isotope) in data.isotopes[start..end].iter().enumerate() {
            let mass = f64::from_bits(isotope.mass.reference_bits);
            let nominal = f64::from(isotope.mass_number_u32);
            if isotope.isotope_row_ordinal != (start + ordinal) as u64
                || isotope.element_row_ordinal != index as u64
                || isotope.atomic_number_u32 != element.atomic_number_u32
                || DecimalKey::new(&isotope.mass.source_decimal).compare(&zero) != Ordering::Greater
                || isotope.mass_number_u32 == 0
                || mass < nominal - 0.5
                || mass >= nominal + 0.5
                || DecimalKey::new(&isotope.abundance_percent.source_decimal).compare(&zero)
                    == Ordering::Less
                || !isotope.abundance_token_present
                    && !is_zero_decimal(&isotope.abundance_percent.source_decimal)
            {
                return Err(MassError::Inconsistent);
            }
        }
        start = end;
    }
    if start != data.isotopes.len() {
        return Err(MassError::Inconsistent);
    }
    let isotope = |ordinal: u64| {
        usize::try_from(ordinal)
            .ok()
            .and_then(|i| data.isotopes.get(i))
    };
    let mut selected = HashSet::new();
    for row in &data.defaults.rows {
        let selected_isotope =
            isotope(row.selected_isotope_row_ordinal).ok_or(MassError::Inconsistent)?;
        let element = usize::try_from(row.element_row_ordinal)
            .ok()
            .and_then(|i| data.elements.get(i))
            .ok_or(MassError::Inconsistent)?;
        if !selected.insert(row.element_row_ordinal)
            || selected_isotope.element_row_ordinal != row.element_row_ordinal
            || selected_isotope.atomic_number_u32 != row.atomic_number_u32
            || selected_isotope.mass_number_u32 != row.mass_number_u32
            || selected_isotope.mass != row.mass
        {
            return Err(MassError::Inconsistent);
        }
        let start = element.first_isotope_row_ordinal as usize;
        let end = start + element.isotope_rows as usize;
        let mut best: Option<(u64, DecimalKey)> = None;
        for candidate in &data.isotopes[start..end] {
            let key = DecimalKey::new(&candidate.abundance_percent.source_decimal);
            if best
                .as_ref()
                .is_none_or(|(_, current)| key.compare(current) == Ordering::Greater)
            {
                best = Some((candidate.isotope_row_ordinal, key));
            }
        }
        if best.as_ref().map(|(ordinal, _)| *ordinal) != Some(row.selected_isotope_row_ordinal) {
            return Err(MassError::Inconsistent);
        }
        if row.all_abundances_zero
            != data.isotopes[start..end]
                .iter()
                .all(|i| is_zero_decimal(&i.abundance_percent.source_decimal))
        {
            return Err(MassError::Inconsistent);
        }
    }
    if selected.len() != data.elements.iter().filter(|e| e.isotope_rows != 0).count() {
        return Err(MassError::Inconsistent);
    }
    for row in &data.aliases {
        let target = isotope(row.source_isotope_row_ordinal).ok_or(MassError::Inconsistent)?;
        if target.atomic_number_u32 != row.atomic_number_u32
            || target.mass_number_u32 != row.mass_number_u32
        {
            return Err(MassError::Inconsistent);
        }
    }
    let bounds = &data.source_bounds;
    let signed = &bounds.signed_charge_coefficient;
    if !bounds.inert_source_lineage_only
        || !bounds.source_offsets_are_not_native_dispatch
        || bounds.original_source_or_cfg_included
        || !is_hash(&bounds.text_source.sha256)
        || !is_hash(&bounds.consumer_source.sha256)
        || !is_hash(&bounds.installed_equivalent_copy_sha256)
        || !is_hash(&signed.sha256)
        || signed
            .source_file_offset
            .checked_add(signed.bytes)
            .is_none_or(|end| end > bounds.consumer_source.bytes)
        || bounds
            .consumer_evidence
            .iter()
            .any(|item| !is_hash(&item.evidence_sha256))
        || bounds.row_source_spans.iter().any(|span| {
            span.source_line_1based == 0
                || !is_hash(&span.source_line_sha256)
                || span
                    .source_file_offset
                    .checked_add(span.source_bytes_including_newline)
                    .is_none_or(|end| end > bounds.text_source.bytes)
        })
    {
        return Err(MassError::Inconsistent);
    }
    let mut referenced_rows = HashSet::new();
    for span in &bounds.row_source_spans {
        let row = usize::try_from(span.row_ordinal).ok();
        let exists = match span.kind {
            LineKind::Element => row.is_some_and(|index| index < data.elements.len()),
            LineKind::Isotope => row.is_some_and(|index| index < data.isotopes.len()),
        };
        if !exists || !referenced_rows.insert((span.kind, span.row_ordinal)) {
            return Err(MassError::Inconsistent);
        }
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ElementsWire {
    elements: Vec<ElementWire>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ElementWire {
    element_row_ordinal: u64,
    atomic_number_u32: u32,
    symbol: String,
    natural_atomic_weight: Value,
    permitted_valences_u32: Vec<u32>,
    first_isotope_row_ordinal: u64,
    isotope_rows: u64,
}
impl ElementWire {
    fn convert(self) -> Result<Element, MassError> {
        Ok(Element {
            element_row_ordinal: self.element_row_ordinal,
            atomic_number_u32: self.atomic_number_u32,
            symbol: self.symbol,
            natural_atomic_weight: decimal(self.natural_atomic_weight)?,
            permitted_valences_u32: self.permitted_valences_u32,
            first_isotope_row_ordinal: self.first_isotope_row_ordinal,
            isotope_rows: self.isotope_rows,
        })
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IsotopesWire {
    isotopes: Vec<IsotopeWire>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IsotopeWire {
    isotope_row_ordinal: u64,
    element_row_ordinal: u64,
    atomic_number_u32: u32,
    mass_number_u32: u32,
    mass: Value,
    abundance_percent: Value,
    abundance_token_present: bool,
    nickname: Value,
}
impl IsotopeWire {
    fn convert(self) -> Result<Isotope, MassError> {
        Ok(Isotope {
            isotope_row_ordinal: self.isotope_row_ordinal,
            element_row_ordinal: self.element_row_ordinal,
            atomic_number_u32: self.atomic_number_u32,
            mass_number_u32: self.mass_number_u32,
            mass: decimal(self.mass)?,
            abundance_percent: decimal(self.abundance_percent)?,
            abundance_token_present: self.abundance_token_present,
            nickname: match self.nickname {
                Value::Null => None,
                Value::String(value) => Some(value),
                _ => return Err(MassError::Record),
            },
        })
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DefaultsWire {
    selection_is_static_table_analysis_not_runtime_capture: bool,
    rows: Vec<DefaultWire>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DefaultWire {
    element_row_ordinal: u64,
    atomic_number_u32: u32,
    selected_isotope_row_ordinal: u64,
    mass_number_u32: u32,
    mass: Value,
    all_abundances_zero: bool,
}
impl DefaultWire {
    fn convert(self) -> Result<DefaultMassRow, MassError> {
        Ok(DefaultMassRow {
            element_row_ordinal: self.element_row_ordinal,
            atomic_number_u32: self.atomic_number_u32,
            selected_isotope_row_ordinal: self.selected_isotope_row_ordinal,
            mass_number_u32: self.mass_number_u32,
            mass: decimal(self.mass)?,
            all_abundances_zero: self.all_abundances_zero,
        })
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AliasesWire {
    aliases: Vec<Alias>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConstantsWire {
    signed_charge_mass_coefficient: Value,
}
