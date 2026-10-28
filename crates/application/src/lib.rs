//! Shared use cases and v1 contracts for HTTP and CLI entry adapters.

mod contract;
mod documents;
mod draft_schedule;
mod draft_writer;
mod error;
pub mod json;
mod operations;
mod repository;
mod turns;

use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Instant;

use chemifree_domain::Capability;
use serde::Serialize;

pub use contract::{
    AtomView, BondOrderDto, BondToDto, BondView, CloseRequest, CloseResponse, CommandDto,
    CopyResponse, CopySource, CreateDocumentRequest, DeleteResponse, DocumentListResponse,
    DocumentResponse, DocumentSummary, DocumentView, DraftDiscardResponse, DraftFailureView,
    DraftRequest, ListQuery, ListSort, PointDto, PositionView, RecoverAsCopyRequest, RecoveryState,
    RecoveryView, RenameRequest, RestoreRequest, RestoreResponse, RevisionRequest, SaveAsRequest,
    SessionView, TransactionRequest, TrashListResponse, TrashedSummary, UnreadableDocument,
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
pub use repository::{DocumentRepository, Draft, DraftInfo, DraftStatus, StorageError, TrashEntry};
pub use turns::TURN_WAIT;

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
            operation_catalog: self.operation_catalog(),
        }
    }

    pub fn list_documents(
        &self,
        query: &ListQuery,
    ) -> Result<DocumentListResponse, ApplicationError> {
        self.documents.list(query)
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
