use std::{cell::Cell, collections::HashMap};

use serde_json::{Value, json};

use super::*;

const CONTEXT: &[u8] = include_bytes!("../data/personal-local-context.json");

struct FixtureSource {
    files: HashMap<String, Vec<u8>>,
    reads: Cell<usize>,
}

impl DataSource for FixtureSource {
    fn root_binding_id(&self) -> Option<&str> {
        Some("mass-v3-fixture-root")
    }

    fn read(&self, path: &str, limit: u64) -> Result<Vec<u8>, crate::SourceError> {
        self.reads.set(self.reads.get() + 1);
        let content = self.files.get(path).ok_or(crate::SourceError::Missing)?;
        if content.len() as u64 > limit {
            return Err(crate::SourceError::TooLarge);
        }
        Ok(content.clone())
    }
}

struct MassFixture {
    source: FixtureSource,
    manifest: Vec<u8>,
    review: Vec<u8>,
    trust: TrustedPersonalRegistration,
    registry: Vec<u8>,
    profile: Vec<u8>,
}

impl MassFixture {
    fn new(invalid_coefficient_span: bool) -> Self {
        let content: [&[u8]; 7] = [
            include_bytes!("../../tests/data/mass-independent-v3/elements.json"),
            include_bytes!("../../tests/data/mass-independent-v3/isotopes.json"),
            include_bytes!("../../tests/data/mass-independent-v3/selection.json"),
            include_bytes!("../../tests/data/mass-independent-v3/aliases.json"),
            include_bytes!("../../tests/data/mass-independent-v3/constants.json"),
            include_bytes!("../../tests/data/mass-independent-v3/features.json"),
            include_bytes!("../../tests/data/mass-independent-v3/sources.json"),
        ];
        let mut encoded: Vec<Vec<u8>> = content.into_iter().map(<[u8]>::to_vec).collect();
        if invalid_coefficient_span {
            let mut sources: Value = serde_json::from_slice(&encoded[6]).unwrap();
            sources["signed_charge_coefficient"]["bytes"] = 0.into();
            encoded[6] = serde_json::to_vec(&sources).unwrap();
        }
        let files: Vec<Value> = mass_input::NORMALIZED_PATHS.iter().zip(&encoded).enumerate()
            .map(|(index, (path, bytes))| json!({
                "file_id": format!("synthetic-{index}"), "relative_path": path,
                "sha256": hash(bytes), "bytes": bytes.len(),
                "role": if *path == mass_input::FEATURE_PATH { "feature_schema" } else { "auxiliary_data" },
                "required": true, "declared_content_class": "declarative_data"
            })).collect();
        let feature_hash = hash(&encoded[5]);
        let manifest = serde_json::to_vec(&json!({
            "manifest_version": 1, "dataset_id": "independent-mass-v3", "dataset_version": "fixture-1",
            "family_id": "independent-fixture", "format_profile_id": "personal-local-atomic-isotope-mass-profile-v1",
            "independent_engine_contract_id": "fixture-contract", "source_category": "independent_synthetic",
            "files": files, "feature_schema_sha256": feature_hash, "rights_review_id": "mass-v3-review",
            "unknown_field_policy": "reject", "metadata": {"provider_label": "Independent fixture", "version_evidence_id": "fixture", "description": "No chemical data."}
        })).unwrap();
        let file_identities: Vec<_> = files.iter().map(|file| json!({
            "file_id": file["file_id"], "asset_id": "fixture", "sha256": file["sha256"],
            "bytes": file["bytes"], "content_classification": "declarative_data", "classification_basis_id": "fixture"
        })).collect();
        let review = serde_json::to_vec(&json!({
            "review_version": 2, "review_id": "mass-v3-review", "dataset_id": "independent-mass-v3", "dataset_version": "fixture-1",
            "context_id": "personal-local-user-attested-v1", "context_sha256": hash(CONTEXT),
            "approval_basis": "user_attested_personal_permission", "attestation_reference_id": "fixture-only",
            "authorization_status": "approved_personal_local", "source_registry_binding_id": "fixture-registry",
            "private_root_binding_id": "mass-v3-fixture-root", "file_identities": file_identities,
            "allowed_operations": ["local_load", "decode"],
            "prohibited_operations": ["redistribution", "remote_processing", "original_module_execution", "secret_lookup", "public_vendor_payload_copy"],
            "approved_by": "fixture-author", "additional_licensor_written_document_required": false,
            "documentary_verification_claimed": false, "technical_availability_claimed": false, "fixture_only": true
        })).unwrap();
        let identity = validate_active_manifest(&manifest).unwrap();
        let trust = TrustedPersonalRegistration {
            registration_id: "mass-v3-registration".into(),
            generation: 9,
            dataset_id: "independent-mass-v3".into(),
            dataset_version: "fixture-1".into(),
            family_id: "independent-fixture".into(),
            profile_id: "personal-local-atomic-isotope-mass-profile-v1".into(),
            engine_contract_id: "fixture-contract".into(),
            manifest_sha256: identity.canonical_sha256,
            manifest_file_sha256: identity.exact_sha256,
            review_sha256: hash(&review),
            source_registry_binding_id: "fixture-registry".into(),
            private_root_binding_id: "mass-v3-fixture-root".into(),
        };
        Self {
            source: FixtureSource {
                files: mass_input::NORMALIZED_PATHS
                    .iter()
                    .zip(encoded)
                    .map(|(path, bytes)| ((*path).into(), bytes))
                    .collect(),
                reads: Cell::new(0),
            },
            manifest,
            review,
            trust,
            registry: b"independent-registry".to_vec(),
            profile: b"independent-profile".to_vec(),
        }
    }

