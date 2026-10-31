//! Inert naming pattern inputs, without a matcher or predicate implementation.

use std::collections::HashSet;

use num_bigint::BigUint;
use serde::{Deserialize, Deserializer, de::DeserializeOwned};

use crate::{json, phase6_profiles};

pub(crate) const QUERY_PATH: &str = "normalized/query-graph-patterns.jsonl";
pub(crate) const RETAINED_PATH: &str = "normalized/retained-name-patterns.jsonl";

/// Selects one of two separate catalog-bound formats.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Profile {
    Query,
    Retained,
}

/// Payload-free reason a pattern stream or catalog could not be read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PatternError {
    FileSet,
    Framing,
    Record,
    CatalogMismatch,
}

impl std::fmt::Display for PatternError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::FileSet => "invalid_file_set",
            Self::Framing => "invalid_data_framing",
            Self::Record => "invalid_data_record",
            Self::CatalogMismatch => "catalog_identity_mismatch",
        })
    }
}

impl std::error::Error for PatternError {}

impl Profile {
    fn identity(self) -> phase6_profiles::CatalogIdentity<'static> {
        match self {
            Self::Query => phase6_profiles::CatalogIdentity {
                dataset_id: "legacy-naming-query-graph-patterns",
                dataset_version: "sha256-set-fa570fa71c8e49035db6427ae5f5cdc68937ea4ab5ca2e82d64561c18d2d95b4",
                registry_sha256: "429987746075bf63e1b624e102340c0933f084a592e87259b7843f1a0240e3ad",
                profile_sha256: "bd84e917262e0f4c358f0be875682579c40427fa9201f71924c94242597a3e99",
                manifest_file_sha256: "d4c843f09ec23e9d221df70640f257c33d0c826f829ca528c4fcbdc8d2e00422",
            },
            Self::Retained => phase6_profiles::CatalogIdentity {
                dataset_id: "legacy-naming-retained-graph-patterns",
                dataset_version: "sha256-set-cfb579548ea719a1b218d72f5614e304cf7274f5c0934512c8af669754807cd8",
                registry_sha256: "c5450e5abe8b511a06d5819cdccfc64ea5f18bc34e1dcaeaaf1d7c006298d463",
                profile_sha256: "af4dd83f84852aa84e395c2272445460f6f7995cd3fab23d189967e21384bdce",
                manifest_file_sha256: "6eeb16f901048e42d145614badfb6c906e76ce0e12de72eece1348c89bba3b83",
            },
        }
    }
}

/// Compare the actual metadata files to the independently published catalog.
///
/// # Errors
/// Returns `CatalogMismatch` when an identity or byte digest differs.
pub fn verify_catalog_files(
    profile: Profile,
    dataset_id: &str,
    dataset_version: &str,
    manifest: &[u8],
    registry: &[u8],
    format_profile: &[u8],
) -> Result<(), PatternError> {
    let expected = profile.identity();
    if dataset_id != expected.dataset_id || dataset_version != expected.dataset_version {
        return Err(PatternError::CatalogMismatch);
    }
    expected
        .check_file_bytes(manifest, registry, format_profile)
        .map_err(|_| PatternError::CatalogMismatch)
}

/// A verified ASCII pair retaining both original bytes and declared text.
#[derive(PartialEq)]
pub struct AsciiText {
    pub text: String,
    pub bytes: Vec<u8>,
}

/// Inert source lineage; offsets are not executable addresses or references.
#[derive(PartialEq)]
pub struct Source {
    pub ordinal: BigUint,
    pub offset: BigUint,
    pub bytes: BigUint,
    pub sha256: String,
}

/// A graph-property declaration. Its predicate is not implemented by this reader.
#[derive(PartialEq)]
pub struct QueryAnnotation {
    pub subject: String,
    pub selection: String,
    pub source_index: Option<BigUint>,
    pub property_or_predicate_id: String,
    pub requires_independent_named_predicate: bool,
}

/// One query carrier. Required predicate IDs remain explicit and unresolved here.
#[derive(PartialEq)]
pub struct QueryPattern {
    pub source: Source,
    pub raw_definition: AsciiText,
    pub smiles: Option<AsciiText>,
    pub source_graph_placeholder: Option<String>,
    pub annotations: Vec<QueryAnnotation>,
    pub named_match_or_generation_predicate_id: Option<String>,
    pub required_predicate_ids: Vec<String>,
}

/// A retained-name annotation; a null source index is different from zero.
#[derive(PartialEq)]
pub struct RetainedAnnotation {
    pub subject: String,
    pub source_index: Option<BigUint>,
    pub property: String,
    pub value: Option<AsciiText>,
}

