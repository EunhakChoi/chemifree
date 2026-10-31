//! Prepared empirical carriers. Units, graph matching and prediction remain unbound.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, de::DeserializeOwned};
use serde_json::value::RawValue;

use crate::{
    json, phase6_profiles,
    prepared_f32::{self, F32Witness},
};

const RAW: [&str; 12] = [
    "P4-CHEMPROP-EMPIRICAL-001",
    "P4-CHEMPROP-EMPIRICAL-002",
    "P4-CHEMPROP-EMPIRICAL-003",
    "P4-CHEMPROP-EMPIRICAL-004",
    "P4-CHEMPROP-EMPIRICAL-005",
    "P4-CHEMPROP-EMPIRICAL-006",
    "P4-CHEMPROP-EMPIRICAL-007",
    "P4-CHEMPROP-EMPIRICAL-008",
    "P4-CHEMPROP-EMPIRICAL-009",
    "P4-CHEMPROP-EMPIRICAL-010",
    "P4-CHEMPROP-EMPIRICAL-011",
    "P4-CHEMPROP-EMPIRICAL-012",
];
const NORMALIZED: [(&str, usize); 5] = [
    ("DERIVED-CHEMPROP-EMPIRICAL-1", 2754),
    ("DERIVED-CHEMPROP-EMPIRICAL-2", 422),
    ("DERIVED-CHEMPROP-EMPIRICAL-3", 23),
    ("DERIVED-CHEMPROP-EMPIRICAL-4", 32077),
    ("DERIVED-CHEMPROP-EMPIRICAL-5", 8926),
];

/// A payload-free reason the prepared generation was refused.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EmpiricalError {
    CatalogMismatch,
    FileSet,
    Framing,
    Record,
    RecordCount,
}
impl std::fmt::Display for EmpiricalError {
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
impl std::error::Error for EmpiricalError {}

/// Verifies the dataset and exact metadata bytes against the public catalog.
///
/// # Errors
/// Rejects a different dataset, version or metadata identity.
pub fn verify_catalog_files(
    dataset_id: &str,
    dataset_version: &str,
    manifest: &[u8],
    registry: &[u8],
    catalog: &[u8],
) -> Result<(), EmpiricalError> {
    let expected = phase6_profiles::CatalogIdentity {
        dataset_id: "legacy-chemprop-empirical",
        dataset_version: "sha256-set-2add60f2984783ef6c3a8564a7b02dc8c5d41533c5f5c31b91995473209a2c4b",
        registry_sha256: "8ffd2e9b4d01ce989c63cb552714e1ff01acb8f766dbbc14bc411f08c4484d25",
        profile_sha256: "5add6a3e7a6250dc5c2f280d9a193dc8aef3ba008e47d10e35e7b263c217c2a2",
        manifest_file_sha256: "60515a6b0af65dc2852a570e8ef18213c869f4b41e36c41e31a21012269131fc",
    };
    if dataset_id != expected.dataset_id || dataset_version != expected.dataset_version {
        return Err(EmpiricalError::CatalogMismatch);
    }
    expected
        .check_file_bytes(manifest, registry, catalog)
        .map_err(|_| EmpiricalError::CatalogMismatch)
}

/// Selects recognized IDs with their declared catalog role and content class.
#[must_use]
pub fn accepts_file(id: &str, role: &str, class: &str) -> bool {
    role == "auxiliary_data"
        && class == "declarative_data"
        && (RAW.contains(&id) || NORMALIZED.iter().any(|(name, _)| *name == id))
}

/// Normalized tables are required for a complete generation.
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

/// A text reference with its source ordinal, whose starting convention is retained.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TextRow {
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub ordinal: u64,
    pub text: String,
}

/// Ordered source text for one explicitly selected catalog file.
#[derive(PartialEq)]
pub struct TextTable {
    pub file_id: &'static str,
    pub rows: Vec<TextRow>,
}

