//! Shared use cases and v1 contracts for HTTP and CLI entry adapters.

mod contract;
mod documents;
mod error;
mod repository;

use chemifree_domain::Capability;
use serde::Serialize;

pub use contract::{
    AtomView, BondOrderDto, BondToDto, BondView, CloseRequest, CloseResponse, CommandDto,
    CopyResponse, CopySource, CreateDocumentRequest, DocumentListResponse, DocumentResponse,
    DocumentSummary, DocumentView, DraftDiscardResponse, DraftRequest, PointDto, PositionView,
    RecoverAsCopyRequest, RecoveryState, RecoveryView, RevisionRequest, SaveAsRequest, SessionView,
    TransactionRequest, UnreadableDocument,
};
pub use documents::{
    DEFAULT_TITLE, MAX_LISTED_DOCUMENTS, MAX_OPEN_SESSIONS, MAX_REQUEST_ID_CHARS, REPLAY_LIMIT,
};
pub use error::{ApplicationError, ErrorCode, ErrorResponse};
pub use repository::{DocumentRepository, Draft, DraftInfo, DraftStatus, StorageError};

use documents::Documents;

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
}

impl<C: CapabilityCatalog, R: DocumentRepository> Application<C, R> {
    pub fn new(catalog: C, repository: R) -> Self {
        Self {
            catalog,
            documents: Documents::new(repository),
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
        }
    }

    pub fn list_documents(&self) -> Result<DocumentListResponse, ApplicationError> {
        self.documents.list()
    }

    pub fn create_document(
        &self,
        request_id: Option<&str>,
        title: Option<&str>,
    ) -> Result<DocumentResponse, ApplicationError> {
        self.documents.create(request_id, title)
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
        self.documents
            .transact(id, request_id, expected_revision, commands)
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

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub api_version: &'static str,
    pub service: &'static str,
    pub version: &'static str,
    pub stage: &'static str,
    pub status: &'static str,
}

#[derive(Debug, Serialize)]
pub struct CapabilitiesResponse {
    pub api_version: &'static str,
    pub capabilities: Vec<CapabilityResponse>,
}

#[derive(Debug, Serialize)]
pub struct CapabilityResponse {
    pub id: &'static str,
    pub label: &'static str,
    pub status: &'static str,
    pub description: &'static str,
}
