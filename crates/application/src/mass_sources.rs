//! Inert provenance checks for the mass profile. No source executable is opened.

use std::collections::{HashMap, HashSet};
use std::num::NonZeroU64;

use serde::Deserialize;
use sha2::{Digest, Sha256};

/// A source identity recorded by the separately supplied dataset.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FileIdentity {
    pub file_name: String,
    pub bytes: u64,
    pub sha256: String,
}

/// A declared coefficient span, not an executable address or a file-opening instruction.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SourceSpan {
    pub source_file_offset: u64,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Copy, Deserialize, Eq, Hash, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SourceRowKind {
    Element,
    Isotope,
}

/// An identified physical line in the optional raw text input.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SourceLine {
    pub kind: SourceRowKind,
    pub row_ordinal: u64,
    pub source_line_1based: u64,
    pub source_file_offset: u64,
    pub source_bytes_including_newline: u64,
    pub source_line_sha256: String,
}

#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EvidenceIdentity {
    pub evidence_path: String,
    pub evidence_sha256: String,
}

/// Retained source metadata; its file names and offsets are never dereferenced here.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MassSources {
    pub binding_id: String,
    pub inert_source_lineage_only: bool,
    pub text_source: FileIdentity,
    pub installed_equivalent_copy_sha256: String,
    pub row_source_spans: Vec<SourceLine>,
    pub consumer_source: FileIdentity,
    pub consumer_evidence: Vec<EvidenceIdentity>,
    pub signed_charge_coefficient: SourceSpan,
    pub source_offsets_are_not_native_dispatch: bool,
    #[serde(rename = "original_source_or_CFG_included")]
    pub original_source_or_cfg_included: bool,
}

/// Payload-free refusal of malformed or contradictory provenance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct InvalidSource;

#[derive(Clone, Copy)]
struct BoundedSpan {
    start: u64,
    length: NonZeroU64,
}

impl BoundedSpan {
    fn admit(start: u64, length: u64, parent_size: u64) -> Result<Self, InvalidSource> {
        let length = NonZeroU64::new(length).ok_or(InvalidSource)?;
        if start
            .checked_add(length.get())
            .is_none_or(|end| end > parent_size)
        {
            return Err(InvalidSource);
        }
        Ok(Self { start, length })
    }

    fn end(self) -> u64 {
        self.start + self.length.get()
    }
}

fn is_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_identity(identity: &FileIdentity) -> bool {
    !identity.file_name.trim().is_empty() && identity.bytes > 0 && is_digest(&identity.sha256)
}

impl MassSources {
    /// Checks metadata and references before admitting any of the source declarations.
    ///
    /// # Errors
    /// Returns a payload-free error when metadata, identity, row or byte bounds conflict.
    pub(crate) fn validate(&self, elements: usize, isotopes: usize) -> Result<(), InvalidSource> {
        if self.binding_id.trim().is_empty()
            || !self.inert_source_lineage_only
            || !self.source_offsets_are_not_native_dispatch
            || self.original_source_or_cfg_included
            || !valid_identity(&self.text_source)
            || !valid_identity(&self.consumer_source)
            || !is_digest(&self.installed_equivalent_copy_sha256)
            || self.consumer_evidence.iter().any(|item| {
                item.evidence_path.trim().is_empty() || !is_digest(&item.evidence_sha256)
            })
            || !is_digest(&self.signed_charge_coefficient.sha256)
        {
            return Err(InvalidSource);
        }
        BoundedSpan::admit(
            self.signed_charge_coefficient.source_file_offset,
            self.signed_charge_coefficient.bytes,
            self.consumer_source.bytes,
        )?;
        let mut targets = HashSet::new();
        let mut physical = Vec::with_capacity(self.row_source_spans.len());
        for row in &self.row_source_spans {
            let row_count = match row.kind {
                SourceRowKind::Element => elements,
                SourceRowKind::Isotope => isotopes,
            };
            let ordinal = usize::try_from(row.row_ordinal).map_err(|_| InvalidSource)?;
            let line = NonZeroU64::new(row.source_line_1based).ok_or(InvalidSource)?;
            if ordinal >= row_count
                || !targets.insert((row.kind, ordinal))
                || !is_digest(&row.source_line_sha256)
            {
                return Err(InvalidSource);
            }
            let range = BoundedSpan::admit(
                row.source_file_offset,
                row.source_bytes_including_newline,
                self.text_source.bytes,
            )?;
            physical.push((line, range));
        }
        physical.sort_unstable_by_key(|(line, _)| *line);
        if physical
            .windows(2)
            .any(|pair| pair[0].0 == pair[1].0 || pair[0].1.end() > pair[1].1.start)
        {
            return Err(InvalidSource);
        }
        Ok(())
    }

    /// Checks an explicitly supplied raw text file against its identity and declared lines.
    /// The line contents are not interpreted as chemistry records.
    ///
    /// # Errors
    /// Returns a payload-free error for a changed file or a contradictory line witness.
    pub(crate) fn verify_text(&self, raw: &[u8]) -> Result<(), InvalidSource> {
        if raw.len() as u64 > crate::MAX_DATA_FILE_BYTES
            || raw.len() as u64 != self.text_source.bytes
            || format!("{:x}", Sha256::digest(raw)) != self.text_source.sha256
        {
            return Err(InvalidSource);
        }
        let wanted: HashMap<_, _> = self
            .row_source_spans
            .iter()
            .map(|row| (row.source_line_1based, row))
            .collect();
        let mut found = 0_usize;
        let mut offset = 0_u64;
        for (index, line) in raw.split_inclusive(|byte| *byte == b'\n').enumerate() {
            let number = index as u64 + 1;
            if let Some(row) = wanted.get(&number) {
                if row.source_file_offset != offset
                    || row.source_bytes_including_newline != line.len() as u64
                    || row.source_line_sha256 != format!("{:x}", Sha256::digest(line))
                {
                    return Err(InvalidSource);
                }
                found += 1;
            }
            offset = offset.checked_add(line.len() as u64).ok_or(InvalidSource)?;
        }
        if found != wanted.len() {
            return Err(InvalidSource);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn any_source_span_must_be_nonempty_and_fit_without_overflow() {
        assert!(BoundedSpan::admit(3, 2, 5).is_ok());
        assert!(BoundedSpan::admit(3, 0, 5).is_err());
        assert!(BoundedSpan::admit(4, 2, 5).is_err());
        assert!(BoundedSpan::admit(u64::MAX, 1, u64::MAX).is_err());
    }
}
