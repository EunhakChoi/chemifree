//! Document editing sessions: working copies, revisions, idempotent replays, and saves.

use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, MutexGuard, PoisonError};

use chemifree_domain::document::{Document, DocumentId, HistoryError, Outcome};

use crate::API_VERSION;
use crate::contract::{
    self, CloseResponse, CommandDto, DocumentListResponse, DocumentResponse, DocumentSummary,
    UnreadableDocument,
};
use crate::error::{ApplicationError, ErrorCode};
use crate::repository::{DocumentRepository, StorageError};

pub const DEFAULT_TITLE: &str = "Untitled";
pub const MAX_OPEN_SESSIONS: usize = 64;
/// Successful mutations remembered per session for `request_id` replay.
pub const REPLAY_LIMIT: usize = 64;
pub const MAX_REQUEST_ID_CHARS: usize = 64;
pub const MAX_LISTED_DOCUMENTS: usize = 1_000;

/// What a request asked for, compared when a `request_id` is seen again.
#[derive(Clone, Debug, PartialEq)]
enum Mutation {
    Create {
        title: Option<String>,
    },
    Transaction {
        expected_revision: u64,
        commands: Vec<CommandDto>,
    },
    Undo {
        expected_revision: u64,
    },
    Redo {
        expected_revision: u64,
    },
    Save {
        expected_revision: u64,
    },
}

#[derive(Default)]
struct ReplayLog(VecDeque<(String, Mutation, DocumentResponse)>);

impl ReplayLog {
    /// `Ok(Some)` replays a completed request; `Err` rejects a reused ID.
    fn find(
        &self,
        request_id: Option<&str>,
        mutation: &Mutation,
    ) -> Result<Option<DocumentResponse>, ApplicationError> {
        let Some(request_id) = request_id else {
            return Ok(None);
        };
        match self.0.iter().find(|(id, _, _)| id == request_id) {
            Some((_, recorded, response)) if recorded == mutation => Ok(Some(response.clone())),
            Some(_) => Err(ApplicationError::new(
                ErrorCode::RequestIdReused,
                format!("request_id {request_id:?} was already used for a different request"),
            )),
            None => Ok(None),
        }
    }

    fn record(
        &mut self,
        request_id: Option<&str>,
        mutation: Mutation,
        response: &DocumentResponse,
    ) {
        if let Some(request_id) = request_id {
            self.0
                .push_back((request_id.to_owned(), mutation, response.clone()));
            if self.0.len() > REPLAY_LIMIT {
                self.0.pop_front();
            }
        }
    }
}

struct Session {
    document: Document,
    saved_revision: u64,
    replays: ReplayLog,
}

impl Session {
    fn response(&self) -> DocumentResponse {
        DocumentResponse::new(&self.document, self.saved_revision)
    }

    fn is_dirty(&self) -> bool {
        self.document.revision() != self.saved_revision
    }

    fn expect_revision(&self, expected: u64) -> Result<(), ApplicationError> {
        let current = self.document.revision();
        if expected == current {
            return Ok(());
        }
        Err(ApplicationError {
            current_revision: Some(current),
            ..ApplicationError::new(
                ErrorCode::RevisionConflict,
                format!("expected revision {expected}, but the document is at revision {current}"),
            )
        })
    }
}

#[derive(Default)]
struct Sessions {
    open: HashMap<DocumentId, Session>,
    created: ReplayLog,
}

/// Owns open sessions. One working copy exists per document per process; a session is
/// opened from storage on first access and replaced only after it is closed.
pub(crate) struct Documents<R> {
    repository: R,
    sessions: Mutex<Sessions>,
}

impl<R: DocumentRepository> Documents<R> {
    pub(crate) fn new(repository: R) -> Self {
        Self {
            repository,
            sessions: Mutex::new(Sessions::default()),
        }
    }

    /// Every mutation is applied to a copy or validated first, so a panic while the lock
    /// is held cannot leave a half-applied document behind; recovering the guard is safe.
    fn lock(&self) -> MutexGuard<'_, Sessions> {
        self.sessions.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn list(&self) -> Result<DocumentListResponse, ApplicationError> {
        let sessions = self.lock();
        let mut ids = self.repository.list_ids().map_err(storage_error)?;
        ids.extend(sessions.open.keys().cloned());
        ids.sort();
        ids.dedup();
        let mut documents = Vec::new();
        let mut unreadable = Vec::new();
        for id in ids.into_iter().take(MAX_LISTED_DOCUMENTS) {
            let summary = |document: &Document, open, unsaved_changes| DocumentSummary {
                id: id.to_string(),
                title: document.title().to_owned(),
                revision: document.revision(),
                atom_count: document.atoms().len(),
                bond_count: document.bonds().len(),
                open,
                unsaved_changes,
            };
            if let Some(session) = sessions.open.get(&id) {
                documents.push(summary(&session.document, true, session.is_dirty()));
                continue;
            }
            match self.repository.load(&id) {
                Ok(Some(document)) => documents.push(summary(&document, false, false)),
                Ok(None) => {}
                Err(error) => unreadable.push(UnreadableDocument {
                    id: id.to_string(),
                    message: error.to_string(),
                }),
            }
        }
        documents.sort_by(|a, b| a.title.cmp(&b.title).then_with(|| a.id.cmp(&b.id)));
        Ok(DocumentListResponse {
            api_version: API_VERSION,
            documents,
            unreadable,
        })
    }

