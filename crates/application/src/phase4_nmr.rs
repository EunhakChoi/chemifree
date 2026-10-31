//! Typed lexical NMR carriers. No spectral inference or executable input is supplied.

use std::collections::BTreeMap;

use num_bigint::BigUint;
use serde::{Deserialize, Deserializer, de::DeserializeOwned};

use crate::{json, phase6_profiles};

const COUNTS: [usize; 19] = [
    207, 1007, 935, 1483, 1438, 7600, 875, 79160, 3835, 141291, 9505, 47889, 14957, 62, 18534,
    1967, 3298, 3476, 3473,
];

/// Payload-free rejection of an invalid set or normalized carrier.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CarrierError {
    FileSet,
    Framing,
    Record,
    RecordCount,
    LineNumber,
    CatalogMismatch,
}

impl std::fmt::Display for CarrierError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::FileSet => "invalid_file_set",
            Self::Framing => "invalid_data_framing",
            Self::Record => "invalid_data_record",
            Self::RecordCount => "invalid_record_count",
            Self::LineNumber => "invalid_line_number",
            Self::CatalogMismatch => "catalog_identity_mismatch",
        })
    }
}

impl std::error::Error for CarrierError {}

/// Checks actual metadata bytes against the independently published catalog identities.
///
/// # Errors
/// Rejects different dataset identities, metadata sizes or hashes.
pub fn verify_catalog_files(
    dataset_id: &str,
    dataset_version: &str,
    manifest: &[u8],
    registry: &[u8],
    catalog: &[u8],
) -> Result<(), CarrierError> {
    let expected = phase6_profiles::CatalogIdentity {
        dataset_id: "legacy-nmr-carriers",
        dataset_version: "sha256-set-a7010be794905df609c0a30a6cbf4a4f0b0367ec1b3b2230a51b9965836efd81",
        registry_sha256: "8ffd2e9b4d01ce989c63cb552714e1ff01acb8f766dbbc14bc411f08c4484d25",
        profile_sha256: "5add6a3e7a6250dc5c2f280d9a193dc8aef3ba008e47d10e35e7b263c217c2a2",
        manifest_file_sha256: "7effdfdad2ed51d90a55c126c050eeeeb9d407076d229cafd4575192222a636a",
    };
    if dataset_id != expected.dataset_id || dataset_version != expected.dataset_version {
        return Err(CarrierError::CatalogMismatch);
    }
    expected
        .check_file_bytes(manifest, registry, catalog)
        .map_err(|_| CarrierError::CatalogMismatch)
}

/// The catalog distinguishes original, decoded and normalized file stages.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Stage {
    Raw,
    Decoded,
    Normalized,
}

/// A catalog file's one-based index (1 through 19) and declared processing stage.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct FileKey {
    pub index: u8,
    pub stage: Stage,
}

/// Parses the explicit catalog file ID without using its filesystem name.
#[must_use]
pub fn parse_file_id(id: &str) -> Option<FileKey> {
    let rest = id.strip_prefix("P4-NMR-CARRIERS-")?;
    let digits = rest.get(..3)?;
    if !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let index: u8 = digits.parse().ok()?;
    if !(1..=19).contains(&index) {
        return None;
    }
    let stage = match rest.get(3..)? {
        "" => Stage::Raw,
        "-decoded" => Stage::Decoded,
        "-normalized" => Stage::Normalized,
        _ => return None,
    };
    Some(FileKey { index, stage })
}

/// Validates a file's inert classification and role for its catalog ID.
#[must_use]
pub fn accepts_file(id: &str, role: &str, class: &str) -> bool {
    let Some(key) = parse_file_id(id) else {
        return false;
    };
    class == "model_parameters"
        && role
            == if key.stage == Stage::Normalized {
                "contribution_table"
            } else {
                "auxiliary_data"
            }
}

/// Tokens retain their original lexical kind and raw text without interpreting a model.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Token {
    pub kind: String,
    pub raw: String,
}

/// Composite tokens additionally preserve their optional ordered lexical parts.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CompositeToken {
    pub kind: String,
    pub raw: String,
    #[serde(default, deserialize_with = "present_value")]
    pub parts: Option<Vec<String>>,
}

