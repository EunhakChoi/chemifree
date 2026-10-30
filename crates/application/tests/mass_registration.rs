use std::cell::Cell;

use chemifree_application::{
    ActiveDataError, DataSource, ProfileIdentityFiles, SourceError, TrustedPersonalRegistration,
    load_personal_dataset_with_profile_files, validate_active_manifest,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const CONTEXT: &[u8] = include_bytes!("../src/data/personal-local-context.json");
const ELEMENTS: &[u8] = include_bytes!("data/mass-synthetic/elements.json");
const ISOTOPES: &[u8] = include_bytes!("data/mass-synthetic/isotopes.json");
const DEFAULTS: &[u8] = include_bytes!("data/mass-synthetic/defaults.json");
const ALIASES: &[u8] = include_bytes!("data/mass-synthetic/aliases.json");
const CONSTANTS: &[u8] = include_bytes!("data/mass-synthetic/constants.json");
const FEATURES: &[u8] = include_bytes!("data/mass-synthetic/features.json");
const BOUNDS: &[u8] = include_bytes!("data/mass-synthetic/bounds.json");

struct Unreadable(Cell<usize>);

impl DataSource for Unreadable {
    fn root_binding_id(&self) -> Option<&str> {
        Some("fixture-root")
    }
    fn read(&self, _: &str, _: u64) -> Result<Vec<u8>, SourceError> {
        self.0.set(self.0.get() + 1);
        Err(SourceError::Missing)
    }
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn fixture(feature_hash: &str) -> (TrustedPersonalRegistration, Vec<u8>, Vec<u8>) {
    let entries = [
        (
            "elements",
            "parameters/elements.json",
            ELEMENTS,
            "dictionary_entries",
        ),
        (
            "isotopes",
            "parameters/isotopes.json",
            ISOTOPES,
            "dictionary_entries",
        ),
        (
            "defaults",
            "parameters/default-monoisotopic-masses.json",
            DEFAULTS,
            "auxiliary_data",
        ),
        (
            "aliases",
            "parameters/aliases.json",
            ALIASES,
            "dictionary_entries",
        ),
        (
            "constants",
            "parameters/mass-constants.json",
            CONSTANTS,
            "auxiliary_data",
        ),
        (
            "features",
            "feature-schema.json",
            FEATURES,
            "feature_schema",
        ),
        ("bounds", "source-bounds.json", BOUNDS, "auxiliary_data"),
    ];
    let files: Vec<Value> = entries
        .iter()
        .map(|(id, path, bytes, role)| {
            json!({
                "file_id": id, "relative_path": path, "sha256": hash(bytes), "bytes": bytes.len(),
                "role": role, "required": true, "declared_content_class": "declarative_data"
            })
        })
        .collect();
    let manifest = serde_json::to_vec(&json!({
        "manifest_version": 1, "dataset_id": "independent-mass-shape", "dataset_version": "1",
        "family_id": "independent-fixture", "format_profile_id": "personal-local-atomic-isotope-mass-profile-v1",
        "independent_engine_contract_id": "fixture-contract", "source_category": "independent_synthetic",
        "files": files, "feature_schema_sha256": feature_hash, "rights_review_id": "fixture-review",
        "unknown_field_policy": "reject", "metadata": {"provider_label": "Independent fixture", "version_evidence_id": "fixture", "description": "No chemistry."}
    })).unwrap();
    let reviewed: Vec<Value> = files.iter().map(|entry| json!({
        "file_id": entry["file_id"], "asset_id": "fixture", "sha256": entry["sha256"], "bytes": entry["bytes"],
        "content_classification": "declarative_data", "classification_basis_id": "fixture"
    })).collect();
    let review = serde_json::to_vec(&json!({
        "review_version": 2, "review_id": "fixture-review", "dataset_id": "independent-mass-shape", "dataset_version": "1",
        "context_id": "personal-local-user-attested-v1", "context_sha256": hash(CONTEXT),
        "approval_basis": "user_attested_personal_permission", "attestation_reference_id": "fixture-only",
        "authorization_status": "approved_personal_local", "source_registry_binding_id": "fixture-registry",
        "private_root_binding_id": "fixture-root", "file_identities": reviewed,
        "allowed_operations": ["local_load", "decode"], "prohibited_operations": ["redistribution", "remote_processing", "original_module_execution", "secret_lookup", "public_vendor_payload_copy"],
        "approved_by": "fixture-author", "additional_licensor_written_document_required": false,
        "documentary_verification_claimed": false, "technical_availability_claimed": false, "fixture_only": true
    })).unwrap();
    let identity = validate_active_manifest(&manifest).unwrap();
    let trust = TrustedPersonalRegistration {
        registration_id: "fixture-registration".into(),
        generation: 1,
        dataset_id: "independent-mass-shape".into(),
        dataset_version: "1".into(),
        family_id: "independent-fixture".into(),
        profile_id: "personal-local-atomic-isotope-mass-profile-v1".into(),
        engine_contract_id: "fixture-contract".into(),
        manifest_sha256: identity.canonical_sha256,
        manifest_file_sha256: identity.exact_sha256,
        review_sha256: hash(&review),
        source_registry_binding_id: "fixture-registry".into(),
        private_root_binding_id: "fixture-root".into(),
    };
    (trust, manifest, review)
}

#[test]
fn fixture_only_review_is_not_authorization_for_production_mass_loading() {
    let (trust, manifest, review) = fixture(&hash(FEATURES));
    let source = Unreadable(Cell::new(0));
    let proof = ProfileIdentityFiles {
        registry: b"fixture-registry",
        profile: b"fixture-profile",
    };
    let error = match load_personal_dataset_with_profile_files(
        &source, &trust, &manifest, &review, CONTEXT, &proof,
    ) {
        Err(error) => error,
        Ok(_) => panic!("a fixture-only review must not be accepted"),
    };
    assert_eq!(error, ActiveDataError::RightsUnverified);
    assert_eq!(source.0.get(), 0);
}

#[test]
fn mass_feature_schema_hash_must_identify_the_feature_file_before_reading() {
    let (trust, manifest, review) = fixture(&"0".repeat(64));
    let source = Unreadable(Cell::new(0));
    let proof = ProfileIdentityFiles {
        registry: b"fixture-registry",
        profile: b"fixture-profile",
    };
    let error = match load_personal_dataset_with_profile_files(
        &source, &trust, &manifest, &review, CONTEXT, &proof,
    ) {
        Err(error) => error,
        Ok(_) => panic!("a mismatched feature schema must not be accepted"),
    };
    assert_eq!(error, ActiveDataError::DataIdentityMismatch);
    assert_eq!(source.0.get(), 0);
}
