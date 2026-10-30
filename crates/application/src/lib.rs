//! Shared use cases and v1 contracts for HTTP and CLI entry adapters.

mod active_data;
mod active_registry;
mod contract;
mod data;
mod data_formats;
mod data_profiles;
mod decimal_input;
mod documents;
mod draft_schedule;
mod draft_writer;
mod error;
pub mod json;
pub mod mass_input;
pub mod mass_sources;
pub mod naming_labels;
pub mod nmr_profile;
mod operations;
pub mod phase6_profiles;
mod preferences;
mod repository;
mod turns;

use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Instant;

use chemifree_domain::Capability;
use chemifree_domain::settings::Settings;
use serde::Serialize;

pub use active_data::{
    ActiveDataError, ManifestIdentity, PersonalDataset, ProfileIdentityFiles,
    TrustedPersonalRegistration, ValidatedModelBinding, load_personal_dataset,
    load_personal_dataset_with_profile_files, validate_active_manifest, validate_model_binding,
};
pub use active_registry::{
    DataRegistry, EngineState, RegistryEntryState, RegistryEntryStatus, RegistryState,
    RegistryStatus, TrustedRegistry,
};

pub use contract::{
    AtomKindView, AtomPositionView, AtomView, BondOrderDto, BondQueryDto, BondToDto, BondView,
    CarbonLabelsDto, CloseRequest, CloseResponse, CommandDto, CopyResponse, CopySource,
    CreateDocumentRequest, DeleteModeDto, DeleteResponse, DisplayUnitDto, DocumentListResponse,
    DocumentResponse, DocumentSummary, DocumentView, DraftDiscardResponse, DraftFailureView,
    DraftRequest, GraphicShapeDto, HydrogensDto, ImportSettingsRequest, InstanceView, LabelFontDto,
    ListQuery, ListSort, MoleculeView, PageView, PointDto, PositionView, QueryBondOrderDto,
    QueryDto, RadicalDto, RecoverAsCopyRequest, RecoveryState, RecoveryView, ReferenceDto,
    ReferenceTargetDto, RenameRequest, ResetSettingsRequest, RestoreRequest, RestoreResponse,
    RevisionRequest, SaveAsRequest, SceneObjectView, SceneView, SelectionDto, SessionView,
    SettingsDto, SettingsEditDto, SettingsSource, TransactionRequest, TransactionSelections,
    TrashListResponse, TrashedSummary, UnreadableDocument, query_label,
};
pub use data::{
    BindingState, BindingStatus, CheckState, DataSnapshot, DataSource, DataStatus,
    DatasetReference, DatasetStatus, DistributionPolicy, FeatureData, FeatureStatus, FileStatus,
    LoadedFile, MANIFEST_FILE, MANIFEST_VERSION, MAX_DATA_FILE_BYTES, MAX_DATA_FILES,
    MAX_DATA_TOTAL_BYTES, MAX_DATASETS, MAX_MANIFEST_BYTES, ManifestState, ManifestStatus,
    SourceError, load_data, load_data_snapshot,
};
pub use data_formats::{Column, DataValue, Parameters, Table, decode_parameters, decode_table};
pub use data_profiles::{
    Contour, DataProfile, ProfileDecodeError, ProfileRecords, RankOperand, ValenceOperand,
    WindingMode, decode_profile,
};
pub use documents::{
    DEFAULT_TITLE, MAX_LISTED_DOCUMENTS, MAX_OPEN_SESSIONS, MAX_REQUEST_ID_CHARS, REPLAY_LIMIT,
};
pub use draft_schedule::{DRAFT_MAX_WAIT, DRAFT_QUIET, DRAFT_RETRY};
pub use draft_writer::DraftWriter;
pub use error::{ApplicationError, ErrorCode, ErrorResponse, StorageReason};
pub use operations::{
    ClosedResult, Diagnostic, DocumentResult, Envelope, Loss, LossDisposition, OPERATION_CONTRACT,
    OperationCatalog, OperationResponse, OperationResult, Outcome, ProfileSupport, SemanticImpact,
    Severity, Support,
};
pub use preferences::{
    ComputationChange, ComputationSetting, ComputationView, ImportPreferencesRequest,
    MemoryPreferences, PreferenceRepository, Preferences, PreferencesChange, PreferencesRequest,
    PreferencesResponse, Theme, UiPreferencesDto,
};
pub use repository::{
    DocumentRepository, Draft, DraftInfo, DraftRecoveryTarget, DraftStatus, StorageError,
    TrashEntry,
};
pub use turns::TURN_WAIT;

use documents::{Documents, SettingsMutation};
use preferences::PreferenceService;

