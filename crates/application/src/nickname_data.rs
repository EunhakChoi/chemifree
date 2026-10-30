//! Declarative nickname and alphabet records, without graph composition or chemistry.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Deserializer, de::DeserializeOwned};
use sha2::{Digest, Sha256};

use crate::{json, phase6_profiles};

pub(crate) const GRAPH_PATH: &str = "normalized/nickname-graphs.jsonl";
pub(crate) const ALPHABET_PATH: &str = "normalized/sequence-alphabets.jsonl";
pub(crate) const REQUIRED_FILES: [&str; 2] = [GRAPH_PATH, ALPHABET_PATH];

/// Payload-free failure when reading a catalog or a normalized record set.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NicknameError {
    FileSet,
    Framing,
    Record,
    UnsupportedEntityDeclaration,
    Inconsistent,
    CatalogMismatch,
}

impl std::fmt::Display for NicknameError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::FileSet => "invalid_file_set",
            Self::Framing => "invalid_data_framing",
            Self::Record => "invalid_data_record",
            Self::UnsupportedEntityDeclaration => "unsupported_xml_entity_declaration",
            Self::Inconsistent => "inconsistent_dataset",
            Self::CatalogMismatch => "catalog_identity_mismatch",
        })
    }
}

impl std::error::Error for NicknameError {}

/// Check the exact metadata bytes and identity against the published catalog.
///
/// # Errors
/// Returns `CatalogMismatch` for a different identity, size limit or digest.
pub fn verify_catalog_files(
    dataset_id: &str,
    dataset_version: &str,
    manifest: &[u8],
    registry: &[u8],
    profile: &[u8],
) -> Result<(), NicknameError> {
    let expected = phase6_profiles::CatalogIdentity {
        dataset_id: "legacy-sequence-nickname-graphs",
        dataset_version: "sha256-set-71abd8af61c6ec68dfaa465245b140196ab3c10cd938fa640a9318381004598d",
        registry_sha256: "dd4a6c5458e02945db4d45193e41063b650b0079e771183673a8be666346abba",
        profile_sha256: "1e83a553adae143df4daf6567669ea8b6fe599f515f61f54447a680b4b74105e",
        manifest_file_sha256: "fc7feb7436bf9cbb66f1d6d54573c5d392f0a9b857ab21e27e8df94f83d95e73",
    };
    if dataset_id != expected.dataset_id || dataset_version != expected.dataset_version {
        return Err(NicknameError::CatalogMismatch);
    }
    expected
        .check_file_bytes(manifest, registry, profile)
        .map_err(|_| NicknameError::CatalogMismatch)
}

/// An inert XML tree with ordered children and explicitly nullable text and tail.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct XmlNode {
    pub tag: String,
    pub attributes: BTreeMap<String, String>,
    #[serde(deserialize_with = "required_option")]
    pub text: Option<String>,
    #[serde(deserialize_with = "required_option")]
    pub tail: Option<String>,
    pub children: Vec<XmlNode>,
}

/// A graph's local object-ID namespace and inert source identity.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct NicknameGraph {
    pub name: String,
    pub source_sha256: String,
    pub source_bytes: u64,
    pub nodes: u64,
    pub bonds: u64,
    pub xml_tree: XmlNode,
}

/// Ordered candidate names are preserved exactly; they are not biological inference.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AlphabetEntry {
    pub alphabet: String,
    pub source_ordinal_0based: u64,
    pub key: String,
    pub display_value: String,
    pub nickname_names: Vec<String>,
    pub source_hotkeys_sha256: String,
}

/// The complete normalized view; no sequence composition or naming is implied.
#[derive(PartialEq)]
pub struct NicknameData {
    pub graphs: Vec<NicknameGraph>,
    pub alphabets: Vec<AlphabetEntry>,
}