    fn load_test_authorized(&self) -> Result<PersonalDataset, ActiveDataError> {
        let registry_hash = hash(&self.registry);
        let profile_hash = hash(&self.profile);
        let identity = phase6_profiles::CatalogIdentity {
            dataset_id: &self.trust.dataset_id,
            dataset_version: &self.trust.dataset_version,
            registry_sha256: &registry_hash,
            profile_sha256: &profile_hash,
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

#[test]
fn independent_fixture_is_never_a_production_rights_grant() {
    let fixture = MassFixture::new(false);
    let proof = ProfileIdentityFiles {
        registry: &fixture.registry,
        profile: &fixture.profile,
    };
    assert!(matches!(
        load_personal_dataset_with_profile_files(
            &fixture.source,
            &fixture.trust,
            &fixture.manifest,
            &fixture.review,
            CONTEXT,
            &proof
        ),
        Err(ActiveDataError::RightsUnverified)
    ));
    assert_eq!(fixture.source.reads.get(), 0);
}

#[test]
fn checked_mass_generation_is_atomic_and_cannot_make_a_model_available() {
    let mut fixture = MassFixture::new(false);
    let loaded = fixture.load_test_authorized().unwrap();
    let ProfileRecords::MassInput(data) = loaded.records() else {
        panic!("wrong dataset type")
    };
    assert_eq!(data.elements.len(), 2);
    assert_eq!(fixture.source.reads.get(), 7);
    let descriptor = serde_json::to_vec(&json!({
        "binding_version": 1, "model_id": "fixture-model", "independent_engine_contract_id": "fixture-contract",
        "independent_engine_version": "1", "data_registrations": [{"registration_id": "mass-v3-registration", "generation": 9, "manifest_sha256": fixture.trust.manifest_sha256, "feature_schema_sha256": null}],
        "method_ids": ["fixture-method"], "convention_profile_id": "fixture", "dependency_model_ids": [],
        "availability": "available_for_declared_profile", "validation_profile_ids": [], "limitations": []
    })).unwrap();
    assert!(matches!(
        validate_model_binding(&descriptor, &hash(&descriptor), &[&loaded]),
        Err(ActiveDataError::ModelUnavailable)
    ));
    fixture
        .source
        .files
        .get_mut("parameters/elements.json")
        .unwrap()[0] ^= 1;
    assert!(matches!(
        fixture.load_test_authorized(),
        Err(ActiveDataError::DataIdentityMismatch)
    ));
    let mut missing = MassFixture::new(false);
    missing.source.files.remove("parameters/isotopes.json");
    assert!(matches!(
        missing.load_test_authorized(),
        Err(ActiveDataError::DataUnavailable)
    ));
    let invalid = MassFixture::new(true);
    assert!(matches!(
        invalid.load_test_authorized(),
        Err(ActiveDataError::ContentInvalid)
    ));
}