pub const API_VERSION: &str = "v1";

/// An application-owned port implemented by infrastructure providers.
pub trait CapabilityCatalog: Send + Sync {
    fn capabilities(&self) -> Vec<Capability>;
}

/// The single application service. Entry adapters translate transport details only;
/// `request_id` is optional here so in-process callers without retries may omit it.
pub struct Application<C, R> {
    catalog: C,
    documents: Documents<R>,
    preferences: PreferenceService,
    data_registry: DataRegistry,
}

impl<C: CapabilityCatalog, R: DocumentRepository> Application<C, R> {
    /// An application whose preferences live in memory for its lifetime; see
    /// [`Application::with_preferences`].
    pub fn new(catalog: C, repository: R) -> Self {
        Self {
            catalog,
            documents: Documents::new(repository),
            preferences: PreferenceService::new(Box::new(MemoryPreferences::default())),
            data_registry: DataRegistry::not_configured(),
        }
    }

    /// Retains the checked data generation for the application lifetime.
    #[must_use]
    pub fn with_data_registry(self, data_registry: DataRegistry) -> Self {
        Self {
            data_registry,
            ..self
        }
    }

    /// Reports registry state without returning data records or source paths.
    #[must_use]
    pub fn data_status(&self) -> DataStatusResponse {
        DataStatusResponse {
            api_version: API_VERSION,
            status: self.data_registry.status().clone(),
        }
    }

    /// The immutable checked generations available to future in-process consumers.
    #[must_use]
    pub fn data_registry(&self) -> &DataRegistry {
        &self.data_registry
    }

    /// Keeps preferences in `store`, as an entry adapter does with its data directory's.
    #[must_use]
    pub fn with_preferences(self, store: impl PreferenceRepository + 'static) -> Self {
        Self {
            preferences: PreferenceService::new(Box::new(store)),
            ..self
        }
    }

    /// Liveness only. This does not assert that a scientific engine is available.
    pub fn health(&self) -> HealthResponse {
        HealthResponse {
            api_version: API_VERSION,
            service: "chemifree",
            version: env!("CARGO_PKG_VERSION"),
            stage: "scaffold",
            status: "ok",
        }
    }

    pub fn capabilities(&self) -> CapabilitiesResponse {
        CapabilitiesResponse {
            api_version: API_VERSION,
            capabilities: self
                .catalog
                .capabilities()
                .into_iter()
                .map(|capability| CapabilityResponse {
                    id: capability.id.as_str(),
                    label: capability.label,
                    status: capability.status.as_str(),
                    description: capability.description,
                })
                .collect(),
            operation_catalog: self.operation_catalog(),
        }
    }

    pub fn list_documents(
        &self,
        query: &ListQuery,
    ) -> Result<DocumentListResponse, ApplicationError> {
        self.documents.list(query)
    }

    /// Creates a document whose settings are copied from the preferences now: the built-in
    /// defaults overlaid with `new_documents`. Later preference changes never reach it.
    pub fn create_document(
        &self,
        request_id: Option<&str>,
        title: Option<&str>,
    ) -> Result<DocumentResponse, ApplicationError> {
        // Read only for a create that runs: a retried one is answered from its record, even
        // when the preferences became unreadable since.
        self.documents.create(request_id, title, || {
            self.preferences.current()?.new_document_settings()
        })
    }

    /// Replaces the document's settings with the built-in defaults, or with the settings a new
    /// document would start from now, as one undoable `update_settings` edit.
    pub fn reset_settings(
        &self,
        id: &str,
        request_id: Option<&str>,
        expected_revision: u64,
        to: SettingsSource,
    ) -> Result<DocumentResponse, ApplicationError> {
        self.reset_settings_selecting(
            id,
            request_id,
            expected_revision,
            to,
            TransactionSelections::default(),
        )
    }

    /// Resets settings with the selection captured when the command was issued.
    pub fn reset_settings_selecting(
        &self,
        id: &str,
        request_id: Option<&str>,
        expected_revision: u64,
        to: SettingsSource,
        selection: TransactionSelections,
    ) -> Result<DocumentResponse, ApplicationError> {
        self.documents.replace_settings(
            id,
            request_id,
            expected_revision,
            SettingsMutation::Reset(to),
            selection,
            || match to {
                SettingsSource::Defaults => Ok(Settings::default()),
                SettingsSource::Preferences => self.preferences.current()?.new_document_settings(),
            },
        )
    }