/// A lexical row that may carry a comment, text and ordered tokens.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CommentedRow {
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub line: BigUint,
    pub kind: String,
    #[serde(default, deserialize_with = "present_value")]
    pub tokens: Option<Vec<Token>>,
    #[serde(default, deserialize_with = "present_value")]
    pub comment: Option<String>,
    #[serde(default, deserialize_with = "present_value")]
    pub text: Option<String>,
}

/// A lexical row with an optional label and ordered tokens.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LabelRow {
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub line: BigUint,
    pub kind: String,
    #[serde(default, deserialize_with = "present_value")]
    pub label: Option<String>,
    #[serde(default, deserialize_with = "present_value")]
    pub tokens: Option<Vec<Token>>,
}

/// A lexical row with optional label, lexemes and tokens.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LexicalRow {
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub line: BigUint,
    pub kind: String,
    #[serde(default, deserialize_with = "present_value")]
    pub lexemes: Option<Vec<String>>,
    #[serde(default, deserialize_with = "present_value")]
    pub label: Option<String>,
    #[serde(default, deserialize_with = "present_value")]
    pub tokens: Option<Vec<Token>>,
}

/// A keyed lexical row with its required token sequence.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct KeyedRow {
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub line: BigUint,
    pub kind: String,
    pub tokens: Vec<Token>,
    pub key_lexeme: String,
}

/// A row whose optional composite tokens can contain ordered parts.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CompositeRow {
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub line: BigUint,
    pub kind: String,
    #[serde(default, deserialize_with = "present_value")]
    pub lexemes: Option<Vec<String>>,
    #[serde(default, deserialize_with = "present_value")]
    pub tokens: Option<Vec<CompositeToken>>,
    #[serde(default, deserialize_with = "present_value")]
    pub key_lexeme: Option<String>,
}

/// The public catalog selects exactly one closed schema for each normalized file.
#[derive(PartialEq)]
pub enum CarrierRow {
    Commented(CommentedRow),
    Label(LabelRow),
    Lexical(LexicalRow),
    Keyed(KeyedRow),
    Composite(CompositeRow),
}

impl CarrierRow {
    fn line(&self) -> &BigUint {
        match self {
            Self::Commented(row) => &row.line,
            Self::Label(row) => &row.line,
            Self::Lexical(row) => &row.line,
            Self::Keyed(row) => &row.line,
            Self::Composite(row) => &row.line,
        }
    }
}

/// Ordered rows for one normalized file; storage index is not a chemical identity.
#[derive(PartialEq)]
pub struct CarrierTable {
    pub key: FileKey,
    pub rows: Vec<CarrierRow>,
}

/// Ancillary source bytes are inert and never used as executable input.
#[derive(PartialEq)]
pub struct OpaqueCarrier {
    pub key: FileKey,
    pub bytes: Vec<u8>,
}

/// A complete normalized collection and any available ancillary source files.
#[derive(PartialEq)]
pub struct NmrCarriers {
    pub tables: Vec<CarrierTable>,
    pub ancillary: Vec<OpaqueCarrier>,
}

#[derive(Clone, Copy)]
enum Shape {
    Commented,
    Label,
    Lexical,
    Keyed,
    Composite,
}

fn shape(index: u8) -> Shape {
    match index {
        1 => Shape::Commented,
        2 | 3 | 16 | 17 => Shape::Label,
        4 | 5 | 18 | 19 => Shape::Lexical,
        6 | 8 | 10 | 12 | 14 => Shape::Keyed,
        7 | 9 | 11 | 13 | 15 => Shape::Composite,
        _ => unreachable!("file keys are checked before shape selection"),
    }
}

fn present_value<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

fn from_line<T: DeserializeOwned>(line: &str) -> Result<T, CarrierError> {
    serde_json::from_str(line).map_err(|_| CarrierError::Record)
}

