//! Independent readers for selected phase 6 normalized operands.

use serde::Deserialize;
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};

use crate::json;

/// A catalog-selected normalized profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Profile {
    ElementSymbols,
    TypeElements,
    NamingRanks,
}

/// The record shape selected by a profile and its exact relative path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileKind {
    ElementSymbols,
    TypeElements,
    ElementOrderA,
    ElementOrderB,
}

/// Payload-free failure while checking framing, a record, or a catalog identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecodeError {
    Framing,
    Record,
    CatalogMismatch,
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Framing => "invalid_data_framing",
            Self::Record => "invalid_data_record",
            Self::CatalogMismatch => "catalog_identity_mismatch",
        })
    }
}

impl std::error::Error for DecodeError {}

/// The trusted catalog's exact identities, not identities asserted by the DATA itself.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct CatalogIdentity<'a> {
    pub dataset_id: &'a str,
    pub dataset_version: &'a str,
    pub registry_sha256: &'a str,
    pub profile_sha256: &'a str,
    pub manifest_file_sha256: &'a str,
}

impl CatalogIdentity<'_> {
    /// Compare the actual metadata bytes against this independently trusted identity.
    ///
    /// # Errors
    /// Returns `CatalogMismatch` for a length or digest mismatch.
    pub fn check_file_bytes(
        &self,
        manifest: &[u8],
        registry: &[u8],
        profile: &[u8],
    ) -> Result<(), DecodeError> {
        let matches = |bytes: &[u8], expected: &str| {
            bytes.len() as u64 <= crate::MAX_MANIFEST_BYTES
                && format!("{:x}", Sha256::digest(bytes)) == expected
        };
        if matches(manifest, self.manifest_file_sha256)
            && matches(registry, self.registry_sha256)
            && matches(profile, self.profile_sha256)
        {
            Ok(())
        } else {
            Err(DecodeError::CatalogMismatch)
        }
    }
}

/// An atomic-number index and its original, undecoded symbol bytes.
#[derive(PartialEq)]
pub struct SymbolOperand {
    pub atomic_number: u32,
    pub symbol_bytes: Vec<u8>,
}
/// A source type identity and its atomic-number operand.
#[derive(PartialEq)]
pub struct TypeOperand {
    pub internal_type: u32,
    pub atomic_number: u32,
}
/// A source rank operand; its meaning depends on the containing context.
#[derive(PartialEq)]
pub struct RankOperand {
    pub atomic_number: u32,
    pub source_rank: u32,
}

/// One decoded file; this alone does not establish a complete registration.
pub enum Records {
    Symbols(Vec<SymbolOperand>),
    Types(Vec<TypeOperand>),
    RanksA(Vec<RankOperand>),
    RanksB(Vec<RankOperand>),
}

/// A complete profile; both naming orders must be validated together.
#[derive(PartialEq)]
pub enum DatasetRecords {
    ElementSymbols(Vec<SymbolOperand>),
    TypeElements(Vec<TypeOperand>),
    NamingRanks {
        order_a: Vec<RankOperand>,
        order_b: Vec<RankOperand>,
    },
}