    /// Applies the settings of document `source` (as this process shows it) to document `id`,
    /// as one undoable `update_settings` edit.
    pub fn import_settings(
        &self,
        id: &str,
        request_id: Option<&str>,
        expected_revision: u64,
        source: &str,
    ) -> Result<DocumentResponse, ApplicationError> {
        self.import_settings_selecting(
            id,
            request_id,
            expected_revision,
            source,
            TransactionSelections::default(),
        )
    }

    /// Imports settings with the selection captured when the command was issued.
    pub fn import_settings_selecting(
        &self,
        id: &str,
        request_id: Option<&str>,
        expected_revision: u64,
        source: &str,
        selection: TransactionSelections,
    ) -> Result<DocumentResponse, ApplicationError> {
        self.documents.replace_settings(
            id,
            request_id,
            expected_revision,
            SettingsMutation::Import(source.to_owned()),
            selection,
            || self.source_settings(source),
        )
    }

    fn source_settings(&self, source: &str) -> Result<Settings, ApplicationError> {
        self.documents
            .settings(source)
            .map_err(|error| ApplicationError {
                message: format!("settings source {source}: {}", error.message),
                ..error
            })
    }

    /// The preferences of this data directory, with the settings new documents start from.
    pub fn preferences(&self) -> Result<PreferencesResponse, ApplicationError> {
        self.preferences.get()
    }

    /// Replaces the preferences, based on `expected_revision`. Computation preferences are not
    /// implemented: naming one is refused and nothing is stored.
    pub fn set_preferences(
        &self,
        request_id: Option<&str>,
        expected_revision: u64,
        change: PreferencesChange,
    ) -> Result<PreferencesResponse, ApplicationError> {
        self.preferences.set(request_id, expected_revision, change)
    }

    /// Resets the preferences to the built-in defaults.
    pub fn reset_preferences(
        &self,
        request_id: Option<&str>,
        expected_revision: u64,
    ) -> Result<PreferencesResponse, ApplicationError> {
        self.preferences.reset(request_id, expected_revision)
    }

    /// Makes new documents start from every setting of `document`, keeping the other
    /// preferences.
    pub fn preferences_from_document(
        &self,
        request_id: Option<&str>,
        expected_revision: u64,
        document: &str,
    ) -> Result<PreferencesResponse, ApplicationError> {
        self.preferences
            .adopt_document(request_id, expected_revision, document, || {
                self.source_settings(document)
            })
    }

    pub fn document(&self, id: &str) -> Result<DocumentResponse, ApplicationError> {
        self.documents.get(id)
    }

    pub fn apply_transaction(
        &self,
        id: &str,
        request_id: Option<&str>,
        expected_revision: u64,
        commands: Vec<CommandDto>,
    ) -> Result<DocumentResponse, ApplicationError> {
        self.documents.transact(
            id,
            request_id,
            expected_revision,
            commands,
            TransactionSelections::default(),
        )
    }

    /// Applies an atomic edit with the selections captured by its caller.
    pub fn apply_transaction_selecting(
        &self,
        id: &str,
        request_id: Option<&str>,
        expected_revision: u64,
        commands: Vec<CommandDto>,
        selection: TransactionSelections,
    ) -> Result<DocumentResponse, ApplicationError> {
        self.documents
            .transact(id, request_id, expected_revision, commands, selection)
    }

    pub fn undo(
        &self,
        id: &str,
        request_id: Option<&str>,
        expected_revision: u64,
    ) -> Result<DocumentResponse, ApplicationError> {
        self.documents.undo(id, request_id, expected_revision)
    }

    pub fn redo(
        &self,
        id: &str,
        request_id: Option<&str>,
        expected_revision: u64,
    ) -> Result<DocumentResponse, ApplicationError> {
        self.documents.redo(id, request_id, expected_revision)
    }

    pub fn save(
        &self,
        id: &str,
        request_id: Option<&str>,
        expected_revision: u64,
    ) -> Result<DocumentResponse, ApplicationError> {
        self.documents.save(id, request_id, expected_revision)
    }

    /// Writes the working copy's recovery draft when it has unsaved changes, or removes the
    /// session's draft when it has none.
    pub fn autosave(
        &self,
        id: &str,
        request_id: Option<&str>,
    ) -> Result<DocumentResponse, ApplicationError> {
        self.documents.autosave(id, request_id)
    }

    /// When the next recovery draft is due, if any unsaved work is undrafted.
    pub fn next_draft_due(&self) -> Option<Instant> {
        self.documents.next_draft_due()
    }

    /// Drafts all unsaved work not yet in a draft, at once; a host calls this before it stops.
    /// Returns the documents that could not be drafted, with the reason.
    pub fn draft_all(&self) -> Vec<(String, ApplicationError)> {
        self.documents.draft_all()
    }

