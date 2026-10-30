//! Validation of the public external-DATA manifest and its canonical identity.

use std::collections::HashSet;

use chemifree_domain::data;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{DataProfile, DataSource, EngineState, ProfileRecords, decode_profile};
use crate::{json, nmr_profile, phase6_profiles};

/// Typed failure reasons for external registration and model-binding validation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActiveDataError {
    ManifestInvalid,
    RegistrationMismatch,
    ProfileUnsupported,
    RightsUnverified,
    DataUnavailable,
    ContentInvalid,
    DataIdentityMismatch,
    BindingUntrusted,
    BindingInvalid,
    ModelUnavailable,
    RegistryInvalid,
    RegistryUnavailable,
    RegistryConfigurationIncomplete,
}

impl ActiveDataError {
    /// Stable payload-free status code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::ManifestInvalid => "manifest_invalid",
            Self::RegistrationMismatch => "registration_mismatch",
            Self::ProfileUnsupported => "profile_unsupported",
            Self::RightsUnverified => "rights_unverified",
            Self::DataUnavailable => "data_unavailable",
            Self::ContentInvalid => "content_invalid",
            Self::DataIdentityMismatch => "data_identity_mismatch",
            Self::BindingUntrusted => "binding_untrusted",
            Self::BindingInvalid => "binding_invalid",
            Self::ModelUnavailable => "model_unavailable",
            Self::RegistryInvalid => "registry_invalid",
            Self::RegistryUnavailable => "registry_unavailable",
            Self::RegistryConfigurationIncomplete => "registry_configuration_incomplete",
        }
    }
}

impl std::fmt::Display for ActiveDataError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.code())
    }
}

impl std::error::Error for ActiveDataError {}

#[derive(Clone)]
enum ReviewTrust {
    Production,
    #[cfg(test)]
    SyntheticFixture,
    #[cfg(test)]
    SyntheticPhase6(FixtureCatalogIdentity),
}

impl ReviewTrust {
    const fn allows_fixture(&self) -> bool {
        match self {
            Self::Production => false,
            #[cfg(test)]
            Self::SyntheticFixture | Self::SyntheticPhase6(_) => true,
        }
    }
}

#[cfg(test)]
#[derive(Clone)]
struct FixtureCatalogIdentity {
    dataset_id: String,
    dataset_version: String,
    registry_sha256: String,
    profile_sha256: String,
    manifest_file_sha256: String,
}

#[cfg(test)]
impl From<&phase6_profiles::CatalogIdentity<'_>> for FixtureCatalogIdentity {
    fn from(identity: &phase6_profiles::CatalogIdentity<'_>) -> Self {
        Self {
            dataset_id: identity.dataset_id.into(),
            dataset_version: identity.dataset_version.into(),
            registry_sha256: identity.registry_sha256.into(),
            profile_sha256: identity.profile_sha256.into(),
            manifest_file_sha256: identity.manifest_file_sha256.into(),
        }
    }
}

/// Exact source registry and format-profile bytes for a pinned phase 6 profile.
pub struct ProfileIdentityFiles<'a> {
    pub registry: &'a [u8],
    pub profile: &'a [u8],
}

pub struct ManifestIdentity {
    pub canonical_sha256: String,
    pub exact_sha256: String,
}

/// Identity and review anchors supplied by a trusted, explicitly selected registry, not by
/// the dataset manifest or an untrusted request.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrustedPersonalRegistration {
    pub registration_id: String,
    pub generation: u64,
    pub dataset_id: String,
    pub dataset_version: String,
    pub family_id: String,
    pub profile_id: String,
    pub engine_contract_id: String,
    pub manifest_sha256: String,
    pub manifest_file_sha256: String,
    pub review_sha256: String,
    pub source_registry_binding_id: String,
    pub private_root_binding_id: String,
}

/// A single completely verified, immutable registration generation.
pub struct PersonalDataset {
    registration_id: String,
    generation: u64,
    manifest_sha256: String,
    feature_schema_sha256: Option<String>,
    engine_contract_id: String,
    records: ProfileRecords,
}

/// A descriptor whose data identities were checked; this is not an implemented model.
pub struct ValidatedModelBinding {
    model_id: String,
}

impl ValidatedModelBinding {
    #[must_use]
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Data registration alone does not supply a chemistry engine.
    #[must_use]
    pub fn engine_status(&self) -> EngineState {
        EngineState::NotImplemented
    }
}