/// Decodes a complete set of files after the caller has authorized and hashed each file.
///
/// # Errors
/// Rejects missing, duplicate or malformed files and an incomplete context-A index set.
pub fn decode_dataset(
    profile: Profile,
    files: &[(&str, &[u8])],
) -> Result<DatasetRecords, DecodeError> {
    let file = |path| {
        let mut matches = files.iter().filter(|(name, _)| *name == path);
        let bytes = matches.next().ok_or(DecodeError::Record)?.1;
        if matches.next().is_some() {
            return Err(DecodeError::Record);
        }
        Ok(bytes)
    };
    match profile {
        Profile::ElementSymbols if files.len() == 1 => {
            let Records::Symbols(rows) = decode(
                FileKind::ElementSymbols,
                file("normalized/element-symbols.jsonl")?,
            )?
            else {
                return Err(DecodeError::Record);
            };
            Ok(DatasetRecords::ElementSymbols(rows))
        }
        Profile::TypeElements if files.len() == 1 => {
            let Records::Types(rows) = decode(
                FileKind::TypeElements,
                file("normalized/type-elements.jsonl")?,
            )?
            else {
                return Err(DecodeError::Record);
            };
            Ok(DatasetRecords::TypeElements(rows))
        }
        Profile::NamingRanks if files.len() == 2 => {
            let Records::RanksA(order_a) = decode(
                FileKind::ElementOrderA,
                file("normalized/element-order-a.jsonl")?,
            )?
            else {
                return Err(DecodeError::Record);
            };
            let Records::RanksB(order_b) = decode(
                FileKind::ElementOrderB,
                file("normalized/element-order-b.jsonl")?,
            )?
            else {
                return Err(DecodeError::Record);
            };
            // The phase 7 direct-reader contract fixes context A's initializer to Z=2–109.
            let mut seen = [false; 108];
            for row in &order_a {
                let Some(index) = row
                    .atomic_number
                    .checked_sub(2)
                    .filter(|index| *index < 108)
                else {
                    return Err(DecodeError::Record);
                };
                let slot = &mut seen[index as usize];
                if std::mem::replace(slot, true) {
                    return Err(DecodeError::Record);
                }
            }
            if seen.contains(&false) {
                return Err(DecodeError::Record);
            }
            Ok(DatasetRecords::NamingRanks { order_a, order_b })
        }
        _ => Err(DecodeError::Record),
    }
}

/// Exact finite binary64 operand; the raw bits retain signed zero.
#[derive(PartialEq, Eq)]
pub struct F64Operand {
    pub bits: u64,
}

/// The exact operand and its declared unit; no conversion is implied by this type.
pub struct F64UnitOperand {
    pub value: F64Operand,
    pub unit: String,
}

/// A source decimal token and its independently stored binary64 reference.
pub struct DecimalReference {
    pub source_decimal: String,
    pub reference: F64Operand,
}

/// Keeps the original decimal spelling and verifies its finite binary64 reference.
///
/// # Errors
/// Rejects malformed or inconsistent decimal and binary64 witnesses.
pub fn decode_decimal_reference(bytes: &[u8]) -> Result<DecimalReference, DecodeError> {
    let parsed = parse_document(bytes)?;
    let wire: DecimalReferenceRow =
        serde_json::from_value(parsed).map_err(|_| DecodeError::Record)?;
    if !decimal_spelling(&wire.source_decimal) {
        return Err(DecodeError::Record);
    }
    let decoded = wire
        .source_decimal
        .parse::<f64>()
        .map_err(|_| DecodeError::Record)?;
    let reference = decode_f64_pair(&wire.reference_f64, &wire.reference_f64_bits_le)?;
    if !decoded.is_finite() || decoded != reference.value() {
        return Err(DecodeError::Record);
    }
    Ok(DecimalReference {
        source_decimal: wire.source_decimal,
        reference,
    })
}

fn decimal_spelling(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut index = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let mut digits = 0;
    while bytes.get(index).is_some_and(u8::is_ascii_digit) {
        index += 1;
        digits += 1;
    }
    if bytes.get(index) == Some(&b'.') {
        index += 1;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return false;
    }
    if matches!(bytes.get(index), Some(b'e' | b'E')) {
        index += 1;
        if matches!(bytes.get(index), Some(b'+' | b'-')) {
            index += 1;
        }
        let start = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        if index == start {
            return false;
        }
    }
    index == bytes.len()
}

/// Validates a numeric witness while retaining the unit exactly as supplied.
///
/// # Errors
/// Rejects malformed or inconsistent numeric and binary64 witnesses.
pub fn decode_f64_with_unit(bytes: &[u8]) -> Result<F64UnitOperand, DecodeError> {
    let parsed = parse_document(bytes)?;
    let wire: F64UnitRow = serde_json::from_value(parsed).map_err(|_| DecodeError::Record)?;
    Ok(F64UnitOperand {
        value: decode_f64_pair(&wire.value, &wire.bits_le)?,
        unit: wire.unit,
    })
}