/// One retained-name carrier, with flags and names preserved independently.
#[derive(PartialEq)]
pub struct RetainedPattern {
    pub source: Source,
    pub raw_definition: AsciiText,
    pub base_category: String,
    pub smiles: AsciiText,
    pub annotations: Vec<RetainedAnnotation>,
    pub allow_substituents: bool,
    pub interpret_stereochemistry: bool,
    pub is_full_name: bool,
    pub has_explicit_substituent_name: bool,
    pub retained_name: AsciiText,
    pub substituent_name: Option<AsciiText>,
}

/// A complete selected profile; decoding does not bind or evaluate its predicates.
#[derive(PartialEq)]
pub enum PatternRows {
    Query(Vec<QueryPattern>),
    Retained(Vec<RetainedPattern>),
}

/// Select the required file for one profile and decode it as an immutable set.
///
/// # Errors
/// Rejects an incomplete or mismatched file set and any invalid record.
pub fn decode_dataset(
    profile: Profile,
    files: &[(&str, &[u8])],
) -> Result<PatternRows, PatternError> {
    if files.len() != 1 {
        return Err(PatternError::FileSet);
    }
    match (profile, files[0]) {
        (Profile::Query, (QUERY_PATH, bytes)) => decode_query(bytes).map(PatternRows::Query),
        (Profile::Retained, (RETAINED_PATH, bytes)) => {
            decode_retained(bytes).map(PatternRows::Retained)
        }
        _ => Err(PatternError::FileSet),
    }
}

/// Decode one authorized query stream. No predicate or graph evaluation is performed.
///
/// # Errors
/// Rejects invalid framing, missing fields, byte/text pairs or malformed provenance.
pub fn decode_query(bytes: &[u8]) -> Result<Vec<QueryPattern>, PatternError> {
    let rows = decode_rows::<QueryWire>(bytes)?
        .into_iter()
        .map(QueryWire::convert)
        .collect::<Result<Vec<_>, _>>()?;
    unique_ordinals(rows, |row| &row.source.ordinal)
}

/// Decode one authorized retained-name stream without interpreting its flags.
///
/// # Errors
/// Rejects invalid framing, missing fields, byte/text pairs or malformed provenance.
pub fn decode_retained(bytes: &[u8]) -> Result<Vec<RetainedPattern>, PatternError> {
    let rows = decode_rows::<RetainedWire>(bytes)?
        .into_iter()
        .map(RetainedWire::convert)
        .collect::<Result<Vec<_>, _>>()?;
    unique_ordinals(rows, |row| &row.source.ordinal)
}

fn unique_ordinals<T>(
    rows: Vec<T>,
    ordinal: impl Fn(&T) -> &BigUint,
) -> Result<Vec<T>, PatternError> {
    let mut seen = HashSet::with_capacity(rows.len());
    if rows.iter().any(|row| !seen.insert(ordinal(row))) {
        return Err(PatternError::Record);
    }
    drop(seen);
    Ok(rows)
}

fn decode_rows<T: DeserializeOwned>(bytes: &[u8]) -> Result<Vec<T>, PatternError> {
    if bytes.len() as u64 > crate::MAX_DATA_FILE_BYTES {
        return Err(PatternError::Framing);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| PatternError::Framing)?;
    if text.starts_with('\u{feff}') || !text.ends_with('\n') {
        return Err(PatternError::Framing);
    }
    let body = &text[..text.len() - 1];
    if body.is_empty() {
        return Err(PatternError::Framing);
    }
    body.split('\n')
        .map(|line| {
            if line.trim().is_empty() {
                return Err(PatternError::Framing);
            }
            json::validate_raw_strict(line.as_bytes()).map_err(|_| PatternError::Record)?;
            serde_json::from_str(line).map_err(|_| PatternError::Record)
        })
        .collect()
}

fn required_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

fn ascii(hex: String, text: String) -> Result<AsciiText, PatternError> {
    let bytes = phase6_profiles::decode_ascii_pair(Some(&hex), Some(&text))
        .map_err(|_| PatternError::Record)?
        .ok_or(PatternError::Record)?;
    Ok(AsciiText { text, bytes })
}

fn optional_ascii(
    hex: Option<String>,
    text: Option<String>,
) -> Result<Option<AsciiText>, PatternError> {
    match (hex, text) {
        (None, None) => Ok(None),
        (Some(hex), Some(text)) => ascii(hex, text).map(Some),
        _ => Err(PatternError::Record),
    }
}

