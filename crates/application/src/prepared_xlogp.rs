//! Verified prepared binary64 operands, without an XLogP calculation engine.

use crate::{json, phase6_profiles};
use num_bigint::BigUint;
use serde::Deserialize;
use serde_json::Number;

pub(crate) const RAW_FILE_ID: &str = "DATA-RANGE-MOLNET-XLOGP-100-raw_data";
pub(crate) const NORMALIZED_FILE_ID: &str = "DATA-RANGE-MOLNET-XLOGP-100-normalized";

/// Payload-free error while checking a catalog or paired records.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreparedError {
    FileSet,
    Framing,
    Record,
    Inconsistent,
    CatalogMismatch,
}

impl std::fmt::Display for PreparedError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::FileSet => "invalid_file_set",
            Self::Framing => "invalid_data_framing",
            Self::Record => "invalid_data_record",
            Self::Inconsistent => "inconsistent_dataset",
            Self::CatalogMismatch => "catalog_identity_mismatch",
        })
    }
}

impl std::error::Error for PreparedError {}

/// An inert binary64 operand and its two distinct source indices.
#[derive(PartialEq)]
pub struct Weight {
    pub storage_index: u64,
    pub bin_family: String,
    pub family_index: BigUint,
    pub bits: u64,
    /// Parsed JSON number, retained separately from the raw binary witness.
    pub value: Number,
}

/// A validated, complete table, not a classifier or predictor.
#[derive(PartialEq)]
pub struct Weights {
    pub rows: Vec<Weight>,
}

/// Verify exact manifest, source registry and public format-catalog bytes.
///
/// # Errors
/// Returns `CatalogMismatch` for a differing identity, size limit or digest.
pub fn verify_catalog_files(
    dataset_id: &str,
    dataset_version: &str,
    manifest: &[u8],
    registry: &[u8],
    format_catalog: &[u8],
) -> Result<(), PreparedError> {
    let expected = phase6_profiles::CatalogIdentity {
        dataset_id: "molnet-xlogp-100-f64-data",
        dataset_version: "sha256-set-164fb893fc80d7856819a151e9ef53c4ec44cdc18c368a2b2320de2851f42565",
        registry_sha256: "8ffd2e9b4d01ce989c63cb552714e1ff01acb8f766dbbc14bc411f08c4484d25",
        profile_sha256: "5add6a3e7a6250dc5c2f280d9a193dc8aef3ba008e47d10e35e7b263c217c2a2",
        manifest_file_sha256: "0febc6ee1ededad2bc7c9be18a1f4102e15df81e20f6d92551f8c2608ebf32f2",
    };
    if dataset_id != expected.dataset_id || dataset_version != expected.dataset_version {
        return Err(PreparedError::CatalogMismatch);
    }
    expected
        .check_file_bytes(manifest, registry, format_catalog)
        .map_err(|_| PreparedError::CatalogMismatch)
}

/// Select both files by the catalog's explicit file IDs and decode them atomically.
///
/// # Errors
/// Rejects a missing, duplicate, unrecognized or invalid file.
pub fn decode_dataset(files: &[(&str, &[u8])]) -> Result<Weights, PreparedError> {
    if files.len() != 2 {
        return Err(PreparedError::FileSet);
    }
    let file = |id| {
        let mut matching = files.iter().filter(|(name, _)| *name == id);
        let bytes = matching.next().ok_or(PreparedError::FileSet)?.1;
        if matching.next().is_some() {
            return Err(PreparedError::FileSet);
        }
        Ok(bytes)
    };
    decode_weights(file(RAW_FILE_ID)?, file(NORMALIZED_FILE_ID)?)
}

/// Decode paired authorized files, comparing every row with the raw byte array.
///
/// # Errors
/// Rejects invalid framing, shapes, finite-value witnesses, indices or file pairs.
pub fn decode_weights(raw: &[u8], normalized: &[u8]) -> Result<Weights, PreparedError> {
    const ROWS: usize = 100;
    const WIDTH: usize = 8;
    if raw.len() != ROWS * WIDTH {
        return Err(PreparedError::Inconsistent);
    }
    if normalized.len() as u64 > crate::MAX_DATA_FILE_BYTES {
        return Err(PreparedError::Framing);
    }
    let text = std::str::from_utf8(normalized).map_err(|_| PreparedError::Framing)?;
    if text.starts_with('\u{feff}') {
        return Err(PreparedError::Framing);
    }
    let mut seen = [false; ROWS];
    let mut rows = Vec::with_capacity(ROWS);
    for line in text.split('\n') {
        // The phase 4 profile skips empty physical lines; whitespace is not a JSON value.
        if line.is_empty() || line == "\r" {
            continue;
        }
        if rows.len() == ROWS {
            return Err(PreparedError::Inconsistent);
        }
        json::validate_raw_strict(line.as_bytes()).map_err(|_| PreparedError::Record)?;
        let wire: WeightWire = serde_json::from_str(line).map_err(|_| PreparedError::Record)?;
        let index =
            usize::try_from(wire.storage_index_0based).map_err(|_| PreparedError::Inconsistent)?;
        let slot = seen.get_mut(index).ok_or(PreparedError::Inconsistent)?;
        if std::mem::replace(slot, true) {
            return Err(PreparedError::Inconsistent);
        }
        let bytes = decode_f64_bytes(&wire.ieee754_f64_le_hex)?;
        let bits = u64::from_le_bytes(bytes);
        let number = wire
            .value
            .as_f64()
            .filter(|number| number.is_finite())
            .ok_or(PreparedError::Record)?;
        let witness = f64::from_bits(bits);
        if !witness.is_finite() || number != witness {
            return Err(PreparedError::Record);
        }
        if raw[index * WIDTH..(index + 1) * WIDTH] != bytes {
            return Err(PreparedError::Inconsistent);
        }
        rows.push(Weight {
            storage_index: wire.storage_index_0based,
            bin_family: wire.bin_family,
            family_index: wire.family_index_0based,
            bits,
            value: wire.value,
        });
    }
    if rows.len() != ROWS || seen.contains(&false) {
        return Err(PreparedError::Inconsistent);
    }
    Ok(Weights { rows })
}

fn decode_f64_bytes(hex: &str) -> Result<[u8; 8], PreparedError> {
    if hex.len() != 16 {
        return Err(PreparedError::Record);
    }
    let nibble = |byte: u8| match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    };
    let mut bytes = [0_u8; 8];
    for (index, pair) in hex.as_bytes().chunks_exact(2).enumerate() {
        bytes[index] = nibble(pair[0]).ok_or(PreparedError::Record)? * 16
            + nibble(pair[1]).ok_or(PreparedError::Record)?;
    }
    Ok(bytes)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WeightWire {
    #[serde(deserialize_with = "json::deserialize_integer")]
    storage_index_0based: u64,
    bin_family: String,
    #[serde(deserialize_with = "json::deserialize_integer")]
    family_index_0based: BigUint,
    ieee754_f64_le_hex: String,
    value: Number,
}
