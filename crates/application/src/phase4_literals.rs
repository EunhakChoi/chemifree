//! Catalog-selected phase 4 literals and inert byte-preserving configuration.
//!
//! These inputs supply neither an executable configuration nor a prediction engine.

use num_bigint::BigInt;
use serde::Deserialize;
use serde_json::{Number, Value};

use crate::{json, phase6_profiles};

pub(crate) const METADATA_FILES: [&str; 2] = ["P4-MOLNET-METADATA-001", "P4-MOLNET-METADATA-002"];
pub(crate) const NUMERIC_FILES: [(&str, usize); 6] = [
    ("DATA-LITERALS-MOLNET-LOGS-normalized", 13),
    ("DATA-LITERALS-MOLNET-PKA-PHENOL-normalized", 6),
    ("DATA-LITERALS-MOLNET-PKA-BENZOIC-ACID-normalized", 6),
    ("DATA-LITERALS-MOLNET-PKA-ALIPHATIC-ACID-normalized", 6),
    ("DATA-LITERALS-MOLNET-PKA-ALIPHATIC-AMINE-normalized", 6),
    ("DATA-LITERALS-MOLNET-PKA-ALIPHATIC-ALCOHOL-normalized", 5),
];
const THERMODYNAMIC_FILE: &str = "DATA-LITERALS-CHEMPROP-THERMODYNAMIC-normalized";
pub(crate) const REAL48_FILES: [(&str, usize); 3] = [
    ("DATA-LITERALS-CHEMPROP-BROTO-normalized", 243),
    ("DATA-LITERALS-CHEMPROP-CRIPPEN-VISWANADHAN-normalized", 304),
    (THERMODYNAMIC_FILE, 87),
];

/// A supported catalog-bound family of input files.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Profile {
    Metadata,
    Numeric,
    ChempropTables,
}

/// Payload-free reason a complete input set could not be loaded.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputError {
    FileSet,
    Framing,
    Record,
    RecordCount,
    CatalogMismatch,
}

impl std::fmt::Display for InputError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::FileSet => "invalid_file_set",
            Self::Framing => "invalid_data_framing",
            Self::Record => "invalid_data_record",
            Self::RecordCount => "invalid_record_count",
            Self::CatalogMismatch => "catalog_identity_mismatch",
        })
    }
}

impl std::error::Error for InputError {}

impl Profile {
    pub(crate) const fn file_count(self) -> usize {
        match self {
            Self::Metadata => METADATA_FILES.len(),
            Self::Numeric => NUMERIC_FILES.len(),
            Self::ChempropTables => REAL48_FILES.len(),
        }
    }

    pub(crate) fn accepts_file(self, id: &str, role: &str, class: &str) -> bool {
        match self {
            Self::Metadata => {
                METADATA_FILES.contains(&id)
                    && role == "auxiliary_data"
                    && class == "declarative_data"
            }
            Self::Numeric => {
                NUMERIC_FILES.iter().any(|(expected, _)| *expected == id)
                    && role == "calibration"
                    && class == "model_parameters"
            }
            Self::ChempropTables => {
                REAL48_FILES.iter().any(|(expected, _)| *expected == id)
                    && role == "contribution_table"
                    && class == "model_parameters"
            }
        }
    }

    fn identity(self) -> phase6_profiles::CatalogIdentity<'static> {
        let (dataset_id, dataset_version, manifest_file_sha256) = match self {
            Self::Metadata => (
                "legacy-molnet-metadata",
                "sha256-set-28cd9dc5852838a9abdb8a385870e7dcfab468f1ab3338fa4f16387dad2bf437",
                "22cc59f3482fff49777d54ae8de8d56387e8175ea8de95716ae2976ec22c48b5",
            ),
            Self::Numeric => (
                "molnet-supporting-numeric-literals",
                "sha256-set-6472f01e148c5db2d03fc2f62d941b2a61c6beec9d5829fcc8943d4633b78ca1",
                "739fc959c48c419b89788e97b799f7e578f6ebb65dec6efeabf7dfa60d676dd9",
            ),
            Self::ChempropTables => (
                "chemprop-typed-parameter-tables",
                "sha256-set-8c55c6a8c52c13bbb945a9f60de076d9a8c3c21bd0c0ecedadc3bd19c9cfce2f",
                "869cb45c6ca8cd106592584317bbae8e805e17d53dcdf8f9fb87509791807b4d",
            ),
        };
        phase6_profiles::CatalogIdentity {
            dataset_id,
            dataset_version,
            registry_sha256: "8ffd2e9b4d01ce989c63cb552714e1ff01acb8f766dbbc14bc411f08c4484d25",
            profile_sha256: "5add6a3e7a6250dc5c2f280d9a193dc8aef3ba008e47d10e35e7b263c217c2a2",
            manifest_file_sha256,
        }
    }
}

