use std::{cell::Cell, collections::HashMap};

use serde_json::{Value, json};

use super::*;

const CONTEXT: &[u8] = include_bytes!("../data/personal-local-context.json");
const FIXTURE: [&[u8]; 7] = [
    include_bytes!("../../tests/data/mass-redesign/elements.json"),
    include_bytes!("../../tests/data/mass-redesign/isotopes.json"),
    include_bytes!("../../tests/data/mass-redesign/defaults.json"),
    include_bytes!("../../tests/data/mass-redesign/aliases.json"),
    include_bytes!("../../tests/data/mass-redesign/constants.json"),
    include_bytes!("../../tests/data/mass-redesign/features.json"),
    include_bytes!("../../tests/data/mass-redesign/bounds.json"),
];

struct MemorySource {
    files: HashMap<String, Vec<u8>>,
    reads: Cell<usize>,
}
impl DataSource for MemorySource {
    fn root_binding_id(&self) -> Option<&str> {
        Some("test-root")
    }
    fn read(&self, path: &str, limit: u64) -> Result<Vec<u8>, crate::SourceError> {
        self.reads.set(self.reads.get() + 1);
        let bytes = self.files.get(path).ok_or(crate::SourceError::Missing)?;
        if bytes.len() as u64 > limit {
            return Err(crate::SourceError::TooLarge);
        }
        Ok(bytes.clone())
    }
}

struct Fixture {
    source: MemorySource,
    manifest: Vec<u8>,
    review: Vec<u8>,
    trust: TrustedPersonalRegistration,
    registry: Vec<u8>,
    profile: Vec<u8>,
}

impl Fixture {
    fn load(&self) -> Result<PersonalDataset, ActiveDataError> {
        let registry = hash(&self.registry);
        let profile = hash(&self.profile);
        let identity = phase6_profiles::CatalogIdentity {
            dataset_id: &self.trust.dataset_id,
            dataset_version: &self.trust.dataset_version,
            registry_sha256: &registry,
            profile_sha256: &profile,
            manifest_file_sha256: &self.trust.manifest_file_sha256,
        };
        let proof = ProfileIdentityFiles {
            registry: &self.registry,
            profile: &self.profile,
        };
        load_personal_impl_with_metadata(
            &self.source,
            &self.trust,
            &self.manifest,
            &self.review,
            CONTEXT,
            Some(&proof),
            ReviewTrust::SyntheticPhase6((&identity).into()),
        )
    }
}