/// Decodes a byte/text witness and verifies the stated Latin-1 mapping.
///
/// # Errors
/// Rejects malformed hex or a text value that differs from the byte-for-codepoint mapping.
pub fn decode_latin1_pair(hex: &str, text: &str) -> Result<Vec<u8>, DecodeError> {
    let bytes = decode_hex(hex)?;
    if !bytes.iter().copied().map(char::from).eq(text.chars()) {
        return Err(DecodeError::Record);
    }
    Ok(bytes)
}

/// Preserves an explicit null pair and otherwise verifies an exact ASCII byte/text witness.
/// The containing reader must separately establish that both fields were present.
///
/// # Errors
/// Rejects a partial pair, malformed hex or nonmatching ASCII text.
pub fn decode_ascii_pair(
    hex: Option<&str>,
    text: Option<&str>,
) -> Result<Option<Vec<u8>>, DecodeError> {
    match (hex, text) {
        (None, None) => Ok(None),
        (Some(hex), Some(text)) => {
            let bytes = decode_hex(hex)?;
            if !bytes.is_ascii() || bytes != text.as_bytes() {
                return Err(DecodeError::Record);
            }
            Ok(Some(bytes))
        }
        _ => Err(DecodeError::Record),
    }
}

impl F64Operand {
    pub fn value(&self) -> f64 {
        f64::from_bits(self.bits)
    }
}

/// Decodes one exact unsigned integer without a binary64 intermediary.
///
/// # Errors
/// Rejects values that are not exact integers, negative values, overflow and malformed JSON.
pub fn decode_u64(bytes: &[u8]) -> Result<u64, DecodeError> {
    // Validate framing first, then use the original token instead of its rounded f64 form.
    parse_document(bytes)?;
    let raw: Box<serde_json::value::RawValue> =
        serde_json::from_slice(bytes).map_err(|_| DecodeError::Record)?;
    json::parse_integer_raw(&raw).ok_or(DecodeError::Record)
}

/// Validates a JSON number against its little-endian binary64 witness.
///
/// # Errors
/// Rejects malformed, nonfinite or inconsistent numeric and bit witnesses.
pub fn decode_f64_literal(bytes: &[u8]) -> Result<F64Operand, DecodeError> {
    let parsed = parse_document(bytes)?;
    let wire: F64LiteralRow = serde_json::from_value(parsed).map_err(|_| DecodeError::Record)?;
    decode_f64_pair(&wire.value, &wire.bits_le)
}

pub(crate) fn decode_f64_pair(
    value: &serde_json::Number,
    bits_le: &str,
) -> Result<F64Operand, DecodeError> {
    let number = value
        .as_f64()
        .filter(|number| number.is_finite())
        .ok_or(DecodeError::Record)?;
    if bits_le.len() != 16 {
        return Err(DecodeError::Record);
    }
    let bytes: [u8; 8] = decode_hex(bits_le)?
        .try_into()
        .map_err(|_| DecodeError::Record)?;
    let bits = u64::from_le_bytes(bytes);
    let witness = f64::from_bits(bits);
    // Signed zeros compare numerically equal; the separately retained witness supplies its sign.
    if !witness.is_finite() || number != witness {
        return Err(DecodeError::Record);
    }
    Ok(F64Operand { bits })
}

