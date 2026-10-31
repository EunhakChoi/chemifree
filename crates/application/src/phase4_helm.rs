//! HELM input records; attachment, sequence and stereo semantics remain unbound.

use num_bigint::BigInt;
use serde::{Deserialize, Deserializer};

use crate::{json, phase6_profiles};

/// The independently published catalog identifies the one source file explicitly.
pub const FILE_ID: &str = "PRIVATE-HELM-CORE-FECC972";

/// Payload-free rejection of a HELM input generation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HelmError {
    CatalogMismatch,
    FileSet,
    Framing,
    Record,
    RecordCount,
}

impl std::fmt::Display for HelmError {
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
impl std::error::Error for HelmError {}

/// Verifies the dataset and the exact public catalog metadata identities.
///
/// # Errors
/// Rejects a different dataset, version or metadata byte identity.
pub fn verify_catalog_files(
    dataset_id: &str,
    dataset_version: &str,
    manifest: &[u8],
    registry: &[u8],
    catalog: &[u8],
) -> Result<(), HelmError> {
    let expected = phase6_profiles::CatalogIdentity {
        dataset_id: "independent-HELM-CORE-FECC972",
        dataset_version: "sha256-set-6451ebcea0fa0099b71b75c952325b9b92ea77b1b822363f68e5f2a58e04ac86",
        registry_sha256: "8ffd2e9b4d01ce989c63cb552714e1ff01acb8f766dbbc14bc411f08c4484d25",
        profile_sha256: "5add6a3e7a6250dc5c2f280d9a193dc8aef3ba008e47d10e35e7b263c217c2a2",
        manifest_file_sha256: "cd7fb740c40e94c272bdba81b3813f35af4e80712787b9ab7ea632e9aeb4a089",
    };
    if dataset_id != expected.dataset_id || dataset_version != expected.dataset_version {
        return Err(HelmError::CatalogMismatch);
    }
    expected
        .check_file_bytes(manifest, registry, catalog)
        .map_err(|_| HelmError::CatalogMismatch)
}

/// Checks the catalog file ID, role and content class.
#[must_use]
pub fn accepts_file(id: &str, role: &str, class: &str) -> bool {
    id == FILE_ID && role == "document_resource" && class == "declarative_data"
}

fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

/// Both source spellings are retained independently; neither is normalized into the other.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Attachment {
    #[serde(rename = "capGroupSmiles", default, deserialize_with = "present")]
    pub cap_group_smiles: Option<String>,
    #[serde(rename = "capGroupSMILES", default, deserialize_with = "present")]
    pub cap_group_smiles_upper: Option<String>,
    #[serde(rename = "alternateId")]
    pub alternate_id: String,
    #[serde(rename = "capGroupName")]
    pub cap_group_name: String,
    pub label: String,
}

/// A source row is descriptive data, not a verified or executable graph template.
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Monomer {
    #[serde(rename = "monomerType")]
    pub monomer_type: String,
    pub smiles: String,
    pub name: String,
    pub author: String,
    pub molfile: String,
    #[serde(rename = "naturalAnalog")]
    pub natural_analog: String,
    pub rgroups: Vec<Attachment>,
    #[serde(rename = "createDate")]
    pub create_date: (),
    #[serde(deserialize_with = "json::deserialize_integer")]
    pub id: BigInt,
    #[serde(rename = "polymerType")]
    pub polymer_type: String,
    pub symbol: String,
}

/// Ordered source records; duplicate IDs and unresolved variants are not reinterpreted.
#[derive(PartialEq)]
pub struct HelmData {
    pub monomers: Vec<Monomer>,
}

