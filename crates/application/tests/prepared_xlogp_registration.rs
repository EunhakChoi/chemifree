use std::cell::Cell;

use chemifree_application::{
    ActiveDataError, DataSource, ProfileIdentityFiles, SourceError, TrustedPersonalRegistration,
    load_personal_dataset_with_profile_files, validate_active_manifest,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const CONTEXT: &[u8] = include_bytes!("../src/data/personal-local-context.json");

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

fn reject_fixture_review(profile_id: &str, entries: &[(&str, &str, &[u8])]) {
    let files: Vec<Value> = entries
        .iter()
        .map(|(id, path, bytes)| {
            json!({
                "file_id": id, "relative_path": path, "sha256": hash(bytes), "bytes": bytes.len(),
                "role": "weights", "required": true, "declared_content_class": "model_parameters"
            })
        })
        .collect();
    let manifest = serde_json::to_vec(&json!({
        "manifest_version": 1, "dataset_id": "independent-prepared-shape", "dataset_version": "1",
        "family_id": "independent-fixture", "format_profile_id": profile_id,
        "independent_engine_contract_id": "fixture-contract", "source_category": "independent_synthetic",
        "files": files, "feature_schema_sha256": null, "rights_review_id": "fixture-review",
        "unknown_field_policy": "reject", "metadata": {"provider_label": "Independent fixture", "version_evidence_id": "fixture", "description": "No chemistry."}
    })).unwrap();
    let reviewed: Vec<_> = files.iter().map(|entry| json!({
        "file_id": entry["file_id"], "asset_id": "fixture", "sha256": entry["sha256"], "bytes": entry["bytes"],
        "content_classification": "model_parameters", "classification_basis_id": "fixture"
    })).collect();
    let review = serde_json::to_vec(&json!({
        "review_version": 2, "review_id": "fixture-review", "dataset_id": "independent-prepared-shape", "dataset_version": "1",
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
        dataset_id: "independent-prepared-shape".into(),
        dataset_version: "1".into(),
        family_id: "independent-fixture".into(),
        profile_id: profile_id.into(),
        engine_contract_id: "fixture-contract".into(),
        manifest_sha256: identity.canonical_sha256,
        manifest_file_sha256: identity.exact_sha256,
        review_sha256: hash(&review),
        source_registry_binding_id: "fixture-registry".into(),
        private_root_binding_id: "fixture-root".into(),
    };
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
fn fixture_review_does_not_authorize_prepared_numeric_data_in_production() {
    reject_fixture_review(
        "private-data-molnet-xlogp-100-f64-data-v1",
        &[
            (
                "DATA-RANGE-MOLNET-XLOGP-100-raw_data",
                "renamed/raw.bin",
                &[0_u8; 800],
            ),
            (
                "DATA-RANGE-MOLNET-XLOGP-100-normalized",
                "renamed/records.jsonl",
                b"independent fixture",
            ),
        ],
    );
}

#[test]
fn fixture_review_does_not_authorize_either_pattern_profile_in_production() {
    for (profile, path) in [
        (
            "private-naming-query-patterns-v1",
            "normalized/query-graph-patterns.jsonl",
        ),
        (
            "private-naming-retained-patterns-v1",
            "normalized/retained-name-patterns.jsonl",
        ),
    ] {
        reject_fixture_review(profile, &[("pattern", path, b"independent fixture")]);
    }
}