fn decode_rows(
    bytes: &[u8],
    shape: Shape,
    expected: usize,
) -> Result<Vec<CarrierRow>, CarrierError> {
    let text = std::str::from_utf8(bytes).map_err(|_| CarrierError::Framing)?;
    if text.starts_with('\u{feff}') {
        return Err(CarrierError::Framing);
    }
    let mut rows = Vec::with_capacity(expected);
    for line in text.split('\n') {
        if line.is_empty() || line == "\r" {
            continue;
        }
        if rows.len() == expected {
            return Err(CarrierError::RecordCount);
        }
        json::validate_raw_strict(line.as_bytes()).map_err(|_| CarrierError::Record)?;
        let row = match shape {
            Shape::Commented => CarrierRow::Commented(from_line(line)?),
            Shape::Label => CarrierRow::Label(from_line(line)?),
            Shape::Lexical => CarrierRow::Lexical(from_line(line)?),
            Shape::Keyed => CarrierRow::Keyed(from_line(line)?),
            Shape::Composite => CarrierRow::Composite(from_line(line)?),
        };
        if row.line() == &BigUint::from(0_u8) {
            return Err(CarrierError::LineNumber);
        }
        rows.push(row);
    }
    if rows.len() != expected {
        return Err(CarrierError::RecordCount);
    }
    Ok(rows)
}