fn source(
    ordinal: BigUint,
    offset: BigUint,
    bytes: BigUint,
    sha256: String,
) -> Result<Source, PatternError> {
    if sha256.len() != 64
        || !sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(PatternError::Record);
    }
    Ok(Source {
        ordinal,
        offset,
        bytes,
        sha256,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QueryAnnotationWire {
    subject: String,
    selection: String,
    #[serde(deserialize_with = "json::deserialize_nullable_integer")]
    source_index_0based: Option<BigUint>,
    property_or_predicate_id: String,
    requires_independent_named_predicate: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QueryWire {
    #[serde(deserialize_with = "json::deserialize_integer")]
    entry_ordinal_0based: BigUint,
    #[serde(deserialize_with = "json::deserialize_integer")]
    source_file_offset: BigUint,
    #[serde(deserialize_with = "json::deserialize_integer")]
    source_bytes: BigUint,
    source_sha256: String,
    raw_definition_bytes_hex: String,
    raw_definition_ascii: String,
    #[serde(deserialize_with = "required_option")]
    smiles_ascii: Option<String>,
    #[serde(deserialize_with = "required_option")]
    smiles_bytes_hex: Option<String>,
    #[serde(deserialize_with = "required_option")]
    source_graph_placeholder: Option<String>,
    graph_property_annotations: Vec<QueryAnnotationWire>,
    #[serde(deserialize_with = "required_option")]
    named_match_or_generation_predicate_id: Option<String>,
    required_independent_predicate_ids: Vec<String>,
}

impl QueryWire {
    fn convert(self) -> Result<QueryPattern, PatternError> {
        Ok(QueryPattern {
            source: source(
                self.entry_ordinal_0based,
                self.source_file_offset,
                self.source_bytes,
                self.source_sha256,
            )?,
            raw_definition: ascii(self.raw_definition_bytes_hex, self.raw_definition_ascii)?,
            smiles: optional_ascii(self.smiles_bytes_hex, self.smiles_ascii)?,
            source_graph_placeholder: self.source_graph_placeholder,
            annotations: self
                .graph_property_annotations
                .into_iter()
                .map(|row| QueryAnnotation {
                    subject: row.subject,
                    selection: row.selection,
                    source_index: row.source_index_0based,
                    property_or_predicate_id: row.property_or_predicate_id,
                    requires_independent_named_predicate: row.requires_independent_named_predicate,
                })
                .collect(),
            named_match_or_generation_predicate_id: self.named_match_or_generation_predicate_id,
            required_predicate_ids: self.required_independent_predicate_ids,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RetainedAnnotationWire {
    subject: String,
    #[serde(deserialize_with = "json::deserialize_nullable_integer")]
    source_index_0based: Option<BigUint>,
    property: String,
    #[serde(deserialize_with = "required_option")]
    value_bytes_hex: Option<String>,
    #[serde(deserialize_with = "required_option")]
    value_ascii: Option<String>,
}

impl RetainedAnnotationWire {
    fn convert(self) -> Result<RetainedAnnotation, PatternError> {
        Ok(RetainedAnnotation {
            subject: self.subject,
            source_index: self.source_index_0based,
            property: self.property,
            value: optional_ascii(self.value_bytes_hex, self.value_ascii)?,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RetainedWire {
    #[serde(deserialize_with = "json::deserialize_integer")]
    entry_ordinal_0based: BigUint,
    #[serde(deserialize_with = "json::deserialize_integer")]
    source_file_offset: BigUint,
    #[serde(deserialize_with = "json::deserialize_integer")]
    source_bytes: BigUint,
    source_sha256: String,
    raw_definition_bytes_hex: String,
    raw_definition_ascii: String,
    base_category: String,
    smiles_bytes_hex: String,
    smiles_ascii: String,
    graph_property_annotations: Vec<RetainedAnnotationWire>,
    allow_substituents: bool,
    interpret_stereochemistry: bool,
    is_full_name: bool,
    has_explicit_substituent_name: bool,
    retained_name_bytes_hex: String,
    retained_name_ascii: String,
    #[serde(deserialize_with = "required_option")]
    substituent_name_bytes_hex: Option<String>,
    #[serde(deserialize_with = "required_option")]
    substituent_name_ascii: Option<String>,
}

impl RetainedWire {
    fn convert(self) -> Result<RetainedPattern, PatternError> {
        Ok(RetainedPattern {
            source: source(
                self.entry_ordinal_0based,
                self.source_file_offset,
                self.source_bytes,
                self.source_sha256,
            )?,
            raw_definition: ascii(self.raw_definition_bytes_hex, self.raw_definition_ascii)?,
            base_category: self.base_category,
            smiles: ascii(self.smiles_bytes_hex, self.smiles_ascii)?,
            annotations: self
                .graph_property_annotations
                .into_iter()
                .map(RetainedAnnotationWire::convert)
                .collect::<Result<_, _>>()?,
            allow_substituents: self.allow_substituents,
            interpret_stereochemistry: self.interpret_stereochemistry,
            is_full_name: self.is_full_name,
            has_explicit_substituent_name: self.has_explicit_substituent_name,
            retained_name: ascii(self.retained_name_bytes_hex, self.retained_name_ascii)?,
            substituent_name: optional_ascii(
                self.substituent_name_bytes_hex,
                self.substituent_name_ascii,
            )?,
        })
    }
}
