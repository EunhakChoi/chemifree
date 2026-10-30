use std::cell::Cell;

use chemifree_application::{
    ActiveDataError, DataSource, ProfileIdentityFiles, SourceError, TrustedPersonalRegistration,
    load_personal_dataset_with_profile_files, validate_active_manifest,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const CONTEXT: &[u8] = include_bytes!("../src/data/personal-local-context.json");
const FILES: [(&str, &[u8]); 7] = [
    (
        "parameters/elements.json",
        include_bytes!("data/mass-redesign/elements.json"),
    ),
    (
        "parameters/isotopes.json",
        include_bytes!("data/mass-redesign/isotopes.json"),
    ),
    (
        "parameters/default-monoisotopic-masses.json",
        include_bytes!("data/mass-redesign/defaults.json"),
    ),
    (
        "parameters/aliases.json",
        include_bytes!("data/mass-redesign/aliases.json"),
    ),
    (
        "parameters/mass-constants.json",
        include_bytes!("data/mass-redesign/constants.json"),
    ),
    (
        "feature-schema.json",
        include_bytes!("data/mass-redesign/features.json"),
    ),
    (
        "source-bounds.json",
        include_bytes!("data/mass-redesign/bounds.json"),
    ),
];

fn digest(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
}
struct NoFiles(Cell<usize>);
impl DataSource for NoFiles {
    fn root_binding_id(&self) -> Option<&str> {
        Some("test-root")
    }
    fn read(&self, _: &str, _: u64) -> Result<Vec<u8>, SourceError> {
        self.0.set(self.0.get() + 1);
        Err(SourceError::Missing)
    }
}

fn registration(feature_digest: &str) -> (TrustedPersonalRegistration, Vec<u8>, Vec<u8>) {
    let files: Vec<Value> = FILES.iter().enumerate().map(|(index, (path, bytes))| json!({
        "file_id": format!("invented-{index}"), "relative_path": path, "sha256": digest(bytes), "bytes": bytes.len(),
        "role": "auxiliary_data", "required": true, "declared_content_class": "declarative_data"
    })).collect();
    let manifest = serde_json::to_vec(&json!({
        "manifest_version": 1, "dataset_id": "test-mass", "dataset_version": "1", "family_id": "test-family",
        "format_profile_id": "personal-local-atomic-isotope-mass-profile-v1", "independent_engine_contract_id": "test-contract",
        "source_category": "independent_synthetic", "files": files, "feature_schema_sha256": feature_digest,
        "rights_review_id": "test-review", "unknown_field_policy": "reject",
        "metadata": {"provider_label": "Synthetic", "version_evidence_id": "test", "description": "No chemistry."}
    })).unwrap();
    let reviewed: Vec<_> = files.iter().map(|entry| json!({
        "file_id": entry["file_id"], "asset_id": "test", "sha256": entry["sha256"], "bytes": entry["bytes"],
        "content_classification": "declarative_data", "classification_basis_id": "test"
    })).collect();
    let review = serde_json::to_vec(&json!({
        "review_version": 2, "review_id": "test-review", "dataset_id": "test-mass", "dataset_version": "1",
        "context_id": "personal-local-user-attested-v1", "context_sha256": digest(CONTEXT),
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
        review_sha256: digest(&review),
        source_registry_binding_id: "test-registry".into(),
        private_root_binding_id: "test-root".into(),
    };
    (trust, manifest, review)
}

fn check(feature_digest: &str) -> (ActiveDataError, usize) {
    let (trust, manifest, review) = registration(feature_digest);
    let source = NoFiles(Cell::new(0));
    let identity = ProfileIdentityFiles {
        registry: b"test-registry",
        profile: b"test-profile",
    };
    let result = load_personal_dataset_with_profile_files(
        &source, &trust, &manifest, &review, CONTEXT, &identity,
    );
    let Err(reason) = result else {
        panic!("fixture-only review must never grant personal access")
    };
    (reason, source.0.get())
}

#[test]
fn fixture_only_review_cannot_authorize_real_loading() {
    assert_eq!(
        check(&digest(FILES[5].1)),
        (ActiveDataError::RightsUnverified, 0)
    );
}

#[test]
fn feature_digest_mismatch_is_rejected_before_payload_reads() {
    assert_eq!(
        check(&"0".repeat(64)),
        (ActiveDataError::DataIdentityMismatch, 0)
    );
}