/// Decode all 19 normalized streams and any supplied ancillary files atomically.
///
/// # Errors
/// Rejects unknown, repeated or incomplete files, excessive size and invalid records.
pub fn decode_dataset(files: &[(&str, &[u8])]) -> Result<NmrCarriers, CarrierError> {
    let mut indexed = BTreeMap::new();
    let mut total = 0_u64;
    for (id, bytes) in files {
        let key = parse_file_id(id).ok_or(CarrierError::FileSet)?;
        total = total
            .checked_add(bytes.len() as u64)
            .ok_or(CarrierError::Framing)?;
        if bytes.len() as u64 > crate::MAX_DATA_FILE_BYTES
            || total > crate::MAX_DATA_TOTAL_BYTES
            || indexed.insert(key, *bytes).is_some()
        {
            return Err(CarrierError::FileSet);
        }
    }
    let mut tables = Vec::with_capacity(19);
    let mut ancillary = Vec::new();
    for (key, bytes) in indexed {
        if key.stage == Stage::Normalized {
            let rows = decode_rows(bytes, shape(key.index), COUNTS[usize::from(key.index - 1)])?;
            tables.push(CarrierTable { key, rows });
        } else {
            ancillary.push(OpaqueCarrier {
                key,
                bytes: bytes.to_vec(),
            });
        }
    }
    if tables.len() != COUNTS.len() {
        return Err(CarrierError::FileSet);
    }
    Ok(NmrCarriers { tables, ancillary })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_identity_requires_the_exact_public_metadata_bytes() {
        assert_eq!(
            verify_catalog_files(
                "legacy-nmr-carriers",
                "sha256-set-a7010be794905df609c0a30a6cbf4a4f0b0367ec1b3b2230a51b9965836efd81",
                b"independent manifest",
                b"independent registry",
                b"independent catalog",
            ),
            Err(CarrierError::CatalogMismatch)
        );
    }

    #[test]
    fn explicit_file_ids_select_the_declared_stage_and_role() {
        assert_eq!(
            parse_file_id("P4-NMR-CARRIERS-001"),
            Some(FileKey {
                index: 1,
                stage: Stage::Raw
            })
        );
        assert_eq!(
            parse_file_id("P4-NMR-CARRIERS-019-decoded"),
            Some(FileKey {
                index: 19,
                stage: Stage::Decoded
            })
        );
        assert_eq!(
            parse_file_id("P4-NMR-CARRIERS-010-normalized"),
            Some(FileKey {
                index: 10,
                stage: Stage::Normalized
            })
        );
        for id in [
            "P4-NMR-CARRIERS-000",
            "P4-NMR-CARRIERS-020",
            "P4-NMR-CARRIERS-01",
            "P4-NMR-CARRIERS-001-unknown",
            "P4-NMR-CARRIERS-α01",
        ] {
            assert_eq!(parse_file_id(id), None);
        }
        assert!(accepts_file(
            "P4-NMR-CARRIERS-001-normalized",
            "contribution_table",
            "model_parameters"
        ));
        assert!(accepts_file(
            "P4-NMR-CARRIERS-001-decoded",
            "auxiliary_data",
            "model_parameters"
        ));
        assert!(!accepts_file(
            "P4-NMR-CARRIERS-001-normalized",
            "auxiliary_data",
            "model_parameters"
        ));
        assert!(!accepts_file(
            "P4-NMR-CARRIERS-001",
            "auxiliary_data",
            "declarative_data"
        ));
    }

    #[test]
    fn each_carrier_shape_keeps_exact_large_and_decimal_source_lines() {
        let line = "9".repeat(400);
        let cases = [
            (Shape::Commented, ""),
            (Shape::Label, ""),
            (Shape::Lexical, ""),
            (Shape::Keyed, ",\"tokens\":[],\"key_lexeme\":\"x\""),
            (Shape::Composite, ""),
        ];
        for (shape, extra) in cases {
            let text = format!("{{\"line\":{line},\"kind\":\"fixture\"{extra}}}");
            let rows = decode_rows(text.as_bytes(), shape, 1).unwrap();
            assert_eq!(rows[0].line().to_string(), line);
            let decimal = format!("{{\"line\":1.0,\"kind\":\"fixture\"{extra}}}");
            assert_eq!(
                decode_rows(decimal.as_bytes(), shape, 1).unwrap()[0]
                    .line()
                    .to_string(),
                "1"
            );
            let exponent = format!("{{\"line\":1e0,\"kind\":\"fixture\"{extra}}}");
            assert_eq!(
                decode_rows(exponent.as_bytes(), shape, 1).unwrap()[0]
                    .line()
                    .to_string(),
                "1"
            );
            let fraction =
                format!("{{\"line\":1.00000000000000000001,\"kind\":\"fixture\"{extra}}}");
            assert_eq!(
                decode_rows(fraction.as_bytes(), shape, 1).err(),
                Some(CarrierError::Record)
            );
        }
        assert_eq!(
            decode_rows(br#"{"line":0.0,"kind":"fixture"}"#, Shape::Label, 1).err(),
            Some(CarrierError::LineNumber)
        );
        let overlong = format!("{{\"line\":{},\"kind\":\"fixture\"}}", "9".repeat(4097));
        assert_eq!(
            decode_rows(overlong.as_bytes(), Shape::Label, 1).err(),
            Some(CarrierError::Record)
        );
    }

    #[test]
    fn each_catalog_shape_keeps_its_distinct_lexical_fields() {
        let examples: [(Shape, &[u8]); 5] = [
            (Shape::Commented, r#"{"line":1,"kind":"comment","comment":"independent","tokens":[{"kind":"word","raw":"α"}]}"#.as_bytes()),
            (Shape::Label, br#"{"line":1,"kind":"label","label":"independent","tokens":[]}"#),
            (Shape::Lexical, br#"{"line":1,"kind":"lexical","label":"x","lexemes":["a","b"]}"#),
            (Shape::Keyed, br#"{"line":1,"kind":"key","key_lexeme":"x","tokens":[{"kind":"number","raw":"1"}]}"#),
            (Shape::Composite, br#"{"line":1,"kind":"composite","tokens":[{"kind":"group","raw":"x,y","parts":["x","y"]}]}"#),
        ];
        for (shape, example) in examples {
            let rows = decode_rows(example, shape, 1).unwrap();
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].line().to_string(), "1");
        }
        let rows = decode_rows(examples[4].1, Shape::Composite, 1).unwrap();
        let CarrierRow::Composite(row) = &rows[0] else {
            panic!("wrong shape")
        };
        assert_eq!(
            row.tokens.as_ref().unwrap()[0].parts.as_deref(),
            Some(["x".into(), "y".into()].as_slice())
        );
        assert_eq!(
            decode_rows(examples[0].1, Shape::Label, 1).err(),
            Some(CarrierError::Record)
        );
    }

    #[test]
    fn framing_optional_values_and_source_lines_follow_the_public_rules() {
        let rows = b"\r\n{\"line\":3,\"kind\":\"a\"}\r\n\r\n{\"line\":3,\"kind\":\"b\"}\r\n";
        let decoded = decode_rows(rows, Shape::Label, 2).unwrap();
        assert_eq!(
            decoded
                .iter()
                .map(|row| row.line().to_string())
                .collect::<Vec<_>>(),
            ["3", "3"]
        );
        assert_eq!(
            decode_rows(b"{\"line\":0,\"kind\":\"a\"}", Shape::Label, 1).err(),
            Some(CarrierError::LineNumber)
        );
        assert_eq!(
            decode_rows(
                b"{\"line\":1,\"kind\":\"a\",\"label\":null}",
                Shape::Label,
                1
            )
            .err(),
            Some(CarrierError::Record)
        );
        assert_eq!(
            decode_rows(
                b"{\"line\":1,\"kind\":\"a\",\"tokens\":[{\"kind\":\"x\",\"raw\":\"x\",\"parts\":null}]}",
                Shape::Composite,
                1,
            )
            .err(),
            Some(CarrierError::Record)
        );
        assert_eq!(
            decode_rows(
                b"{\"line\":1,\"kind\":\"a\",\"key_lexeme\":\"x\"}",
                Shape::Keyed,
                1
            )
            .err(),
            Some(CarrierError::Record)
        );
        assert_eq!(
            decode_rows(b"{\"line\":1,\"kind\":\"a\",\"line\":2}", Shape::Label, 1).err(),
            Some(CarrierError::Record)
        );
        assert_eq!(
            decode_rows(b"\xef\xbb\xbf{}", Shape::Label, 1).err(),
            Some(CarrierError::Framing)
        );
        assert_eq!(
            decode_rows(b"{\"line\":1,\"kind\":\"a\"}\n", Shape::Label, 2).err(),
            Some(CarrierError::RecordCount)
        );
    }

    #[test]
    fn unknown_and_duplicate_ids_cannot_produce_a_dataset() {
        assert_eq!(
            decode_dataset(&[("unknown", b"fixture")]).err(),
            Some(CarrierError::FileSet)
        );
        assert_eq!(
            decode_dataset(&[("P4-NMR-CARRIERS-001", b"a"), ("P4-NMR-CARRIERS-001", b"b")]).err(),
            Some(CarrierError::FileSet)
        );
        assert_eq!(
            decode_dataset(&[("P4-NMR-CARRIERS-001", b"fixture")]).err(),
            Some(CarrierError::FileSet)
        );
    }

    #[test]
    fn all_nineteen_normalized_files_are_needed_and_keep_opaque_sources_separate() {
        // These counts and shapes are copied from the public format catalog; the rows are fictional.
        const DECLARED: [(usize, &str); 19] = [
            (207, "ordinary"),
            (1007, "ordinary"),
            (935, "ordinary"),
            (1483, "ordinary"),
            (1438, "ordinary"),
            (7600, "keyed"),
            (875, "ordinary"),
            (79160, "keyed"),
            (3835, "ordinary"),
            (141291, "keyed"),
            (9505, "ordinary"),
            (47889, "keyed"),
            (14957, "ordinary"),
            (62, "keyed"),
            (18534, "ordinary"),
            (1967, "ordinary"),
            (3298, "ordinary"),
            (3476, "ordinary"),
            (3473, "ordinary"),
        ];
        let mut files: Vec<(String, Vec<u8>)> = DECLARED
            .iter()
            .enumerate()
            .map(|(offset, (count, kind))| {
                let row = if *kind == "keyed" {
                    "{\"line\":1,\"kind\":\"fixture\",\"tokens\":[],\"key_lexeme\":\"x\"}\n"
                } else {
                    "{\"line\":1,\"kind\":\"fixture\"}\n"
                };
                (
                    format!("P4-NMR-CARRIERS-{:03}-normalized", offset + 1),
                    row.repeat(*count).into_bytes(),
                )
            })
            .collect();
        files.push((
            "P4-NMR-CARRIERS-001".into(),
            b"fictional inert bytes".to_vec(),
        ));
        let refs: Vec<_> = files
            .iter()
            .map(|(id, bytes)| (id.as_str(), bytes.as_slice()))
            .collect();
        let data = decode_dataset(&refs).unwrap();
        assert_eq!(data.tables.len(), 19);
        assert_eq!(
            data.tables
                .iter()
                .map(|table| table.rows.len())
                .sum::<usize>(),
            340_992
        );
        assert_eq!(data.tables[9].key.index, 10);
        assert_eq!(data.ancillary.len(), 1);
        assert_eq!(data.ancillary[0].bytes, b"fictional inert bytes");

        let mut duplicated = refs.clone();
        duplicated.push((files[19].0.as_str(), files[19].1.as_slice()));
        assert_eq!(
            decode_dataset(&duplicated).err(),
            Some(CarrierError::FileSet)
        );

        files[0].1.clear();
        let refs: Vec<_> = files
            .iter()
            .map(|(id, bytes)| (id.as_str(), bytes.as_slice()))
            .collect();
        assert_eq!(decode_dataset(&refs).err(), Some(CarrierError::RecordCount));
    }
}
