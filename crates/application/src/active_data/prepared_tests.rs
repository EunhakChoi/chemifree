use std::{cell::Cell, collections::HashMap};

use serde_json::{Value, json};

use super::*;

const CONTEXT: &[u8] = include_bytes!("../data/personal-local-context.json");
const QUERY: &[u8] = include_bytes!("../../tests/data/naming-patterns-synthetic/query.jsonl");
const RETAINED: &[u8] = include_bytes!("../../tests/data/naming-patterns-synthetic/retained.jsonl");

struct Files {
    entries: HashMap<String, Vec<u8>>,
    reads: Cell<usize>,
}

impl DataSource for Files {
    fn root_binding_id(&self) -> Option<&str> {
        Some("fixture-root")
    }

    fn read(&self, path: &str, limit: u64) -> Result<Vec<u8>, crate::SourceError> {
        self.reads.set(self.reads.get() + 1);
        let bytes = self.entries.get(path).ok_or(crate::SourceError::Missing)?;
        if bytes.len() as u64 > limit {
            return Err(crate::SourceError::TooLarge);
        }
        Ok(bytes.clone())
    }
}

pub(super) struct Fixture {
    source: Files,
    manifest: Vec<u8>,
    review: Vec<u8>,
    trust: TrustedPersonalRegistration,
    registry: Vec<u8>,
    profile: Vec<u8>,
}

impl Fixture {
    pub(super) fn load(&self) -> Result<PersonalDataset, ActiveDataError> {
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
            ReviewTrust::SyntheticCatalog((&identity).into()),
        )
    }

    pub(super) fn load_production(&self) -> Result<PersonalDataset, ActiveDataError> {
        let proof = ProfileIdentityFiles {
            registry: &self.registry,
            profile: &self.profile,
        };
        load_personal_dataset_with_profile_files(
            &self.source,
            &self.trust,
            &self.manifest,
            &self.review,
            CONTEXT,
            &proof,
        )
    }

    pub(super) fn reads(&self) -> usize {
        self.source.reads.get()
    }

    pub(super) fn corrupt(&mut self, path: &str) {
        self.source.entries.get_mut(path).unwrap()[0] ^= 1;
    }

    pub(super) fn remove(&mut self, path: &str) {
        self.source.entries.remove(path).unwrap();
    }

    pub(super) fn insert(&mut self, path: &str, bytes: &[u8]) {
        self.source.entries.insert(path.into(), bytes.to_vec());
    }

    pub(super) fn bind_feature(&mut self, path: &str) {
        let mut manifest: Value = serde_json::from_slice(&self.manifest).unwrap();
        manifest["feature_schema_sha256"] = hash(self.source.entries.get(path).unwrap()).into();
        let file = manifest["files"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|file| file["relative_path"] == path)
            .unwrap();
        file["role"] = "feature_schema".into();
        self.manifest = serde_json::to_vec(&manifest).unwrap();
        let identity = validate_active_manifest(&self.manifest).unwrap();
        self.trust.manifest_sha256 = identity.canonical_sha256;
        self.trust.manifest_file_sha256 = identity.exact_sha256;
    }

    pub(super) fn set_feature_hash(&mut self, feature_hash: Option<&str>) {
        let mut manifest: Value = serde_json::from_slice(&self.manifest).unwrap();
        manifest["feature_schema_sha256"] = feature_hash.into();
        self.manifest = serde_json::to_vec(&manifest).unwrap();
        let identity = validate_active_manifest(&self.manifest).unwrap();
        self.trust.manifest_sha256 = identity.canonical_sha256;
        self.trust.manifest_file_sha256 = identity.exact_sha256;
    }

    pub(super) fn mark_optional(&mut self, path: &str) {
        let mut manifest: Value = serde_json::from_slice(&self.manifest).unwrap();
        let file = manifest["files"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|file| file["relative_path"] == path)
            .unwrap();
        file["required"] = false.into();
        self.manifest = serde_json::to_vec(&manifest).unwrap();
        let identity = validate_active_manifest(&self.manifest).unwrap();
        self.trust.manifest_sha256 = identity.canonical_sha256;
        self.trust.manifest_file_sha256 = identity.exact_sha256;
    }

    pub(super) fn set_role(&mut self, path: &str, role: &str) {
        let mut manifest: Value = serde_json::from_slice(&self.manifest).unwrap();
        let file = manifest["files"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|file| file["relative_path"] == path)
            .unwrap();
        file["role"] = role.into();
        self.manifest = serde_json::to_vec(&manifest).unwrap();
        let identity = validate_active_manifest(&self.manifest).unwrap();
        self.trust.manifest_sha256 = identity.canonical_sha256;
        self.trust.manifest_file_sha256 = identity.exact_sha256;
    }

    pub(super) fn remove_declared(&mut self, path: &str) {
        let mut manifest: Value = serde_json::from_slice(&self.manifest).unwrap();
        let files = manifest["files"].as_array_mut().unwrap();
        let index = files
            .iter()
            .position(|file| file["relative_path"] == path)
            .unwrap();
        let removed = files.remove(index);
        let mut review: Value = serde_json::from_slice(&self.review).unwrap();
        review["file_identities"]
            .as_array_mut()
            .unwrap()
            .retain(|file| file["file_id"] != removed["file_id"]);
        self.manifest = serde_json::to_vec(&manifest).unwrap();
        self.review = serde_json::to_vec(&review).unwrap();
        let identity = validate_active_manifest(&self.manifest).unwrap();
        self.trust.manifest_sha256 = identity.canonical_sha256;
        self.trust.manifest_file_sha256 = identity.exact_sha256;
        self.trust.review_sha256 = hash(&self.review);
    }

    pub(super) fn add_missing_optional(&mut self, id: &str, path: &str, bytes: &[u8]) {
        let mut manifest: Value = serde_json::from_slice(&self.manifest).unwrap();
        manifest["files"].as_array_mut().unwrap().push(json!({
            "file_id": id, "relative_path": path, "sha256": hash(bytes), "bytes": bytes.len(),
            "role": "auxiliary_data", "required": false, "declared_content_class": "declarative_data"
        }));
        let mut review: Value = serde_json::from_slice(&self.review).unwrap();
        review["file_identities"]
            .as_array_mut()
            .unwrap()
            .push(json!({
                "file_id": id, "asset_id": "fixture", "sha256": hash(bytes), "bytes": bytes.len(),
                "content_classification": "declarative_data", "classification_basis_id": "fixture"
            }));
        self.manifest = serde_json::to_vec(&manifest).unwrap();
        self.review = serde_json::to_vec(&review).unwrap();
        let identity = validate_active_manifest(&self.manifest).unwrap();
        self.trust.manifest_sha256 = identity.canonical_sha256;
        self.trust.manifest_file_sha256 = identity.exact_sha256;
        self.trust.review_sha256 = hash(&self.review);
    }
}