/// Check the actual catalog, source registry and manifest bytes against public identities.
///
/// # Errors
/// Returns `CatalogMismatch` when an identity, length or digest differs.
pub fn verify_catalog_files(
    profile: Profile,
    dataset_id: &str,
    dataset_version: &str,
    manifest: &[u8],
    registry: &[u8],
    format_catalog: &[u8],
) -> Result<(), InputError> {
    let expected = profile.identity();
    if dataset_id != expected.dataset_id || dataset_version != expected.dataset_version {
        return Err(InputError::CatalogMismatch);
    }
    expected
        .check_file_bytes(manifest, registry, format_catalog)
        .map_err(|_| InputError::CatalogMismatch)
}

/// Uninterpreted configuration bytes. Paths and commands are never executed by the reader.
#[derive(PartialEq)]
pub struct MetadataFile {
    pub id: &'static str,
    pub bytes: Vec<u8>,
}

/// One declared numeric operand. Private source location is not exposed.
#[derive(PartialEq)]
pub struct NumericOperand {
    pub bits: u64,
    /// The parsed JSON value is independent of the original binary64 bit witness.
    pub value: Number,
    pub semantic_role: String,
}

/// Ordered source rows for one explicit catalog file ID.
#[derive(PartialEq)]
pub struct NumericFile {
    pub id: &'static str,
    pub rows: Vec<NumericOperand>,
}

/// A byte-for-code-point Latin-1 witness preserved alongside the source bytes.
#[derive(PartialEq)]
pub struct Latin1Key {
    pub text: String,
    pub bytes: Vec<u8>,
}

/// Raw six-byte storage and an exact rational view, retained as separate witnesses.
#[derive(PartialEq)]
pub struct RationalColumn {
    pub column_byte_offset: u64,
    pub raw: [u8; 6],
    pub numerator: BigInt,
    pub denominator: BigInt,
}

/// Distinct source table fields, without interpreting them as chemical features.
#[derive(PartialEq)]
pub enum Real48Context {
    Typed { aux_word: i64, flag_byte: i64 },
    Thermodynamic { secondary_key: String },
}

/// An ordered table row; storage labels are preserved without index conversion.
#[derive(PartialEq)]
pub struct Real48Row {
    pub storage_index: i64,
    pub key: Latin1Key,
    pub columns: Vec<RationalColumn>,
    pub context: Real48Context,
}

/// One explicitly selected table, in original record order.
#[derive(PartialEq)]
pub struct Real48File {
    pub id: &'static str,
    pub rows: Vec<Real48Row>,
}

/// A complete profile; none of the variants constitutes a model or predictor.
#[derive(PartialEq)]
pub enum Records {
    Metadata(Vec<MetadataFile>),
    Numeric(Vec<NumericFile>),
    ChempropTables(Vec<Real48File>),
}

