//! Explicit trusted-registry selection and payload-free status reporting.

use std::collections::HashSet;

use chemifree_domain::data;
use serde::{Deserialize, Serialize};

use crate::active_data::load_personal_layout_dataset;
use crate::layout_data_binding::LayoutReader;
use crate::{
    ActiveDataError, DataSource, PersonalDataset, ProfileIdentityFiles, ProfileRecords,
    TrustedPersonalRegistration, load_personal_dataset, load_personal_dataset_with_profile_files,
};

/// A host-selected registry descriptor, kept separate from the selected DATA directory.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrustedRegistry {
    registry_version: u64,
    root_binding_id: String,
    registrations: Vec<RegistryEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistryEntry {
    trust: TrustedPersonalRegistration,
    manifest_path: String,
    review_path: String,
    #[serde(default)]
    data_root: Option<String>,
    #[serde(default)]
    source_registry_path: Option<String>,
    #[serde(default)]
    format_profile_path: Option<String>,
    #[serde(default)]
    layout_reader: Option<LayoutReader>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RegistryState {
    NotConfigured,
    Refused,
    Checked,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RegistryEntryState {
    Loaded,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineState {
    NotImplemented,
}

impl std::fmt::Display for EngineState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotImplemented => formatter.write_str("not_implemented"),
        }
    }
}

impl std::fmt::Display for RegistryEntryState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Loaded => "loaded",
            Self::Unavailable => "unavailable",
        })
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct RegistryEntryStatus {
    pub registration_id: String,
    pub state: RegistryEntryState,
    pub reason: Option<&'static str>,
    /// Number of source columns kept as opaque metadata; no names or values are exposed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retained_extra_columns: Option<usize>,
}

