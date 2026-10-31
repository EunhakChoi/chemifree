//! Inert CMR byte arrays and normalized records, without atom typing or a CMR engine.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::value::RawValue;

use crate::{json, phase6_profiles};

/// The public catalog binds each raw array to a normalized stream and record width.
const FILES: [(&str, &str, usize, usize); 4] = [
    (
        "DATA-RANGE-BB-CMR-1-raw_data",
        "DATA-RANGE-BB-CMR-1-normalized",
        100,
        4,
    ),
    (
        "DATA-RANGE-BB-CMR-2-raw_data",
        "DATA-RANGE-BB-CMR-2-normalized",
        100,
        4,
    ),
    (
        "DATA-RANGE-BB-CMR-3-raw_data",
        "DATA-RANGE-BB-CMR-3-normalized",
        62,
        4,
    ),
    (
        "DATA-RANGE-BB-CMR-4-raw_data",
        "DATA-RANGE-BB-CMR-4-normalized",
        62,
        5,
    ),
];

/// A payload-free rejection of catalog identities, file sets or record content.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CmrError {
    CatalogMismatch,
    FileSet,
    Framing,
    Record,
    Count,
    Index,
    RawMismatch,
}

impl std::fmt::Display for CmrError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::CatalogMismatch => "catalog_identity_mismatch",
            Self::FileSet => "invalid_file_set",
            Self::Framing => "invalid_data_framing",
            Self::Record => "invalid_data_record",
            Self::Count => "invalid_record_count",
            Self::Index => "invalid_storage_index",
            Self::RawMismatch => "raw_record_mismatch",
        })
    }
}
impl std::error::Error for CmrError {}

/// Checks the exact public metadata identities without reading any model payload.
///
/// # Errors
/// Rejects differing dataset IDs, versions, metadata sizes or byte digests.
pub fn verify_catalog_files(
    dataset_id: &str,
    dataset_version: &str,
    manifest: &[u8],
    registry: &[u8],
    catalog: &[u8],
) -> Result<(), CmrError> {
    let expected = phase6_profiles::CatalogIdentity {
        dataset_id: "biobyte-cmr-static-data",
        dataset_version: "sha256-set-ee879a4478ab00c7dedcdee756c2f1c18d5660aca1f38b08ca894051dfbe5273",
        registry_sha256: "8ffd2e9b4d01ce989c63cb552714e1ff01acb8f766dbbc14bc411f08c4484d25",
        profile_sha256: "5add6a3e7a6250dc5c2f280d9a193dc8aef3ba008e47d10e35e7b263c217c2a2",
        manifest_file_sha256: "0179783bef72ed0c2899ce1ce1e84d24541be758efde5a1deac785916006c28b",
    };
    if dataset_id != expected.dataset_id || dataset_version != expected.dataset_version {
        return Err(CmrError::CatalogMismatch);
    }
    expected
        .check_file_bytes(manifest, registry, catalog)
        .map_err(|_| CmrError::CatalogMismatch)
}

/// Accepts only a catalog file ID with its declared role and classification.
#[must_use]
pub fn accepts_file(id: &str, role: &str, class: &str) -> bool {
    class == "model_parameters"
        && FILES
            .iter()
            .enumerate()
            .any(|(index, (raw, normalized, _, _))| {
                (id == *raw || id == *normalized)
                    && role
                        == if index == 3 {
                            "fragment_reference"
                        } else {
                            "weights"
                        }
            })
}

/// An exact JSON number lexeme, retained without conversion to a machine float.
#[derive(PartialEq)]
pub struct NumericLexeme(String);