/// Decode normalized streams and any listed raw XML after authorization and hashing.
///
/// # Errors
/// Rejects incomplete files, invalid framing, graph IDs, endpoints or mismatched raw XML.
pub fn decode_dataset(files: &[(&str, &[u8])]) -> Result<NicknameData, NicknameError> {
    if files.len() < REQUIRED_FILES.len() {
        return Err(NicknameError::FileSet);
    }
    let file = |path| {
        let mut matching = files.iter().filter(|(name, _)| *name == path);
        let bytes = matching.next().ok_or(NicknameError::FileSet)?.1;
        if matching.next().is_some() {
            return Err(NicknameError::FileSet);
        }
        Ok(bytes)
    };
    let graphs: Vec<NicknameGraph> = decode_rows(file(GRAPH_PATH)?)?;
    let alphabets: Vec<AlphabetEntry> = decode_rows(file(ALPHABET_PATH)?)?;
    let mut names = HashSet::new();
    for graph in &graphs {
        if !is_hash(&graph.source_sha256) {
            return Err(NicknameError::Record);
        }
        if !names.insert(graph.name.as_str()) {
            return Err(NicknameError::Inconsistent);
        }
        validate_graph(graph)?;
    }
    let mut alphabet_keys = HashSet::new();
    let mut alphabet_ordinals = HashSet::new();
    for entry in &alphabets {
        if !is_hash(&entry.source_hotkeys_sha256) {
            return Err(NicknameError::Record);
        }
        if !alphabet_keys.insert((entry.alphabet.as_str(), entry.key.as_str()))
            || !alphabet_ordinals.insert((entry.alphabet.as_str(), entry.source_ordinal_0based))
            || entry
                .nickname_names
                .iter()
                .any(|name| !names.contains(name.as_str()))
        {
            return Err(NicknameError::Inconsistent);
        }
    }
    let raw: Vec<_> = files
        .iter()
        .filter(|(path, _)| !REQUIRED_FILES.contains(path))
        .collect();
    if !raw.is_empty() {
        let mut paths = HashSet::new();
        let mut used = vec![false; graphs.len()];
        for (path, bytes) in raw {
            let name = path
                .strip_prefix("raw/nicknames/")
                .ok_or(NicknameError::FileSet)?;
            if !name.ends_with(".cdxml") || name.contains('/') || !paths.insert(*path) {
                return Err(NicknameError::FileSet);
            }
            if bytes.len() as u64 > crate::MAX_DATA_FILE_BYTES {
                return Err(NicknameError::Framing);
            }
            let text = std::str::from_utf8(bytes).map_err(|_| NicknameError::Record)?;
            if has_entity_declaration(text) {
                return Err(NicknameError::UnsupportedEntityDeclaration);
            }
            let options = roxmltree::ParsingOptions {
                allow_dtd: true,
                nodes_limit: 1_000_000,
                entity_resolver: None,
            };
            let xml = roxmltree::Document::parse_with_options(text, options)
                .map_err(|_| NicknameError::Record)?;
            let digest = format!("{:x}", Sha256::digest(bytes));
            let Some(index) = graphs.iter().enumerate().position(|(index, graph)| {
                !used[index]
                    && graph.source_bytes == bytes.len() as u64
                    && graph.source_sha256 == digest
                    && graph.xml_tree.tail.is_none()
                    && same_xml_tree(&graph.xml_tree, xml.root_element())
            }) else {
                return Err(NicknameError::Inconsistent);
            };
            used[index] = true;
        }
    }
    Ok(NicknameData { graphs, alphabets })
}

// Raw CDXML may name an external DTD, which remains inert. User-defined entities are
// rejected before XML parsing so repeated references cannot expand beyond the file bound.
fn has_entity_declaration(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let rest = &bytes[index..];
        if rest.starts_with(b"<!--") {
            let Some(end) = rest[4..].windows(3).position(|part| part == b"-->") else {
                return false; // The XML parser will reject the unterminated comment.
            };
            index += 4 + end + 3;
        } else if rest.starts_with(b"<![CDATA[") {
            let Some(end) = rest[9..].windows(3).position(|part| part == b"]]>") else {
                return false;
            };
            index += 9 + end + 3;
        } else if rest.starts_with(b"<?") {
            let Some(end) = rest[2..].windows(2).position(|part| part == b"?>") else {
                return false;
            };
            index += 2 + end + 2;
        } else if rest.starts_with(b"<!DOCTYPE") {
            match scan_doctype(bytes, index + b"<!DOCTYPE".len()) {
                DoctypeScan::EntityDeclaration => return true,
                DoctypeScan::End(end) => index = end,
            }
        } else {
            index += 1;
        }
    }
    false
}

enum DoctypeScan {
    EntityDeclaration,
    End(usize),
}

fn scan_doctype(bytes: &[u8], mut index: usize) -> DoctypeScan {
    let mut quote = None;
    let mut subset_depth = 0_u32;
    while index < bytes.len() {
        let rest = &bytes[index..];
        if let Some(delimiter) = quote {
            if bytes[index] == delimiter {
                quote = None;
            }
            index += 1;
        } else if rest.starts_with(b"<!--") {
            let Some(end) = rest[4..].windows(3).position(|part| part == b"-->") else {
                return DoctypeScan::End(bytes.len());
            };
            index += 4 + end + 3;
        } else if rest.starts_with(b"<?") {
            let Some(end) = rest[2..].windows(2).position(|part| part == b"?>") else {
                return DoctypeScan::End(bytes.len());
            };
            index += 2 + end + 2;
        } else if rest.starts_with(b"<!ENTITY") {
            return DoctypeScan::EntityDeclaration;
        } else {
            match bytes[index] {
                b'\'' | b'"' => quote = Some(bytes[index]),
                b'[' => subset_depth = subset_depth.saturating_add(1),
                b']' => subset_depth = subset_depth.saturating_sub(1),
                b'>' if subset_depth == 0 => return DoctypeScan::End(index + 1),
                _ => {}
            }
            index += 1;
        }
    }
    DoctypeScan::End(index)
}

