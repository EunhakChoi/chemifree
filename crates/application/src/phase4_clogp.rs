//! Prepared fragment carriers. No graph matching or CLogP prediction is provided.

use std::collections::BTreeMap;

use num_bigint::{BigInt, BigUint};
use serde::{Deserialize, Deserializer, de::DeserializeOwned};
use serde_json::value::RawValue;

use crate::{
    json, phase6_profiles,
    prepared_f32::{self, F32Witness},
};

const RAW_DECLARATIVE: [&str; 3] = [
    "P4-CLOGP-CARRIERS-001",
    "P4-CLOGP-CARRIERS-014",
    "P4-CLOGP-CARRIERS-016",
];
const RAW_MODEL: [&str; 6] = [
    "P4-CLOGP-CARRIERS-002",
    "P4-CLOGP-CARRIERS-003",
    "P4-CLOGP-CARRIERS-004",
    "P4-CLOGP-CARRIERS-005",
    "P4-CLOGP-CARRIERS-009",
    "P4-CLOGP-CARRIERS-010",
];
const NORMALIZED: [(&str, usize); 10] = [
    ("DERIVED-CLOGP-CARRIERS-1", 11),
    ("DERIVED-CLOGP-CARRIERS-2", 32),
    ("DERIVED-CLOGP-CARRIERS-3", 26),
    ("DERIVED-CLOGP-CARRIERS-4", 34),
    ("DERIVED-CLOGP-CARRIERS-5", 14),
    ("DERIVED-CLOGP-CARRIERS-6", 141),
    ("DERIVED-CLOGP-CARRIERS-7", 10026),
    ("DERIVED-CLOGP-CARRIERS-8", 12),
    ("DERIVED-CLOGP-CARRIERS-9", 5),
    ("DERIVED-BIOBYTE-ACTIVE-1686", 1686),
];

/// Payload-free rejection of a fragment carrier generation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClogpError {
    CatalogMismatch,
    FileSet,
    Framing,
    Record,
    RecordCount,
}
impl std::fmt::Display for ClogpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::CatalogMismatch => "catalog_identity_mismatch",
            Self::FileSet => "invalid_file_set",
            Self::Framing => "invalid_data_framing",
            Self::Record => "invalid_data_record",
            Self::RecordCount => "invalid_record_count",
        })
    }
}
impl std::error::Error for ClogpError {}

/// Verify the dataset and exact public metadata identities.
///
/// # Errors
/// Rejects a different dataset, version or metadata identity.
pub fn verify_catalog_files(
    dataset_id: &str,
    dataset_version: &str,
    manifest: &[u8],
    registry: &[u8],
    catalog: &[u8],
) -> Result<(), ClogpError> {
    let expected = phase6_profiles::CatalogIdentity {
        dataset_id: "legacy-clogp-carriers",
        dataset_version: "sha256-set-af8e2f6aa0d026a40301fd6a2d8c9687d1ae97822ec54f490a962697734e70a3",
        registry_sha256: "8ffd2e9b4d01ce989c63cb552714e1ff01acb8f766dbbc14bc411f08c4484d25",
        profile_sha256: "5add6a3e7a6250dc5c2f280d9a193dc8aef3ba008e47d10e35e7b263c217c2a2",
        manifest_file_sha256: "b421eba8807927e77c68f6d9fdb6c3f42e7a15b09ebc9d67e78910b0870c3df4",
    };
    if dataset_id != expected.dataset_id || dataset_version != expected.dataset_version {
        return Err(ClogpError::CatalogMismatch);
    }
    expected
        .check_file_bytes(manifest, registry, catalog)
        .map_err(|_| ClogpError::CatalogMismatch)
}

/// Checks catalog file IDs, roles and classifications without interpreting filenames.
#[must_use]
pub fn accepts_file(id: &str, role: &str, class: &str) -> bool {
    (role == "auxiliary_data"
        && ((RAW_DECLARATIVE.contains(&id) && class == "declarative_data")
            || (RAW_MODEL.contains(&id) && class == "model_parameters")))
        || (is_normalized(id) && role == "fragment_reference" && class == "model_parameters")
}
/// Identifies normalized streams required for a complete generation.
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
fn hex_valid(text: &str) -> bool {
    text.len().is_multiple_of(2) && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}