impl NumericLexeme {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A four-byte record and the exact JSON number lexeme declared alongside it.
/// No unbound floating-point encoding or unit is inferred from the bytes.
#[derive(PartialEq)]
pub struct NumericRow {
    pub storage_index: usize,
    pub raw: [u8; 4],
    pub value: NumericLexeme,
}

/// A five-byte key record with its independently declared Latin-1 text.
#[derive(PartialEq)]
pub struct KeyRow {
    pub storage_index: usize,
    pub raw: [u8; 5],
    pub key_text_latin1: String,
}

/// The source order is retained; storage indices name positions in the paired raw array.
#[derive(PartialEq)]
pub enum CmrRows {
    Numeric(Vec<NumericRow>),
    Keys(Vec<KeyRow>),
}

/// A single catalog-selected raw/normalized pair.
#[derive(PartialEq)]
pub struct CmrArray {
    pub raw_file_id: &'static str,
    pub normalized_file_id: &'static str,
    pub rows: CmrRows,
}

/// All four arrays form one immutable dataset, not a working CMR model.
#[derive(PartialEq)]
pub struct CmrData {
    pub arrays: Vec<CmrArray>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NumericWire<'a> {
    #[serde(deserialize_with = "json::deserialize_integer")]
    storage_index_0based: u64,
    raw_hex: String,
    #[serde(borrow)]
    value: &'a RawValue,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyWire {
    #[serde(deserialize_with = "json::deserialize_integer")]
    storage_index_0based: u64,
    raw_hex: String,
    key_text_latin1: String,
}

fn fixed_hex<const N: usize>(text: &str) -> Result<[u8; N], CmrError> {
    if text.len() != N * 2 {
        return Err(CmrError::Record);
    }
    let digit = |byte| match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    };
    let mut bytes = [0; N];
    for (i, pair) in text.as_bytes().chunks_exact(2).enumerate() {
        bytes[i] = digit(pair[0]).ok_or(CmrError::Record)? * 16
            + digit(pair[1]).ok_or(CmrError::Record)?;
    }
    Ok(bytes)
}

fn index_and_raw<const N: usize>(
    index: u64,
    text: &str,
    raw: &[u8],
    seen: &mut [bool],
) -> Result<(usize, [u8; N]), CmrError> {
    let index = usize::try_from(index).map_err(|_| CmrError::Index)?;
    let slot = seen.get_mut(index).ok_or(CmrError::Index)?;
    if *slot {
        return Err(CmrError::Index);
    }
    *slot = true;
    let bytes = fixed_hex::<N>(text)?;
    let start = index.checked_mul(N).ok_or(CmrError::Index)?;
    let end = start.checked_add(N).ok_or(CmrError::Index)?;
    if raw.get(start..end) != Some(bytes.as_slice()) {
        return Err(CmrError::RawMismatch);
    }
    Ok((index, bytes))
}

fn lines(bytes: &[u8]) -> Result<impl Iterator<Item = &str>, CmrError> {
    if bytes.len() as u64 > crate::MAX_DATA_FILE_BYTES {
        return Err(CmrError::Framing);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| CmrError::Framing)?;
    if text.starts_with('\u{feff}') {
        return Err(CmrError::Framing);
    }
    Ok(text
        .split('\n')
        .filter(|line| !line.is_empty() && *line != "\r"))
}

fn numeric_rows(raw: &[u8], bytes: &[u8], count: usize) -> Result<Vec<NumericRow>, CmrError> {
    if raw.len() != count * 4 {
        return Err(CmrError::Count);
    }
    let mut seen = vec![false; count];
    let mut rows = Vec::with_capacity(count);
    for line in lines(bytes)? {
        if rows.len() == count {
            return Err(CmrError::Count);
        }
        let row: NumericWire<'_> = serde_json::from_str(line).map_err(|_| CmrError::Record)?;
        let lexeme = row.value.get();
        if !matches!(lexeme.as_bytes().first(), Some(b'-' | b'0'..=b'9')) {
            return Err(CmrError::Record);
        }
        let (index, raw) =
            index_and_raw::<4>(row.storage_index_0based, &row.raw_hex, raw, &mut seen)?;
        rows.push(NumericRow {
            storage_index: index,
            raw,
            value: NumericLexeme(lexeme.into()),
        });
    }
    if rows.len() != count {
        return Err(CmrError::Count);
    }
    Ok(rows)
}