impl RegistryEntryStatus {
    pub(crate) fn loaded(dataset: &PersonalDataset) -> Self {
        let retained_extra_columns = match dataset.records() {
            ProfileRecords::Phase4AqSolDb(data) if !data.extra_columns.is_empty() => {
                Some(data.extra_columns.len())
            }
            _ => None,
        };
        Self {
            registration_id: dataset.registration_id().to_owned(),
            state: RegistryEntryState::Loaded,
            reason: None,
            retained_extra_columns,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct RegistryStatus {
    pub state: RegistryState,
    pub reason: Option<&'static str>,
    pub registrations: Vec<RegistryEntryStatus>,
    pub engine: EngineState,
}

/// Contains verified data privately and provides only payload-free status to entry adapters.
pub struct DataRegistry {
    status: RegistryStatus,
    datasets: Vec<PersonalDataset>,
}

impl DataRegistry {
    /// Synthetic test registry, compiled only for tests or the conformance feature.
    #[cfg(any(test, feature = "conformance-fixtures"))]
    pub(crate) fn fixture(datasets: Vec<PersonalDataset>) -> Self {
        let registrations = datasets.iter().map(RegistryEntryStatus::loaded).collect();
        Self {
            status: RegistryStatus {
                state: RegistryState::Checked,
                reason: None,
                registrations,
                engine: EngineState::NotImplemented,
            },
            datasets,
        }
    }

    /// Creates a clearly labeled synthetic registry for the conformance executable only.
    /// This feature does not add an HTTP/CLI input for untrusted mass data.
    #[cfg(feature = "conformance-fixtures")]
    #[must_use]
    pub fn synthetic_conformance(records: ProfileRecords) -> Self {
        Self::fixture(vec![PersonalDataset::fixture(records)])
    }

    #[must_use]
    pub fn not_configured() -> Self {
        Self {
            status: RegistryStatus {
                state: RegistryState::NotConfigured,
                reason: None,
                registrations: Vec::new(),
                engine: EngineState::NotImplemented,
            },
            datasets: Vec::new(),
        }
    }

    #[must_use]
    pub fn refused(reason: ActiveDataError) -> Self {
        Self {
            status: RegistryStatus {
                state: RegistryState::Refused,
                reason: Some(reason.code()),
                registrations: Vec::new(),
                engine: EngineState::NotImplemented,
            },
            datasets: Vec::new(),
        }
    }

    #[must_use]
    pub fn status(&self) -> &RegistryStatus {
        &self.status
    }

    #[must_use]
    pub fn datasets(&self) -> &[PersonalDataset] {
        &self.datasets
    }
}

impl TrustedRegistry {
    /// Parses the explicitly trusted descriptor without reading DATA.
    ///
    /// # Errors
    /// Rejects malformed, duplicate or unsafe metadata.
    pub fn parse(bytes: &[u8]) -> Result<Self, ActiveDataError> {
        if bytes.len() as u64 > crate::MAX_MANIFEST_BYTES {
            return Err(ActiveDataError::RegistryInvalid);
        }
        let value =
            crate::json::parse_strict(bytes).map_err(|_| ActiveDataError::RegistryInvalid)?;
        let registry: Self =
            serde_json::from_value(value).map_err(|_| ActiveDataError::RegistryInvalid)?;
        let valid_text = |text: &str| {
            !text.is_empty() && text.chars().count() <= 1000 && !text.chars().any(char::is_control)
        };
        let valid_hash = |text: &str| {
            text.len() == 64
                && text
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        };
        if registry.registry_version != 1
            || !valid_text(&registry.root_binding_id)
            || registry.registrations.is_empty()
            || registry.registrations.len() > crate::MAX_DATASETS
        {
            return Err(ActiveDataError::RegistryInvalid);
        }
        let mut ids = HashSet::new();
        for entry in &registry.registrations {
            let trust = &entry.trust;
            if !valid_text(&trust.registration_id)
                || !ids.insert(&trust.registration_id)
                || !valid_text(&trust.dataset_id)
                || !valid_text(&trust.dataset_version)
                || !valid_text(&trust.family_id)
                || !valid_text(&trust.profile_id)
                || !valid_text(&trust.engine_contract_id)
                || !valid_text(&trust.source_registry_binding_id)
                || trust.private_root_binding_id != registry.root_binding_id
                || !valid_hash(&trust.manifest_sha256)
                || !valid_hash(&trust.manifest_file_sha256)
                || !valid_hash(&trust.review_sha256)
                || entry.manifest_path == entry.review_path
                || data::check_relative_path(&entry.manifest_path).is_err()
                || data::check_relative_path(&entry.review_path).is_err()
                || data::has_executable_extension(&entry.manifest_path)
                || data::has_executable_extension(&entry.review_path)
                || entry
                    .data_root
                    .as_deref()
                    .is_some_and(|path| data::check_relative_path(path).is_err())
                || entry.source_registry_path.is_some() != entry.format_profile_path.is_some()
                || entry.layout_reader.is_some() && entry.source_registry_path.is_none()
                || entry.source_registry_path.as_deref().is_some_and(|path| {
                    data::check_relative_path(path).is_err() || data::has_executable_extension(path)
                })
                || entry.format_profile_path.as_deref().is_some_and(|path| {
                    data::check_relative_path(path).is_err() || data::has_executable_extension(path)
                })
                || entry.source_registry_path.is_some()
                    && entry.source_registry_path == entry.format_profile_path
            {
                return Err(ActiveDataError::RegistryInvalid);
            }
        }
        Ok(registry)
    }

    #[must_use]
    pub fn root_binding_id(&self) -> &str {
        &self.root_binding_id
    }

    /// Loads individually complete registrations; rejected entries expose no data.
    #[must_use]
    pub fn load(&self, source: &dyn DataSource) -> DataRegistry {
        if source.root_binding_id() != Some(self.root_binding_id.as_str()) {
            return DataRegistry::refused(ActiveDataError::RightsUnverified);
        }
        let mut datasets = Vec::new();
        let mut statuses = Vec::with_capacity(self.registrations.len());
        for entry in &self.registrations {
            let loaded = (|| {
                let scoped = entry
                    .data_root
                    .as_deref()
                    .map(|path| source.scope(path))
                    .transpose()
                    .map_err(|_| ActiveDataError::DataUnavailable)?;
                let data_source = scoped.as_deref().unwrap_or(source);
                let manifest = source
                    .read(&entry.manifest_path, crate::MAX_MANIFEST_BYTES)
                    .map_err(|_| ActiveDataError::DataUnavailable)?;
                let review = source
                    .read(&entry.review_path, crate::MAX_MANIFEST_BYTES)
                    .map_err(|_| ActiveDataError::DataUnavailable)?;
                let context = include_bytes!("data/personal-local-context.json");
                match (&entry.source_registry_path, &entry.format_profile_path) {
                    (Some(registry_path), Some(profile_path)) => {
                        let registry = source
                            .read(registry_path, crate::MAX_MANIFEST_BYTES)
                            .map_err(|_| ActiveDataError::DataUnavailable)?;
                        let profile = source
                            .read(profile_path, crate::MAX_MANIFEST_BYTES)
                            .map_err(|_| ActiveDataError::DataUnavailable)?;
                        let files = ProfileIdentityFiles {
                            registry: &registry,
                            profile: &profile,
                        };
                        match entry.layout_reader {
                            Some(reader) => load_personal_layout_dataset(
                                data_source,
                                &entry.trust,
                                &manifest,
                                &review,
                                context,
                                &files,
                                reader,
                            ),
                            None => load_personal_dataset_with_profile_files(
                                data_source,
                                &entry.trust,
                                &manifest,
                                &review,
                                context,
                                &files,
                            ),
                        }
                    }
                    (None, None) => load_personal_dataset(
                        data_source,
                        &entry.trust,
                        &manifest,
                        &review,
                        context,
                    ),
                    _ => Err(ActiveDataError::RegistryInvalid),
                }
            })();
            let status = match loaded {
                Ok(dataset) => {
                    let status = RegistryEntryStatus::loaded(&dataset);
                    datasets.push(dataset);
                    status
                }
                Err(reason) => RegistryEntryStatus {
                    registration_id: entry.trust.registration_id.clone(),
                    state: RegistryEntryState::Unavailable,
                    reason: Some(reason.code()),
                    retained_extra_columns: None,
                },
            };
            statuses.push(status);
        }
        DataRegistry {
            status: RegistryStatus {
                state: RegistryState::Checked,
                reason: None,
                registrations: statuses,
                engine: EngineState::NotImplemented,
            },
            datasets,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::Cell;

    struct Missing {
        reads: Cell<usize>,
    }
    impl DataSource for Missing {
        fn root_binding_id(&self) -> Option<&str> {
            Some("fixture-root")
        }
        fn read(&self, _: &str, _: u64) -> Result<Vec<u8>, crate::SourceError> {
            self.reads.set(self.reads.get() + 1);
            Err(crate::SourceError::Missing)
        }
    }

    fn descriptor() -> serde_json::Value {
        let entry = json!({"trust": {"registration_id": "independent-fixture", "generation": 0, "dataset_id": "fixture", "dataset_version": "1", "family_id": "fixture", "profile_id": "fixture", "engine_contract_id": "fixture", "manifest_sha256": "0".repeat(64), "manifest_file_sha256": "0".repeat(64), "review_sha256": "0".repeat(64), "source_registry_binding_id": "fixture-registry", "private_root_binding_id": "fixture-root"}, "manifest_path": "manifest.json", "review_path": "review.json"});
        json!({"registry_version": 1, "root_binding_id": "fixture-root", "registrations": [entry]})
    }

    #[test]
    fn selected_layout_role_requires_both_metadata_files_and_a_known_role() {
        let mut value = descriptor();
        let entry = &mut value["registrations"][0];
        entry["layout_reader"] = "large_twenty_seed".into();
        entry["source_registry_path"] = "catalog/registry.json".into();
        entry["format_profile_path"] = "catalog/schema.json".into();
        assert!(TrustedRegistry::parse(&serde_json::to_vec(&value).unwrap()).is_ok());

        let mut missing = value.clone();
        missing["registrations"][0]
            .as_object_mut()
            .unwrap()
            .remove("format_profile_path");
        assert!(TrustedRegistry::parse(&serde_json::to_vec(&missing).unwrap()).is_err());
        let mut no_metadata = value.clone();
        let entry = no_metadata["registrations"][0].as_object_mut().unwrap();
        entry.remove("source_registry_path");
        entry.remove("format_profile_path");
        assert!(TrustedRegistry::parse(&serde_json::to_vec(&no_metadata).unwrap()).is_err());
        let mut unknown = value;
        unknown["registrations"][0]["layout_reader"] = "unknown".into();
        assert!(TrustedRegistry::parse(&serde_json::to_vec(&unknown).unwrap()).is_err());
    }

    #[test]
    fn selected_registry_reports_missing_data_without_activating_an_engine() {
        let descriptor = descriptor();
        let registry = TrustedRegistry::parse(&serde_json::to_vec(&descriptor).unwrap()).unwrap();
        let source = Missing {
            reads: Cell::new(0),
        };
        let loaded = registry.load(&source);
        assert_eq!(loaded.status().state, RegistryState::Checked);
        assert_eq!(
            loaded.status().registrations[0].reason,
            Some("data_unavailable")
        );
        assert_eq!(loaded.status().engine, EngineState::NotImplemented);
        assert!(loaded.datasets().is_empty());
        assert_eq!(source.reads.get(), 1);
    }

    #[test]
    fn untrusted_registry_cannot_escape_root_or_duplicate_a_registration() {
        let mut unsafe_path = descriptor();
        unsafe_path["registrations"][0]["manifest_path"] = "../outside".into();
        assert!(TrustedRegistry::parse(&serde_json::to_vec(&unsafe_path).unwrap()).is_err());
        let mut duplicate = descriptor();
        let entry = duplicate["registrations"][0].clone();
        duplicate["registrations"]
            .as_array_mut()
            .unwrap()
            .push(entry);
        assert!(TrustedRegistry::parse(&serde_json::to_vec(&duplicate).unwrap()).is_err());
    }

    #[test]
    fn sources_without_stable_scoping_do_not_load_a_dataset_root() {
        use std::cell::RefCell;
        struct Tracking(RefCell<Vec<String>>);
        impl DataSource for Tracking {
            fn root_binding_id(&self) -> Option<&str> {
                Some("fixture-root")
            }
            fn read(&self, path: &str, _limit: u64) -> Result<Vec<u8>, crate::SourceError> {
                self.0.borrow_mut().push(path.to_owned());
                Ok(b"fixture".to_vec())
            }
        }
        let source = Tracking(RefCell::new(Vec::new()));
        let mut value = descriptor();
        value["registrations"][0]["data_root"] = "dataset-a".into();
        let registry = TrustedRegistry::parse(&serde_json::to_vec(&value).unwrap()).unwrap();
        let result = registry.load(&source);
        assert!(result.datasets().is_empty());
        assert_eq!(
            result.status().registrations[0].reason,
            Some("data_unavailable")
        );
        assert!(
            source.0.borrow().is_empty(),
            "unscoped sources must not read payloads"
        );
    }

    #[test]
    fn phase6_metadata_paths_must_be_safe_and_supplied_together() {
        let mut value = descriptor();
        let entry = &mut value["registrations"][0];
        entry["data_root"] = "datasets/example".into();
        entry["source_registry_path"] = "catalog/registry.json".into();
        entry["format_profile_path"] = "catalog/profile.json".into();
        assert!(TrustedRegistry::parse(&serde_json::to_vec(&value).unwrap()).is_ok());
        let mut incomplete = value.clone();
        incomplete["registrations"][0]
            .as_object_mut()
            .unwrap()
            .remove("format_profile_path");
        assert!(TrustedRegistry::parse(&serde_json::to_vec(&incomplete).unwrap()).is_err());
        for key in ["data_root", "source_registry_path", "format_profile_path"] {
            let mut invalid = value.clone();
            invalid["registrations"][0][key] = "../escape".into();
            assert!(TrustedRegistry::parse(&serde_json::to_vec(&invalid).unwrap()).is_err());
        }
    }

    #[test]
    fn missing_phase6_metadata_never_activates_a_registration() {
        use std::cell::RefCell;
        struct MetadataOnly(RefCell<Vec<String>>);
        impl DataSource for MetadataOnly {
            fn root_binding_id(&self) -> Option<&str> {
                Some("fixture-root")
            }
            fn read(&self, path: &str, _: u64) -> Result<Vec<u8>, crate::SourceError> {
                self.0.borrow_mut().push(path.to_owned());
                match path {
                    "manifest.json" | "review.json" => Ok(b"{}".to_vec()),
                    _ => Err(crate::SourceError::Missing),
                }
            }
        }
        let mut value = descriptor();
        let entry = &mut value["registrations"][0];
        entry["trust"]["profile_id"] = "private-naming-element-ranks-v1".into();
        entry["source_registry_path"] = "catalog/registry.json".into();
        entry["format_profile_path"] = "catalog/profile.json".into();
        let registry = TrustedRegistry::parse(&serde_json::to_vec(&value).unwrap()).unwrap();
        let source = MetadataOnly(RefCell::new(Vec::new()));
        let loaded = registry.load(&source);
        assert!(loaded.datasets().is_empty());
        assert_eq!(
            loaded.status().registrations[0].reason,
            Some("data_unavailable")
        );
        assert_eq!(
            &*source.0.borrow(),
            &["manifest.json", "review.json", "catalog/registry.json"]
        );
    }
}
