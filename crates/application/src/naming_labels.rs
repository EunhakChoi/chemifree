//! Inert labels from the independently selected phase 6 naming DATA profile.

use num_bigint::BigUint;
use serde::Deserialize;

use crate::{json, phase6_profiles};

pub(crate) const KIND_PATH: &str = "normalized/kind-labels.jsonl";
pub(crate) const SUBTYPE_PATH: &str = "normalized/subtype-labels.jsonl";
pub(crate) const REQUIRED_FILES: [&str; 2] = [KIND_PATH, SUBTYPE_PATH];

/// Payload-free failure during file selection, decoding, or catalog validation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LabelError {
    FileSet,
    Framing,
    Record,
    CatalogMismatch,
}

impl std::fmt::Display for LabelError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::FileSet => "invalid_file_set",
            Self::Framing => "invalid_data_framing",
            Self::Record => "invalid_data_record",
            Self::CatalogMismatch => "catalog_identity_mismatch",
        })
    }
}

impl std::error::Error for LabelError {}

/// A source label with its original bytes, code, order and inert lineage.
#[derive(PartialEq)]
pub struct Label {
    pub source_ordinal: BigUint,
    pub code: u32,
    pub label_ascii: String,
    pub label_bytes: Vec<u8>,
    pub source_row_file_offset: BigUint,
    pub source_label_file_offset: BigUint,
    pub source_row_sha256: String,
}

/// Distinct kind and subtype contexts; neither is an executable dispatch map.
#[derive(PartialEq)]
pub struct NamingLabels {
    pub kind: Vec<Label>,
    pub subtype: Vec<Label>,
}

/// Check actual metadata bytes and dataset identity against the public catalog.
///
/// # Errors
/// Returns `CatalogMismatch` for any identity or file digest mismatch.
pub fn verify_catalog_files(
    dataset_id: &str,
    dataset_version: &str,
    manifest: &[u8],
    registry: &[u8],
    profile: &[u8],
) -> Result<(), LabelError> {
    let expected = phase6_profiles::CatalogIdentity {
        dataset_id: "legacy-naming-enum-labels",
        dataset_version: "sha256-set-5eafac8138576244941e5aec63ad04175e7dadd0f1c5f441fbac8ab044278134",
        registry_sha256: "b08d5a85f1fbe80193252ce863c8912f970274945ab89e38e4262348842316f2",
        profile_sha256: "a27a911d251c319a0cff840db4b87090dc6f1fd5521e5a66527adca55db2f9b1",
        manifest_file_sha256: "416484d8719758c85e61db6bbc01a2dd96fb680b988b51934eabb0d9b02e6635",
    };
    if dataset_id != expected.dataset_id || dataset_version != expected.dataset_version {
        return Err(LabelError::CatalogMismatch);
    }
    expected
        .check_file_bytes(manifest, registry, profile)
        .map_err(|_| LabelError::CatalogMismatch)
}

/// Decode both selected files after caller authorization and exact-byte verification.
///
/// # Errors
/// Rejects missing or duplicate files, invalid framing, and malformed records.
pub fn decode_labels(files: &[(&str, &[u8])]) -> Result<NamingLabels, LabelError> {
    if files.len() != REQUIRED_FILES.len() {
        return Err(LabelError::FileSet);
    }
    let file = |path| {
        let mut matches = files.iter().filter(|(name, _)| *name == path);
        let bytes = matches.next().ok_or(LabelError::FileSet)?.1;
        if matches.next().is_some() {
            return Err(LabelError::FileSet);
        }
        Ok(bytes)
    };
    Ok(NamingLabels {
        kind: decode_rows(file(KIND_PATH)?)?,
        subtype: decode_rows(file(SUBTYPE_PATH)?)?,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LabelWire {
    #[serde(deserialize_with = "json::deserialize_integer")]
    source_ordinal_0based: BigUint,
    #[serde(deserialize_with = "json::deserialize_integer")]
    code_u32: u32,
    label_ascii: String,
    label_bytes_hex: String,
    #[serde(deserialize_with = "json::deserialize_integer")]
    source_row_file_offset: BigUint,
    #[serde(deserialize_with = "json::deserialize_integer")]
    source_label_file_offset: BigUint,
    source_row_sha256: String,
}

fn decode_rows(bytes: &[u8]) -> Result<Vec<Label>, LabelError> {
    if bytes.len() as u64 > crate::MAX_DATA_FILE_BYTES {
        return Err(LabelError::Framing);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| LabelError::Framing)?;
    if text.starts_with('\u{feff}') || !text.ends_with('\n') {
        return Err(LabelError::Framing);
    }
    let body = &text[..text.len() - 1];
    if body.is_empty() {
        return Err(LabelError::Framing);
    }
    let mut rows = Vec::new();
    for line in body.split('\n') {
        if line.trim().is_empty() {
            return Err(LabelError::Framing);
        }
        json::validate_raw_strict(line.as_bytes()).map_err(|_| LabelError::Record)?;
        let wire: LabelWire = serde_json::from_str(line).map_err(|_| LabelError::Record)?;
        if wire.source_row_sha256.len() != 64
            || !wire
                .source_row_sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(LabelError::Record);
        }
        let label_bytes = phase6_profiles::decode_ascii_pair(
            Some(&wire.label_bytes_hex),
            Some(&wire.label_ascii),
        )
        .map_err(|_| LabelError::Record)?
        .ok_or(LabelError::Record)?;
        rows.push(Label {
            source_ordinal: wire.source_ordinal_0based,
            code: wire.code_u32,
            label_ascii: wire.label_ascii,
            label_bytes,
            source_row_file_offset: wire.source_row_file_offset,
            source_label_file_offset: wire.source_label_file_offset,
            source_row_sha256: wire.source_row_sha256,
        });
    }
    Ok(rows)
}