pub(super) fn fixture(profile_id: &str, entries: Vec<(&str, &str, Vec<u8>)>) -> Fixture {
    let (classification, role) = match profile_id {
        "private-data-molnet-xlogp-100-f64-data-v1" => ("model_parameters", "weights"),
        "private-data-molnet-supporting-numeric-literals-v1" => ("model_parameters", "calibration"),
        "private-data-chemprop-typed-parameter-tables-v1" => {
            ("model_parameters", "contribution_table")
        }
        "private-data-nmr-carriers-v1" => ("model_parameters", "auxiliary_data"),
        "private-data-biopolymer-template-candidates-v1" => {
            ("document_resource", "document_resource")
        }
        "private-data-biobyte-cmr-static-data-v1" => ("model_parameters", "weights"),
        _ => ("declarative_data", "auxiliary_data"),
    };
    let files: Vec<Value> = entries
        .iter()
        .map(|(id, path, bytes)| {
            json!({
                "file_id": id, "relative_path": path, "sha256": hash(bytes), "bytes": bytes.len(),
                "role": role, "required": true, "declared_content_class": classification
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
        "content_classification": classification, "classification_basis_id": "fixture"
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
    Fixture {
        source: Files {
            entries: entries
                .into_iter()
                .map(|(_, path, bytes)| (path.into(), bytes))
                .collect(),
            reads: Cell::new(0),
        },
        manifest,
        review,
        trust,
        registry: b"fixture-registry".to_vec(),
        profile: b"fixture-profile".to_vec(),
    }
}

fn xlogp_files(wrong_value: bool) -> Vec<(&'static str, &'static str, Vec<u8>)> {
    let raw = vec![0_u8; 800];
    let rows = (0..100).map(|index| {
        let value = if wrong_value && index == 1 { 1 } else { 0 };
        format!("{{\"storage_index_0based\":{index},\"bin_family\":\"fixture\",\"family_index_0based\":{index},\"ieee754_f64_le_hex\":\"0000000000000000\",\"value\":{value}}}\n")
    }).collect::<String>();
    vec![
        (prepared_xlogp::RAW_FILE_ID, "renamed/raw.bin", raw),
        (
            prepared_xlogp::NORMALIZED_FILE_ID,
            "renamed/records.jsonl",
            rows.into_bytes(),
        ),
    ]
}

pub(super) fn assert_binding_unavailable(fixture: &Fixture, dataset: &PersonalDataset) {
    let binding = serde_json::to_vec(&json!({
        "binding_version": 1, "model_id": "fixture-model", "independent_engine_contract_id": "fixture-contract",
        "independent_engine_version": "1", "data_registrations": [{"registration_id": "fixture-registration", "generation": 1, "manifest_sha256": fixture.trust.manifest_sha256, "feature_schema_sha256": null}],
        "method_ids": ["fixture-method"], "convention_profile_id": "fixture", "dependency_model_ids": [],
        "availability": "available_for_declared_profile", "validation_profile_ids": [], "limitations": []
    })).unwrap();
    assert!(matches!(
        validate_model_binding(&binding, &hash(&binding), &[dataset]),
        Err(ActiveDataError::ModelUnavailable)
    ));
}

#[test]
fn prepared_numeric_dispatches_by_id_and_exposes_one_atomic_dataset() {
    let mut source_fixture = fixture(
        "private-data-molnet-xlogp-100-f64-data-v1",
        xlogp_files(false),
    );
    let loaded = source_fixture.load().unwrap();
    let ProfileRecords::PreparedXlogp(rows) = loaded.records() else {
        panic!("wrong dataset type")
    };
    assert_eq!(rows.rows.len(), 100);
    assert_eq!(rows.rows[1].family_index, 1);
    assert_eq!(source_fixture.source.reads.get(), 2);
    assert_binding_unavailable(&source_fixture, &loaded);
    source_fixture
        .source
        .entries
        .get_mut("renamed/raw.bin")
        .unwrap()[0] ^= 1;
    assert!(matches!(
        source_fixture.load(),
        Err(ActiveDataError::DataIdentityMismatch)
    ));
    let mismatched = fixture(
        "private-data-molnet-xlogp-100-f64-data-v1",
        xlogp_files(true),
    );
    assert!(matches!(
        mismatched.load(),
        Err(ActiveDataError::ContentInvalid)
    ));
}

#[test]
fn pattern_profiles_preserve_unresolved_predicates_and_reject_corruption_atomically() {
    for (profile, path, payload) in [
        (
            "private-naming-query-patterns-v1",
            naming_patterns::QUERY_PATH,
            QUERY,
        ),
        (
            "private-naming-retained-patterns-v1",
            naming_patterns::RETAINED_PATH,
            RETAINED,
        ),
    ] {
        let mut fixture = fixture(profile, vec![("pattern", path, payload.to_vec())]);
        let loaded = fixture.load().unwrap();
        let ProfileRecords::NamingPatterns(patterns) = loaded.records() else {
            panic!("wrong dataset type")
        };
        match patterns.as_ref() {
            naming_patterns::PatternRows::Query(rows) => {
                assert_eq!(profile, "private-naming-query-patterns-v1");
                assert_eq!(rows[0].required_predicate_ids, ["fixture-predicate"]);
            }
            naming_patterns::PatternRows::Retained(rows) => {
                assert_eq!(profile, "private-naming-retained-patterns-v1");
                assert!(rows[0].allow_substituents);
            }
        }
        assert_binding_unavailable(&fixture, &loaded);
        fixture.source.entries.get_mut(path).unwrap()[0] ^= 1;
        assert!(matches!(
            fixture.load(),
            Err(ActiveDataError::DataIdentityMismatch)
        ));
    }
}
