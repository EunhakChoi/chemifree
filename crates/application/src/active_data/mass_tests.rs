use std::{cell::Cell, collections::HashMap};

use serde_json::{Value, json};

use super::*;

const CONTEXT: &[u8] = include_bytes!("../data/personal-local-context.json");
const ELEMENTS: &[u8] = include_bytes!("../../tests/data/mass-synthetic/elements.json");
const ISOTOPES: &[u8] = include_bytes!("../../tests/data/mass-synthetic/isotopes.json");
const DEFAULTS: &[u8] = include_bytes!("../../tests/data/mass-synthetic/defaults.json");
const ALIASES: &[u8] = include_bytes!("../../tests/data/mass-synthetic/aliases.json");
const CONSTANTS: &[u8] = include_bytes!("../../tests/data/mass-synthetic/constants.json");
const FEATURES: &[u8] = include_bytes!("../../tests/data/mass-synthetic/features.json");
const BOUNDS: &[u8] = include_bytes!("../../tests/data/mass-synthetic/bounds.json");
const RAW: &[u8] = b"synthetic\nrest\n";

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

struct Fixture {
    source: Files,
    manifest: Vec<u8>,
    review: Vec<u8>,
    trust: TrustedPersonalRegistration,
    registry: Vec<u8>,
    profile: Vec<u8>,
}