fn parse_document(bytes: &[u8]) -> Result<serde_json::Value, DecodeError> {
    if bytes.len() as u64 > crate::MAX_DATA_FILE_BYTES {
        return Err(DecodeError::Framing);
    }
    json::parse_integer_zero_strict(bytes).map_err(|_| DecodeError::Record)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct F64LiteralRow {
    value: serde_json::Number,
    bits_le: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct F64UnitRow {
    value: serde_json::Number,
    bits_le: String,
    unit: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DecimalReferenceRow {
    source_decimal: String,
    reference_f64: serde_json::Number,
    reference_f64_bits_le: String,
}

impl Profile {
    pub const fn required_file_count(self) -> usize {
        match self {
            Self::NamingRanks => 2,
            Self::ElementSymbols | Self::TypeElements => 1,
        }
    }
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "private-caw-element-symbols-v1" => Some(Self::ElementSymbols),
            "private-caw-type-elements-v1" => Some(Self::TypeElements),
            "private-naming-element-ranks-v1" => Some(Self::NamingRanks),
            _ => None,
        }
    }

    pub fn file_kind(self, path: &str) -> Option<FileKind> {
        match (self, path) {
            (Self::ElementSymbols, "normalized/element-symbols.jsonl") => {
                Some(FileKind::ElementSymbols)
            }
            (Self::TypeElements, "normalized/type-elements.jsonl") => Some(FileKind::TypeElements),
            (Self::NamingRanks, "normalized/element-order-a.jsonl") => {
                Some(FileKind::ElementOrderA)
            }
            (Self::NamingRanks, "normalized/element-order-b.jsonl") => {
                Some(FileKind::ElementOrderB)
            }
            _ => None,
        }
    }

    fn catalog_identity(self) -> CatalogIdentity<'static> {
        match self {
            Self::ElementSymbols => CatalogIdentity {
                dataset_id: "legacy-caw-element-symbol-operands",
                dataset_version: "sha256-set-3a9716aacf119d6a927e1886ad7790fdf6b7b2d890856e2fe59830a35422f4a7",
                registry_sha256: "c1c012b6ce48eb7a3f60e23237a354a29cea1eb62dbc3c1a6123daba378917b9",
                profile_sha256: "d389e3b31461b708e62d897e8ff15ed5162cca4f3220d9f32ec946139bd899cd",
                manifest_file_sha256: "0e00bda8d9806053445e8f7b90e039bd6e6c818c988bc95849693a87e6411073",
            },
            Self::TypeElements => CatalogIdentity {
                dataset_id: "legacy-caw-internal-type-elements",
                dataset_version: "sha256-set-1b952eb5ed2812217f8deebaad52bbd69654ca9aa57e7b48a78e5d6e20092e91",
                registry_sha256: "3a6f93a097d6a8e9e43a4e7db083ef2007603318cc4717b81cf41455aabd245e",
                profile_sha256: "d66189d21c546cb578d3ea55886d4beb100dc9f0e4998f3af63fa2e41f6404da",
                manifest_file_sha256: "d5dde01383fc0b5c342e46085c8163c85c4035ba0614191513bc2ec741b3abfb",
            },
            Self::NamingRanks => CatalogIdentity {
                dataset_id: "legacy-naming-element-priority-operands",
                dataset_version: "sha256-set-11cf9820814312415b52fedb6f9c9d3f8a5a8a7a515d32cd2ac7a2d285e018d8",
                registry_sha256: "237b961511043df4eac475d243b6cbe650d2f6ed08ae9feeb1afb75c0bbc544f",
                profile_sha256: "077826ec2c6971956504b61c8684c97323f62c06119f9ee9ea664ab8bf0def13",
                manifest_file_sha256: "f460a2ce2f4c68ce94a2c94d687c974d5dfe40c65c526127d5b9a5b3f94e4e03",
            },
        }
    }

    /// Compare every identity in the public phase 6 catalog, before any payload is admitted.
    ///
    /// # Errors
    /// Returns `CatalogMismatch` if any identity differs.
    pub fn check_identity(self, identity: &CatalogIdentity<'_>) -> Result<(), DecodeError> {
        let expected = self.catalog_identity();
        if *identity == expected {
            Ok(())
        } else {
            Err(DecodeError::CatalogMismatch)
        }
    }

    /// Verify exact manifest, source registry and profile bytes against the public catalog.
    ///
    /// # Errors
    /// Returns `CatalogMismatch` if an identity, file size or digest differs.
    pub fn verify_files(
        self,
        dataset_id: &str,
        dataset_version: &str,
        manifest: &[u8],
        registry: &[u8],
        profile: &[u8],
    ) -> Result<(), DecodeError> {
        let expected = self.catalog_identity();
        if dataset_id != expected.dataset_id || dataset_version != expected.dataset_version {
            return Err(DecodeError::CatalogMismatch);
        }
        expected.check_file_bytes(manifest, registry, profile)
    }
}

