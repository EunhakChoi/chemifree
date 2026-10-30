//! Inert naming lexicon and text-map inputs, without a naming grammar or engine.

use std::collections::HashSet;

use serde::{Deserialize, de::DeserializeOwned};

use crate::{json, phase6_profiles};

pub(crate) const LEXICON_PATH: &str = "normalized/lexicon-entries.jsonl";
pub(crate) const PRIMARY_PATH: &str = "normalized/text-map-primary.jsonl";
pub(crate) const SECONDARY_PATH: &str = "normalized/text-map-secondary.jsonl";
pub(crate) const REQUIRED_FILES: [&str; 3] = [LEXICON_PATH, PRIMARY_PATH, SECONDARY_PATH];
pub(crate) const RAW_PATH: &str = "raw/NameToStructure.dat";

/// Payload-free failure during catalog verification or complete-set decoding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LexiconError {
    FileSet,
    Framing,
    Record,
    Inconsistent,
    CatalogMismatch,
}

impl std::fmt::Display for LexiconError {
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

impl std::error::Error for LexiconError {}

/// Verify the exact metadata bytes and identity against the published catalog.
///
/// # Errors
/// Returns `CatalogMismatch` for a differing identity, length limit, or digest.
pub fn verify_catalog_files(
    dataset_id: &str,
    dataset_version: &str,
    manifest: &[u8],
    registry: &[u8],
    profile: &[u8],
) -> Result<(), LexiconError> {
    let expected = phase6_profiles::CatalogIdentity {
        dataset_id: "legacy-naming-lexicon-and-templates",
        dataset_version: "sha256-set-384790510be0a0b8c53fdf2b4839bde2e18bf5cca7cb375d88c0b2a3015547e7",
        registry_sha256: "02c178af3a48d15004153da3c59485cf4955cd229b911f012d46848c1e86971f",
        profile_sha256: "d7f51a39a670ca50fb904ad1e5a40e9095aef2e763c40993bb3df283b19d3885",
        manifest_file_sha256: "21310a42bfb711f83ddf905db06a64a05ba0800588cf48e9ccbe20a53e7ff06c",
    };
    if dataset_id != expected.dataset_id || dataset_version != expected.dataset_version {
        return Err(LexiconError::CatalogMismatch);
    }
    expected
        .check_file_bytes(manifest, registry, profile)
        .map_err(|_| LexiconError::CatalogMismatch)
}

/// A verified byte-for-code-point Latin-1 pair, without normalization or folding.
#[derive(PartialEq)]
pub struct Latin1Text {
    pub text: String,
    pub bytes: Vec<u8>,
}

/// A source span retained as inert lineage, never an I/O or dispatch instruction.
#[derive(PartialEq)]
pub struct SourceSpan {
    pub ordinal: u64,
    pub file_offset: u64,
    pub bytes: u64,
}

#[derive(PartialEq)]
pub struct Attribute {
    pub name: Latin1Text,
    pub value: u8,
    pub role: String,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AtomReference {
    #[serde(rename = "atom_index_0based")]
    pub atom_index: u64,
    #[serde(rename = "value_u8")]
    pub value: u8,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SignedAnnotation {
    #[serde(rename = "atom_index_0based")]
    pub atom_index: u64,
    pub signed_value: i32,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CodedAnnotation {
    #[serde(rename = "atom_index_0based")]
    pub atom_index: u64,
    #[serde(rename = "source_code_u8")]
    pub source_code: u8,
}

/// The fifth column is a valence override, not a hydrogen count.
#[derive(PartialEq)]
pub struct ValenceOverride {
    pub atom_index: u64,
    pub signed_value: i32,
}

impl ValenceOverride {
    /// Interpret the declared low byte; zero means no explicit override.
    #[must_use]
    pub fn explicit_valence(&self) -> Option<u8> {
        self.signed_value.to_le_bytes()[0].checked_sub(1)
    }
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Bond {
    #[serde(rename = "begin_atom_0based")]
    pub begin_atom: u64,
    #[serde(rename = "end_atom_0based")]
    pub end_atom: u64,
    #[serde(rename = "source_bond_code_u8")]
    pub source_code: u8,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BondAnnotation {
    #[serde(rename = "bond_index_0based")]
    pub bond_index: u64,
    #[serde(rename = "source_code_u8")]
    pub source_code: u8,
}

/// Distinct ordered annotation columns and their referenced graph arrays.
#[derive(PartialEq)]
pub struct Graph {
    pub atomic_number_codes: Vec<u8>,
    pub charge: Vec<SignedAnnotation>,
    pub nominal_isotope: Vec<SignedAnnotation>,
    pub stereo: Vec<CodedAnnotation>,
    pub radical: Vec<CodedAnnotation>,
    pub valence_override: Vec<ValenceOverride>,
    pub bonds: Vec<Bond>,
    pub bond_annotations: Vec<BondAnnotation>,
}

#[derive(PartialEq)]
pub struct LexiconEntry {
    pub source: SourceSpan,
    pub token: Latin1Text,
    pub kind: u8,
    pub subtype: u8,
    pub signed_scalar: i32,
    pub attributes: Vec<Attribute>,
    pub atom_references: [Vec<AtomReference>; 2],
    pub graph: Graph,
}

/// A substitution record; preserving it does not define a substitution algorithm.
#[derive(PartialEq)]
pub struct TextMapEntry {
    pub source: SourceSpan,
    pub partition: u64,
    pub divider_marker: bool,
    pub original: Latin1Text,
    pub replacement: Latin1Text,
}

/// Three contexts are checked together and keep their original row order.
#[derive(PartialEq)]
pub struct NamingLexicon {
    pub entries: Vec<LexiconEntry>,
    pub primary: Vec<TextMapEntry>,
    pub secondary: Vec<TextMapEntry>,
}

/// Decode an authorized, hashed complete set; a listed raw source is also bounded.
///
/// The raw file is not parsed and does not establish graph or naming semantics.
///
/// # Errors
/// Rejects an incomplete set, malformed records, inconsistent references or bounds.
pub fn decode_dataset(files: &[(&str, &[u8])]) -> Result<NamingLexicon, LexiconError> {
    if files.len() != REQUIRED_FILES.len() && files.len() != REQUIRED_FILES.len() + 1 {
        return Err(LexiconError::FileSet);
    }
    let file = |path| {
        let mut matching = files.iter().filter(|(name, _)| *name == path);
        let bytes = matching.next().ok_or(LexiconError::FileSet)?.1;
        if matching.next().is_some() {
            return Err(LexiconError::FileSet);
        }
        Ok(bytes)
    };
    let raw = if files.len() == REQUIRED_FILES.len() + 1 {
        let bytes = file(RAW_PATH)?;
        if bytes.len() as u64 > crate::MAX_DATA_FILE_BYTES {
            return Err(LexiconError::Framing);
        }
        Some(bytes)
    } else {
        None
    };
    let entries = decode_rows::<EntryWire>(file(LEXICON_PATH)?)?
        .into_iter()
        .map(|entry| entry.convert(raw))
        .collect::<Result<Vec<_>, _>>()?;
    let entries = unique_ordinals(entries, |entry| entry.source.ordinal)?;
    let maps = |path| -> Result<Vec<TextMapEntry>, LexiconError> {
        let rows = decode_rows::<TextMapWire>(file(path)?)?
            .into_iter()
            .map(TextMapWire::convert)
            .collect::<Result<Vec<_>, _>>()?;
        unique_ordinals(rows, |row| row.source.ordinal)
    };
    Ok(NamingLexicon {
        entries,
        primary: maps(PRIMARY_PATH)?,
        secondary: maps(SECONDARY_PATH)?,
    })
}

fn unique_ordinals<T>(rows: Vec<T>, ordinal: impl Fn(&T) -> u64) -> Result<Vec<T>, LexiconError> {
    let mut seen = HashSet::with_capacity(rows.len());
    if rows.iter().any(|row| !seen.insert(ordinal(row))) {
        return Err(LexiconError::Inconsistent);
    }
    Ok(rows)
}

fn decode_rows<T: DeserializeOwned>(bytes: &[u8]) -> Result<Vec<T>, LexiconError> {
    if bytes.len() as u64 > crate::MAX_DATA_FILE_BYTES {
        return Err(LexiconError::Framing);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| LexiconError::Framing)?;
    if text.starts_with('\u{feff}') || !text.ends_with('\n') {
        return Err(LexiconError::Framing);
    }
    let body = &text[..text.len() - 1];
    if body.is_empty() {
        return Err(LexiconError::Framing);
    }
    body.split('\n')
        .map(|line| {
            if line.trim().is_empty() {
                return Err(LexiconError::Framing);
            }
            let value = json::parse_integer_zero_strict(line.as_bytes())
                .map_err(|_| LexiconError::Record)?;
            serde_json::from_value(value).map_err(|_| LexiconError::Record)
        })
        .collect()
}

fn latin1(hex: String, text: String) -> Result<Latin1Text, LexiconError> {
    let bytes =
        phase6_profiles::decode_latin1_pair(&hex, &text).map_err(|_| LexiconError::Record)?;
    Ok(Latin1Text { text, bytes })
}

fn source(
    ordinal: u64,
    file_offset: u64,
    bytes: u64,
    raw: Option<&[u8]>,
) -> Result<SourceSpan, LexiconError> {
    let end = file_offset
        .checked_add(bytes)
        .ok_or(LexiconError::Inconsistent)?;
    if raw.is_some_and(|raw| end > raw.len() as u64) {
        return Err(LexiconError::Inconsistent);
    }
    Ok(SourceSpan {
        ordinal,
        file_offset,
        bytes,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AttributeWire {
    name_hex: String,
    name_latin1: String,
    value_u8: u8,
    role: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GraphWire {
    atomic_number_codes_u8: Vec<u8>,
    atom_annotation_columns: AnnotationColumns,
    bonds: Vec<Bond>,
    bond_annotations: Vec<BondAnnotation>,
}

#[derive(Deserialize)]
struct AnnotationColumns(
    Vec<SignedAnnotation>,
    Vec<SignedAnnotation>,
    Vec<CodedAnnotation>,
    Vec<CodedAnnotation>,
    Vec<SignedAnnotation>,
);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EntryWire {
    entry_ordinal_0based: u64,
    source_file_offset: u64,
    source_bytes: u64,
    token_hex: String,
    token_latin1: String,
    kind_code_u8: u8,
    subtype_code_u8: u8,
    signed_scalar: i32,
    attributes: Vec<AttributeWire>,
    atom_reference_lists: [Vec<AtomReference>; 2],
    graph: GraphWire,
}

impl EntryWire {
    fn convert(self, raw: Option<&[u8]>) -> Result<LexiconEntry, LexiconError> {
        let GraphWire {
            atomic_number_codes_u8,
            atom_annotation_columns:
                AnnotationColumns(charge, nominal_isotope, stereo, radical, valence),
            bonds,
            bond_annotations,
        } = self.graph;
        let atom_count = atomic_number_codes_u8.len();
        let atom_exists = |index: u64| {
            usize::try_from(index)
                .ok()
                .is_some_and(|index| index < atom_count)
        };
        let bond_exists = |index: u64| {
            usize::try_from(index)
                .ok()
                .is_some_and(|index| index < bonds.len())
        };
        if self
            .atom_reference_lists
            .iter()
            .flatten()
            .any(|r| !atom_exists(r.atom_index))
            || charge
                .iter()
                .chain(&nominal_isotope)
                .chain(&valence)
                .any(|a| !atom_exists(a.atom_index))
            || stereo
                .iter()
                .chain(&radical)
                .any(|a| !atom_exists(a.atom_index))
            || bonds
                .iter()
                .any(|b| !atom_exists(b.begin_atom) || !atom_exists(b.end_atom))
            || bond_annotations.iter().any(|a| !bond_exists(a.bond_index))
        {
            return Err(LexiconError::Inconsistent);
        }
        let attributes = self
            .attributes
            .into_iter()
            .map(|item| {
                Ok(Attribute {
                    name: latin1(item.name_hex, item.name_latin1)?,
                    value: item.value_u8,
                    role: item.role,
                })
            })
            .collect::<Result<_, LexiconError>>()?;
        Ok(LexiconEntry {
            source: source(
                self.entry_ordinal_0based,
                self.source_file_offset,
                self.source_bytes,
                raw,
            )?,
            token: latin1(self.token_hex, self.token_latin1)?,
            kind: self.kind_code_u8,
            subtype: self.subtype_code_u8,
            signed_scalar: self.signed_scalar,
            attributes,
            atom_references: self.atom_reference_lists,
            graph: Graph {
                atomic_number_codes: atomic_number_codes_u8,
                charge,
                nominal_isotope,
                stereo,
                radical,
                valence_override: valence
                    .into_iter()
                    .map(|a| ValenceOverride {
                        atom_index: a.atom_index,
                        signed_value: a.signed_value,
                    })
                    .collect(),
                bonds,
                bond_annotations,
            },
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TextMapWire {
    pair_ordinal_0based: u64,
    source_file_offset: u64,
    source_bytes: u64,
    partition: u64,
    divider_marker: bool,
    source_hex: String,
    replacement_hex: String,
    source_latin1: String,
    replacement_latin1: String,
}

impl TextMapWire {
    fn convert(self) -> Result<TextMapEntry, LexiconError> {
        Ok(TextMapEntry {
            source: source(
                self.pair_ordinal_0based,
                self.source_file_offset,
                self.source_bytes,
                None,
            )?,
            partition: self.partition,
            divider_marker: self.divider_marker,
            original: latin1(self.source_hex, self.source_latin1)?,
            replacement: latin1(self.replacement_hex, self.replacement_latin1)?,
        })
    }
}