impl Fixture {
    fn load(&self) -> Result<PersonalDataset, ActiveDataError> {
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

fn fixture(include_raw: bool) -> Fixture {
    let mut bounds: Value = serde_json::from_slice(BOUNDS).unwrap();
    if include_raw {
        bounds["text_source"]["bytes"] = RAW.len().into();
        bounds["text_source"]["sha256"] = hash(RAW).into();
        bounds["row_source_spans"][0]["source_bytes_including_newline"] = 10.into();
        bounds["row_source_spans"][0]["source_line_sha256"] = hash(&RAW[..10]).into();
    }
    let bounds = serde_json::to_vec(&bounds).unwrap();
    let mut entries: Vec<(&str, &str, &[u8], &str, bool)> = vec![
        (
            "elements",
            "parameters/elements.json",
            ELEMENTS,
            "dictionary_entries",
            true,
        ),
        (
            "isotopes",
            "parameters/isotopes.json",
            ISOTOPES,
            "dictionary_entries",
            true,
        ),
        (
            "defaults",
            "parameters/default-monoisotopic-masses.json",
            DEFAULTS,
            "auxiliary_data",
            true,
        ),
        (
            "aliases",
            "parameters/aliases.json",
            ALIASES,
            "dictionary_entries",
            true,
        ),
        (
            "constants",
            "parameters/mass-constants.json",
            CONSTANTS,
            "auxiliary_data",
            true,
        ),
        (
            "features",
            "feature-schema.json",
            FEATURES,
            "feature_schema",
            true,
        ),
        (
            "bounds",
            "source-bounds.json",
            &bounds,
            "auxiliary_data",
            true,
        ),
    ];
    if include_raw {
        entries.push(("raw", mass_profile::RAW_PATH, RAW, "auxiliary_data", false));
    }
    let files: Vec<Value> = entries
        .iter()
        .map(|(id, path, bytes, role, required)| {
            json!({
                "file_id": id, "relative_path": path, "sha256": hash(bytes), "bytes": bytes.len(),
                "role": role, "required": required, "declared_content_class": "declarative_data"
            })
        })
        .collect();
    let manifest = serde_json::to_vec(&json!({
        "manifest_version": 1, "dataset_id": "independent-mass-shape", "dataset_version": "1",
        "family_id": "independent-fixture", "format_profile_id": "personal-local-atomic-isotope-mass-profile-v1",
        "independent_engine_contract_id": "fixture-contract", "source_category": "independent_synthetic",
        "files": files, "feature_schema_sha256": hash(FEATURES), "rights_review_id": "fixture-review",
        "unknown_field_policy": "reject", "metadata": {"provider_label": "Independent fixture", "version_evidence_id": "fixture", "description": "No chemistry."}
    })).unwrap();
    let reviewed: Vec<_> = files.iter().map(|entry| json!({
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
    Fixture {
        source: Files {
            entries: entries
                .iter()
                .map(|(_, path, bytes, _, _)| ((*path).into(), bytes.to_vec()))
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

#[test]
fn complete_mass_dataset_is_atomic_and_does_not_enable_a_model() {
    let mut fixture = fixture(false);
    let loaded = fixture.load().unwrap();
    assert_eq!(
        loaded.feature_schema_sha256(),
        Some(hash(FEATURES).as_str())
    );
    let ProfileRecords::Mass(data) = loaded.records() else {
        panic!("wrong dataset type")
    };
    assert_eq!(
        data.elements[1].natural_atomic_weight.source_decimal,
        "20.000"
    );
    assert!(!data.isotopes[2].abundance_token_present);
    assert_eq!(fixture.source.reads.get(), 7);

    let binding = serde_json::to_vec(&json!({
        "binding_version": 1, "model_id": "fixture-model", "independent_engine_contract_id": "fixture-contract",
        "independent_engine_version": "1", "data_registrations": [{"registration_id": "fixture-registration", "generation": 1, "manifest_sha256": fixture.trust.manifest_sha256, "feature_schema_sha256": hash(FEATURES)}],
        "method_ids": ["fixture-method"], "convention_profile_id": "fixture", "dependency_model_ids": [],
        "availability": "available_for_declared_profile", "validation_profile_ids": [], "limitations": []
    })).unwrap();
    assert!(matches!(
        validate_model_binding(&binding, &hash(&binding), &[&loaded]),
        Err(ActiveDataError::ModelUnavailable)
    ));
    fixture
        .source
        .entries
        .get_mut("parameters/aliases.json")
        .unwrap()[0] ^= 1;
    assert!(matches!(
        fixture.load(),
        Err(ActiveDataError::DataIdentityMismatch)
    ));

    // A matching review and hash do not make a cross-file reference valid.
    let invalid_alias = std::str::from_utf8(ALIASES)
        .unwrap()
        .replace(
            "\"source_isotope_row_ordinal\":0",
            "\"source_isotope_row_ordinal\":2",
        )
        .into_bytes();
    let invalid_hash = hash(&invalid_alias);
    fixture
        .source
        .entries
        .insert("parameters/aliases.json".into(), invalid_alias);
    let mut manifest: Value = serde_json::from_slice(&fixture.manifest).unwrap();
    manifest["files"][3]["sha256"] = invalid_hash.clone().into();
    fixture.manifest = serde_json::to_vec(&manifest).unwrap();
    let mut review: Value = serde_json::from_slice(&fixture.review).unwrap();
    review["file_identities"][3]["sha256"] = invalid_hash.into();
    fixture.review = serde_json::to_vec(&review).unwrap();
    let identity = validate_active_manifest(&fixture.manifest).unwrap();
    fixture.trust.manifest_sha256 = identity.canonical_sha256;
    fixture.trust.manifest_file_sha256 = identity.exact_sha256;
    fixture.trust.review_sha256 = hash(&fixture.review);
    assert!(matches!(
        fixture.load(),
        Err(ActiveDataError::ContentInvalid)
    ));
}

#[test]
fn listed_optional_raw_file_is_reviewed_hashed_and_checked_with_the_whole_dataset() {
    let mut fixture = fixture(true);
    let loaded = fixture.load().unwrap();
    assert!(matches!(loaded.records(), ProfileRecords::Mass(_)));
    assert_eq!(fixture.source.reads.get(), 8);
    fixture
        .source
        .entries
        .get_mut(mass_profile::RAW_PATH)
        .unwrap()[0] ^= 1;
    assert!(matches!(
        fixture.load(),
        Err(ActiveDataError::DataIdentityMismatch)
    ));
    fixture.source.entries.remove(mass_profile::RAW_PATH);
    assert!(matches!(
        fixture.load(),
        Err(ActiveDataError::DataUnavailable)
    ));
}