    /// Creates an empty document and commits it to storage before acknowledging.
    pub(crate) fn create(
        &self,
        request_id: Option<&str>,
        title: Option<&str>,
    ) -> Result<DocumentResponse, ApplicationError> {
        validate_request_id(request_id)?;
        let mutation = Mutation::Create {
            title: title.map(str::to_owned),
        };
        let mut sessions = self.lock();
        if let Some(response) = sessions.created.find(request_id, &mutation)? {
            return Ok(response);
        }
        make_room(&mut sessions.open, None)?;
        let id = self.repository.allocate_id().map_err(storage_error)?;
        let document =
            Document::new(id.clone(), title.unwrap_or(DEFAULT_TITLE)).map_err(|error| {
                ApplicationError::new(ErrorCode::InvalidRequest, format!("title: {error}"))
            })?;
        self.repository
            .save(&document, None)
            .map_err(storage_error)?;
        let session = Session {
            saved_revision: document.revision(),
            document,
            replays: ReplayLog::default(),
        };
        let response = session.response();
        sessions.created.record(request_id, mutation, &response);
        sessions.open.insert(id, session);
        Ok(response)
    }

    /// Returns the working copy, opening it from storage if needed.
    pub(crate) fn get(&self, id: &str) -> Result<DocumentResponse, ApplicationError> {
        let mut sessions = self.lock();
        Ok(self.session(&mut sessions, id)?.response())
    }

    pub(crate) fn transact(
        &self,
        id: &str,
        request_id: Option<&str>,
        expected_revision: u64,
        commands: Vec<CommandDto>,
    ) -> Result<DocumentResponse, ApplicationError> {
        let domain_commands = contract::to_commands(&commands)?;
        let mutation = Mutation::Transaction {
            expected_revision,
            commands,
        };
        self.mutate(id, request_id, expected_revision, mutation, |session| {
            session
                .document
                .apply(&domain_commands)
                .map(|_: Outcome| ())
                .map_err(|error| ApplicationError {
                    command_index: error.index,
                    ..ApplicationError::new(ErrorCode::InvalidCommand, error.to_string())
                })
        })
    }

    pub(crate) fn undo(
        &self,
        id: &str,
        request_id: Option<&str>,
        expected_revision: u64,
    ) -> Result<DocumentResponse, ApplicationError> {
        let mutation = Mutation::Undo { expected_revision };
        self.mutate(id, request_id, expected_revision, mutation, |session| {
            session.document.undo().map_err(history_error)
        })
    }

    pub(crate) fn redo(
        &self,
        id: &str,
        request_id: Option<&str>,
        expected_revision: u64,
    ) -> Result<DocumentResponse, ApplicationError> {
        let mutation = Mutation::Redo { expected_revision };
        self.mutate(id, request_id, expected_revision, mutation, |session| {
            session.document.redo().map_err(history_error)
        })
    }

    /// Commits the working copy. The acknowledgement is sent only after the provider
    /// reports a durable commit; on failure the session keeps its unsaved changes.
    pub(crate) fn save(
        &self,
        id: &str,
        request_id: Option<&str>,
        expected_revision: u64,
    ) -> Result<DocumentResponse, ApplicationError> {
        let repository = &self.repository;
        let mutation = Mutation::Save { expected_revision };
        self.mutate(id, request_id, expected_revision, mutation, |session| {
            if session.is_dirty() {
                repository
                    .save(&session.document, Some(session.saved_revision))
                    .map_err(|error| match error {
                        StorageError::Unavailable(message) => ApplicationError::new(
                            ErrorCode::StorageFailed,
                            format!(
                                "save failed; this session keeps its unsaved changes: {message}"
                            ),
                        ),
                        error => storage_error(error),
                    })?;
                session.saved_revision = session.document.revision();
            }
            Ok(())
        })
    }