fn key_rows(raw: &[u8], bytes: &[u8], count: usize) -> Result<Vec<KeyRow>, CmrError> {
    if raw.len() != count * 5 {
        return Err(CmrError::Count);
    }
    let mut seen = vec![false; count];
    let mut rows = Vec::with_capacity(count);
    for line in lines(bytes)? {
        if rows.len() == count {
            return Err(CmrError::Count);
        }
        let row: KeyWire = serde_json::from_str(line).map_err(|_| CmrError::Record)?;
        if row.key_text_latin1.chars().any(|ch| u32::from(ch) > 255) {
            return Err(CmrError::Record);
        }
        let (index, raw) =
            index_and_raw::<5>(row.storage_index_0based, &row.raw_hex, raw, &mut seen)?;
        rows.push(KeyRow {
            storage_index: index,
            raw,
            key_text_latin1: row.key_text_latin1,
        });
    }
    if rows.len() != count {
        return Err(CmrError::Count);
    }
    Ok(rows)
}

/// Decodes the complete catalog-selected file set without activating a model.
///
/// # Errors
/// Rejects incomplete or repeated files, wrong raw widths, indices or normalized rows.
pub fn decode_dataset(files: &[(&str, &[u8])]) -> Result<CmrData, CmrError> {
    if files.len() != 8 {
        return Err(CmrError::FileSet);
    }
    let mut indexed = BTreeMap::new();
    let mut total = 0_u64;
    for (id, bytes) in files {
        total = total
            .checked_add(bytes.len() as u64)
            .ok_or(CmrError::Framing)?;
        if !FILES
            .iter()
            .any(|(raw, normalized, _, _)| id == raw || id == normalized)
            || bytes.len() as u64 > crate::MAX_DATA_FILE_BYTES
            || total > crate::MAX_DATA_TOTAL_BYTES
            || indexed.insert(*id, *bytes).is_some()
        {
            return Err(CmrError::FileSet);
        }
    }
    let mut arrays = Vec::with_capacity(FILES.len());
    for (raw_id, normalized_id, count, width) in FILES {
        let raw = indexed.get(raw_id).ok_or(CmrError::FileSet)?;
        let normalized = indexed.get(normalized_id).ok_or(CmrError::FileSet)?;
        let rows = match width {
            4 => CmrRows::Numeric(numeric_rows(raw, normalized, count)?),
            5 => CmrRows::Keys(key_rows(raw, normalized, count)?),
            _ => return Err(CmrError::FileSet),
        };
        arrays.push(CmrArray {
            raw_file_id: raw_id,
            normalized_file_id: normalized_id,
            rows,
        });
    }
    Ok(CmrData { arrays })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn example_files() -> Vec<(&'static str, Vec<u8>)> {
        let mut files = Vec::new();
        for (raw_id, normalized_id, count, width) in FILES {
            let (raw_record, hex) = if width == 4 {
                (vec![0, 1, 2, 3], "00010203")
            } else {
                (vec![0xe9, 0, 0, 0, 0], "E900000000")
            };
            let raw = raw_record.repeat(count);
            let rows: String = (0..count).map(|index| {
                if width == 4 {
                    format!("{{\"storage_index_0based\":{index},\"raw_hex\":\"{hex}\",\"value\":0.1000000000000000000000000000001}}\n")
                } else {
                    format!("{{\"storage_index_0based\":{index},\"raw_hex\":\"{hex}\",\"key_text_latin1\":\"é\"}}\n")
                }
            }).collect();
            files.push((raw_id, raw));
            files.push((normalized_id, rows.into_bytes()));
        }
        files
    }

    fn references<'a>(files: &'a [(&'static str, Vec<u8>)]) -> Vec<(&'a str, &'a [u8])> {
        files
            .iter()
            .map(|(id, bytes)| (*id, bytes.as_slice()))
            .collect()
    }

    #[test]
    fn catalog_identities_and_file_roles_are_explicit() {
        assert_eq!(
            verify_catalog_files(
                "biobyte-cmr-static-data",
                "sha256-set-ee879a4478ab00c7dedcdee756c2f1c18d5660aca1f38b08ca894051dfbe5273",
                b"fixture manifest",
                b"fixture registry",
                b"fixture catalog"
            ),
            Err(CmrError::CatalogMismatch)
        );
        assert!(accepts_file(FILES[0].0, "weights", "model_parameters"));
        assert!(accepts_file(
            FILES[3].1,
            "fragment_reference",
            "model_parameters"
        ));
        assert!(!accepts_file(
            FILES[0].0,
            "fragment_reference",
            "model_parameters"
        ));
        assert!(!accepts_file(FILES[0].0, "weights", "declarative_data"));
        assert!(!accepts_file("unknown", "weights", "model_parameters"));
    }

    #[test]
    fn four_complete_arrays_retain_source_bytes_and_exact_numeric_lexemes() {
        let mut files = example_files();
        files.reverse();
        let decoded = decode_dataset(&references(&files)).unwrap();
        assert_eq!(decoded.arrays.len(), 4);
        let CmrRows::Numeric(rows) = &decoded.arrays[0].rows else {
            panic!("wrong array kind")
        };
        assert_eq!(rows.len(), 100);
        assert_eq!(rows[0].raw, [0, 1, 2, 3]);
        assert_eq!(rows[0].value.as_str(), "0.1000000000000000000000000000001");
        let CmrRows::Keys(rows) = &decoded.arrays[3].rows else {
            panic!("wrong array kind")
        };
        assert_eq!(rows.len(), 62);
        assert_eq!(rows[0].raw, [0xe9, 0, 0, 0, 0]);
        assert_eq!(rows[0].key_text_latin1, "é");
    }

    #[test]
    fn schema_integral_indices_leave_raw_numeric_lexemes_unchanged() {
        let numeric = br#"{"storage_index_0based":-0.0,"raw_hex":"00010203","value":-0.0}"#;
        let decoded = numeric_rows(&[0, 1, 2, 3], numeric, 1).unwrap();
        assert_eq!(decoded[0].storage_index, 0);
        assert_eq!(decoded[0].value.as_str(), "-0.0");
        let key = r#"{"storage_index_0based":0e0,"raw_hex":"E900000000","key_text_latin1":"é"}"#;
        let decoded = key_rows(&[0xe9, 0, 0, 0, 0], key.as_bytes(), 1).unwrap();
        assert_eq!(decoded[0].storage_index, 0);
        assert_eq!(decoded[0].key_text_latin1, "é");
        let invalid =
            br#"{"storage_index_0based":0.00000000000000000001,"raw_hex":"00010203","value":-0.0}"#;
        assert_eq!(
            numeric_rows(&[0, 1, 2, 3], invalid, 1).err(),
            Some(CmrError::Record)
        );
    }

    #[test]
    fn number_lexemes_are_preserved_without_floating_point_coercion() {
        let raw = [0, 1, 2, 3];
        for lexeme in ["-0", "1e9999", "-1e-9999", "123456789012345678901234567890"] {
            let line = format!(
                "{{\"storage_index_0based\":0,\"raw_hex\":\"00010203\",\"value\":{lexeme}}}"
            );
            let rows = numeric_rows(&raw, line.as_bytes(), 1).unwrap();
            assert_eq!(rows[0].value.as_str(), lexeme);
        }
        for value in ["null", "true", "\"1\"", "NaN"] {
            let line = format!(
                "{{\"storage_index_0based\":0,\"raw_hex\":\"00010203\",\"value\":{value}}}"
            );
            assert_eq!(
                numeric_rows(&raw, line.as_bytes(), 1).err(),
                Some(CmrError::Record)
            );
        }
    }

    #[test]
    fn raw_bytes_and_storage_indices_must_match_the_normalized_rows() {
        let raw = [0, 1, 2, 3, 4, 5, 6, 7];
        let good = b"{\"storage_index_0based\":0,\"raw_hex\":\"00010203\",\"value\":1}\n{\"storage_index_0based\":1,\"raw_hex\":\"04050607\",\"value\":2}\n";
        assert!(numeric_rows(&raw, good, 2).is_ok());
        let duplicate = b"{\"storage_index_0based\":0,\"raw_hex\":\"00010203\",\"value\":1}\n{\"storage_index_0based\":0,\"raw_hex\":\"00010203\",\"value\":2}\n";
        assert_eq!(
            numeric_rows(&raw, duplicate, 2).err(),
            Some(CmrError::Index)
        );
        let out_of_range = b"{\"storage_index_0based\":2,\"raw_hex\":\"00010203\",\"value\":1}\n";
        assert_eq!(
            numeric_rows(&raw, out_of_range, 2).err(),
            Some(CmrError::Index)
        );
        let wrong_bytes = b"{\"storage_index_0based\":0,\"raw_hex\":\"04050607\",\"value\":1}\n{\"storage_index_0based\":1,\"raw_hex\":\"04050607\",\"value\":2}\n";
        assert_eq!(
            numeric_rows(&raw, wrong_bytes, 2).err(),
            Some(CmrError::RawMismatch)
        );
        assert_eq!(
            numeric_rows(&raw[..4], good, 2).err(),
            Some(CmrError::Count)
        );
        let short_rows = b"{\"storage_index_0based\":0,\"raw_hex\":\"00010203\",\"value\":1}\n";
        assert_eq!(
            numeric_rows(&raw, short_rows, 2).err(),
            Some(CmrError::Count)
        );
    }

    #[test]
    fn key_text_must_be_representable_as_latin1() {
        let raw = [0xe9, 0, 0, 0, 0];
        let valid =
            "{\"storage_index_0based\":0,\"raw_hex\":\"e900000000\",\"key_text_latin1\":\"é\"}";
        assert!(key_rows(&raw, valid.as_bytes(), 1).is_ok());
        let invalid =
            "{\"storage_index_0based\":0,\"raw_hex\":\"e900000000\",\"key_text_latin1\":\"€\"}";
        assert_eq!(
            key_rows(&raw, invalid.as_bytes(), 1).err(),
            Some(CmrError::Record)
        );
    }

    #[test]
    fn framing_closed_shapes_and_file_sets_are_checked() {
        let raw = [0, 1, 2, 3];
        let crlf = b"\r\n{\"storage_index_0based\":0,\"raw_hex\":\"00010203\",\"value\":1}\r\n";
        assert!(numeric_rows(&raw, crlf, 1).is_ok());
        let duplicate_key =
            b"{\"storage_index_0based\":0,\"raw_hex\":\"00010203\",\"value\":1,\"value\":2}";
        assert_eq!(
            numeric_rows(&raw, duplicate_key, 1).err(),
            Some(CmrError::Record)
        );
        let unknown =
            b"{\"storage_index_0based\":0,\"raw_hex\":\"00010203\",\"value\":1,\"extra\":0}";
        assert_eq!(numeric_rows(&raw, unknown, 1).err(), Some(CmrError::Record));
        assert_eq!(
            numeric_rows(&raw, b"\xef\xbb\xbf{}", 1).err(),
            Some(CmrError::Framing)
        );
        let files = example_files();
        let mut refs = references(&files);
        refs[1].0 = refs[0].0;
        assert_eq!(decode_dataset(&refs).err(), Some(CmrError::FileSet));
        refs.pop();
        assert_eq!(decode_dataset(&refs).err(), Some(CmrError::FileSet));
    }
}
