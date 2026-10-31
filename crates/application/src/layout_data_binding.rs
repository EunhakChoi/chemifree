//! Explicit host-selected layout DATA roles and independently published identity checks.

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{DataProfile, ProfileRecords, decode_profile};

const SCHEMA_SHA256: &str = "97be978878cb36e44d435cd3fef637d8df08dff00f9540f5dd9dd139f1540a0c";
const FIXED_REGISTRY_SHA256: &str =
    "d287183510cb8e628f5c83f1485bf2ddfe6a35d42a13c6d50dded02dccff3afe";
const LARGE_REGISTRY_SHA256: &str =
    "57aed0ab186c18aaf985626d7d44f9e2929512c7dca2795b15e77076bc5a87f5";
const LARGE_IDS: [&str; 2] = ["LARGE-CONTOUR-EXACT-001", "LARGE-CONTOUR-EXACT-002"];

/// A host-selected role; this is not a claim that the registry proves manifest membership.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum LayoutReader {
    FixedContours,
    LargeTwentySeed,
}

/// Validated operands with their explicitly selected source role.
#[derive(PartialEq)]
pub struct LayoutOperands {
    pub role: LayoutReader,
    pub contours: Vec<crate::Contour>,
}

/// Payload-free failure while checking a selected role's metadata or operands.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LayoutBindingError {
    Identity,
    Operands,
}
impl std::fmt::Display for LayoutBindingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Identity => "layout_metadata_identity_mismatch",
            Self::Operands => "invalid_layout_operands",
        })
    }
}
impl std::error::Error for LayoutBindingError {}

fn matches_identity(
    registry: &[u8],
    schema: &[u8],
    registry_sha256: &str,
    schema_sha256: &str,
    schema_len: usize,
) -> bool {
    registry.len() as u64 <= crate::MAX_MANIFEST_BYTES
        && schema.len() == schema_len
        && format!("{:x}", Sha256::digest(registry)) == registry_sha256
        && format!("{:x}", Sha256::digest(schema)) == schema_sha256
}

/// Verify exact published registry and schema bytes for the selected layout role.
///
/// This check does not by itself prove that a manifest belongs to the registry. The host's
/// separately trusted registration binds the manifest and selected role.
///
/// # Errors
/// Rejects changed, oversized or mismatched metadata bytes.
pub fn verify_identity(
    reader: LayoutReader,
    registry: &[u8],
    schema: &[u8],
) -> Result<(), LayoutBindingError> {
    let expected = match reader {
        LayoutReader::FixedContours => FIXED_REGISTRY_SHA256,
        LayoutReader::LargeTwentySeed => LARGE_REGISTRY_SHA256,
    };
    if matches_identity(registry, schema, expected, SCHEMA_SHA256, 1230) {
        Ok(())
    } else {
        Err(LayoutBindingError::Identity)
    }
}