    /// Reopens an abandoned recovery draft as the working copy, with its history, unsaved.
    pub fn recover(
        &self,
        id: &str,
        request_id: Option<&str>,
    ) -> Result<DocumentResponse, ApplicationError> {
        self.documents.recover(id, request_id)
    }

    /// Saves an abandoned recovery draft as a new document and removes the draft.
    pub fn recover_as_copy(
        &self,
        id: &str,
        request_id: Option<&str>,
        title: Option<&str>,
    ) -> Result<CopyResponse, ApplicationError> {
        self.documents.recover_as_copy(id, request_id, title)
    }

    pub fn discard_draft(
        &self,
        id: &str,
        request_id: Option<&str>,
    ) -> Result<DraftDiscardResponse, ApplicationError> {
        self.documents.discard_draft(id, request_id)
    }

    /// Saves the working copy as a new, independent document; the source keeps its last save.
    pub fn save_as(
        &self,
        id: &str,
        request_id: Option<&str>,
        expected_revision: u64,
        title: Option<&str>,
    ) -> Result<CopyResponse, ApplicationError> {
        self.documents
            .save_as(id, request_id, expected_revision, title)
    }

    /// Renames a document without unsaved changes, committing the new title at once.
    pub fn rename(
        &self,
        id: &str,
        request_id: Option<&str>,
        expected_revision: u64,
        title: &str,
    ) -> Result<DocumentResponse, ApplicationError> {
        self.documents
            .rename(id, request_id, expected_revision, title)
    }

    /// Moves a document without unsaved changes or recovery drafts to the trash.
    pub fn delete(
        &self,
        id: &str,
        request_id: Option<&str>,
        expected_revision: u64,
    ) -> Result<DeleteResponse, ApplicationError> {
        self.documents.delete(id, request_id, expected_revision)
    }

    pub fn list_trash(&self) -> Result<TrashListResponse, ApplicationError> {
        self.documents.list_trash()
    }

    pub fn restore(
        &self,
        id: &str,
        request_id: Option<&str>,
    ) -> Result<RestoreResponse, ApplicationError> {
        self.documents.restore(id, request_id)
    }

    pub fn close(
        &self,
        id: &str,
        request_id: Option<&str>,
        discard_unsaved: bool,
        expected_revision: Option<u64>,
    ) -> Result<CloseResponse, ApplicationError> {
        self.documents
            .close(id, request_id, discard_unsaved, expected_revision)
    }
}

impl<C, R> Application<C, R>
where
    C: CapabilityCatalog + 'static,
    R: DocumentRepository + 'static,
{
    /// Starts writing the recovery drafts due at `now`, each on its own thread, and returns
    /// those threads by document. A long-running host calls this regularly, so unsaved work is
    /// drafted even when no client autosaves; see [`DRAFT_QUIET`], [`DRAFT_MAX_WAIT`], and
    /// [`DRAFT_RETRY`]. A document whose draft is still being written is not started again,
    /// so a write that stalls holds up neither the caller nor other documents' drafts.
    ///
    /// A document whose thread cannot be started is due again on the next call.
    pub fn start_due_drafts(self: &Arc<Self>, now: Instant) -> Vec<(String, DraftAttempt)> {
        self.documents
            .due_drafts(now)
            .into_iter()
            .filter_map(|id| {
                let application = Arc::clone(self);
                let document = id.clone();
                thread::Builder::new()
                    .name(format!("draft {id}"))
                    .spawn(move || application.documents.write_due_draft(&document, now))
                    .ok()
                    .map(|attempt| (id, attempt))
            })
            .collect()
    }
}

/// A draft write started by [`Application::start_due_drafts`]; joining it gives its outcome,
/// which is also kept on the document's session.
pub type DraftAttempt = JoinHandle<Result<DocumentResponse, ApplicationError>>;

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub api_version: &'static str,
    pub service: &'static str,
    pub version: &'static str,
    pub stage: &'static str,
    pub status: &'static str,
}

#[derive(Debug, Serialize)]
pub struct DataStatusResponse {
    pub api_version: &'static str,
    #[serde(flatten)]
    pub status: RegistryStatus,
}

#[derive(Debug, Serialize)]
pub struct CapabilitiesResponse {
    pub api_version: &'static str,
    pub capabilities: Vec<CapabilityResponse>,
    /// Support per operation of the operation contract, as `capabilities.inspect` reports it.
    /// Added within v1.
    pub operation_catalog: OperationCatalog,
}

#[derive(Debug, Serialize)]
pub struct CapabilityResponse {
    pub id: &'static str,
    pub label: &'static str,
    pub status: &'static str,
    pub description: &'static str,
}