fn fixture(invalid_weight: bool, include_raw: bool) -> Fixture {
    let mut payloads: Vec<Vec<u8>> = FIXTURE.iter().map(|bytes| bytes.to_vec()).collect();
    if invalid_weight {
        let mut elements: Value = serde_json::from_slice(&payloads[0]).unwrap();
        elements["elements"][0]["natural_atomic_weight"] = json!({"source_decimal":"0", "reference_f64":0, "reference_f64_bits_le":"0000000000000000"});
        payloads[0] = serde_json::to_vec(&elements).unwrap();
    }
    let raw = b"row\nentry\nspare\n";
    if include_raw {
        let mut bounds: Value = serde_json::from_slice(&payloads[6]).unwrap();
        bounds["text_source"]["bytes"] = raw.len().into();
        bounds["text_source"]["sha256"] = hash(raw).into();
        bounds["row_source_spans"][0]["source_line_sha256"] = hash(&raw[..4]).into();
        bounds["row_source_spans"][1]["source_bytes_including_newline"] = 6.into();
        bounds["row_source_spans"][1]["source_line_sha256"] = hash(&raw[4..10]).into();
        payloads[6] = serde_json::to_vec(&bounds).unwrap();
    }
    let mut entries: Vec<(&str, Vec<u8>, bool)> = mass_data::FILES
        .into_iter()
        .zip(payloads)
        .map(|(path, bytes)| (path, bytes, true))
        .collect();
    if include_raw {
        entries.push((mass_data::RAW, raw.to_vec(), false));
    }
    let files: Vec<Value> = entries.iter().enumerate().map(|(index, (path, bytes, required))| json!({
        "file_id": format!("synthetic-{index}"), "relative_path": path, "sha256": hash(bytes), "bytes": bytes.len(),
        "role": "auxiliary_data", "required": required, "declared_content_class": "declarative_data"
    })).collect();
    let manifest = serde_json::to_vec(&json!({
        "manifest_version": 1, "dataset_id": "test-mass", "dataset_version": "1", "family_id": "test-family",
        "format_profile_id": "personal-local-atomic-isotope-mass-profile-v1", "independent_engine_contract_id": "test-contract",
        "source_category": "independent_synthetic", "files": files, "feature_schema_sha256": hash(&entries[5].1),
        "rights_review_id": "test-review", "unknown_field_policy": "reject",
        "metadata": {"provider_label": "Synthetic", "version_evidence_id": "test", "description": "No chemistry."}
    })).unwrap();
    let reviewed: Vec<_> = files.iter().map(|entry| json!({
        "file_id": entry["file_id"], "asset_id": "test", "sha256": entry["sha256"], "bytes": entry["bytes"],
        "content_classification": "declarative_data", "classification_basis_id": "test"
    })).collect();
    let review = serde_json::to_vec(&json!({
        "review_version": 2, "review_id": "test-review", "dataset_id": "test-mass", "dataset_version": "1",
        "context_id": "personal-local-user-attested-v1", "context_sha256": hash(CONTEXT),
        "approval_basis": "user_attested_personal_permission", "attestation_reference_id": "fixture-only",
        "authorization_status": "approved_personal_local", "source_registry_binding_id": "test-registry",
        "private_root_binding_id": "test-root", "file_identities": reviewed, "allowed_operations": ["local_load", "decode"],
        "prohibited_operations": ["redistribution", "remote_processing", "original_module_execution", "secret_lookup", "public_vendor_payload_copy"],
        "approved_by": "fixture-author", "additional_licensor_written_document_required": false,
        "documentary_verification_claimed": false, "technical_availability_claimed": false, "fixture_only": true
    })).unwrap();
    let identity = validate_active_manifest(&manifest).unwrap();
    let trust = TrustedPersonalRegistration {
        registration_id: "test-registration".into(),
        generation: 1,
        dataset_id: "test-mass".into(),
        dataset_version: "1".into(),
        family_id: "test-family".into(),
        profile_id: "personal-local-atomic-isotope-mass-profile-v1".into(),
        engine_contract_id: "test-contract".into(),
        manifest_sha256: identity.canonical_sha256,
        manifest_file_sha256: identity.exact_sha256,
        review_sha256: hash(&review),
        source_registry_binding_id: "test-registry".into(),
        private_root_binding_id: "test-root".into(),
    };
    Fixture {
        source: MemorySource {
            files: entries
                .into_iter()
                .map(|(path, bytes, _)| (path.into(), bytes))
                .collect(),
            reads: Cell::new(0),
        },
        manifest,
        review,
        trust,
        registry: b"test-registry".to_vec(),
        profile: b"test-profile".to_vec(),
    }
}

#[test]
fn a_checked_generation_exposes_only_the_complete_typed_input() {
    let mut sample = fixture(false, false);
    let loaded = sample.load().unwrap();
    let ProfileRecords::Mass(data) = loaded.records() else {
        panic!("wrong profile")
    };
    assert_eq!(
        data.elements[0].natural_atomic_weight.source_decimal,
        "14.1250"
    );
    assert_eq!(sample.source.reads.get(), 7);
    sample
        .source
        .files
        .get_mut("parameters/aliases.json")
        .unwrap()[0] ^= 1;
    assert!(matches!(
        sample.load(),
        Err(ActiveDataError::DataIdentityMismatch)
    ));
    assert!(matches!(
        fixture(true, false).load(),
        Err(ActiveDataError::ContentInvalid)
    ));
}

#[test]
fn listed_raw_source_is_part_of_the_same_atomic_generation() {
    let mut fixture = fixture(false, true);
    assert!(matches!(
        fixture.load().unwrap().records(),
        ProfileRecords::Mass(_)
    ));
    assert_eq!(fixture.source.reads.get(), 8);
    fixture.source.files.remove(mass_data::RAW);
    assert!(matches!(
        fixture.load(),
        Err(ActiveDataError::DataUnavailable)
    ));
}
