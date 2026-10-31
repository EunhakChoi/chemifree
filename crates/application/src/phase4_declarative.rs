//! Declarative naming rows. They are inert inputs, not a naming grammar or engine.

use std::collections::BTreeMap;

use serde::{Deserialize, de::DeserializeOwned};

use crate::{json, phase6_profiles};

const RAW: [&str; 2] = ["P4-NAMING-DECLARATIVE-001", "P4-NAMING-DECLARATIVE-002"];
const NORMALIZED: [&str; 3] = [
    "DERIVED-NAMING-DECLARATIVE-1",
    "DERIVED-NAMING-DECLARATIVE-2",
    "DERIVED-FDA-VALIDATED-159",
];

/// A payload-free rejection reason for a declarative input generation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeclarativeError {
    CatalogMismatch,
    FileSet,
    Framing,
    Record,
    RecordCount,
    LineNumber,
}

impl std::fmt::Display for DeclarativeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::CatalogMismatch => "catalog_identity_mismatch",
            Self::FileSet => "invalid_file_set",
            Self::Framing => "invalid_data_framing",
            Self::Record => "invalid_data_record",
            Self::RecordCount => "invalid_record_count",
            Self::LineNumber => "invalid_line_number",
        })
    }
}

impl std::error::Error for DeclarativeError {}

/// Verify actual metadata bytes against the public catalog identity.
///
/// # Errors
/// Rejects a different dataset, version or metadata byte identity.
pub fn verify_catalog_files(
    dataset_id: &str,
    dataset_version: &str,
    manifest: &[u8],
    registry: &[u8],
    catalog: &[u8],
) -> Result<(), DeclarativeError> {
    let expected = phase6_profiles::CatalogIdentity {
        dataset_id: "legacy-naming-declarative",
        dataset_version: "sha256-set-4965b090f79f67a497e6145aca736666fa49163d58681d472ea8c97bc7207200",
        registry_sha256: "8ffd2e9b4d01ce989c63cb552714e1ff01acb8f766dbbc14bc411f08c4484d25",
        profile_sha256: "5add6a3e7a6250dc5c2f280d9a193dc8aef3ba008e47d10e35e7b263c217c2a2",
        manifest_file_sha256: "ac43107199635e157196a9073ae1cd07e9ee2ae8612312cb5df30d613cf95d95",
    };
    if dataset_id != expected.dataset_id || dataset_version != expected.dataset_version {
        return Err(DeclarativeError::CatalogMismatch);
    }
    expected
        .check_file_bytes(manifest, registry, catalog)
        .map_err(|_| DeclarativeError::CatalogMismatch)
}

/// Checks the explicit file identity, role and content class from the public catalog.
#[must_use]
pub fn accepts_file(id: &str, role: &str, class: &str) -> bool {
    class == "declarative_data"
        && ((RAW.contains(&id) && role == "auxiliary_data")
            || (NORMALIZED.contains(&id) && role == "dictionary_entries"))
}

/// True only for a normalized file needed to build the complete typed dataset.
#[must_use]
pub fn is_normalized(id: &str) -> bool {
    NORMALIZED.contains(&id)
}

/// A source text line; its position may repeat or skip.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TextLine {
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub line: u64,
    pub text: String,
}

/// Uninterpreted source attributes; no molecular parsing is performed here.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Attributes {
    pub name: String,
    pub smiles: String,
}

/// A source record preserves its producer ordinal and attributes.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SourceRow {
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub ordinal: u64,
    pub attributes: Attributes,
}

/// The producer's independent-parse report is retained as data, not verified by this loader.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ParseReport {
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub source_ordinal: u64,
    pub source_attributes: Attributes,
    pub independent_parse_ok: bool,
    pub canonical_isomeric_smiles: String,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub formal_charge: i64,
    pub formula: String,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub atoms: u64,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub bonds: u64,
}

/// Optional raw material is preserved as bytes and is never interpreted or executed.
#[derive(PartialEq)]
pub struct OpaqueFile {
    pub file_id: &'static str,
    pub bytes: Vec<u8>,
}

/// Complete typed normalized inputs and any present ancillary source bytes.
#[derive(PartialEq)]
pub struct DeclarativeData {
    pub text: Vec<TextLine>,
    pub source: Vec<SourceRow>,
    pub reports: Vec<ParseReport>,
    pub ancillary: Vec<OpaqueFile>,
}