/// Decode the complete catalog-selected JSON array.
///
/// # Errors
/// Rejects an unrecognized file set, oversized or invalid JSON and a wrong record count.
pub fn decode_dataset(files: &[(&str, &[u8])]) -> Result<HelmData, HelmError> {
    let [(id, bytes)] = files else {
        return Err(HelmError::FileSet);
    };
    if *id != FILE_ID {
        return Err(HelmError::FileSet);
    }
    if bytes.len() as u64 > crate::MAX_DATA_FILE_BYTES
        || bytes.len() as u64 > crate::MAX_DATA_TOTAL_BYTES
    {
        return Err(HelmError::Framing);
    }
    let monomers: Vec<Monomer> = serde_json::from_slice(bytes).map_err(|_| HelmError::Record)?;
    if monomers.len() != 705 {
        return Err(HelmError::RecordCount);
    }
    Ok(HelmData { monomers })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROW: &str = r#"{"monomerType":"Terminal","smiles":"C","name":"synthetic","author":"fixture","molfile":"synthetic","naturalAnalog":"X","rgroups":[{"capGroupSmiles":"C","capGroupSMILES":"N","alternateId":"R1","capGroupName":"cap","label":"R1"}],"createDate":null,"id":0,"polymerType":"synthetic","symbol":"X"}"#;

    fn document(row: &str, count: usize) -> Vec<u8> {
        format!("[{}]", vec![row; count].join(",")).into_bytes()
    }

    #[test]
    fn schema_integral_numbers_preserve_monomer_text() {
        let row = ROW
            .replace("\"id\":0", "\"id\":1.0")
            .replace("\"name\":\"synthetic\"", "\"name\":\"-0.0\"");
        let bytes = document(&row, 705);
        let data = decode_dataset(&[(FILE_ID, &bytes)]).unwrap();
        assert_eq!(data.monomers[0].id.to_string(), "1");
        assert_eq!(data.monomers[0].name, "-0.0");
        let invalid = row.replace("\"id\":1.0", "\"id\":1.00000000000000000001");
        assert_eq!(
            decode_dataset(&[(FILE_ID, &document(&invalid, 705))]).err(),
            Some(HelmError::Record)
        );
    }

    #[test]
    fn monomer_identifier_preserves_a_large_signed_schema_integer() {
        let id = format!("-{}", "9".repeat(400));
        let row = ROW.replace("\"id\":0", &format!("\"id\":{id}"));
        assert!(row.contains(&format!("\"id\":{id}")));
        let bytes = document(&row, 705);
        let data = decode_dataset(&[(FILE_ID, &bytes)]).unwrap();
        assert_eq!(data.monomers[0].id.to_string(), id);
        let overlong = ROW.replace("\"id\":0", &format!("\"id\":-{}", "9".repeat(4097)));
        assert_eq!(
            decode_dataset(&[(FILE_ID, &document(&overlong, 705))]).err(),
            Some(HelmError::Record)
        );
    }

    #[test]
    fn records_keep_terminal_and_both_attachment_spellings_without_interpretation() {
        let bytes = document(ROW, 705);
        let data = decode_dataset(&[(FILE_ID, &bytes)]).unwrap();
        assert_eq!(data.monomers.len(), 705);
        assert_eq!(data.monomers[0].monomer_type, "Terminal");
        assert_eq!(data.monomers[0].id.to_string(), "0");
        assert_eq!(data.monomers[1].id.to_string(), "0");
        assert_eq!(
            data.monomers[0].rgroups[0].cap_group_smiles.as_deref(),
            Some("C")
        );
        assert_eq!(
            data.monomers[0].rgroups[0]
                .cap_group_smiles_upper
                .as_deref(),
            Some("N")
        );
    }

    #[test]
    fn nullable_date_and_optional_spellings_follow_the_closed_schema() {
        let missing_date = ROW.replace(",\"createDate\":null", "");
        let invalid_date = ROW.replace("\"createDate\":null", "\"createDate\":\"2020\"");
        let null_attachment = ROW.replace("\"capGroupSmiles\":\"C\"", "\"capGroupSmiles\":null");
        let extra = ROW.replace("\"symbol\":\"X\"", "\"symbol\":\"X\",\"extra\":true");
        let extra_attachment = ROW.replace("\"label\":\"R1\"", "\"label\":\"R1\",\"extra\":true");
        let duplicate = ROW.replace("\"id\":0", "\"id\":0,\"id\":1");
        let duplicate_attachment =
            ROW.replace("\"label\":\"R1\"", "\"label\":\"R1\",\"label\":\"R2\"");
        for row in [
            missing_date,
            invalid_date,
            null_attachment,
            extra,
            extra_attachment,
            duplicate,
            duplicate_attachment,
        ] {
            let bytes = document(&row, 705);
            assert_eq!(
                decode_dataset(&[(FILE_ID, &bytes)]).err(),
                Some(HelmError::Record)
            );
        }
        let no_optional = ROW.replace("\"capGroupSmiles\":\"C\",\"capGroupSMILES\":\"N\",", "");
        let bytes = document(&no_optional, 705);
        let data = decode_dataset(&[(FILE_ID, &bytes)]).unwrap();
        assert!(data.monomers[0].rgroups[0].cap_group_smiles.is_none());
        assert!(data.monomers[0].rgroups[0].cap_group_smiles_upper.is_none());
    }

    #[test]
    fn wrong_file_sets_counts_and_invalid_json_are_rejected() {
        let bytes = document(ROW, 704);
        assert_eq!(
            decode_dataset(&[(FILE_ID, &bytes)]).err(),
            Some(HelmError::RecordCount)
        );
        assert_eq!(decode_dataset(&[]).err(), Some(HelmError::FileSet));
        assert_eq!(
            decode_dataset(&[("other", &bytes)]).err(),
            Some(HelmError::FileSet)
        );
        assert_eq!(
            decode_dataset(&[(FILE_ID, b"not json")]).err(),
            Some(HelmError::Record)
        );
    }

    #[test]
    fn catalog_identity_and_file_role_are_explicit() {
        assert!(accepts_file(
            FILE_ID,
            "document_resource",
            "declarative_data"
        ));
        assert!(!accepts_file(FILE_ID, "auxiliary_data", "declarative_data"));
        assert!(!accepts_file(
            "other",
            "document_resource",
            "declarative_data"
        ));
        assert!(!accepts_file(
            FILE_ID,
            "document_resource",
            "model_parameters"
        ));
        assert_eq!(
            verify_catalog_files("independent-HELM-CORE-FECC972", "wrong", b"m", b"r", b"c"),
            Err(HelmError::CatalogMismatch)
        );
    }
}