/// Decode selected operand rows and enforce the explicit public large-seed role contract.
///
/// # Errors
/// Rejects malformed rows, wrong counts and large-seed IDs or ring sizes.
pub fn decode_selected(
    reader: LayoutReader,
    bytes: &[u8],
) -> Result<LayoutOperands, LayoutBindingError> {
    if bytes.len() as u64 > crate::MAX_DATA_FILE_BYTES {
        return Err(LayoutBindingError::Operands);
    }
    let profile = match reader {
        LayoutReader::FixedContours => DataProfile::FixedContours,
        LayoutReader::LargeTwentySeed => DataProfile::LargeRingSeeds,
    };
    let records = decode_profile(profile, bytes).map_err(|_| LayoutBindingError::Operands)?;
    let ProfileRecords::Contours(rows) = records else {
        return Err(LayoutBindingError::Operands);
    };
    if reader == LayoutReader::LargeTwentySeed
        && (rows.len() != LARGE_IDS.len()
            || LARGE_IDS.iter().any(|id| {
                !rows
                    .iter()
                    .any(|row| row.operand_id == *id && row.ring_size == 20)
            }))
    {
        return Err(LayoutBindingError::Operands);
    }
    Ok(LayoutOperands {
        role: reader,
        contours: rows,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn row(id: &str, size: usize) -> String {
        let zeros = vec!["0000000000000000"; size];
        let lengths = vec!["000000000000f03f"; size];
        json!({"operand_id":id,"ring_size":size,"calibration_reference_radians_f64le":"0000000000000080","constructor_heading_radians_f64le":"0000000000000000","winding_mode":1,"joint_policy_flag":false,"turn_degrees_f64le":zeros,"observed_heading_radians_f64le":zeros,"edge_length_multipliers_f64le":lengths}).to_string()
    }
    fn stream(rows: &[String]) -> Vec<u8> {
        format!("{}\n", rows.join("\n")).into_bytes()
    }

    #[test]
    fn contour_schema_integers_allow_exact_decimal_forms_without_rounding() {
        let mut first: serde_json::Value = serde_json::from_str(&row(LARGE_IDS[0], 20)).unwrap();
        first["ring_size"] = json!(20.0);
        first["winding_mode"] = json!(1.0);
        let valid = stream(&[first.to_string(), row(LARGE_IDS[1], 20)]);
        assert!(decode_selected(LayoutReader::LargeTwentySeed, &valid).is_ok());

        let inexact = String::from_utf8(valid).unwrap().replace(
            "\"ring_size\":20.0",
            "\"ring_size\":20.00000000000000000001",
        );
        assert_eq!(
            decode_selected(LayoutReader::LargeTwentySeed, inexact.as_bytes()).err(),
            Some(LayoutBindingError::Operands)
        );
    }

    #[test]
    fn naming_rank_profile_keeps_its_stricter_integer_token_rule() {
        let rows: Vec<_> = (1..=22)
            .map(|n| json!({"atomic_number_u32":n,"source_rank_u32":n}).to_string())
            .collect();
        let valid = stream(&rows);
        assert!(decode_profile(DataProfile::RingRank, &valid).is_ok());
        let decimal = String::from_utf8(valid).unwrap().replacen(
            "\"atomic_number_u32\":1,",
            "\"atomic_number_u32\":1.0,",
            1,
        );
        assert!(decode_profile(DataProfile::RingRank, decimal.as_bytes()).is_err());
    }

    #[test]
    fn large_seed_role_requires_both_public_ids_and_twenty_member_operands() {
        let valid = stream(&[row(LARGE_IDS[1], 20), row(LARGE_IDS[0], 20)]);
        let input = decode_selected(LayoutReader::LargeTwentySeed, &valid).unwrap();
        assert_eq!(input.role, LayoutReader::LargeTwentySeed);
        let rows = input.contours;
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].calibration_reference_bits, (-0.0_f64).to_bits());
        let wrong_id = stream(&[row(LARGE_IDS[0], 20), row("synthetic-other", 20)]);
        assert_eq!(
            decode_selected(LayoutReader::LargeTwentySeed, &wrong_id).err(),
            Some(LayoutBindingError::Operands)
        );
        let wrong_size = stream(&[row(LARGE_IDS[0], 3), row(LARGE_IDS[1], 20)]);
        assert_eq!(
            decode_selected(LayoutReader::LargeTwentySeed, &wrong_size).err(),
            Some(LayoutBindingError::Operands)
        );
        let duplicate = stream(&[row(LARGE_IDS[0], 20), row(LARGE_IDS[0], 20)]);
        assert_eq!(
            decode_selected(LayoutReader::LargeTwentySeed, &duplicate).err(),
            Some(LayoutBindingError::Operands)
        );
    }
    #[test]
    fn fixed_role_retains_exactly_158_distinct_operand_records() {
        let records: Vec<_> = (0..158)
            .map(|index| row(&format!("synthetic-{index}"), 3))
            .collect();
        let valid = stream(&records);
        let input = decode_selected(LayoutReader::FixedContours, &valid).unwrap();
        assert_eq!(input.role, LayoutReader::FixedContours);
        let rows = input.contours;
        assert_eq!(rows.len(), 158);
        let incomplete = stream(&records[..157]);
        assert_eq!(
            decode_selected(LayoutReader::FixedContours, &incomplete).err(),
            Some(LayoutBindingError::Operands)
        );
    }
    #[test]
    fn exact_metadata_identity_rejects_changes_and_role_confusion() {
        let registry = b"independent registry";
        let schema = b"independent schema";
        let registry_hash = format!("{:x}", Sha256::digest(registry));
        let schema_hash = format!("{:x}", Sha256::digest(schema));
        assert!(matches_identity(
            registry,
            schema,
            &registry_hash,
            &schema_hash,
            schema.len()
        ));
        assert!(!matches_identity(
            registry,
            schema,
            &registry_hash,
            &schema_hash,
            schema.len() + 1
        ));
        let oversized_registry = vec![0; crate::MAX_MANIFEST_BYTES as usize + 1];
        let oversized_hash = format!("{:x}", Sha256::digest(&oversized_registry));
        assert!(!matches_identity(
            &oversized_registry,
            schema,
            &oversized_hash,
            &schema_hash,
            schema.len()
        ));
        assert!(!matches_identity(
            b"changed",
            schema,
            &registry_hash,
            &schema_hash,
            schema.len()
        ));
        assert!(!matches_identity(
            registry,
            b"changed",
            &registry_hash,
            &schema_hash,
            schema.len()
        ));
        let mut changed_schema = schema.to_vec();
        changed_schema[0] ^= 1;
        assert!(!matches_identity(
            registry,
            &changed_schema,
            &registry_hash,
            &schema_hash,
            schema.len()
        ));
        assert_eq!(
            verify_identity(LayoutReader::FixedContours, registry, schema),
            Err(LayoutBindingError::Identity)
        );
        assert_eq!(
            verify_identity(LayoutReader::LargeTwentySeed, registry, schema),
            Err(LayoutBindingError::Identity)
        );
    }
}