/// Parses record framing and schema; authorization and cross-file validation are separate.
///
/// # Errors
/// Rejects invalid framing, unsupported record contents and malformed fields.
pub fn decode(kind: FileKind, bytes: &[u8]) -> Result<Records, DecodeError> {
    match kind {
        FileKind::ElementSymbols => {
            let rows: Vec<SymbolRow> = read_rows(bytes)?;
            let mut result = Vec::with_capacity(rows.len());
            for row in rows {
                result.push(SymbolOperand {
                    atomic_number: row.atomic_number_u32,
                    symbol_bytes: decode_hex(&row.symbol_hex)?,
                });
            }
            Ok(Records::Symbols(result))
        }
        FileKind::TypeElements => {
            let rows: Vec<TypeRow> = read_rows(bytes)?;
            Ok(Records::Types(
                rows.into_iter()
                    .map(|row| TypeOperand {
                        internal_type: row.internal_type_u32,
                        atomic_number: row.atomic_number_u32,
                    })
                    .collect(),
            ))
        }
        FileKind::ElementOrderA | FileKind::ElementOrderB => {
            let rows: Vec<RankRow> = read_rows(bytes)?;
            let result = rows
                .into_iter()
                .map(|row| RankOperand {
                    atomic_number: row.atomic_number_u32,
                    source_rank: row.source_rank_u32,
                })
                .collect();
            if kind == FileKind::ElementOrderA {
                Ok(Records::RanksA(result))
            } else {
                Ok(Records::RanksB(result))
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SymbolRow {
    #[serde(deserialize_with = "json::deserialize_integer")]
    atomic_number_u32: u32,
    symbol_hex: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TypeRow {
    #[serde(deserialize_with = "json::deserialize_integer")]
    internal_type_u32: u32,
    #[serde(deserialize_with = "json::deserialize_integer")]
    atomic_number_u32: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RankRow {
    #[serde(deserialize_with = "json::deserialize_integer")]
    atomic_number_u32: u32,
    #[serde(deserialize_with = "json::deserialize_integer")]
    source_rank_u32: u32,
}

fn read_rows<T: DeserializeOwned>(bytes: &[u8]) -> Result<Vec<T>, DecodeError> {
    if bytes.len() as u64 > crate::MAX_DATA_FILE_BYTES {
        return Err(DecodeError::Framing);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| DecodeError::Framing)?;
    if text.starts_with('\u{feff}') || !text.ends_with('\n') {
        return Err(DecodeError::Framing);
    }
    let body = &text[..text.len() - 1];
    if body.is_empty() {
        return Err(DecodeError::Framing);
    }
    body.split('\n')
        .map(|line| {
            if line.trim().is_empty() {
                return Err(DecodeError::Framing);
            }
            json::parse_integer_zero_strict(line.as_bytes()).map_err(|_| DecodeError::Record)?;
            serde_json::from_str(line).map_err(|_| DecodeError::Record)
        })
        .collect()
}

fn decode_hex(value: &str) -> Result<Vec<u8>, DecodeError> {
    let bytes = value.as_bytes();
    if !bytes.len().is_multiple_of(2)
        || !bytes
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
    {
        return Err(DecodeError::Record);
    }
    let digit = |byte: u8| {
        if byte.is_ascii_digit() {
            byte - b'0'
        } else {
            byte - b'a' + 10
        }
    };
    Ok(bytes
        .chunks_exact(2)
        .map(|pair| digit(pair[0]) * 16 + digit(pair[1]))
        .collect())
}
