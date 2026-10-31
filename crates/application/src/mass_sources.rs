//! Inert provenance checks for the mass profile. No source executable is opened.

use std::collections::{HashMap, HashSet};

use num_bigint::BigUint;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::json;

/// A source identity recorded by the separately supplied dataset.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FileIdentity {
    pub file_name: String,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub bytes: BigUint,
    pub sha256: String,
}

/// A declared coefficient span, not an executable address or a file-opening instruction.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SourceSpan {
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub source_file_offset: BigUint,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub bytes: BigUint,
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
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub row_ordinal: u64,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub source_line_1based: BigUint,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub source_file_offset: BigUint,
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub source_bytes_including_newline: BigUint,
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

struct BoundedSpan<'a> {
    start: &'a BigUint,
    end: BigUint,
}

impl<'a> BoundedSpan<'a> {
    fn admit(
        start: &'a BigUint,
        length: &BigUint,
        parent_size: &BigUint,
    ) -> Result<Self, InvalidSource> {
        let end = start + length;
        if length == &BigUint::from(0_u8) || &end > parent_size {
            return Err(InvalidSource);
        }
        Ok(Self { start, end })
    }
}

fn is_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_identity(identity: &FileIdentity) -> bool {
    !identity.file_name.trim().is_empty()
        && identity.bytes != BigUint::from(0_u8)
        && is_digest(&identity.sha256)
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
            &self.signed_charge_coefficient.source_file_offset,
            &self.signed_charge_coefficient.bytes,
            &self.consumer_source.bytes,
        )?;
        let mut targets = HashSet::new();
        let mut physical = Vec::with_capacity(self.row_source_spans.len());
        let zero = BigUint::from(0_u8);
        let one = BigUint::from(1_u8);
        for row in &self.row_source_spans {
            let row_count = match row.kind {
                SourceRowKind::Element => elements,
                SourceRowKind::Isotope => isotopes,
            };
            let ordinal = usize::try_from(row.row_ordinal).map_err(|_| InvalidSource)?;
            let line = &row.source_line_1based;
            if line == &zero {
                return Err(InvalidSource);
            }
            if ordinal >= row_count
                || (line == &one && row.source_file_offset != zero)
                || row.source_file_offset < line - &one
                || !targets.insert((row.kind, ordinal))
                || !is_digest(&row.source_line_sha256)
            {
                return Err(InvalidSource);
            }
            let range = BoundedSpan::admit(
                &row.source_file_offset,
                &row.source_bytes_including_newline,
                &self.text_source.bytes,
            )?;
            physical.push((line, range));
        }
        physical.sort_unstable_by(|left, right| left.0.cmp(right.0));
        if physical
            .windows(2)
            .any(|pair| pair[0].0 == pair[1].0 || &pair[0].1.end > pair[1].1.start)
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
            || BigUint::from(raw.len()) != self.text_source.bytes
            || format!("{:x}", Sha256::digest(raw)) != self.text_source.sha256
        {
            return Err(InvalidSource);
        }
        let wanted: HashMap<_, _> = self
            .row_source_spans
            .iter()
            .map(|row| {
                Ok((
                    u64::try_from(&row.source_line_1based).map_err(|_| InvalidSource)?,
                    row,
                ))
            })
            .collect::<Result<_, InvalidSource>>()?;
        let mut found = 0_usize;
        let mut offset = 0_u64;
        for (index, line) in raw.split_inclusive(|byte| *byte == b'\n').enumerate() {
            let number = index as u64 + 1;
            if let Some(row) = wanted.get(&number) {
                if row.source_file_offset != BigUint::from(offset)
                    || row.source_bytes_including_newline != BigUint::from(line.len())
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
        let n = BigUint::from;
        assert!(BoundedSpan::admit(&n(3_u64), &n(2_u64), &n(5_u64)).is_ok());
        assert!(BoundedSpan::admit(&n(3_u64), &n(0_u64), &n(5_u64)).is_err());
        assert!(BoundedSpan::admit(&n(4_u64), &n(2_u64), &n(5_u64)).is_err());
        assert!(BoundedSpan::admit(&n(u64::MAX), &n(1_u64), &n(u64::MAX)).is_err());
    }
}