/// Validates a trusted descriptor against complete immutable dataset generations.
///
/// # Errors
/// Rejects untrusted, malformed, missing, mismatched, or unsupported bindings.
pub fn validate_model_binding(
    bytes: &[u8],
    trusted_exact_sha256: &str,
    datasets: &[&PersonalDataset],
) -> Result<ValidatedModelBinding, ActiveDataError> {
    if bytes.len() as u64 > crate::data::MAX_MANIFEST_BYTES
        || !is_hash(trusted_exact_sha256)
        || hash(bytes) != trusted_exact_sha256
    {
        return Err(ActiveDataError::BindingUntrusted);
    }
    let value =
        json::parse_integer_zero_strict(bytes).map_err(|_| ActiveDataError::BindingInvalid)?;
    if !integer_numbers_only(&value)
        || value
            .get("data_registrations")
            .and_then(Value::as_array)
            .is_none_or(|registrations| {
                registrations
                    .iter()
                    .any(|r| r.get("feature_schema_sha256").is_none())
            })
    {
        return Err(ActiveDataError::BindingInvalid);
    }
    let binding: ModelBinding =
        serde_json::from_value(value).map_err(|_| ActiveDataError::BindingInvalid)?;
    let distinct_nonempty = |values: &[String]| {
        let unique: HashSet<_> = values.iter().collect();
        unique.len() == values.len() && values.iter().all(|value| !value.is_empty())
    };
    if binding.binding_version != 1
        || binding.model_id.is_empty()
        || binding.independent_engine_contract_id.is_empty()
        || binding.independent_engine_version.is_empty()
        || binding.convention_profile_id.is_empty()
        || binding.data_registrations.is_empty()
        || binding.method_ids.is_empty()
        || !distinct_nonempty(&binding.method_ids)
        || !distinct_nonempty(&binding.dependency_model_ids)
        || !distinct_nonempty(&binding.validation_profile_ids)
        || binding.limitations.iter().any(String::is_empty)
        || !matches!(
            binding.availability.as_str(),
            "unavailable" | "available_for_declared_profile"
        )
    {
        return Err(ActiveDataError::BindingInvalid);
    }
    if binding.availability == "unavailable" || !binding.dependency_model_ids.is_empty() {
        return Err(ActiveDataError::ModelUnavailable);
    }
    let mut seen = HashSet::new();
    for registration in &binding.data_registrations {
        if registration.registration_id.is_empty()
            || !seen.insert(&registration.registration_id)
            || !is_hash(&registration.manifest_sha256)
            || registration
                .feature_schema_sha256
                .as_deref()
                .is_some_and(|value| !is_hash(value))
        {
            return Err(ActiveDataError::BindingInvalid);
        }
        let mut matches = datasets
            .iter()
            .filter(|dataset| dataset.registration_id == registration.registration_id);
        let Some(dataset) = matches.next() else {
            return Err(ActiveDataError::ModelUnavailable);
        };
        if matches.next().is_some()
            || dataset.generation != registration.generation
            || dataset.manifest_sha256 != registration.manifest_sha256
            || dataset.engine_contract_id != binding.independent_engine_contract_id
            || registration.feature_schema_sha256.is_some()
        {
            return Err(ActiveDataError::ModelUnavailable);
        }
    }
    Ok(ValidatedModelBinding {
        model_id: binding.model_id,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelBinding {
    binding_version: u64,
    model_id: String,
    independent_engine_contract_id: String,
    independent_engine_version: String,
    data_registrations: Vec<ModelRegistration>,
    method_ids: Vec<String>,
    convention_profile_id: String,
    dependency_model_ids: Vec<String>,
    availability: String,
    validation_profile_ids: Vec<String>,
    limitations: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelRegistration {
    registration_id: String,
    generation: u64,
    manifest_sha256: String,
    feature_schema_sha256: Option<String>,
}

impl PersonalDataset {
    #[must_use]
    pub fn registration_id(&self) -> &str {
        &self.registration_id
    }
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation
    }
    #[must_use]
    pub fn manifest_sha256(&self) -> &str {
        &self.manifest_sha256
    }
    /// The verified exact feature-schema file digest, if this profile has one.
    #[must_use]
    pub fn feature_schema_sha256(&self) -> Option<&str> {
        self.feature_schema_sha256.as_deref()
    }
    #[must_use]
    pub fn records(&self) -> &ProfileRecords {
        &self.records
    }
}

/// Loads a supported profile using trusted registry and review identities. The caller must
/// acquire the registry anchors independently of the selected data directory.
///
/// # Errors
/// Returns a payload-free reason; a failed registration exposes no partial contents.
pub fn load_personal_dataset(
    source: &dyn DataSource,
    trust: &TrustedPersonalRegistration,
    manifest: &[u8],
    review: &[u8],
    context: &[u8],
) -> Result<PersonalDataset, ActiveDataError> {
    load_personal_impl(
        source,
        trust,
        manifest,
        review,
        context,
        ReviewTrust::Production,
    )
}

/// Loads a supported phase 6 profile after verifying its catalog-pinned metadata files.
///
/// # Errors
/// Returns a payload-free reason; no incomplete registration is returned.
pub fn load_personal_dataset_with_profile_files(
    source: &dyn DataSource,
    trust: &TrustedPersonalRegistration,
    manifest: &[u8],
    review: &[u8],
    context: &[u8],
    files: &ProfileIdentityFiles<'_>,
) -> Result<PersonalDataset, ActiveDataError> {
    load_personal_impl_with_metadata(
        source,
        trust,
        manifest,
        review,
        context,
        Some(files),
        ReviewTrust::Production,
    )
}

fn load_personal_impl(
    source: &dyn DataSource,
    trust: &TrustedPersonalRegistration,
    manifest_bytes: &[u8],
    review_bytes: &[u8],
    context: &[u8],
    review_trust: ReviewTrust,
) -> Result<PersonalDataset, ActiveDataError> {
    load_personal_impl_with_metadata(
        source,
        trust,
        manifest_bytes,
        review_bytes,
        context,
        None,
        review_trust,
    )
}

#[derive(Clone, Copy)]
enum SelectedProfile {
    Phase7(DataProfile),
    Phase6(phase6_profiles::Profile),
    Nmr,
}

fn select_profile(manifest: &Manifest) -> Result<SelectedProfile, ActiveDataError> {
    let phase7 = match manifest.format_profile_id.as_str() {
        "private-naming-ring-element-ranks-v1" => (
            DataProfile::RingRank,
            "naming-ring-element-rank-operands",
            "normalized/ring-element-order-c.jsonl",
        ),
        "private-naming-valence-reference-v1" => (
            DataProfile::ValenceReference,
            "naming-valence-reference-operands",
            "normalized/valence-reference.jsonl",
        ),
        "private-naming-element-endpoint-v1" => (
            DataProfile::ElementEndpoint,
            "naming-element-rank-endpoint-operands",
            "normalized/element-order-a-hydrogen.jsonl",
        ),
        "personal-local-nmr-coupling-parameters-v1" => {
            if manifest.files.len() != nmr_profile::REQUIRED_FILES.len()
                || manifest.files.iter().any(|file| {
                    !file.required
                        || !nmr_profile::REQUIRED_FILES.contains(&file.relative_path.as_str())
                        || !matches!(
                            file.declared_content_class.as_str(),
                            "declarative_data" | "model_parameters"
                        )
                })
            {
                return Err(ActiveDataError::ProfileUnsupported);
            }
            let feature = manifest
                .files
                .iter()
                .find(|file| file.relative_path == nmr_profile::FEATURES_PATH)
                .ok_or(ActiveDataError::ProfileUnsupported)?;
            if manifest.feature_schema_sha256.as_deref() != Some(feature.sha256.as_str()) {
                return Err(ActiveDataError::DataIdentityMismatch);
            }
            return Ok(SelectedProfile::Nmr);
        }
        other => {
            let profile = phase6_profiles::Profile::parse(other)
                .ok_or(ActiveDataError::ProfileUnsupported)?;
            if manifest.feature_schema_sha256.is_some()
                || manifest.files.len() != profile.required_file_count()
                || manifest.files.iter().any(|file| {
                    !file.required
                        || profile.file_kind(&file.relative_path).is_none()
                        || !matches!(
                            file.declared_content_class.as_str(),
                            "declarative_data" | "model_parameters"
                        )
                })
            {
                return Err(ActiveDataError::ProfileUnsupported);
            }
            return Ok(SelectedProfile::Phase6(profile));
        }
    };
    let (profile, family, path) = phase7;
    if manifest.family_id != family
        || manifest.files.len() != 1
        || manifest.feature_schema_sha256.is_some()
    {
        return Err(ActiveDataError::ProfileUnsupported);
    }
    let file = &manifest.files[0];
    if file.relative_path != path
        || !file.required
        || file.declared_content_class != "declarative_data"
        || (profile == DataProfile::RingRank && file.role != "dictionary_entries")
    {
        return Err(ActiveDataError::ProfileUnsupported);
    }
    Ok(SelectedProfile::Phase7(profile))
}

fn load_personal_impl_with_metadata(
    source: &dyn DataSource,
    trust: &TrustedPersonalRegistration,
    manifest_bytes: &[u8],
    review_bytes: &[u8],
    context: &[u8],
    metadata: Option<&ProfileIdentityFiles<'_>>,
    review_trust: ReviewTrust,
) -> Result<PersonalDataset, ActiveDataError> {
    let (manifest, identity) = parse_manifest(manifest_bytes)?;
    if trust.registration_id.is_empty()
        || identity.exact_sha256 != trust.manifest_file_sha256
        || identity.canonical_sha256 != trust.manifest_sha256
        || manifest.dataset_id != trust.dataset_id
        || manifest.dataset_version != trust.dataset_version
        || manifest.family_id != trust.family_id
        || manifest.format_profile_id != trust.profile_id
        || manifest.independent_engine_contract_id != trust.engine_contract_id
    {
        return Err(ActiveDataError::RegistrationMismatch);
    }
    let selected = select_profile(&manifest)?;
    if metadata.is_some() && matches!(selected, SelectedProfile::Phase7(_)) {
        return Err(ActiveDataError::ProfileUnsupported);
    }
    const PERSONAL_CONTEXT_SHA256: &str =
        "c49536c0e285094acb5f354bee58b0436007e98529b1d2840e0ae595a9feaa25";
    if hash(context) != PERSONAL_CONTEXT_SHA256
        || !is_hash(&trust.review_sha256)
        || review_bytes.len() as u64 > crate::data::MAX_MANIFEST_BYTES
        || hash(review_bytes) != trust.review_sha256
    {
        return Err(ActiveDataError::RightsUnverified);
    }
    let review_value =
        json::parse_strict(review_bytes).map_err(|_| ActiveDataError::RightsUnverified)?;
    let review: PersonalReview =
        serde_json::from_value(review_value).map_err(|_| ActiveDataError::RightsUnverified)?;
    review.validate(&manifest, trust, PERSONAL_CONTEXT_SHA256, &review_trust)?;
    if source.root_binding_id() != Some(trust.private_root_binding_id.as_str()) {
        return Err(ActiveDataError::RightsUnverified);
    }

    if !matches!(selected, SelectedProfile::Phase7(_)) {
        let proof = metadata.ok_or(ActiveDataError::ProfileUnsupported)?;
        #[cfg(test)]
        let verify = match &review_trust {
            ReviewTrust::SyntheticPhase6(fixture) => {
                let expected = phase6_profiles::CatalogIdentity {
                    dataset_id: &fixture.dataset_id,
                    dataset_version: &fixture.dataset_version,
                    registry_sha256: &fixture.registry_sha256,
                    profile_sha256: &fixture.profile_sha256,
                    manifest_file_sha256: &fixture.manifest_file_sha256,
                };
                if manifest.dataset_id != expected.dataset_id
                    || manifest.dataset_version != expected.dataset_version
                {
                    Err(ActiveDataError::RegistrationMismatch)
                } else {
                    expected
                        .check_file_bytes(manifest_bytes, proof.registry, proof.profile)
                        .map_err(|_| ActiveDataError::RegistrationMismatch)
                }
            }
            _ => verify_catalog(selected, &manifest, manifest_bytes, proof),
        };
        #[cfg(not(test))]
        let verify = verify_catalog(selected, &manifest, manifest_bytes, proof);
        verify?;
    }

    // Read once after authorization, then hash and decode the same immutable bytes.
    let mut payloads = Vec::with_capacity(manifest.files.len());
    for file in &manifest.files {
        let bytes = source
            .read(&file.relative_path, file.bytes)
            .map_err(|_| ActiveDataError::DataUnavailable)?;
        if data::has_executable_extension(&file.relative_path)
            || data::executable_signature(&bytes).is_some()
        {
            return Err(ActiveDataError::ContentInvalid);
        }
        if bytes.len() as u64 != file.bytes || hash(&bytes) != file.sha256 {
            return Err(ActiveDataError::DataIdentityMismatch);
        }
        payloads.push((file.relative_path.as_str(), bytes));
    }
    let records = match selected {
        SelectedProfile::Phase7(profile) => {
            decode_profile(profile, &payloads[0].1).map_err(|_| ActiveDataError::ContentInvalid)?
        }
        SelectedProfile::Phase6(profile) => {
            let files: Vec<_> = payloads
                .iter()
                .map(|(path, bytes)| (*path, bytes.as_slice()))
                .collect();
            ProfileRecords::Phase6(
                phase6_profiles::decode_dataset(profile, &files)
                    .map_err(|_| ActiveDataError::ContentInvalid)?,
            )
        }
        SelectedProfile::Nmr => {
            let files: Vec<_> = payloads
                .iter()
                .map(|(path, bytes)| (*path, bytes.as_slice()))
                .collect();
            ProfileRecords::Nmr(Box::new(
                nmr_profile::decode_nmr(&files).map_err(|_| ActiveDataError::ContentInvalid)?,
            ))
        }
    };
    Ok(PersonalDataset {
        registration_id: trust.registration_id.clone(),
        generation: trust.generation,
        manifest_sha256: identity.canonical_sha256,
        feature_schema_sha256: manifest.feature_schema_sha256,
        engine_contract_id: manifest.independent_engine_contract_id,
        records,
    })
}

fn verify_catalog(
    selected: SelectedProfile,
    manifest: &Manifest,
    manifest_bytes: &[u8],
    proof: &ProfileIdentityFiles<'_>,
) -> Result<(), ActiveDataError> {
    match selected {
        SelectedProfile::Phase6(profile) => profile
            .verify_files(
                &manifest.dataset_id,
                &manifest.dataset_version,
                manifest_bytes,
                proof.registry,
                proof.profile,
            )
            .map_err(|_| ActiveDataError::RegistrationMismatch),
        SelectedProfile::Nmr => nmr_profile::verify_catalog_files(
            &manifest.dataset_id,
            &manifest.dataset_version,
            manifest_bytes,
            proof.registry,
            proof.profile,
        )
        .map_err(|_| ActiveDataError::RegistrationMismatch),
        SelectedProfile::Phase7(_) => Err(ActiveDataError::ProfileUnsupported),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PersonalReview {
    review_version: u64,
    review_id: String,
    dataset_id: String,
    dataset_version: String,
    context_id: String,
    context_sha256: String,
    approval_basis: String,
    attestation_reference_id: String,
    authorization_status: String,
    source_registry_binding_id: String,
    private_root_binding_id: String,
    file_identities: Vec<ReviewedFile>,
    allowed_operations: Vec<String>,
    prohibited_operations: Vec<String>,
    approved_by: String,
    additional_licensor_written_document_required: bool,
    documentary_verification_claimed: bool,
    technical_availability_claimed: bool,
    fixture_only: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewedFile {
    file_id: String,
    asset_id: String,
    sha256: String,
    bytes: u64,
    content_classification: String,
    classification_basis_id: String,
}

impl PersonalReview {
    fn validate(
        &self,
        manifest: &Manifest,
        trust: &TrustedPersonalRegistration,
        context_hash: &str,
        review_trust: &ReviewTrust,
    ) -> Result<(), ActiveDataError> {
        let allowed: HashSet<&str> = self.allowed_operations.iter().map(String::as_str).collect();
        let valid_operations = [
            "metadata_inspection",
            "local_load",
            "decode",
            "normalization",
            "inference",
            "private_cache",
            "private_result_use",
        ];
        let prohibited = [
            "redistribution",
            "remote_processing",
            "original_module_execution",
            "secret_lookup",
            "public_vendor_payload_copy",
        ];
        if self.review_version != 2
            || self.review_id != manifest.rights_review_id
            || self.dataset_id != manifest.dataset_id
            || self.dataset_version != manifest.dataset_version
            || self.context_id != "personal-local-user-attested-v1"
            || self.context_sha256 != context_hash
            || self.approval_basis != "user_attested_personal_permission"
            || self.attestation_reference_id.is_empty()
            || self.authorization_status != "approved_personal_local"
            || self.source_registry_binding_id != trust.source_registry_binding_id
            || self.private_root_binding_id != trust.private_root_binding_id
            || self.source_registry_binding_id.is_empty()
            || self.private_root_binding_id.is_empty()
            || self.approved_by.is_empty()
            || self.additional_licensor_written_document_required
            || self.documentary_verification_claimed
            || self.technical_availability_claimed
            || (self.fixture_only && !review_trust.allows_fixture())
            || allowed.len() != self.allowed_operations.len()
            || !allowed.contains("local_load")
            || !allowed.contains("decode")
            || !allowed.iter().all(|op| valid_operations.contains(op))
            || self
                .prohibited_operations
                .iter()
                .map(String::as_str)
                .ne(prohibited)
            || self.file_identities.len() != manifest.files.len()
        {
            return Err(ActiveDataError::RightsUnverified);
        }
        let mut seen = HashSet::new();
        for reviewed in &self.file_identities {
            let Some(file) = manifest
                .files
                .iter()
                .find(|file| file.file_id == reviewed.file_id)
            else {
                return Err(ActiveDataError::RightsUnverified);
            };
            if !seen.insert(&reviewed.file_id)
                || reviewed.asset_id.is_empty()
                || reviewed.classification_basis_id.is_empty()
                || reviewed.sha256 != file.sha256
                || reviewed.bytes != file.bytes
                || reviewed.content_classification != file.declared_content_class
                || !matches!(
                    reviewed.content_classification.as_str(),
                    "declarative_data" | "model_parameters" | "document_resource"
                )
            {
                return Err(ActiveDataError::RightsUnverified);
            }
        }
        Ok(())
    }
}

/// Validates a manifest and returns canonical and exact-file identities separately.
///
/// # Errors
/// Rejects invalid schema, number spelling, duplicate keys and malformed identities.
pub fn validate_active_manifest(bytes: &[u8]) -> Result<ManifestIdentity, ActiveDataError> {
    let (_, identity) = parse_manifest(bytes)?;
    Ok(identity)
}

// ManifestV1 is closed under both policies; retain-opaque applies only to payload metadata
// that the selected profile explicitly permits, never to unknown manifest members.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    manifest_version: u64,
    dataset_id: String,
    dataset_version: String,
    family_id: String,
    format_profile_id: String,
    independent_engine_contract_id: String,
    source_category: String,
    files: Vec<ManifestFile>,
    feature_schema_sha256: Option<String>,
    rights_review_id: String,
    unknown_field_policy: String,
    metadata: ManifestMetadata,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestFile {
    file_id: String,
    relative_path: String,
    sha256: String,
    bytes: u64,
    role: String,
    required: bool,
    declared_content_class: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestMetadata {
    provider_label: String,
    version_evidence_id: String,
    description: String,
}

fn parse_manifest(bytes: &[u8]) -> Result<(Manifest, ManifestIdentity), ActiveDataError> {
    if bytes.len() as u64 > crate::data::MAX_MANIFEST_BYTES || bytes.starts_with(b"\xef\xbb\xbf") {
        return Err(ActiveDataError::ManifestInvalid);
    }
    // serde_json represents -0 as floating point. It is a legal integer token in this
    // contract, so normalize that token only, before parsing; exact-file identity uses bytes.
    let mut value =
        json::parse_integer_zero_strict(bytes).map_err(|_| ActiveDataError::ManifestInvalid)?;
    if !integer_numbers_only(&value) || value.get("feature_schema_sha256").is_none() {
        return Err(ActiveDataError::ManifestInvalid);
    }
    let manifest: Manifest =
        serde_json::from_value(value.clone()).map_err(|_| ActiveDataError::ManifestInvalid)?;
    manifest.validate()?;
    value.sort_all_objects();
    let canonical = serde_json::to_vec(&value).map_err(|_| ActiveDataError::ManifestInvalid)?;
    Ok((
        manifest,
        ManifestIdentity {
            canonical_sha256: hash(&canonical),
            exact_sha256: hash(bytes),
        },
    ))
}

impl Manifest {
    fn validate(&self) -> Result<(), ActiveDataError> {
        let text = |value: &str| {
            !value.is_empty()
                && value.chars().count() <= 1000
                && !value.chars().any(char::is_control)
        };
        if self.manifest_version != 1
            || !text(&self.dataset_id)
            || !text(&self.dataset_version)
            || !text(&self.family_id)
            || !text(&self.format_profile_id)
            || !text(&self.independent_engine_contract_id)
            || !text(&self.rights_review_id)
            || !text(&self.metadata.provider_label)
            || !text(&self.metadata.version_evidence_id)
            || self.metadata.description.chars().count() > 1000
            || !matches!(
                self.source_category.as_str(),
                "separately_supplied_legacy_data"
                    | "approved_external_data"
                    | "user_owned_observations"
                    | "independent_synthetic"
            )
            || !matches!(
                self.unknown_field_policy.as_str(),
                "reject" | "retain_opaque_and_report"
            )
            || self
                .feature_schema_sha256
                .as_deref()
                .is_some_and(|digest| !is_hash(digest))
            || self.files.is_empty()
            || self.files.len() > crate::data::MAX_DATA_FILES
        {
            return Err(ActiveDataError::ManifestInvalid);
        }
        let mut ids = HashSet::new();
        let mut paths = HashSet::new();
        let mut total = 0_u64;
        for file in &self.files {
            if !text(&file.file_id)
                || !ids.insert(&file.file_id)
                || !paths.insert(&file.relative_path)
                || data::check_relative_path(&file.relative_path).is_err()
                || !is_hash(&file.sha256)
                || file.bytes > crate::data::MAX_DATA_FILE_BYTES
                || !matches!(
                    file.role.as_str(),
                    "dictionary_entries"
                        | "fragment_reference"
                        | "contribution_table"
                        | "weights"
                        | "feature_schema"
                        | "calibration"
                        | "user_corrections"
                        | "document_resource"
                        | "auxiliary_data"
                )
                || !matches!(
                    file.declared_content_class.as_str(),
                    "declarative_data" | "model_parameters" | "document_resource" | "ambiguous"
                )
            {
                return Err(ActiveDataError::ManifestInvalid);
            }
            total = total
                .checked_add(file.bytes)
                .ok_or(ActiveDataError::ManifestInvalid)?;
        }
        if total > crate::data::MAX_DATA_TOTAL_BYTES {
            return Err(ActiveDataError::ManifestInvalid);
        }
        Ok(())
    }
}

fn is_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn integer_numbers_only(value: &Value) -> bool {
    match value {
        Value::Number(n) => n.is_i64() || n.is_u64(),
        Value::Array(items) => items.iter().all(integer_numbers_only),
        Value::Object(fields) => fields.values().all(integer_numbers_only),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::Cell;

    const MANIFEST: &str = include_str!("../tests/data/synthetic-active-manifest.json");
    const CONTEXT: &[u8] = include_bytes!("data/personal-local-context.json");

    struct SyntheticSource {
        bytes: Vec<u8>,
        reads: Cell<usize>,
    }
    impl DataSource for SyntheticSource {
        fn root_binding_id(&self) -> Option<&str> {
            Some("fixture-root")
        }

        fn read(&self, path: &str, limit: u64) -> Result<Vec<u8>, crate::SourceError> {
            self.reads.set(self.reads.get() + 1);
            if path != "normalized/element-order-a-hydrogen.jsonl" {
                return Err(crate::SourceError::Missing);
            }
            if self.bytes.len() as u64 > limit {
                return Err(crate::SourceError::TooLarge);
            }
            Ok(self.bytes.clone())
        }
    }

    fn synthetic_registration() -> (
        SyntheticSource,
        TrustedPersonalRegistration,
        Vec<u8>,
        serde_json::Value,
    ) {
        let source = SyntheticSource {
            bytes: b"{\"atomic_number_u32\":1,\"source_rank_u32\":123}\n".to_vec(),
            reads: Cell::new(0),
        };
        let data_hash = hash(&source.bytes);
        let manifest = json!({
            "manifest_version": 1, "dataset_id": "synthetic-endpoint", "dataset_version": "1",
            "family_id": "naming-element-rank-endpoint-operands", "format_profile_id": "private-naming-element-endpoint-v1",
            "independent_engine_contract_id": "synthetic-contract", "source_category": "independent_synthetic",
            "files": [{"file_id": "endpoint", "relative_path": "normalized/element-order-a-hydrogen.jsonl", "sha256": data_hash, "bytes": source.bytes.len(), "role": "dictionary_entries", "required": true, "declared_content_class": "declarative_data"}],
            "feature_schema_sha256": null, "rights_review_id": "fixture-review", "unknown_field_policy": "reject",
            "metadata": {"provider_label": "Independent fixture", "version_evidence_id": "synthetic-v1", "description": "No chemical meaning."}
        });
        let manifest = serde_json::to_vec(&manifest).unwrap();
        let review = json!({
            "review_version": 2, "review_id": "fixture-review", "dataset_id": "synthetic-endpoint", "dataset_version": "1",
            "context_id": "personal-local-user-attested-v1", "context_sha256": hash(CONTEXT),
            "approval_basis": "user_attested_personal_permission", "attestation_reference_id": "fixture-only",
            "authorization_status": "approved_personal_local", "source_registry_binding_id": "fixture-registry",
            "private_root_binding_id": "fixture-root", "file_identities": [{"file_id": "endpoint", "asset_id": "fixture-asset", "sha256": data_hash, "bytes": source.bytes.len(), "content_classification": "declarative_data", "classification_basis_id": "fixture-shape"}],
            "allowed_operations": ["local_load", "decode"], "prohibited_operations": ["redistribution", "remote_processing", "original_module_execution", "secret_lookup", "public_vendor_payload_copy"],
            "approved_by": "fixture-author", "additional_licensor_written_document_required": false,
            "documentary_verification_claimed": false, "technical_availability_claimed": false, "fixture_only": true
        });
        let trust = TrustedPersonalRegistration {
            registration_id: "fixture-registration".into(),
            generation: 3,
            dataset_id: "synthetic-endpoint".into(),
            dataset_version: "1".into(),
            family_id: "naming-element-rank-endpoint-operands".into(),
            profile_id: "private-naming-element-endpoint-v1".into(),
            engine_contract_id: "synthetic-contract".into(),
            manifest_sha256: validate_active_manifest(&manifest)
                .unwrap()
                .canonical_sha256,
            manifest_file_sha256: hash(&manifest),
            review_sha256: hash(&serde_json::to_vec(&review).unwrap()),
            source_registry_binding_id: "fixture-registry".into(),
            private_root_binding_id: "fixture-root".into(),
        };
        (source, trust, manifest, review)
    }

    #[test]
    fn trusted_review_and_exact_file_identity_precede_typed_loading() {
        let (mut source, mut trust, manifest, mut review) = synthetic_registration();
        let review_bytes = serde_json::to_vec(&review).unwrap();
        assert!(
            matches!(
                load_personal_dataset(&source, &trust, &manifest, &review_bytes, CONTEXT),
                Err(ActiveDataError::RightsUnverified)
            ),
            "test-only review is not production authority"
        );
        assert_eq!(source.reads.get(), 0);
        let loaded = load_personal_impl(
            &source,
            &trust,
            &manifest,
            &review_bytes,
            CONTEXT,
            ReviewTrust::SyntheticFixture,
        )
        .unwrap();
        assert_eq!(loaded.generation(), 3);
        assert_eq!(loaded.registration_id(), "fixture-registration");
        assert!(matches!(
            loaded.records(),
            ProfileRecords::ElementEndpoint(123)
        ));
        assert_eq!(source.reads.get(), 1);

        review["file_identities"][0]["sha256"] = "0".repeat(64).into();
        let mismatched_review = serde_json::to_vec(&review).unwrap();
        trust.review_sha256 = hash(&mismatched_review);
        assert!(matches!(
            load_personal_impl(
                &source,
                &trust,
                &manifest,
                &mismatched_review,
                CONTEXT,
                ReviewTrust::SyntheticFixture
            ),
            Err(ActiveDataError::RightsUnverified)
        ));
        assert_eq!(
            source.reads.get(),
            1,
            "unbound review must fail before reading payload"
        );

        trust.review_sha256 = hash(&review_bytes);
        source.bytes[0] ^= 1;
        assert!(
            load_personal_impl(
                &source,
                &trust,
                &manifest,
                &review_bytes,
                CONTEXT,
                ReviewTrust::SyntheticFixture
            )
            .is_err()
        );
        assert!(
            matches!(loaded.records(), ProfileRecords::ElementEndpoint(123)),
            "previous immutable generation survives file changes"
        );
    }

    #[test]
    fn phase7_registration_does_not_claim_phase6_metadata_was_verified() {
        let (source, trust, manifest, review) = synthetic_registration();
        let review = serde_json::to_vec(&review).unwrap();
        let files = ProfileIdentityFiles {
            registry: b"untrusted",
            profile: b"untrusted",
        };
        assert!(matches!(
            load_personal_impl_with_metadata(
                &source,
                &trust,
                &manifest,
                &review,
                CONTEXT,
                Some(&files),
                ReviewTrust::SyntheticFixture
            ),
            Err(ActiveDataError::ProfileUnsupported)
        ));
        assert_eq!(source.reads.get(), 0);
    }

    #[test]
    fn denied_or_unbound_review_never_reads_payload() {
        for (field, replacement) in [
            ("authorization_status", json!("revoked")),
            ("allowed_operations", json!(["local_load"])),
            (
                "allowed_operations",
                json!(["local_load", "decode", "decode"]),
            ),
            ("source_registry_binding_id", json!("different-registry")),
            ("private_root_binding_id", json!("different-root")),
            ("prohibited_operations", json!(["redistribution"])),
            ("technical_availability_claimed", json!(true)),
        ] {
            let (source, mut trust, manifest, mut review) = synthetic_registration();
            review[field] = replacement;
            let review = serde_json::to_vec(&review).unwrap();
            trust.review_sha256 = hash(&review);
            assert!(
                matches!(
                    load_personal_impl(
                        &source,
                        &trust,
                        &manifest,
                        &review,
                        CONTEXT,
                        ReviewTrust::SyntheticFixture
                    ),
                    Err(ActiveDataError::RightsUnverified)
                ),
                "{field}"
            );
            assert_eq!(source.reads.get(), 0, "{field}");
        }
        let (source, trust, manifest, review) = synthetic_registration();
        let review = serde_json::to_vec(&review).unwrap();
        assert!(matches!(
            load_personal_impl(
                &source,
                &trust,
                &manifest,
                &review,
                b"wrong context",
                ReviewTrust::SyntheticFixture
            ),
            Err(ActiveDataError::RightsUnverified)
        ));
        assert_eq!(source.reads.get(), 0);
    }

    #[test]
    fn a_source_without_the_trusted_root_binding_cannot_load_personal_data() {
        struct Unbound;
        impl DataSource for Unbound {
            fn read(&self, _: &str, _: u64) -> Result<Vec<u8>, crate::SourceError> {
                panic!("an unbound source must not be read")
            }
        }
        let (_, trust, manifest, review) = synthetic_registration();
        let review = serde_json::to_vec(&review).unwrap();
        assert!(matches!(
            load_personal_impl(
                &Unbound,
                &trust,
                &manifest,
                &review,
                CONTEXT,
                ReviewTrust::SyntheticFixture
            ),
            Err(ActiveDataError::RightsUnverified)
        ));
    }

    #[test]
    fn registration_rejects_a_reformatted_manifest_when_exact_file_identity_is_pinned() {
        let (source, trust, manifest, review) = synthetic_registration();
        let review = serde_json::to_vec(&review).unwrap();
        let mut reformatted = manifest.clone();
        reformatted.push(b'\n');
        assert_eq!(
            validate_active_manifest(&reformatted)
                .unwrap()
                .canonical_sha256,
            trust.manifest_sha256
        );
        assert!(matches!(
            load_personal_impl(
                &source,
                &trust,
                &reformatted,
                &review,
                CONTEXT,
                ReviewTrust::SyntheticFixture
            ),
            Err(ActiveDataError::RegistrationMismatch)
        ));
        assert_eq!(source.reads.get(), 0);
    }

    #[test]
    fn binding_pins_registration_generation_and_manifest_without_claiming_an_engine() {
        let (source, trust, manifest, review) = synthetic_registration();
        let review = serde_json::to_vec(&review).unwrap();
        let loaded = load_personal_impl(
            &source,
            &trust,
            &manifest,
            &review,
            CONTEXT,
            ReviewTrust::SyntheticFixture,
        )
        .unwrap();
        let mut binding = json!({
            "binding_version": 1, "model_id": "synthetic-model", "independent_engine_contract_id": "synthetic-contract", "independent_engine_version": "1",
            "data_registrations": [{"registration_id": "fixture-registration", "generation": 3, "manifest_sha256": trust.manifest_sha256, "feature_schema_sha256": null}],
            "method_ids": ["synthetic-method"], "convention_profile_id": "synthetic-convention", "dependency_model_ids": [],
            "availability": "available_for_declared_profile", "validation_profile_ids": [], "limitations": ["No chemistry"]
        });
        let bytes = serde_json::to_vec(&binding).unwrap();
        let verified = validate_model_binding(&bytes, &hash(&bytes), &[&loaded]).unwrap();
        assert_eq!(verified.model_id(), "synthetic-model");
        assert_eq!(verified.engine_status(), EngineState::NotImplemented);
        assert!(matches!(
            validate_model_binding(&bytes, &"0".repeat(64), &[&loaded]),
            Err(ActiveDataError::BindingUntrusted)
        ));
        assert!(matches!(
            validate_model_binding(&bytes, &hash(&bytes), &[]),
            Err(ActiveDataError::ModelUnavailable)
        ));
        binding["data_registrations"][0]["generation"] = 4.into();
        let mismatched = serde_json::to_vec(&binding).unwrap();
        assert!(matches!(
            validate_model_binding(&mismatched, &hash(&mismatched), &[&loaded]),
            Err(ActiveDataError::ModelUnavailable)
        ));
        binding["data_registrations"][0]["generation"] = 3.into();
        binding["dependency_model_ids"] = json!(["unvalidated-dependency"]);
        let dependency = serde_json::to_vec(&binding).unwrap();
        assert!(matches!(
            validate_model_binding(&dependency, &hash(&dependency), &[&loaded]),
            Err(ActiveDataError::ModelUnavailable)
        ));
    }

    #[test]
    fn canonical_binding_ignores_whitespace_but_exact_file_digest_does_not() {
        let identity = validate_active_manifest(MANIFEST.as_bytes()).unwrap();
        assert_eq!(
            identity.canonical_sha256,
            "0f9ff11af0898c7ae4852460b0a6e241c9ea27290ee36fa2101000ecbf83f291"
        );
        assert_eq!(
            identity.exact_sha256,
            "526fddfc3375cda1d5248fee133259ac3737699eeaf8efc5533e3d0d8c31a5e6"
        );
        let compact: serde_json::Value = serde_json::from_str(MANIFEST).unwrap();
        let compact = serde_json::to_vec(&compact).unwrap();
        let other = validate_active_manifest(&compact).unwrap();
        assert_eq!(identity.canonical_sha256, other.canonical_sha256);
        assert_ne!(identity.exact_sha256, other.exact_sha256);
        let negative_zero = MANIFEST.replace("\"bytes\": 0", "\"bytes\": -0");
        assert_eq!(
            validate_active_manifest(negative_zero.as_bytes())
                .unwrap()
                .canonical_sha256,
            identity.canonical_sha256
        );
        let unicode = MANIFEST.replace("Synthetic, no chemical meaning.", "한글\\t\\u0001/é");
        assert_eq!(
            validate_active_manifest(unicode.as_bytes())
                .unwrap()
                .canonical_sha256,
            "9fcb4e7ce7d1a2f139f38a4a3344e599a5732ee1894fd84c51845cf9635cb67d"
        );
    }

    #[test]
    fn invalid_or_ambiguous_manifests_never_get_a_binding_digest() {
        for invalid in [
            MANIFEST.replace("\"manifest_version\": 1", "\"manifest_version\": 1.0"),
            MANIFEST.replace("\"manifest_version\": 1", "\"manifest_version\": 1e0"),
            MANIFEST.replace(
                "\"manifest_version\": 1",
                "\"manifest_version\": 1, \"manifest_version\": 1",
            ),
            MANIFEST.replace("\"feature_schema_sha256\": null,", ""),
            MANIFEST.replace("\"dataset_id\":", "\"unknown\": 7, \"dataset_id\":"),
            MANIFEST.replace("normalized/synthetic.jsonl", "../escape.jsonl"),
        ] {
            assert!(validate_active_manifest(invalid.as_bytes()).is_err());
        }
    }

    #[test]
    fn retain_policy_never_weakens_the_manifest_or_a_closed_payload_profile() {
        let (mut source, mut trust, manifest, mut review) = synthetic_registration();
        let mut manifest: Value = serde_json::from_slice(&manifest).unwrap();
        manifest["unknown_field_policy"] = "retain_opaque_and_report".into();
        let manifest_bytes = serde_json::to_vec(&manifest).unwrap();
        let identity = validate_active_manifest(&manifest_bytes).unwrap();
        trust.manifest_sha256 = identity.canonical_sha256;
        trust.manifest_file_sha256 = identity.exact_sha256;
        let review_bytes = serde_json::to_vec(&review).unwrap();
        let loaded = load_personal_impl(
            &source,
            &trust,
            &manifest_bytes,
            &review_bytes,
            CONTEXT,
            ReviewTrust::SyntheticFixture,
        )
        .unwrap();
        assert!(matches!(
            loaded.records(),
            ProfileRecords::ElementEndpoint(123)
        ));

        let mut extended_manifest = manifest.clone();
        extended_manifest["future_extension"] = json!({"value": 1});
        assert!(matches!(
            validate_active_manifest(&serde_json::to_vec(&extended_manifest).unwrap()),
            Err(ActiveDataError::ManifestInvalid)
        ));

        source.bytes = b"{\"atomic_number_u32\":1,\"source_rank_u32\":123,\"extra\":1}\n".to_vec();
        let data_hash = hash(&source.bytes);
        manifest["files"][0]["sha256"] = data_hash.clone().into();
        manifest["files"][0]["bytes"] = source.bytes.len().into();
        review["file_identities"][0]["sha256"] = data_hash.into();
        review["file_identities"][0]["bytes"] = source.bytes.len().into();
        let manifest_bytes = serde_json::to_vec(&manifest).unwrap();
        let identity = validate_active_manifest(&manifest_bytes).unwrap();
        trust.manifest_sha256 = identity.canonical_sha256;
        trust.manifest_file_sha256 = identity.exact_sha256;
        let review_bytes = serde_json::to_vec(&review).unwrap();
        trust.review_sha256 = hash(&review_bytes);
        assert!(matches!(
            load_personal_impl(
                &source,
                &trust,
                &manifest_bytes,
                &review_bytes,
                CONTEXT,
                ReviewTrust::SyntheticFixture
            ),
            Err(ActiveDataError::ContentInvalid)
        ));
    }

    #[test]
    fn verified_phase6_files_form_one_atomic_typed_registration() {
        use crate::phase6_profiles::{CatalogIdentity, DatasetRecords};
        use std::collections::HashMap;

        struct Files {
            contents: HashMap<String, Vec<u8>>,
            reads: Cell<usize>,
        }
        impl DataSource for Files {
            fn root_binding_id(&self) -> Option<&str> {
                Some("fixture-root")
            }
            fn read(&self, path: &str, limit: u64) -> Result<Vec<u8>, crate::SourceError> {
                self.reads.set(self.reads.get() + 1);
                let bytes = self.contents.get(path).ok_or(crate::SourceError::Missing)?;
                if bytes.len() as u64 > limit {
                    return Err(crate::SourceError::TooLarge);
                }
                Ok(bytes.clone())
            }
        }
        let a: String = (2..=109)
            .map(|z| format!("{{\"atomic_number_u32\":{z},\"source_rank_u32\":{z}}}\n"))
            .collect();
        let b = b"{\"atomic_number_u32\":7,\"source_rank_u32\":321}\n".to_vec();
        let mut source = Files {
            contents: HashMap::from([
                ("normalized/element-order-a.jsonl".into(), a.into_bytes()),
                ("normalized/element-order-b.jsonl".into(), b),
            ]),
            reads: Cell::new(0),
        };
        let specs: Vec<_> = ["a", "b"].iter().map(|label| {
            let path = format!("normalized/element-order-{label}.jsonl");
            let bytes = &source.contents[&path];
            json!({"file_id": label, "relative_path": path, "sha256": hash(bytes), "bytes": bytes.len(), "role": "dictionary_entries", "required": true, "declared_content_class": "declarative_data"})
        }).collect();
        let manifest = serde_json::to_vec(&json!({
            "manifest_version": 1, "dataset_id": "fixture-ranks", "dataset_version": "1",
            "family_id": "fixture-family", "format_profile_id": "private-naming-element-ranks-v1",
            "independent_engine_contract_id": "fixture-contract", "source_category": "independent_synthetic",
            "files": specs, "feature_schema_sha256": null, "rights_review_id": "fixture-review", "unknown_field_policy": "reject",
            "metadata": {"provider_label": "Independent fixture", "version_evidence_id": "fixture-v1", "description": "No chemical meaning."}
        })).unwrap();
        let mut incomplete: Value = serde_json::from_slice(&manifest).unwrap();
        incomplete["files"].as_array_mut().unwrap().pop();
        let (incomplete, _) = parse_manifest(&serde_json::to_vec(&incomplete).unwrap()).unwrap();
        assert!(matches!(
            select_profile(&incomplete),
            Err(ActiveDataError::ProfileUnsupported)
        ));
        let mut optional: Value = serde_json::from_slice(&manifest).unwrap();
        optional["files"][1]["required"] = false.into();
        let (optional, _) = parse_manifest(&serde_json::to_vec(&optional).unwrap()).unwrap();
        assert!(matches!(
            select_profile(&optional),
            Err(ActiveDataError::ProfileUnsupported)
        ));
        let reviewed: Vec<_> = specs.iter().map(|file| json!({
            "file_id": file["file_id"], "asset_id": "synthetic", "sha256": file["sha256"], "bytes": file["bytes"],
            "content_classification": "declarative_data", "classification_basis_id": "fixture-shape"
        })).collect();
        let review = serde_json::to_vec(&json!({
            "review_version": 2, "review_id": "fixture-review", "dataset_id": "fixture-ranks", "dataset_version": "1",
            "context_id": "personal-local-user-attested-v1", "context_sha256": hash(CONTEXT),
            "approval_basis": "user_attested_personal_permission", "attestation_reference_id": "fixture-only",
            "authorization_status": "approved_personal_local", "source_registry_binding_id": "fixture-registry",
            "private_root_binding_id": "fixture-root", "file_identities": reviewed,
            "allowed_operations": ["local_load", "decode"], "prohibited_operations": ["redistribution", "remote_processing", "original_module_execution", "secret_lookup", "public_vendor_payload_copy"],
            "approved_by": "fixture-author", "additional_licensor_written_document_required": false,
            "documentary_verification_claimed": false, "technical_availability_claimed": false, "fixture_only": true
        })).unwrap();
        let parsed_identity = validate_active_manifest(&manifest).unwrap();
        let trust = TrustedPersonalRegistration {
            registration_id: "fixture-registration".into(),
            generation: 1,
            dataset_id: "fixture-ranks".into(),
            dataset_version: "1".into(),
            family_id: "fixture-family".into(),
            profile_id: "private-naming-element-ranks-v1".into(),
            engine_contract_id: "fixture-contract".into(),
            manifest_sha256: parsed_identity.canonical_sha256,
            manifest_file_sha256: parsed_identity.exact_sha256,
            review_sha256: hash(&review),
            source_registry_binding_id: "fixture-registry".into(),
            private_root_binding_id: "fixture-root".into(),
        };
        let registry = b"synthetic registry";
        let profile = b"synthetic profile";
        let registry_hash = hash(registry);
        let profile_hash = hash(profile);
        let identity = CatalogIdentity {
            dataset_id: "fixture-ranks",
            dataset_version: "1",
            registry_sha256: &registry_hash,
            profile_sha256: &profile_hash,
            manifest_file_sha256: &trust.manifest_file_sha256,
        };
        let fixture_trust = ReviewTrust::SyntheticPhase6((&identity).into());
        let proof = ProfileIdentityFiles { registry, profile };
        let bad_proof = ProfileIdentityFiles {
            registry: b"changed",
            profile,
        };
        assert!(matches!(
            load_personal_impl_with_metadata(
                &source,
                &trust,
                &manifest,
                &review,
                CONTEXT,
                Some(&bad_proof),
                fixture_trust.clone()
            ),
            Err(ActiveDataError::RegistrationMismatch)
        ));
        assert_eq!(
            source.reads.get(),
            0,
            "wrong registry is rejected before any payload read"
        );
        let loaded = load_personal_impl_with_metadata(
            &source,
            &trust,
            &manifest,
            &review,
            CONTEXT,
            Some(&proof),
            fixture_trust.clone(),
        )
        .unwrap();
        let ProfileRecords::Phase6(DatasetRecords::NamingRanks { order_a, order_b }) =
            loaded.records()
        else {
            panic!("wrong profile")
        };
        assert_eq!(order_a.len(), 108);
        assert_eq!(order_b[0].source_rank, 321);
        assert_eq!(source.reads.get(), 2);

        source
            .contents
            .get_mut("normalized/element-order-b.jsonl")
            .unwrap()[0] ^= 1;
        assert!(matches!(
            load_personal_impl_with_metadata(
                &source,
                &trust,
                &manifest,
                &review,
                CONTEXT,
                Some(&proof),
                fixture_trust
            ),
            Err(ActiveDataError::DataIdentityMismatch)
        ));
        assert!(matches!(
            load_personal_dataset_with_profile_files(
                &source, &trust, &manifest, &review, CONTEXT, &proof
            ),
            Err(ActiveDataError::RightsUnverified)
        ));
    }

    #[test]
    fn a_complete_nmr_dataset_keeps_feature_identity_without_enabling_a_model() {
        use std::collections::HashMap;

        struct Files(HashMap<String, Vec<u8>>);
        impl DataSource for Files {
            fn root_binding_id(&self) -> Option<&str> {
                Some("fixture-root")
            }
            fn read(&self, path: &str, limit: u64) -> Result<Vec<u8>, crate::SourceError> {
                let bytes = self.0.get(path).ok_or(crate::SourceError::Missing)?;
                if bytes.len() as u64 > limit {
                    return Err(crate::SourceError::TooLarge);
                }
                Ok(bytes.clone())
            }
        }
        let entries: [(&str, &str, &[u8], &str); 5] = [
            (
                "lookup",
                nmr_profile::LOOKUP_PATH,
                include_bytes!("../tests/data/nmr-synthetic/lookup.json"),
                "auxiliary_data",
            ),
            (
                "amplitudes",
                nmr_profile::AMPLITUDES_PATH,
                include_bytes!("../tests/data/nmr-synthetic/amplitudes.json"),
                "weights",
            ),
            (
                "widths",
                nmr_profile::WIDTHS_PATH,
                include_bytes!("../tests/data/nmr-synthetic/widths.json"),
                "auxiliary_data",
            ),
            (
                "features",
                nmr_profile::FEATURES_PATH,
                include_bytes!("../tests/data/nmr-synthetic/feature-schema.json"),
                "feature_schema",
            ),
            (
                "bounds",
                nmr_profile::BOUNDS_PATH,
                include_bytes!("../tests/data/nmr-synthetic/source-bounds.json"),
                "auxiliary_data",
            ),
        ];
        let files: Vec<Value> = entries.iter().map(|(id, path, bytes, role)| json!({
            "file_id": id, "relative_path": path, "sha256": hash(bytes), "bytes": bytes.len(),
            "role": role, "required": true, "declared_content_class": "declarative_data"
        })).collect();
        let feature_hash = hash(entries[3].2);
        let manifest = serde_json::to_vec(&json!({
            "manifest_version": 1, "dataset_id": "independent-nmr-shape", "dataset_version": "1",
            "family_id": "independent-fixture", "format_profile_id": "personal-local-nmr-coupling-parameters-v1",
            "independent_engine_contract_id": "fixture-contract", "source_category": "independent_synthetic",
            "files": files, "feature_schema_sha256": feature_hash, "rights_review_id": "fixture-review",
            "unknown_field_policy": "reject", "metadata": {"provider_label": "Independent fixture", "version_evidence_id": "fixture", "description": "No chemistry."}
        })).unwrap();
        let reviewed: Vec<_> = files.iter().map(|entry| json!({
            "file_id": entry["file_id"], "asset_id": "fixture", "sha256": entry["sha256"], "bytes": entry["bytes"],
            "content_classification": "declarative_data", "classification_basis_id": "fixture"
        })).collect();
        let review = serde_json::to_vec(&json!({
            "review_version": 2, "review_id": "fixture-review", "dataset_id": "independent-nmr-shape", "dataset_version": "1",
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
            dataset_id: "independent-nmr-shape".into(),
            dataset_version: "1".into(),
            family_id: "independent-fixture".into(),
            profile_id: "personal-local-nmr-coupling-parameters-v1".into(),
            engine_contract_id: "fixture-contract".into(),
            manifest_sha256: identity.canonical_sha256,
            manifest_file_sha256: identity.exact_sha256,
            review_sha256: hash(&review),
            source_registry_binding_id: "fixture-registry".into(),
            private_root_binding_id: "fixture-root".into(),
        };
        let registry = b"fixture-registry";
        let profile = b"fixture-profile";
        let registry_hash = hash(registry);
        let profile_hash = hash(profile);
        let fixture_identity = phase6_profiles::CatalogIdentity {
            dataset_id: &trust.dataset_id,
            dataset_version: &trust.dataset_version,
            manifest_file_sha256: &trust.manifest_file_sha256,
            registry_sha256: &registry_hash,
            profile_sha256: &profile_hash,
        };
        let test_trust = ReviewTrust::SyntheticPhase6((&fixture_identity).into());
        let proof = ProfileIdentityFiles { registry, profile };
        let mut source = Files(
            entries
                .iter()
                .map(|(_, path, bytes, _)| ((*path).into(), bytes.to_vec()))
                .collect(),
        );
        let loaded = load_personal_impl_with_metadata(
            &source,
            &trust,
            &manifest,
            &review,
            CONTEXT,
            Some(&proof),
            test_trust.clone(),
        )
        .unwrap();
        assert_eq!(loaded.feature_schema_sha256(), Some(feature_hash.as_str()));
        let ProfileRecords::Nmr(data) = loaded.records() else {
            panic!("wrong dataset type")
        };
        assert_eq!(data.lookup.rows[0].parameter.bits, (-0.0_f64).to_bits());
        assert_eq!(data.widths.parameters[0].unit, "fixture-w");

        let binding = serde_json::to_vec(&json!({
            "binding_version": 1, "model_id": "fixture-model", "independent_engine_contract_id": "fixture-contract",
            "independent_engine_version": "1", "data_registrations": [{"registration_id": "fixture-registration", "generation": 1, "manifest_sha256": trust.manifest_sha256, "feature_schema_sha256": feature_hash}],
            "method_ids": ["fixture-method"], "convention_profile_id": "fixture", "dependency_model_ids": [],
            "availability": "available_for_declared_profile", "validation_profile_ids": [], "limitations": []
        })).unwrap();
        assert!(matches!(
            validate_model_binding(&binding, &hash(&binding), &[&loaded]),
            Err(ActiveDataError::ModelUnavailable)
        ));

        source.0.get_mut(nmr_profile::BOUNDS_PATH).unwrap()[0] ^= 1;
        assert!(matches!(
            load_personal_impl_with_metadata(
                &source,
                &trust,
                &manifest,
                &review,
                CONTEXT,
                Some(&proof),
                test_trust
            ),
            Err(ActiveDataError::DataIdentityMismatch)
        ));
    }
}