fn latin1(text: &str) -> bool {
    text.chars().all(|ch| u32::from(ch) <= 255)
}

/// A source line and its uninterpreted lexical text.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TextRow {
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub line: BigUint,
    pub text: String,
}
/// A tagged line with ordered lexical fields.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TaggedRow {
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub line: BigUint,
    pub tag: String,
    pub fields: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FloatWire {
    ieee754_f32_le_hex: String,
    value: Box<RawValue>,
    finite: bool,
}
impl FloatWire {
    fn decode(self) -> Result<F32Witness, ClogpError> {
        if !self.finite {
            return Err(ClogpError::Record);
        }
        prepared_f32::decode_f32(&self.ieee754_f32_le_hex, &self.value)
            .map_err(|_| ClogpError::Record)
    }
}

/// An explicitly typed integer or finite F32 witness from a parameter array.
#[derive(PartialEq)]
pub enum ParameterValue {
    Integer(BigInt),
    F32(F32Witness),
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ParameterWire {
    #[serde(deserialize_with = "json::deserialize_integer")]
    ordinal: BigUint,
    data_role: String,
    #[serde(deserialize_with = "json::deserialize_integer")]
    count: BigUint,
    #[serde(deserialize_with = "json::deserialize_integer")]
    element_bytes: BigUint,
    raw_payload_hex: String,
    storage_kind: String,
    #[serde(default, deserialize_with = "present")]
    values: Option<Vec<Box<RawValue>>>,
}
/// Array framing and optional values; storage meaning and feature binding remain unresolved.
#[derive(PartialEq)]
pub struct ParameterRow {
    pub ordinal: BigUint,
    pub data_role: String,
    pub count: BigUint,
    pub element_bytes: BigUint,
    pub raw_payload_hex: String,
    pub storage_kind: String,
    pub values: Option<Vec<ParameterValue>>,
}
impl TryFrom<ParameterWire> for ParameterRow {
    type Error = ClogpError;
    fn try_from(row: ParameterWire) -> Result<Self, Self::Error> {
        if !hex_valid(&row.raw_payload_hex) {
            return Err(ClogpError::Record);
        }
        let values = row
            .values
            .map(|items| {
                items
                    .into_iter()
                    .map(|item| {
                        let raw = item.get();
                        if raw.starts_with('{') {
                            let float: FloatWire =
                                serde_json::from_str(raw).map_err(|_| ClogpError::Record)?;
                            Ok(ParameterValue::F32(float.decode()?))
                        } else {
                            let value = json::parse_integer_raw::<BigInt>(&item)
                                .ok_or(ClogpError::Record)?;
                            Ok(ParameterValue::Integer(value))
                        }
                    })
                    .collect::<Result<Vec<_>, ClogpError>>()
            })
            .transpose()?;
        Ok(Self {
            ordinal: row.ordinal,
            data_role: row.data_role,
            count: row.count,
            element_bytes: row.element_bytes,
            raw_payload_hex: row.raw_payload_hex,
            storage_kind: row.storage_kind,
            values,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ActiveWire {
    #[serde(deserialize_with = "json::deserialize_integer")]
    fragment_storage_index_0based: BigUint,
    key_latin1: String,
    key_hex: String,
    #[serde(deserialize_with = "json::deserialize_integer")]
    value_slot_1based: BigUint,
    environment_raw_hex: String,
    environment_trimmed_latin1: String,
    #[serde(rename = "FVALUE_f32_le_hex")]
    fvalue_f32_le_hex: String,
    #[serde(rename = "FVALUE")]
    fvalue: Box<RawValue>,
    #[serde(deserialize_with = "json::deserialize_integer")]
    quality_integer: BigInt,
    #[serde(deserialize_with = "json::deserialize_integer")]
    zwitter_category: BigInt,
}
/// A source fragment record with separately retained text, bytes and finite F32 witness.
#[derive(PartialEq)]
pub struct ActiveRow {
    pub fragment_storage_index_0based: BigUint,
    pub key_latin1: String,
    pub key_hex: String,
    pub value_slot_1based: BigUint,
    pub environment_raw_hex: String,
    pub environment_trimmed_latin1: String,
    pub fvalue: F32Witness,
    pub quality_integer: BigInt,
    pub zwitter_category: BigInt,
}
impl TryFrom<ActiveWire> for ActiveRow {
    type Error = ClogpError;
    fn try_from(row: ActiveWire) -> Result<Self, Self::Error> {
        if row.value_slot_1based == BigUint::from(0_u8)
            || !latin1(&row.key_latin1)
            || !latin1(&row.environment_trimmed_latin1)
            || !hex_valid(&row.key_hex)
            || !hex_valid(&row.environment_raw_hex)
        {
            return Err(ClogpError::Record);
        }
        let fvalue = prepared_f32::decode_f32(&row.fvalue_f32_le_hex, &row.fvalue)
            .map_err(|_| ClogpError::Record)?;
        Ok(Self {
            fragment_storage_index_0based: row.fragment_storage_index_0based,
            key_latin1: row.key_latin1,
            key_hex: row.key_hex,
            value_slot_1based: row.value_slot_1based,
            environment_raw_hex: row.environment_raw_hex,
            environment_trimmed_latin1: row.environment_trimmed_latin1,
            fvalue,
            quality_integer: row.quality_integer,
            zwitter_category: row.zwitter_category,
        })
    }
}

/// The public catalog selects one of four closed record shapes for each stream.
#[derive(PartialEq)]
pub enum CarrierRows {
    Text(Vec<TextRow>),
    Tagged(Vec<TaggedRow>),
    Parameters(Vec<ParameterRow>),
    Active(Vec<ActiveRow>),
}
/// Ordered records for an explicit normalized file ID.
#[derive(PartialEq)]
pub struct CarrierTable {
    pub file_id: &'static str,
    pub rows: CarrierRows,
}
/// Optional raw bytes remain inert and retain their catalog identity.
#[derive(PartialEq)]
pub struct OpaqueFile {
    pub file_id: &'static str,
    pub bytes: Vec<u8>,
}
/// Complete typed streams and present raw carriers are published as one generation.
#[derive(PartialEq)]
pub struct ClogpData {
    pub tables: Vec<CarrierTable>,
    pub ancillary: Vec<OpaqueFile>,
}

fn lines<T: DeserializeOwned, U>(
    bytes: &[u8],
    count: usize,
    mut convert: impl FnMut(T) -> Result<U, ClogpError>,
) -> Result<Vec<U>, ClogpError> {
    let text = std::str::from_utf8(bytes).map_err(|_| ClogpError::Framing)?;
    if text.starts_with('\u{feff}') {
        return Err(ClogpError::Framing);
    }
    let mut result = Vec::with_capacity(count);
    for line in text.split('\n') {
        if line.is_empty() || line == "\r" {
            continue;
        }
        if result.len() == count {
            return Err(ClogpError::RecordCount);
        }
        let value = serde_json::from_str(line).map_err(|_| ClogpError::Record)?;
        result.push(convert(value)?);
    }
    if result.len() != count {
        return Err(ClogpError::RecordCount);
    }
    Ok(result)
}
fn text_row(row: TextRow) -> Result<TextRow, ClogpError> {
    if row.line == BigUint::from(0_u8) {
        Err(ClogpError::Record)
    } else {
        Ok(row)
    }
}
fn tagged_row(row: TaggedRow) -> Result<TaggedRow, ClogpError> {
    if row.line == BigUint::from(0_u8) {
        Err(ClogpError::Record)
    } else {
        Ok(row)
    }
}

/// Decode all ten normalized streams and any present raw files atomically.
///
/// # Errors
/// Rejects unknown, duplicate, oversized, incomplete or malformed files and records.
pub fn decode_dataset(files: &[(&str, &[u8])]) -> Result<ClogpData, ClogpError> {
    let mut index = BTreeMap::new();
    let mut total = 0_u64;
    for (id, bytes) in files {
        total = total
            .checked_add(bytes.len() as u64)
            .ok_or(ClogpError::Framing)?;
        if !(RAW_DECLARATIVE.contains(id) || RAW_MODEL.contains(id) || is_normalized(id))
            || bytes.len() as u64 > crate::MAX_DATA_FILE_BYTES
            || total > crate::MAX_DATA_TOTAL_BYTES
            || index.insert(*id, *bytes).is_some()
        {
            return Err(ClogpError::FileSet);
        }
    }
    let mut tables = Vec::with_capacity(NORMALIZED.len());
    for (position, (file_id, count)) in NORMALIZED.into_iter().enumerate() {
        let bytes = index.get(file_id).ok_or(ClogpError::FileSet)?;
        let rows = match position {
            0 | 1 | 7 | 8 => CarrierRows::Text(lines(bytes, count, text_row)?),
            2 => CarrierRows::Parameters(lines::<ParameterWire, _>(
                bytes,
                count,
                ParameterRow::try_from,
            )?),
            3..=6 => CarrierRows::Tagged(lines(bytes, count, tagged_row)?),
            9 => CarrierRows::Active(lines::<ActiveWire, _>(bytes, count, ActiveRow::try_from)?),
            _ => return Err(ClogpError::FileSet),
        };
        tables.push(CarrierTable { file_id, rows });
    }
    let ancillary = RAW_DECLARATIVE
        .iter()
        .chain(RAW_MODEL.iter())
        .filter_map(|&file_id| {
            index.get(file_id).map(|bytes| OpaqueFile {
                file_id,
                bytes: bytes.to_vec(),
            })
        })
        .collect();
    Ok(ClogpData { tables, ancillary })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = r#"{"line":1,"text":"synthetic"}"#;
    const TAGGED: &str = r#"{"line":1,"tag":"synthetic","fields":["a","b"]}"#;
    const PARAMETER: &str = r#"{"ordinal":0,"data_role":"synthetic","count":0,"element_bytes":0,"raw_payload_hex":"00","storage_kind":"synthetic","values":[42,{"ieee754_f32_le_hex":"0100803f","value":1.000000059604644775390626,"finite":true}]}"#;
    const ACTIVE: &str = r#"{"fragment_storage_index_0based":0,"key_latin1":"é","key_hex":"00","value_slot_1based":1,"environment_raw_hex":"aa","environment_trimmed_latin1":"é","FVALUE_f32_le_hex":"00000080","FVALUE":-0.0,"quality_integer":0,"zwitter_category":0}"#;

    fn fixture() -> Vec<(&'static str, Vec<u8>)> {
        let mut files = Vec::new();
        for (position, (file_id, count)) in NORMALIZED.into_iter().enumerate() {
            let row = match position {
                0 | 1 | 7 | 8 => TEXT,
                2 => PARAMETER,
                3..=6 => TAGGED,
                9 => ACTIVE,
                _ => unreachable!(),
            };
            files.push((file_id, format!("{row}\n").repeat(count).into_bytes()));
        }
        files.push((RAW_MODEL[2], vec![0, 1, 0xff]));
        files
    }
    fn references<'a>(files: &'a [(&'static str, Vec<u8>)]) -> Vec<(&'a str, &'a [u8])> {
        files
            .iter()
            .map(|(id, bytes)| (*id, bytes.as_slice()))
            .collect()
    }

    #[test]
    fn schema_integer_variants_preserve_f32_values_and_source_text() {
        let text = br#"{"line":1.0,"text":"-0.0"}"#;
        let rows = lines::<TextRow, _>(text, 1, text_row).unwrap();
        assert_eq!(rows[0].line, BigUint::from(1_u8));
        assert_eq!(rows[0].text, "-0.0");
        let parameter = br#"{"ordinal":-0.0,"data_role":"1.0","count":1e0,"element_bytes":0.0,"raw_payload_hex":"00","storage_kind":"synthetic","values":[1.0,{"ieee754_f32_le_hex":"00000080","value":-0,"finite":true}]}"#;
        let rows = lines::<ParameterWire, _>(parameter, 1, ParameterRow::try_from).unwrap();
        assert_eq!(rows[0].ordinal, BigUint::from(0_u8));
        let values = rows[0].values.as_ref().unwrap();
        assert!(matches!(&values[0], ParameterValue::Integer(value) if value == &BigInt::from(1)));
        let ParameterValue::F32(witness) = &values[1] else {
            panic!("expected F32")
        };
        assert_eq!(witness.bits(), 0x8000_0000);
        assert_eq!(witness.decimal(), "-0");
        let active = br#"{"fragment_storage_index_0based":-0.0,"key_latin1":"a","key_hex":"61","value_slot_1based":1.0,"environment_raw_hex":"","environment_trimmed_latin1":"","FVALUE_f32_le_hex":"00000080","FVALUE":-0,"quality_integer":-1e0,"zwitter_category":-0.0}"#;
        let rows = lines::<ActiveWire, _>(active, 1, ActiveRow::try_from).unwrap();
        assert_eq!(rows[0].value_slot_1based, BigUint::from(1_u8));
        assert_eq!(rows[0].quality_integer, BigInt::from(-1));
        assert_eq!(rows[0].zwitter_category, BigInt::from(0));
        assert_eq!(rows[0].fvalue.decimal(), "-0");
        for token in ["0.00000000000000000001", "1.00000000000000000001"] {
            let invalid = std::str::from_utf8(parameter)
                .unwrap()
                .replace("\"ordinal\":-0.0", &format!("\"ordinal\":{token}"));
            assert_eq!(
                lines::<ParameterWire, _>(invalid.as_bytes(), 1, ParameterRow::try_from).err(),
                Some(ClogpError::Record)
            );
        }
    }

    #[test]
    fn large_source_line_numbers_are_retained_without_reindexing() {
        let large = format!("1{}", "0".repeat(200));
        let input = format!(r#"{{"line":{large}.0,"text":"synthetic"}}"#);
        let rows = lines::<TextRow, _>(input.as_bytes(), 1, text_row).unwrap();
        assert_eq!(rows[0].line.to_string(), large);
        let tagged = TAGGED.replace("\"line\":1", &format!("\"line\":{large}.0"));
        let rows = lines::<TaggedRow, _>(tagged.as_bytes(), 1, tagged_row).unwrap();
        assert_eq!(rows[0].line.to_string(), large);
        let too_long = format!(r#"{{"line":{},"text":"synthetic"}}"#, "9".repeat(4097));
        assert_eq!(
            lines::<TextRow, _>(too_long.as_bytes(), 1, text_row).err(),
            Some(ClogpError::Record)
        );
    }

    #[test]
    fn large_parameter_values_and_f32_witness_are_retained_separately() {
        let large = format!("1{}", "0".repeat(200));
        let negative = format!("-{large}");
        let input = PARAMETER
            .replace("\"ordinal\":0", &format!("\"ordinal\":{large}.0"))
            .replace("\"count\":0", &format!("\"count\":{large}"))
            .replace("\"element_bytes\":0", &format!("\"element_bytes\":{large}"))
            .replace("\"values\":[42,", &format!("\"values\":[{negative}.0,"));
        assert!(input.contains(&format!("\"values\":[{negative}.0,")));
        let rows = lines::<ParameterWire, _>(input.as_bytes(), 1, ParameterRow::try_from).unwrap();
        assert_eq!(rows[0].ordinal.to_string(), large);
        assert_eq!(rows[0].count.to_string(), large);
        assert_eq!(rows[0].element_bytes.to_string(), large);
        let values = rows[0].values.as_ref().unwrap();
        let ParameterValue::Integer(integer) = &values[0] else {
            panic!("wrong variant")
        };
        assert_eq!(integer.to_string(), negative);
        let ParameterValue::F32(witness) = &values[1] else {
            panic!("wrong variant")
        };
        assert_eq!(witness.bits(), 0x3f80_0001);
    }

    #[test]
    fn large_active_source_numbers_keep_one_based_and_signed_fields() {
        let large = format!("1{}", "0".repeat(200));
        let negative = format!("-{large}");
        let input = ACTIVE
            .replace(
                "\"fragment_storage_index_0based\":0",
                &format!("\"fragment_storage_index_0based\":{large}"),
            )
            .replace(
                "\"value_slot_1based\":1",
                &format!("\"value_slot_1based\":{large}"),
            )
            .replace(
                "\"quality_integer\":0",
                &format!("\"quality_integer\":{negative}"),
            )
            .replace(
                "\"zwitter_category\":0",
                &format!("\"zwitter_category\":{large}"),
            );
        assert!(input.contains(&format!("\"quality_integer\":{negative}")));
        let rows = lines::<ActiveWire, _>(input.as_bytes(), 1, ActiveRow::try_from).unwrap();
        assert_eq!(rows[0].fragment_storage_index_0based.to_string(), large);
        assert_eq!(rows[0].value_slot_1based.to_string(), large);
        assert_eq!(rows[0].quality_integer.to_string(), negative);
        assert_eq!(rows[0].zwitter_category.to_string(), large);
        assert_eq!(rows[0].fvalue.bits(), 0x8000_0000);
        let zero_slot = input.replace(
            &format!("\"value_slot_1based\":{large}"),
            "\"value_slot_1based\":0",
        );
        assert_eq!(
            lines::<ActiveWire, _>(zero_slot.as_bytes(), 1, ActiveRow::try_from).err(),
            Some(ClogpError::Record)
        );
    }

    #[test]
    fn complete_carriers_preserve_lexical_and_numeric_witnesses_without_a_model() {
        let mut files = fixture();
        files.reverse();
        let data = decode_dataset(&references(&files)).unwrap();
        assert_eq!(data.tables.len(), 10);
        let CarrierRows::Parameters(rows) = &data.tables[2].rows else {
            panic!("wrong table")
        };
        assert_eq!(rows.len(), 26);
        let values = rows[0].values.as_ref().unwrap();
        let ParameterValue::Integer(integer) = &values[0] else {
            panic!("wrong value")
        };
        assert_eq!(integer, &BigInt::from(42));
        let ParameterValue::F32(float) = &values[1] else {
            panic!("wrong value")
        };
        assert_eq!(float.bits(), 0x3f80_0001);
        assert_eq!(float.decimal(), "1.000000059604644775390626");
        assert_eq!(rows[0].raw_payload_hex, "00");
        let CarrierRows::Active(rows) = &data.tables[9].rows else {
            panic!("wrong table")
        };
        assert_eq!(rows.len(), 1686);
        assert_eq!(rows[0].key_latin1, "é");
        assert_eq!(rows[0].key_hex, "00");
        assert_eq!(rows[0].fvalue.bits(), 0x8000_0000);
        assert_eq!(data.ancillary.len(), 1);
        assert_eq!(data.ancillary[0].file_id, RAW_MODEL[2]);
    }

    #[test]
    fn parameter_values_preserve_variant_shapes_and_reject_malformed_witnesses() {
        let cases = [
            PARAMETER.replace("0100803f", "0000803f"),
            PARAMETER.replace("\"finite\":true", "\"finite\":false"),
            PARAMETER.replace("\"raw_payload_hex\":\"00\"", "\"raw_payload_hex\":\"0\""),
            PARAMETER.replace("\"values\":[42,", "\"values\":[true,"),
            PARAMETER.replace("\"values\":[42,", "\"values\":[1.1,"),
            PARAMETER.replace("\"values\":[42,{\"ieee754_f32_le_hex\":\"0100803f\",\"value\":1.000000059604644775390626,\"finite\":true}]", "\"values\":null"),
        ];
        for invalid in cases {
            assert_eq!(
                lines::<ParameterWire, _>(invalid.as_bytes(), 1, ParameterRow::try_from).err(),
                Some(ClogpError::Record)
            );
        }
        let no_values = r#"{"ordinal":0,"data_role":"x","count":0,"element_bytes":0,"raw_payload_hex":"00","storage_kind":"x"}"#;
        assert!(lines::<ParameterWire, _>(no_values.as_bytes(), 1, ParameterRow::try_from).is_ok());
    }

    #[test]
    fn active_rows_validate_positions_latin1_hex_and_f32_witness() {
        for invalid in [
            ACTIVE.replace("\"value_slot_1based\":1", "\"value_slot_1based\":0"),
            ACTIVE.replace("\"key_latin1\":\"é\"", "\"key_latin1\":\"€\""),
            ACTIVE.replace(
                "\"environment_raw_hex\":\"aa\"",
                "\"environment_raw_hex\":\"az\"",
            ),
            ACTIVE.replace("\"FVALUE\":-0.0", "\"FVALUE\":0"),
        ] {
            assert_eq!(
                lines::<ActiveWire, _>(invalid.as_bytes(), 1, ActiveRow::try_from).err(),
                Some(ClogpError::Record)
            );
        }
    }

    #[test]
    fn line_files_counts_framing_and_closed_schemas_are_checked() {
        assert_eq!(
            lines(
                TEXT.replace("\"line\":1", "\"line\":0").as_bytes(),
                1,
                text_row
            )
            .err(),
            Some(ClogpError::Record)
        );
        assert_eq!(
            lines(
                TAGGED.replace("\"line\":1", "\"line\":0").as_bytes(),
                1,
                tagged_row
            )
            .err(),
            Some(ClogpError::Record)
        );
        assert_eq!(
            lines(TEXT.as_bytes(), 2, text_row).err(),
            Some(ClogpError::RecordCount)
        );
        assert_eq!(
            lines::<TextRow, _>(b"\xef\xbb\xbf{}", 1, Ok).err(),
            Some(ClogpError::Framing)
        );
        for bad in [
            &b"{\"line\":1,\"text\":\"x\",\"extra\":1}"[..],
            &b"{\"line\":1,\"line\":2,\"text\":\"x\"}"[..],
        ] {
            assert_eq!(
                lines::<TextRow, _>(bad, 1, Ok).err(),
                Some(ClogpError::Record)
            );
        }
        let files = fixture();
        let mut refs = references(&files);
        refs.remove(0);
        assert_eq!(decode_dataset(&refs).err(), Some(ClogpError::FileSet));
        let mut refs = references(&files);
        refs.push(refs[0]);
        assert_eq!(decode_dataset(&refs).err(), Some(ClogpError::FileSet));
    }

    #[test]
    fn file_ids_roles_classifications_and_catalog_identity_are_explicit() {
        assert!(accepts_file(
            RAW_DECLARATIVE[0],
            "auxiliary_data",
            "declarative_data"
        ));
        assert!(accepts_file(
            RAW_MODEL[0],
            "auxiliary_data",
            "model_parameters"
        ));
        assert!(accepts_file(
            NORMALIZED[0].0,
            "fragment_reference",
            "model_parameters"
        ));
        assert!(!accepts_file(
            RAW_MODEL[0],
            "auxiliary_data",
            "declarative_data"
        ));
        assert!(!accepts_file(
            NORMALIZED[0].0,
            "auxiliary_data",
            "model_parameters"
        ));
        assert!(!accepts_file(
            "unknown",
            "auxiliary_data",
            "declarative_data"
        ));
        assert_eq!(
            verify_catalog_files("legacy-clogp-carriers", "wrong", b"m", b"r", b"c"),
            Err(ClogpError::CatalogMismatch)
        );
    }
}