/// Decode a complete file set after its authorization and hashes have been verified.
///
/// # Errors
/// Rejects an incomplete or duplicate file set, invalid shapes, values or record counts.
pub fn decode_dataset(profile: Profile, files: &[(&str, &[u8])]) -> Result<Records, InputError> {
    let file = |id| {
        let mut matching = files.iter().filter(|(name, _)| *name == id);
        let bytes = matching.next().ok_or(InputError::FileSet)?.1;
        if matching.next().is_some() {
            return Err(InputError::FileSet);
        }
        if bytes.len() as u64 > crate::MAX_DATA_FILE_BYTES {
            return Err(InputError::Framing);
        }
        Ok(bytes)
    };
    match profile {
        Profile::Metadata if files.len() == METADATA_FILES.len() => METADATA_FILES
            .iter()
            .map(|&id| {
                Ok(MetadataFile {
                    id,
                    bytes: file(id)?.to_vec(),
                })
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Records::Metadata),
        Profile::Numeric if files.len() == NUMERIC_FILES.len() => NUMERIC_FILES
            .iter()
            .map(|&(id, count)| {
                Ok(NumericFile {
                    id,
                    rows: decode_numeric(file(id)?, count)?,
                })
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Records::Numeric),
        Profile::ChempropTables if files.len() == REAL48_FILES.len() => REAL48_FILES
            .iter()
            .map(|&(id, count)| {
                let shape = if id == THERMODYNAMIC_FILE {
                    Real48Shape::Thermodynamic
                } else {
                    Real48Shape::Typed
                };
                Ok(Real48File {
                    id,
                    rows: decode_chemprop_rows(file(id)?, count, shape)?,
                })
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Records::ChempropTables),
        _ => Err(InputError::FileSet),
    }
}

fn decode_rows<T>(
    bytes: &[u8],
    count: usize,
    mut decode: impl FnMut(Value) -> Result<T, InputError>,
) -> Result<Vec<T>, InputError> {
    let text = std::str::from_utf8(bytes).map_err(|_| InputError::Framing)?;
    if text.starts_with('\u{feff}') {
        return Err(InputError::Framing);
    }
    let mut rows = Vec::with_capacity(count);
    for line in text.split('\n') {
        if line.is_empty() || line == "\r" {
            continue;
        }
        if rows.len() == count {
            return Err(InputError::RecordCount);
        }
        let value =
            json::parse_integer_zero_strict(line.as_bytes()).map_err(|_| InputError::Record)?;
        rows.push(decode(value)?);
    }
    if rows.len() != count {
        return Err(InputError::RecordCount);
    }
    Ok(rows)
}

fn decode_numeric(bytes: &[u8], count: usize) -> Result<Vec<NumericOperand>, InputError> {
    decode_rows(bytes, count, |value| {
        let wire: NumericWire = serde_json::from_value(value).map_err(|_| InputError::Record)?;
        let bits = decode_bits(&wire.ieee754_f64_le_hex)?;
        let reference = f64::from_bits(bits);
        let parsed = wire
            .value
            .as_f64()
            .filter(|value| value.is_finite())
            .ok_or(InputError::Record)?;
        if !reference.is_finite() || reference != parsed {
            return Err(InputError::Record);
        }
        Ok(NumericOperand {
            bits,
            value: wire.value,
            semantic_role: wire.semantic_role,
        })
    })
}

fn decode_bits(hex: &str) -> Result<u64, InputError> {
    Ok(u64::from_le_bytes(decode_fixed_hex(hex)?))
}

fn decode_fixed_hex<const N: usize>(hex: &str) -> Result<[u8; N], InputError> {
    if hex.len() != N * 2 {
        return Err(InputError::Record);
    }
    let digit = |byte: u8| match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    };
    let mut bytes = [0_u8; N];
    for (index, pair) in hex.as_bytes().chunks_exact(2).enumerate() {
        bytes[index] = digit(pair[0]).ok_or(InputError::Record)? * 16
            + digit(pair[1]).ok_or(InputError::Record)?;
    }
    Ok(bytes)
}

#[derive(Clone, Copy)]
enum Real48Shape {
    Typed,
    Thermodynamic,
}

fn decode_chemprop_rows(
    bytes: &[u8],
    count: usize,
    shape: Real48Shape,
) -> Result<Vec<Real48Row>, InputError> {
    decode_rows(bytes, count, |value| match shape {
        Real48Shape::Typed => {
            let row: TypedReal48Wire =
                serde_json::from_value(value).map_err(|_| InputError::Record)?;
            real48_row(
                row.storage_index,
                row.key_hex,
                row.key_latin1,
                row.real48_columns,
                Real48Context::Typed {
                    aux_word: row.aux_word,
                    flag_byte: row.flag_byte,
                },
            )
        }
        Real48Shape::Thermodynamic => {
            let row: ThermodynamicReal48Wire =
                serde_json::from_value(value).map_err(|_| InputError::Record)?;
            real48_row(
                row.storage_index,
                row.key_hex,
                row.key_latin1,
                row.real48_columns,
                Real48Context::Thermodynamic {
                    secondary_key: row.secondary_key_latin1,
                },
            )
        }
    })
}

fn real48_row(
    storage_index: i64,
    key_hex: String,
    key_latin1: String,
    columns: Vec<RationalWire>,
    context: Real48Context,
) -> Result<Real48Row, InputError> {
    // Phase 4 permits either hex case; the shared phase 6 reader accepts lowercase only.
    let bytes = phase6_profiles::decode_latin1_pair(&key_hex.to_ascii_lowercase(), &key_latin1)
        .map_err(|_| InputError::Record)?;
    let columns = columns
        .into_iter()
        .map(|column| {
            let numerator = parse_rational_integer(&column.exact_rational_numerator)?;
            let denominator = parse_rational_integer(&column.exact_rational_denominator)?;
            if denominator == BigInt::from(0_u8) {
                return Err(InputError::Record);
            }
            Ok(RationalColumn {
                column_byte_offset: column.column_byte_offset,
                raw: decode_fixed_hex(&column.raw_real48_le_hex)?,
                numerator,
                denominator,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Real48Row {
        storage_index,
        key: Latin1Key {
            text: key_latin1,
            bytes,
        },
        columns,
        context,
    })
}

fn parse_rational_integer(text: &str) -> Result<BigInt, InputError> {
    // A bound keeps malicious strings from forcing unlimited arbitrary-precision work.
    let digits = text.strip_prefix(['+', '-']).unwrap_or(text);
    if digits.is_empty() || digits.len() > 4096 || !digits.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(InputError::Record);
    }
    text.parse().map_err(|_| InputError::Record)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NumericWire {
    #[serde(rename = "preferred_data_va")]
    _preferred_data_va: String,
    #[serde(rename = "source_file_offset")]
    _source_file_offset: u64,
    #[serde(rename = "source_section")]
    _source_section: String,
    ieee754_f64_le_hex: String,
    value: Number,
    semantic_role: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RationalWire {
    column_byte_offset: u64,
    raw_real48_le_hex: String,
    exact_rational_numerator: String,
    exact_rational_denominator: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TypedReal48Wire {
    storage_index: i64,
    key_hex: String,
    key_latin1: String,
    real48_columns: Vec<RationalWire>,
    aux_word: i64,
    flag_byte: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ThermodynamicReal48Wire {
    storage_index: i64,
    key_hex: String,
    key_latin1: String,
    real48_columns: Vec<RationalWire>,
    secondary_key_latin1: String,
}
