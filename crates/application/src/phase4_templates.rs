//! Prepared template framing records; no active template or sequence engine is implied.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer};

use crate::{json, phase6_profiles};

const RAW: [&str; 5] = [
    "P4-BIOPOLYMER-TEMPLATE-CANDIDATES-001",
    "P4-BIOPOLYMER-TEMPLATE-CANDIDATES-003",
    "P4-BIOPOLYMER-TEMPLATE-CANDIDATES-005",
    "P4-BIOPOLYMER-TEMPLATE-CANDIDATES-007",
    "P4-BIOPOLYMER-TEMPLATE-CANDIDATES-009",
];
const NORMALIZED: [(&str, usize); 5] = [
    ("DERIVED-CTP-FRAMING-1", 3556),
    ("DERIVED-CTP-FRAMING-2", 2008),
    ("DERIVED-CTP-FRAMING-3", 5060),
    ("DERIVED-CTP-FRAMING-4", 4942),
    ("DERIVED-CTP-FRAMING-5", 4901),
];

/// Payload-free rejection of a template carrier generation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TemplateError {
    CatalogMismatch,
    FileSet,
    Framing,
    Record,
    RecordCount,
}

impl std::fmt::Display for TemplateError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::CatalogMismatch => "catalog_identity_mismatch",
            Self::FileSet => "invalid_file_set",
            Self::Framing => "invalid_data_framing",
            Self::Record => "invalid_data_record",
            Self::RecordCount => "invalid_record_count",
        })
    }
}
impl std::error::Error for TemplateError {}

/// Verifies the public dataset and exact catalog metadata identities.
///
/// # Errors
/// Rejects the wrong dataset, version, or metadata bytes.
pub fn verify_catalog_files(
    dataset_id: &str,
    dataset_version: &str,
    manifest: &[u8],
    registry: &[u8],
    catalog: &[u8],
) -> Result<(), TemplateError> {
    let expected = phase6_profiles::CatalogIdentity {
        dataset_id: "legacy-biopolymer-template-candidates",
        dataset_version: "sha256-set-823af92a8a17a1a23e7c8de12d78d66352d589139e27709d431ca822f3376abc",
        registry_sha256: "8ffd2e9b4d01ce989c63cb552714e1ff01acb8f766dbbc14bc411f08c4484d25",
        profile_sha256: "5add6a3e7a6250dc5c2f280d9a193dc8aef3ba008e47d10e35e7b263c217c2a2",
        manifest_file_sha256: "2344620b85111ec871c3209aaa758b584dfe941c8b203396ad20c370e0f7c31a",
    };
    if dataset_id != expected.dataset_id || dataset_version != expected.dataset_version {
        return Err(TemplateError::CatalogMismatch);
    }
    expected
        .check_file_bytes(manifest, registry, catalog)
        .map_err(|_| TemplateError::CatalogMismatch)
}

/// Selects files by catalog ID and their explicitly declared role and class.
#[must_use]
pub fn accepts_file(id: &str, role: &str, class: &str) -> bool {
    role == "document_resource"
        && class == "document_resource"
        && (RAW.contains(&id) || NORMALIZED.iter().any(|(name, _)| *name == id))
}

/// Identifies the streams needed to decode the complete prepared collection.
#[must_use]
pub fn is_normalized(id: &str) -> bool {
    NORMALIZED.iter().any(|(name, _)| *name == id)
}

fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

/// Structural metadata retained exactly; indices and offsets do not trigger I/O.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ObjectFraming {
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub index: i64,
    #[serde(deserialize_with = "json::deserialize_nullable_integer")]
    pub parent_index: Option<i64>,
    pub tag: String,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub object_id: i64,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub header_offset: i64,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub header_bytes: i64,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub depth: i64,
    pub implicit_document_root: bool,
    #[serde(deserialize_with = "json::deserialize_integer_vec")]
    pub property_indices: Vec<i64>,
    #[serde(deserialize_with = "json::deserialize_integer_vec")]
    pub child_indices: Vec<i64>,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub end_marker_offset: i64,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub range_bytes: i64,
}

/// Inert property-framing metadata; it is not a native address or dispatch target.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PropertyFraming {
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub index: i64,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub object_index: i64,
    pub tag: String,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub header_offset: i64,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub header_bytes: i64,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub length_field_u16: i64,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub payload_offset: i64,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub payload_bytes: i64,
    pub payload_sha256: String,
    pub user_defined_bit14: bool,
}

/// Prepared variants allow these independent fields; their combined semantics are unresolved.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FramingRow {
    pub kind: String,
    #[serde(default, deserialize_with = "present")]
    pub data: Option<ObjectFraming>,
    #[serde(default, deserialize_with = "present")]
    pub framing: Option<PropertyFraming>,
    #[serde(default, deserialize_with = "present")]
    pub payload_hex: Option<String>,
}

/// Ordered source records selected by an explicit normalized file ID.
#[derive(PartialEq)]
pub struct FramingTable {
    pub file_id: &'static str,
    pub rows: Vec<FramingRow>,
}