/// Reference ordinals are retained even if their target is unavailable.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ReferenceOrdinals {
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub literature: u64,
    #[serde(default, deserialize_with = "json::deserialize_optional_integer")]
    pub notes: Option<u64>,
    #[serde(default, deserialize_with = "json::deserialize_optional_integer")]
    pub units: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FloatWire {
    ieee754_f32_le_hex: String,
    value: Box<RawValue>,
    finite: bool,
}
impl FloatWire {
    fn decode(self) -> Result<F32Witness, EmpiricalError> {
        if !self.finite {
            return Err(EmpiricalError::Record);
        }
        prepared_f32::decode_f32(&self.ieee754_f32_le_hex, &self.value)
            .map_err(|_| EmpiricalError::Record)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MeasurementWire {
    #[serde(deserialize_with = "json::deserialize_integer")]
    molecule_id: i64,
    #[serde(
        rename = "DAT_field_id",
        deserialize_with = "json::deserialize_integer"
    )]
    dat_field_id: i64,
    #[serde(deserialize_with = "json::deserialize_integer")]
    source_offset: u64,
    #[serde(deserialize_with = "json::deserialize_integer")]
    flag_byte: u8,
    raw_record_hex: String,
    #[serde(default, deserialize_with = "present")]
    text: Option<String>,
    #[serde(default, deserialize_with = "present")]
    primary: Option<FloatWire>,
    #[serde(default, deserialize_with = "present")]
    reference_ordinals: Option<ReferenceOrdinals>,
    #[serde(default, deserialize_with = "present")]
    temperature: Option<FloatWire>,
    #[serde(default, deserialize_with = "present")]
    deviation: Option<FloatWire>,
}

/// An empirical row with inert provenance, flags and optional explicitly F32-bound witnesses.
#[derive(PartialEq)]
pub struct Measurement {
    pub molecule_id: i64,
    pub dat_field_id: i64,
    pub source_offset: u64,
    pub flag_byte: u8,
    pub raw_record_hex: String,
    pub text: Option<String>,
    pub primary: Option<F32Witness>,
    pub reference_ordinals: Option<ReferenceOrdinals>,
    pub temperature: Option<F32Witness>,
    pub deviation: Option<F32Witness>,
}
impl TryFrom<MeasurementWire> for Measurement {
    type Error = EmpiricalError;
    fn try_from(row: MeasurementWire) -> Result<Self, Self::Error> {
        if !hex_valid(&row.raw_record_hex) {
            return Err(EmpiricalError::Record);
        }
        Ok(Self {
            molecule_id: row.molecule_id,
            dat_field_id: row.dat_field_id,
            source_offset: row.source_offset,
            flag_byte: row.flag_byte,
            raw_record_hex: row.raw_record_hex,
            text: row.text,
            primary: row.primary.map(FloatWire::decode).transpose()?,
            reference_ordinals: row.reference_ordinals,
            temperature: row.temperature.map(FloatWire::decode).transpose()?,
            deviation: row.deviation.map(FloatWire::decode).transpose()?,
        })
    }
}

/// Source endpoints are 1-based positions, not document atom IDs.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Bond {
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub a_1based: u64,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub b_1based: u64,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub source_bond_code: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GraphWire {
    #[serde(deserialize_with = "json::deserialize_integer")]
    molecule_id: i64,
    #[serde(deserialize_with = "json::deserialize_integer_vec")]
    atomic_numbers: Vec<i64>,
    #[serde(
        rename = "header_H_count",
        deserialize_with = "json::deserialize_integer"
    )]
    header_h_count: i64,
    bonds: Vec<Bond>,
    unresolved_indexed_annotations: Box<RawValue>,
    header_raw_hex: String,
    #[serde(rename = "COS_row_raw_hex")]
    cos_row_raw_hex: String,
}