fn expanded_name(namespace: Option<&str>, local: &str) -> String {
    namespace.map_or_else(
        || local.to_owned(),
        |namespace| format!("{{{namespace}}}{local}"),
    )
}

fn nonempty(value: &str) -> Option<&str> {
    if value.is_empty() { None } else { Some(value) }
}

// ElementTree-style text/tail preserve mixed content while comments and PIs remain inert.
fn same_xml_tree(normalized: &XmlNode, xml: roxmltree::Node<'_, '_>) -> bool {
    if normalized.tag != expanded_name(xml.tag_name().namespace(), xml.tag_name().name())
        || normalized.attributes.len() != xml.attributes().len()
        || xml.attributes().any(|attribute| {
            normalized
                .attributes
                .get(&expanded_name(attribute.namespace(), attribute.name()))
                .is_none_or(|value| value != attribute.value())
        })
    {
        return false;
    }
    let mut children = Vec::new();
    let mut text_slots = vec![String::new()];
    for child in xml.children() {
        if child.is_element() {
            children.push(child);
            text_slots.push(String::new());
        } else if child.is_text() {
            text_slots
                .last_mut()
                .expect("the initial text slot exists")
                .push_str(child.text().unwrap_or(""));
        }
    }
    if normalized.text.as_deref() != nonempty(&text_slots[0])
        || normalized.children.len() != children.len()
    {
        return false;
    }
    normalized
        .children
        .iter()
        .zip(children)
        .enumerate()
        .all(|(index, (child, xml_child))| {
            child.tail.as_deref() == nonempty(&text_slots[index + 1])
                && same_xml_tree(child, xml_child)
        })
}

fn required_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

fn is_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn decode_rows<T: DeserializeOwned>(bytes: &[u8]) -> Result<Vec<T>, NicknameError> {
    if bytes.len() as u64 > crate::MAX_DATA_FILE_BYTES {
        return Err(NicknameError::Framing);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| NicknameError::Framing)?;
    if text.starts_with('\u{feff}') || !text.ends_with('\n') {
        return Err(NicknameError::Framing);
    }
    let body = &text[..text.len() - 1];
    if body.is_empty() {
        return Err(NicknameError::Framing);
    }
    body.split('\n')
        .map(|line| {
            if line.trim().is_empty() {
                return Err(NicknameError::Framing);
            }
            let value = json::parse_integer_zero_strict(line.as_bytes())
                .map_err(|_| NicknameError::Record)?;
            serde_json::from_value(value).map_err(|_| NicknameError::Record)
        })
        .collect()
}

fn validate_graph(graph: &NicknameGraph) -> Result<(), NicknameError> {
    let mut pending = vec![&graph.xml_tree];
    let mut ids = HashSet::new();
    let mut atom_ids = HashSet::new();
    let mut endpoints = Vec::new();
    let mut nodes = 0_u64;
    let mut bonds = 0_u64;
    while let Some(node) = pending.pop() {
        pending.extend(node.children.iter().rev());
        if let Some(id) = node.attributes.get("id")
            && (id.is_empty() || !ids.insert(id.as_str()))
        {
            return Err(NicknameError::Inconsistent);
        }
        match node.tag.as_str() {
            "n" => {
                nodes = nodes.checked_add(1).ok_or(NicknameError::Inconsistent)?;
                let id = node
                    .attributes
                    .get("id")
                    .ok_or(NicknameError::Inconsistent)?;
                atom_ids.insert(id.as_str());
            }
            "b" => {
                bonds = bonds.checked_add(1).ok_or(NicknameError::Inconsistent)?;
                let begin = node
                    .attributes
                    .get("B")
                    .ok_or(NicknameError::Inconsistent)?;
                let end = node
                    .attributes
                    .get("E")
                    .ok_or(NicknameError::Inconsistent)?;
                endpoints.push((begin.as_str(), end.as_str()));
            }
            _ => {}
        }
    }
    if nodes != graph.nodes
        || bonds != graph.bonds
        || endpoints
            .iter()
            .any(|(begin, end)| !atom_ids.contains(begin) || !atom_ids.contains(end))
    {
        return Err(NicknameError::Inconsistent);
    }
    Ok(())
}