/// Opaque raw material is inert and retains its catalog identity.
#[derive(PartialEq)]
pub struct OpaqueFile {
    pub file_id: &'static str,
    pub bytes: Vec<u8>,
}

/// Complete prepared streams and any available ancillary source bytes.
#[derive(PartialEq)]
pub struct TemplateCandidates {
    pub tables: Vec<FramingTable>,
    pub ancillary: Vec<OpaqueFile>,
}

fn rows(bytes: &[u8], expected: usize) -> Result<Vec<FramingRow>, TemplateError> {
    let text = std::str::from_utf8(bytes).map_err(|_| TemplateError::Framing)?;
    if text.starts_with('\u{feff}') {
        return Err(TemplateError::Framing);
    }
    let mut rows = Vec::with_capacity(expected);
    for line in text.split('\n') {
        if line.is_empty() || line == "\r" {
            continue;
        }
        if rows.len() == expected {
            return Err(TemplateError::RecordCount);
        }
        let row: FramingRow = serde_json::from_str(line).map_err(|_| TemplateError::Record)?;
        if row.payload_hex.as_ref().is_some_and(|text| {
            text.len() % 2 != 0 || !text.bytes().all(|byte| byte.is_ascii_hexdigit())
        }) {
            return Err(TemplateError::Record);
        }
        rows.push(row);
    }
    if rows.len() != expected {
        return Err(TemplateError::RecordCount);
    }
    Ok(rows)
}