/// An unresolved JSON object is retained exactly, without treating it as a feature or command.
#[derive(PartialEq)]
pub struct OpaqueAnnotations(String);
impl OpaqueAnnotations {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Source graph framing, not a canonical chemical graph or a prediction result.
#[derive(PartialEq)]
pub struct GraphRow {
    pub molecule_id: i64,
    pub atomic_numbers: Vec<i64>,
    pub header_h_count: i64,
    pub bonds: Vec<Bond>,
    pub unresolved_indexed_annotations: OpaqueAnnotations,
    pub header_raw_hex: String,
    pub cos_row_raw_hex: String,
}
impl TryFrom<GraphWire> for GraphRow {
    type Error = EmpiricalError;
    fn try_from(row: GraphWire) -> Result<Self, Self::Error> {
        let atom_count = row.atomic_numbers.len() as u64;
        if !hex_valid(&row.header_raw_hex)
            || !hex_valid(&row.cos_row_raw_hex)
            || !row.unresolved_indexed_annotations.get().starts_with('{')
            || row.bonds.iter().any(|bond| {
                bond.a_1based == 0
                    || bond.b_1based == 0
                    || bond.a_1based > atom_count
                    || bond.b_1based > atom_count
            })
        {
            return Err(EmpiricalError::Record);
        }
        Ok(Self {
            molecule_id: row.molecule_id,
            atomic_numbers: row.atomic_numbers,
            header_h_count: row.header_h_count,
            bonds: row.bonds,
            unresolved_indexed_annotations: OpaqueAnnotations(
                row.unresolved_indexed_annotations.get().into(),
            ),
            header_raw_hex: row.header_raw_hex,
            cos_row_raw_hex: row.cos_row_raw_hex,
        })
    }
}

/// Opaque raw material is retained only as source data, never executed.
#[derive(PartialEq)]
pub struct OpaqueFile {
    pub file_id: &'static str,
    pub bytes: Vec<u8>,
}

/// All normalized streams and available ancillary source bytes form one generation.
#[derive(PartialEq)]
pub struct EmpiricalData {
    pub text: Vec<TextTable>,
    pub measurements: Vec<Measurement>,
    pub graphs: Vec<GraphRow>,
    pub ancillary: Vec<OpaqueFile>,
}

fn lines<T: DeserializeOwned, U>(
    bytes: &[u8],
    count: usize,
    mut convert: impl FnMut(T) -> Result<U, EmpiricalError>,
) -> Result<Vec<U>, EmpiricalError> {
    let text = std::str::from_utf8(bytes).map_err(|_| EmpiricalError::Framing)?;
    if text.starts_with('\u{feff}') {
        return Err(EmpiricalError::Framing);
    }
    let mut rows = Vec::with_capacity(count);
    for line in text.split('\n') {
        if line.is_empty() || line == "\r" {
            continue;
        }
        if rows.len() == count {
            return Err(EmpiricalError::RecordCount);
        }
        let row = serde_json::from_str(line).map_err(|_| EmpiricalError::Record)?;
        rows.push(convert(row)?);
    }
    if rows.len() != count {
        return Err(EmpiricalError::RecordCount);
    }
    Ok(rows)
}

/// Decode all five normalized streams and optional raw carriers atomically.
///
/// # Errors
/// Rejects unrecognized, duplicate, oversized, missing or invalid files and records.
pub fn decode_dataset(files: &[(&str, &[u8])]) -> Result<EmpiricalData, EmpiricalError> {
    let mut index = BTreeMap::new();
    let mut total = 0_u64;
    for (id, bytes) in files {
        total = total
            .checked_add(bytes.len() as u64)
            .ok_or(EmpiricalError::Framing)?;
        if !(RAW.contains(id) || is_normalized(id))
            || bytes.len() as u64 > crate::MAX_DATA_FILE_BYTES
            || total > crate::MAX_DATA_TOTAL_BYTES
            || index.insert(*id, *bytes).is_some()
        {
            return Err(EmpiricalError::FileSet);
        }
    }
    let get = |id| index.get(id).copied().ok_or(EmpiricalError::FileSet);
    let mut text = Vec::with_capacity(3);
    for (file_id, count) in NORMALIZED[..3].iter().copied() {
        text.push(TextTable {
            file_id,
            rows: lines(get(file_id)?, count, Ok)?,
        });
    }
    let measurements = lines::<MeasurementWire, _>(
        get(NORMALIZED[3].0)?,
        NORMALIZED[3].1,
        Measurement::try_from,
    )?;
    let graphs = lines::<GraphWire, _>(get(NORMALIZED[4].0)?, NORMALIZED[4].1, GraphRow::try_from)?;
    let ancillary = RAW
        .iter()
        .filter_map(|&file_id| {
            index.get(file_id).map(|bytes| OpaqueFile {
                file_id,
                bytes: bytes.to_vec(),
            })
        })
        .collect();
    Ok(EmpiricalData {
        text,
        measurements,
        graphs,
        ancillary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MEASUREMENT: &str = r#"{"molecule_id":7,"DAT_field_id":2,"source_offset":0,"flag_byte":255,"raw_record_hex":"A0ff","text":"synthetic","primary":{"ieee754_f32_le_hex":"0100803f","value":1.000000059604644775390626,"finite":true},"temperature":{"ieee754_f32_le_hex":"00000080","value":-0.0,"finite":true},"reference_ordinals":{"literature":0}}"#;
    const GRAPH: &str = r#"{"molecule_id":7,"atomic_numbers":[6,8],"header_H_count":0,"bonds":[{"a_1based":1,"b_1based":2,"source_bond_code":1}],"unresolved_indexed_annotations":{"unknown_decimal":0.1000000000000000000000001},"header_raw_hex":"AA00","COS_row_raw_hex":"bb"}"#;

    fn fixture() -> Vec<(&'static str, Vec<u8>)> {
        let mut files = Vec::new();
        for (file_id, count) in NORMALIZED {
            let row = if file_id == NORMALIZED[3].0 {
                MEASUREMENT
            } else if file_id == NORMALIZED[4].0 {
                GRAPH
            } else {
                r#"{"ordinal":0,"text":"synthetic"}"#
            };
            files.push((file_id, format!("{row}\n").repeat(count).into_bytes()));
        }
        files.push((RAW[4], vec![0, 1, 0xff]));
        files
    }

    fn references<'a>(files: &'a [(&'static str, Vec<u8>)]) -> Vec<(&'a str, &'a [u8])> {
        files
            .iter()
            .map(|(id, bytes)| (*id, bytes.as_slice()))
            .collect()
    }

    #[test]
    fn complete_generation_retains_exact_witnesses_and_opaque_annotations() {
        let mut files = fixture();
        files.reverse();
        let data = decode_dataset(&references(&files)).unwrap();
        assert_eq!(
            data.text
                .iter()
                .map(|table| table.rows.len())
                .collect::<Vec<_>>(),
            [2754, 422, 23]
        );
        assert_eq!(data.measurements.len(), 32077);
        assert_eq!(data.graphs.len(), 8926);
        let measurement = &data.measurements[0];
        assert_eq!(measurement.flag_byte, 255);
        assert_eq!(measurement.primary.as_ref().unwrap().bits(), 0x3f80_0001);
        assert_eq!(
            measurement.primary.as_ref().unwrap().decimal(),
            "1.000000059604644775390626"
        );
        assert_eq!(
            measurement.temperature.as_ref().unwrap().bits(),
            0x8000_0000
        );
        assert_eq!(data.graphs[0].bonds[0].b_1based, 2);
        assert_eq!(
            data.graphs[0].unresolved_indexed_annotations.as_str(),
            r#"{"unknown_decimal":0.1000000000000000000000001}"#
        );
        assert_eq!(data.ancillary.len(), 1);
        assert_eq!(data.ancillary[0].file_id, RAW[4]);
    }

    #[test]
    fn float_flags_bit_witnesses_and_optional_field_types_are_checked() {
        let cases = [
            MEASUREMENT.replace("0100803f", "0000803f"),
            MEASUREMENT.replace("\"finite\":true", "\"finite\":false"),
            MEASUREMENT.replace("\"primary\":{\"ieee754_f32_le_hex\":\"0100803f\",\"value\":1.000000059604644775390626,\"finite\":true}", "\"primary\":null"),
            MEASUREMENT.replace("A0ff", "A0f"),
            MEASUREMENT.replace("\"flag_byte\":255", "\"flag_byte\":256"),
        ];
        for invalid in cases {
            assert_eq!(
                lines::<MeasurementWire, _>(invalid.as_bytes(), 1, Measurement::try_from).err(),
                Some(EmpiricalError::Record)
            );
        }
        let minimal = r#"{"molecule_id":0,"DAT_field_id":0,"source_offset":0,"flag_byte":0,"raw_record_hex":""}"#;
        assert!(lines::<MeasurementWire, _>(minimal.as_bytes(), 1, Measurement::try_from).is_ok());
    }

    #[test]
    fn graph_endpoints_hex_and_opaque_object_shape_are_checked() {
        for invalid in [
            GRAPH.replace("\"a_1based\":1", "\"a_1based\":0"),
            GRAPH.replace("\"b_1based\":2", "\"b_1based\":3"),
            GRAPH.replace("\"header_raw_hex\":\"AA00\"", "\"header_raw_hex\":\"zz\""),
            GRAPH.replace("\"unresolved_indexed_annotations\":{\"unknown_decimal\":0.1000000000000000000000001}", "\"unresolved_indexed_annotations\":null"),
        ] {
            assert_eq!(lines::<GraphWire, _>(invalid.as_bytes(), 1, GraphRow::try_from).err(), Some(EmpiricalError::Record));
        }
    }

    #[test]
    fn integer_negative_zero_keeps_float_and_opaque_zero_unchanged() {
        let text = b"{\"ordinal\": \t-0 \t,\"text\":\"synthetic\"}";
        let row = lines::<TextRow, _>(text, 1, Ok).unwrap();
        assert_eq!(row[0].ordinal, 0);
        for invalid in [
            &b"{\"ordinal\":-1,\"text\":\"synthetic\"}"[..],
            &b"{\"ordinal\":\"0\",\"text\":\"synthetic\"}"[..],
            &b"{\"ordinal\":null,\"text\":\"synthetic\"}"[..],
        ] {
            assert_eq!(
                lines::<TextRow, _>(invalid, 1, Ok).err(),
                Some(EmpiricalError::Record)
            );
        }

        let measurement = br#"{"molecule_id":-0,"DAT_field_id":-0,"source_offset":-0,"flag_byte":-0,"raw_record_hex":"","reference_ordinals":{"literature":-0,"notes":-0},"primary":{"ieee754_f32_le_hex":"00000080","value":-0,"finite":true}}"#;
        let row = lines::<MeasurementWire, _>(measurement, 1, Measurement::try_from).unwrap();
        assert_eq!(row[0].reference_ordinals.as_ref().unwrap().notes, Some(0));
        let witness = row[0].primary.as_ref().unwrap();
        assert_eq!(witness.bits(), 0x8000_0000);
        assert_eq!(witness.decimal(), "-0");
        let null_note = std::str::from_utf8(measurement)
            .unwrap()
            .replace("\"notes\":-0", "\"notes\":null");
        assert_eq!(
            lines::<MeasurementWire, _>(null_note.as_bytes(), 1, Measurement::try_from).err(),
            Some(EmpiricalError::Record)
        );

        let graph = br#"{"molecule_id":-0,"atomic_numbers":[-0],"header_H_count":-0,"bonds":[{"a_1based":1,"b_1based":1,"source_bond_code":-0}],"unresolved_indexed_annotations":{"preserve":-0},"header_raw_hex":"","COS_row_raw_hex":""}"#;
        let row = lines::<GraphWire, _>(graph, 1, GraphRow::try_from).unwrap();
        assert_eq!(row[0].atomic_numbers, [0]);
        assert_eq!(row[0].bonds[0].source_bond_code, 0);
        assert_eq!(
            row[0].unresolved_indexed_annotations.as_str(),
            r#"{"preserve":-0}"#
        );
    }

    #[test]
    fn schema_integer_values_are_exact_across_decimal_and_exponent_spellings() {
        for (token, expected) in [
            ("0.0", 0),
            ("-0.0", 0),
            ("-0e0", 0),
            ("0e99999999999999999999", 0),
            ("1e0", 1),
            ("1E+0", 1),
            ("120.00e-1", 12),
            ("184467440737095516150e-1", u64::MAX),
        ] {
            let line = format!(r#"{{"ordinal":{token},"text":"synthetic"}}"#);
            let row = lines::<TextRow, _>(line.as_bytes(), 1, Ok).unwrap();
            assert_eq!(row[0].ordinal, expected, "{token}");
        }
        for token in [
            "0.00000000000000000001",
            "1.00000000000000000001",
            "18446744073709551616.0",
            "1e99999999999999999999",
            "1e-99999999999999999999",
        ] {
            let line = format!(r#"{{"ordinal":{token},"text":"synthetic"}}"#);
            assert_eq!(
                lines::<TextRow, _>(line.as_bytes(), 1, Ok).err(),
                Some(EmpiricalError::Record),
                "{token}"
            );
        }
    }

    #[test]
    fn schema_integers_respect_each_field_type_range() {
        let measurement = MEASUREMENT
            .replace(
                "\"molecule_id\":7",
                "\"molecule_id\":-92233720368547758080e-1",
            )
            .replace("\"flag_byte\":255", "\"flag_byte\":255.0");
        let row =
            lines::<MeasurementWire, _>(measurement.as_bytes(), 1, Measurement::try_from).unwrap();
        assert_eq!(row[0].molecule_id, i64::MIN);
        assert_eq!(row[0].flag_byte, u8::MAX);
        for invalid in [
            measurement.replace("-92233720368547758080e-1", "-9223372036854775809.0"),
            measurement.replace("\"flag_byte\":255.0", "\"flag_byte\":256.0"),
            measurement.replace(
                "\"flag_byte\":255.0",
                "\"flag_byte\":254.999999999999999999",
            ),
        ] {
            assert_eq!(
                lines::<MeasurementWire, _>(invalid.as_bytes(), 1, Measurement::try_from).err(),
                Some(EmpiricalError::Record)
            );
        }
    }

    #[test]
    fn files_counts_framing_and_closed_shapes_are_checked() {
        let files = fixture();
        let mut refs = references(&files);
        refs.remove(0);
        assert_eq!(decode_dataset(&refs).err(), Some(EmpiricalError::FileSet));
        let mut refs = references(&files);
        refs.push(refs[0]);
        assert_eq!(decode_dataset(&refs).err(), Some(EmpiricalError::FileSet));
        assert_eq!(
            lines::<TextRow, _>(b"{\"ordinal\":0,\"text\":\"x\"}\n", 2, Ok).err(),
            Some(EmpiricalError::RecordCount)
        );
        assert_eq!(
            lines::<TextRow, _>(b"\xef\xbb\xbf{}", 1, Ok).err(),
            Some(EmpiricalError::Framing)
        );
        for bad in [
            &b"{\"ordinal\":0,\"text\":\"x\",\"extra\":1}"[..],
            &b"{\"ordinal\":0,\"ordinal\":1,\"text\":\"x\"}"[..],
        ] {
            assert_eq!(
                lines::<TextRow, _>(bad, 1, Ok).err(),
                Some(EmpiricalError::Record)
            );
        }
    }

    #[test]
    fn catalog_file_id_role_and_metadata_identity_are_explicit() {
        assert!(is_normalized(NORMALIZED[0].0));
        assert!(!is_normalized(RAW[0]));
        assert!(accepts_file(RAW[0], "auxiliary_data", "declarative_data"));
        assert!(!accepts_file(RAW[0], "weights", "declarative_data"));
        assert!(!accepts_file(
            "unknown",
            "auxiliary_data",
            "declarative_data"
        ));
        assert_eq!(
            verify_catalog_files("legacy-chemprop-empirical", "wrong", b"m", b"r", b"c"),
            Err(EmpiricalError::CatalogMismatch)
        );
    }
}