    /// Ends the session; the next access reopens the committed document from storage.
    pub(crate) fn close(
        &self,
        id: &str,
        request_id: Option<&str>,
        discard_unsaved: bool,
    ) -> Result<CloseResponse, ApplicationError> {
        validate_request_id(request_id)?;
        let document_id = parse_id(id)?;
        let mut sessions = self.lock();
        let discarded_changes = match sessions.open.get(&document_id) {
            Some(session) if session.is_dirty() && !discard_unsaved => {
                return Err(ApplicationError::new(
                    ErrorCode::UnsavedChanges,
                    format!(
                        "revision {} is not saved; save first or close with discard_unsaved",
                        session.document.revision()
                    ),
                ));
            }
            Some(session) => session.is_dirty(),
            None => {
                if self.load(&document_id)?.is_none() {
                    return Err(not_found(&document_id));
                }
                false
            }
        };
        sessions.open.remove(&document_id);
        Ok(CloseResponse {
            api_version: API_VERSION,
            id: document_id.to_string(),
            discarded_changes,
        })
    }

    /// Replays a completed request, or checks the revision and applies `change`.
    fn mutate(
        &self,
        id: &str,
        request_id: Option<&str>,
        expected_revision: u64,
        mutation: Mutation,
        change: impl FnOnce(&mut Session) -> Result<(), ApplicationError>,
    ) -> Result<DocumentResponse, ApplicationError> {
        validate_request_id(request_id)?;
        let mut sessions = self.lock();
        let session = self.session(&mut sessions, id)?;
        if let Some(response) = session.replays.find(request_id, &mutation)? {
            return Ok(response);
        }
        session.expect_revision(expected_revision)?;
        change(session)?;
        let response = session.response();
        session.replays.record(request_id, mutation, &response);
        Ok(response)
    }

    fn session<'a>(
        &self,
        sessions: &'a mut Sessions,
        id: &str,
    ) -> Result<&'a mut Session, ApplicationError> {
        let id = parse_id(id)?;
        if !sessions.open.contains_key(&id) {
            make_room(&mut sessions.open, Some(&id))?;
            let document = self.load(&id)?.ok_or_else(|| not_found(&id))?;
            let session = Session {
                saved_revision: document.revision(),
                document,
                replays: ReplayLog::default(),
            };
            sessions.open.insert(id.clone(), session);
        }
        Ok(sessions
            .open
            .get_mut(&id)
            .expect("session was just ensured"))
    }

    fn load(&self, id: &DocumentId) -> Result<Option<Document>, ApplicationError> {
        self.repository.load(id).map_err(storage_error)
    }
}

/// Evicts one clean session when the table is full. Sessions with unsaved work stay.
fn make_room(
    open: &mut HashMap<DocumentId, Session>,
    keep: Option<&DocumentId>,
) -> Result<(), ApplicationError> {
    if open.len() < MAX_OPEN_SESSIONS {
        return Ok(());
    }
    let clean = open
        .iter()
        .find(|(id, session)| Some(*id) != keep && !session.is_dirty())
        .map(|(id, _)| id.clone());
    match clean {
        Some(id) => {
            open.remove(&id);
            Ok(())
        }
        None => Err(ApplicationError::new(
            ErrorCode::ResourceLimit,
            format!("{MAX_OPEN_SESSIONS} documents have unsaved changes; save or close one first"),
        )),
    }
}

fn validate_request_id(request_id: Option<&str>) -> Result<(), ApplicationError> {
    let valid = request_id.is_none_or(|id| {
        (1..=MAX_REQUEST_ID_CHARS).contains(&id.len())
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    });
    if valid {
        Ok(())
    } else {
        Err(ApplicationError::new(
            ErrorCode::InvalidRequest,
            format!("request_id must be 1-{MAX_REQUEST_ID_CHARS} characters of [A-Za-z0-9_-]"),
        ))
    }
}

fn parse_id(id: &str) -> Result<DocumentId, ApplicationError> {
    DocumentId::parse(id).ok_or_else(|| {
        ApplicationError::new(
            ErrorCode::DocumentNotFound,
            format!("{id:?} is not a document identifier"),
        )
    })
}

fn not_found(id: &DocumentId) -> ApplicationError {
    ApplicationError::new(
        ErrorCode::DocumentNotFound,
        format!("document {id} does not exist"),
    )
}

fn history_error(error: HistoryError) -> ApplicationError {
    let code = match error {
        HistoryError::NothingToUndo => ErrorCode::NothingToUndo,
        HistoryError::NothingToRedo => ErrorCode::NothingToRedo,
    };
    ApplicationError::new(code, error.to_string())
}

fn storage_error(error: StorageError) -> ApplicationError {
    match error {
        StorageError::Conflict { .. } => ApplicationError::new(
            ErrorCode::SaveConflict,
            format!(
                "the stored document changed elsewhere ({error}); close with discard_unsaved and reopen"
            ),
        ),
        StorageError::Corrupt(message) => ApplicationError::new(
            ErrorCode::DocumentUnreadable,
            format!("the stored document is unreadable: {message}"),
        ),
        StorageError::Unavailable(message) => ApplicationError::new(
            ErrorCode::StorageFailed,
            format!("storage failed: {message}"),
        ),
    }
}