/// Decode all normalized streams atomically and retain any supplied raw material.
///
/// # Errors
/// Rejects an incomplete, repeated, oversized, malformed or unrecognized input.
pub fn decode_dataset(files: &[(&str, &[u8])]) -> Result<TemplateCandidates, TemplateError> {
    let mut index = BTreeMap::new();
    let mut total = 0_u64;
    for (id, bytes) in files {
        total = total
            .checked_add(bytes.len() as u64)
            .ok_or(TemplateError::Framing)?;
        if !(RAW.contains(id) || is_normalized(id))
            || bytes.len() as u64 > crate::MAX_DATA_FILE_BYTES
            || total > crate::MAX_DATA_TOTAL_BYTES
            || index.insert(*id, *bytes).is_some()
        {
            return Err(TemplateError::FileSet);
        }
    }
    let mut tables = Vec::with_capacity(NORMALIZED.len());
    for (file_id, count) in NORMALIZED {
        let bytes = index.get(file_id).ok_or(TemplateError::FileSet)?;
        tables.push(FramingTable {
            file_id,
            rows: rows(bytes, count)?,
        });
    }
    let ancillary = RAW
        .iter()
        .filter_map(|&file_id| {
            index.get(file_id).map(|bytes| OpaqueFile {
                file_id,
                bytes: bytes.to_vec(),
            })
        })
        .collect();
    Ok(TemplateCandidates { tables, ancillary })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<(&'static str, Vec<u8>)> {
        let mut files: Vec<_> = NORMALIZED
            .iter()
            .map(|(id, count)| {
                (
                    *id,
                    "{\"kind\":\"synthetic\"}\n".repeat(*count).into_bytes(),
                )
            })
            .collect();
        files.push((RAW[3], vec![0, 1, 0xff]));
        files
    }

    fn references<'a>(files: &'a [(&'static str, Vec<u8>)]) -> Vec<(&'a str, &'a [u8])> {
        files
            .iter()
            .map(|(id, bytes)| (*id, bytes.as_slice()))
            .collect()
    }

    #[test]
    fn all_five_streams_are_typed_in_catalog_order_with_optional_raw_bytes() {
        let mut files = fixture();
        files.reverse();
        let data = decode_dataset(&references(&files)).unwrap();
        assert_eq!(data.tables.len(), 5);
        assert_eq!(
            data.tables
                .iter()
                .map(|table| table.rows.len())
                .collect::<Vec<_>>(),
            [3556, 2008, 5060, 4942, 4901]
        );
        assert_eq!(data.tables[0].rows[0].kind, "synthetic");
        assert!(data.tables[0].rows[0].data.is_none());
        assert_eq!(data.ancillary.len(), 1);
        assert_eq!(data.ancillary[0].file_id, RAW[3]);
        assert_eq!(data.ancillary[0].bytes, [0, 1, 0xff]);
    }

    #[test]
    fn schema_integral_numbers_preserve_nullable_and_array_fields() {
        let row = br#"{"kind":"-0.0","data":{"index":1.0,"parent_index":-0.0,"tag":"-0e0","object_id":1e0,"header_offset":0.0,"header_bytes":2.0,"depth":0e0,"implicit_document_root":true,"property_indices":[1e0,0.0],"child_indices":[],"end_marker_offset":2.0,"range_bytes":2e0}}"#;
        let decoded = rows(row, 1).unwrap();
        let object = decoded[0].data.as_ref().unwrap();
        assert_eq!(decoded[0].kind, "-0.0");
        assert_eq!(object.index, 1);
        assert_eq!(object.parent_index, Some(0));
        assert_eq!(object.property_indices, [1, 0]);
        assert_eq!(object.tag, "-0e0");
        let invalid = std::str::from_utf8(row)
            .unwrap()
            .replace("\"index\":1.0", "\"index\":1.00000000000000000001");
        assert_eq!(
            rows(invalid.as_bytes(), 1).err(),
            Some(TemplateError::Record)
        );
    }

    #[test]
    fn nullable_required_fields_are_distinct_from_missing_or_optional_null() {
        let object = "\"index\":0,\"parent_index\":null,\"tag\":\"x\",\"object_id\":1,\"header_offset\":0,\"header_bytes\":2,\"depth\":0,\"implicit_document_root\":true,\"property_indices\":[],\"child_indices\":[],\"end_marker_offset\":2,\"range_bytes\":2";
        let good =
            format!("{{\"kind\":\"synthetic\",\"data\":{{{object}}},\"payload_hex\":\"00aF\"}}");
        let result = rows(good.as_bytes(), 1).unwrap();
        assert!(result[0].data.as_ref().unwrap().parent_index.is_none());
        assert_eq!(result[0].payload_hex.as_deref(), Some("00aF"));
        let missing = good.replace(",\"parent_index\":null", "");
        assert_eq!(
            rows(missing.as_bytes(), 1).err(),
            Some(TemplateError::Record)
        );
        for bad in [
            "{\"kind\":\"x\",\"data\":null}",
            "{\"kind\":\"x\",\"payload_hex\":null}",
            "{\"kind\":\"x\",\"payload_hex\":\"0\"}",
            "{\"kind\":\"x\",\"payload_hex\":\"gg\"}",
        ] {
            assert_eq!(rows(bad.as_bytes(), 1).err(), Some(TemplateError::Record));
        }
    }

    #[test]
    fn property_metadata_is_typed_and_preserved_as_inert_information() {
        let good = b"{\"kind\":\"source-kind\",\"framing\":{\"index\":1,\"object_index\":2,\"tag\":\"p\",\"header_offset\":3,\"header_bytes\":4,\"length_field_u16\":5,\"payload_offset\":7,\"payload_bytes\":2,\"payload_sha256\":\"synthetic\",\"user_defined_bit14\":false}}";
        let result = rows(good, 1).unwrap();
        let framing = result[0].framing.as_ref().unwrap();
        assert_eq!(framing.object_index, 2);
        assert_eq!(framing.payload_sha256, "synthetic");
        assert!(!framing.user_defined_bit14);
        let extra = std::str::from_utf8(good)
            .unwrap()
            .replace("\"index\":1", "\"index\":1,\"unexpected\":true");
        assert_eq!(rows(extra.as_bytes(), 1).err(), Some(TemplateError::Record));
    }

    #[test]
    fn missing_duplicate_unknown_and_incomplete_inputs_fail() {
        let files = fixture();
        let mut refs = references(&files);
        refs.remove(0);
        assert_eq!(decode_dataset(&refs).err(), Some(TemplateError::FileSet));
        let mut refs = references(&files);
        refs.push(refs[0]);
        assert_eq!(decode_dataset(&refs).err(), Some(TemplateError::FileSet));
        let mut refs = references(&files);
        refs[0].0 = "unknown";
        assert_eq!(decode_dataset(&refs).err(), Some(TemplateError::FileSet));
        assert_eq!(
            rows(b"{\"kind\":\"x\"}\n", 2).err(),
            Some(TemplateError::RecordCount)
        );
    }

    #[test]
    fn closed_shapes_and_physical_line_rules_are_checked() {
        assert!(rows(b"\r\n{\"kind\":\"x\"}\r\n", 1).is_ok());
        assert_eq!(
            rows(b"\xef\xbb\xbf{}", 1).err(),
            Some(TemplateError::Framing)
        );
        for bad in [
            &b"{\"kind\":\"x\",\"extra\":1}"[..],
            &b"{\"kind\":\"x\",\"kind\":\"y\"}"[..],
            &b"{}"[..],
        ] {
            assert_eq!(rows(bad, 1).err(), Some(TemplateError::Record));
        }
    }

    #[test]
    fn explicit_catalog_identity_and_file_roles_are_required() {
        assert!(is_normalized(NORMALIZED[0].0));
        assert!(!is_normalized(RAW[0]));
        assert!(accepts_file(
            RAW[0],
            "document_resource",
            "document_resource"
        ));
        assert!(!accepts_file(RAW[0], "auxiliary_data", "document_resource"));
        assert!(!accepts_file(
            RAW[0],
            "document_resource",
            "model_parameters"
        ));
        assert!(!accepts_file(
            "unknown",
            "document_resource",
            "document_resource"
        ));
        assert_eq!(
            verify_catalog_files(
                "legacy-biopolymer-template-candidates",
                "wrong",
                b"m",
                b"r",
                b"c"
            ),
            Err(TemplateError::CatalogMismatch)
        );
    }
}