fn rows<T: DeserializeOwned>(bytes: &[u8], expected: usize) -> Result<Vec<T>, DeclarativeError> {
    let text = std::str::from_utf8(bytes).map_err(|_| DeclarativeError::Framing)?;
    if text.starts_with('\u{feff}') {
        return Err(DeclarativeError::Framing);
    }
    let mut result = Vec::with_capacity(expected);
    for line in text.split('\n') {
        if line.is_empty() || line == "\r" {
            continue;
        }
        if result.len() == expected {
            return Err(DeclarativeError::RecordCount);
        }
        let row = serde_json::from_str(line).map_err(|_| DeclarativeError::Record)?;
        result.push(row);
    }
    if result.len() != expected {
        return Err(DeclarativeError::RecordCount);
    }
    Ok(result)
}

/// Decode a complete set of three normalized streams and any ancillary bytes.
///
/// # Errors
/// Rejects unknown, duplicate or missing files and malformed or incomplete records.
pub fn decode_dataset(files: &[(&str, &[u8])]) -> Result<DeclarativeData, DeclarativeError> {
    let mut index = BTreeMap::new();
    let mut total = 0_u64;
    for (id, bytes) in files {
        total = total
            .checked_add(bytes.len() as u64)
            .ok_or(DeclarativeError::Framing)?;
        if !(RAW.contains(id) || NORMALIZED.contains(id))
            || bytes.len() as u64 > crate::MAX_DATA_FILE_BYTES
            || total > crate::MAX_DATA_TOTAL_BYTES
            || index.insert(*id, *bytes).is_some()
        {
            return Err(DeclarativeError::FileSet);
        }
    }
    let get = |id| index.get(id).copied().ok_or(DeclarativeError::FileSet);
    let text: Vec<TextLine> = rows(get(NORMALIZED[0])?, 127)?;
    if text.iter().any(|row| row.line == 0) {
        return Err(DeclarativeError::LineNumber);
    }
    let source = rows(get(NORMALIZED[1])?, 159)?;
    let reports = rows(get(NORMALIZED[2])?, 159)?;
    let ancillary = RAW
        .iter()
        .filter_map(|&id| {
            index.get(id).map(|bytes| OpaqueFile {
                file_id: id,
                bytes: bytes.to_vec(),
            })
        })
        .collect();
    Ok(DeclarativeData {
        text,
        source,
        reports,
        ancillary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<(&'static str, Vec<u8>)> {
        let line = "{\"line\":1,\"text\":\"synthetic α\"}\r\n";
        let source = "{\"ordinal\":0,\"attributes\":{\"name\":\"example\",\"smiles\":\"C\"}}\n";
        let report = "{\"source_ordinal\":0,\"source_attributes\":{\"name\":\"example\",\"smiles\":\"C\"},\"independent_parse_ok\":false,\"canonical_isomeric_smiles\":\"\",\"formal_charge\":-1,\"formula\":\"\",\"atoms\":0,\"bonds\":0}\n";
        vec![
            (NORMALIZED[0], line.repeat(127).into_bytes()),
            (NORMALIZED[1], source.repeat(159).into_bytes()),
            (NORMALIZED[2], report.repeat(159).into_bytes()),
            (RAW[1], vec![0, 0xff, 1]),
        ]
    }

    fn references<'a>(files: &'a [(&'static str, Vec<u8>)]) -> Vec<(&'a str, &'a [u8])> {
        files
            .iter()
            .map(|(id, bytes)| (*id, bytes.as_slice()))
            .collect()
    }

    #[test]
    fn schema_integral_numbers_preserve_declarative_strings() {
        let source = br#"{"ordinal":-0.0,"attributes":{"name":"1.0","smiles":"C"}}"#;
        let decoded = rows::<SourceRow>(source, 1).unwrap();
        assert_eq!(decoded[0].ordinal, 0);
        assert_eq!(decoded[0].attributes.name, "1.0");
        let report = br#"{"source_ordinal":1e0,"source_attributes":{"name":"x","smiles":"C"},"independent_parse_ok":false,"canonical_isomeric_smiles":"","formal_charge":-1.0,"formula":"","atoms":1.0,"bonds":0e0}"#;
        let decoded = rows::<ParseReport>(report, 1).unwrap();
        assert_eq!(decoded[0].source_ordinal, 1);
        assert_eq!(decoded[0].formal_charge, -1);
        assert_eq!(decoded[0].atoms, 1);
        assert_eq!(decoded[0].bonds, 0);
        let invalid = std::str::from_utf8(report)
            .unwrap()
            .replace("\"atoms\":1.0", "\"atoms\":1.00000000000000000001");
        assert_eq!(
            rows::<ParseReport>(invalid.as_bytes(), 1).err(),
            Some(DeclarativeError::Record)
        );
    }

    #[test]
    fn typed_rows_preserve_order_producer_values_and_optional_source_bytes() {
        let mut files = fixture();
        files.reverse();
        let data = decode_dataset(&references(&files)).unwrap();
        assert_eq!(data.text.len(), 127);
        assert_eq!(data.text[0].text, "synthetic α");
        assert_eq!(data.source.len(), 159);
        assert_eq!(data.source[1].ordinal, 0);
        assert_eq!(data.source[0].attributes.smiles, "C");
        assert_eq!(data.reports.len(), 159);
        assert!(!data.reports[0].independent_parse_ok);
        assert_eq!(data.reports[0].formal_charge, -1);
        assert_eq!(data.ancillary.len(), 1);
        assert_eq!(data.ancillary[0].file_id, RAW[1]);
        assert_eq!(data.ancillary[0].bytes, [0, 0xff, 1]);
    }

    #[test]
    fn missing_repeated_unknown_or_incomplete_files_are_rejected() {
        let files = fixture();
        let mut refs = references(&files);
        refs.remove(0);
        assert_eq!(decode_dataset(&refs).err(), Some(DeclarativeError::FileSet));
        let mut refs = references(&files);
        refs.push(refs[0]);
        assert_eq!(decode_dataset(&refs).err(), Some(DeclarativeError::FileSet));
        let mut refs = references(&files);
        refs[0].0 = "unknown";
        assert_eq!(decode_dataset(&refs).err(), Some(DeclarativeError::FileSet));
        let mut files = fixture();
        files[0].1.clear();
        assert_eq!(
            decode_dataset(&references(&files)).err(),
            Some(DeclarativeError::RecordCount)
        );
    }

    #[test]
    fn shapes_framing_and_line_boundaries_are_enforced() {
        assert!(rows::<TextLine>(b"\r\n{\"line\":1,\"text\":\"x\"}\r\n", 1).is_ok());
        assert_eq!(
            rows::<TextLine>(b"\xef\xbb\xbf{}", 1).err(),
            Some(DeclarativeError::Framing)
        );
        for bad in [
            &b"{\"line\":1,\"text\":\"x\",\"extra\":1}"[..],
            &b"{\"line\":1,\"text\":\"x\",\"line\":2}"[..],
            &b"{\"line\":1}"[..],
        ] {
            assert_eq!(
                rows::<TextLine>(bad, 1).err(),
                Some(DeclarativeError::Record)
            );
        }
        let mut files = fixture();
        files[0].1 = "{\"line\":0,\"text\":\"x\"}\n".repeat(127).into_bytes();
        assert_eq!(
            decode_dataset(&references(&files)).err(),
            Some(DeclarativeError::LineNumber)
        );
    }

    #[test]
    fn catalog_uses_explicit_file_roles_and_metadata() {
        assert!(is_normalized(NORMALIZED[0]));
        assert!(!is_normalized(RAW[0]));
        assert!(accepts_file(RAW[0], "auxiliary_data", "declarative_data"));
        assert!(accepts_file(
            NORMALIZED[0],
            "dictionary_entries",
            "declarative_data"
        ));
        assert!(!accepts_file(
            RAW[0],
            "dictionary_entries",
            "declarative_data"
        ));
        assert!(!accepts_file(
            NORMALIZED[0],
            "dictionary_entries",
            "model_parameters"
        ));
        assert_eq!(
            verify_catalog_files("legacy-naming-declarative", "wrong", b"m", b"r", b"c"),
            Err(DeclarativeError::CatalogMismatch)
        );
    }
}
