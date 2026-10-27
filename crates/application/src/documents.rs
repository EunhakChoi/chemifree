//! Document editing sessions: working copies, revisions, idempotent replays, and saves.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Mutex, MutexGuard, PoisonError};

use chemifree_domain::document::{
    Document, DocumentId, EditError, HistoryError, Numbering, Outcome,
};

use crate::API_VERSION;
use crate::contract::{
    self, CloseResponse, CommandDto, DocumentListResponse, DocumentResponse, DocumentSummary,
    UnreadableDocument,
};
use crate::error::{ApplicationError, ErrorCode};
use crate::repository::{DocumentRepository, StorageError};

pub const DEFAULT_TITLE: &str = "Untitled";
pub const MAX_OPEN_SESSIONS: usize = 64;
/// Successful mutations remembered per session (and per process for creates and closes)
/// for `request_id` replay.
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
    Close {
        document: DocumentId,
        discard_unsaved: bool,
        expected_revision: Option<u64>,
    },
}

struct ReplayLog<T>(VecDeque<(String, Mutation, T)>);

impl<T> Default for ReplayLog<T> {
    fn default() -> Self {
        Self(VecDeque::new())
    }
}

impl<T: Clone> ReplayLog<T> {
    /// `Ok(Some)` replays a completed request; `Err` rejects a reused ID.
    fn find(
        &self,
        request_id: Option<&str>,
        mutation: &Mutation,
    ) -> Result<Option<T>, ApplicationError> {
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

    fn record(&mut self, request_id: Option<&str>, mutation: Mutation, response: &T) {
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
    /// The revision storage holds: the base the next save must find there.
    stored_revision: u64,
    /// This session's revision whose content is the stored commit. It is above
    /// `stored_revision` when the copy continued after numbers handed out by a retired copy.
    saved_revision: u64,
    replays: ReplayLog<DocumentResponse>,
}

impl Session {
    /// A working copy of a stored document, numbered after what earlier copies handed out.
    fn open(mut document: Document, retired: Option<Retired>) -> Self {
        let stored_revision = document.revision();
        if let Some(retired) = retired {
            document.continue_after(retired.continue_after(stored_revision));
        }
        Self {
            stored_revision,
            saved_revision: document.revision(),
            document,
            replays: ReplayLog::default(),
        }
    }

    fn response(&self) -> DocumentResponse {
        DocumentResponse::new(&self.document, self.saved_revision)
    }

    fn is_dirty(&self) -> bool {
        self.document.revision() != self.saved_revision
    }

    /// What a later copy must not hand out again once this one ends, if anything.
    fn retirement(&self) -> Option<Retired> {
        let reached = self.document.numbering();
        if self.is_dirty() {
            Some(Retired {
                reached,
                clean_on: None,
            })
        } else if self.saved_revision != self.stored_revision {
            Some(Retired {
                reached,
                clean_on: Some(self.stored_revision),
            })
        } else {
            None
        }
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

/// Numbering a closed or evicted copy reached beyond what storage holds: revisions and
/// identifiers of discarded edits, or a revision renumbered to name the stored commit.
#[derive(Clone, Copy)]
struct Retired {
    reached: Numbering,
    /// Set when the copy ended clean: its revision named the commit stored at this revision,
    /// so a copy reopened on that same commit may name it the same way again.
    clean_on: Option<u64>,
}

impl Retired {
    /// The numbering a copy opened on the commit stored at `stored_revision` continues after.
    fn continue_after(self, stored_revision: u64) -> Numbering {
        let mut reached = self.reached;
        if self.clean_on == Some(stored_revision) {
            reached.revision = reached.revision.saturating_sub(1);
        }
        reached
    }

    fn merge(self, newer: Self) -> Self {
        Self {
            reached: self.reached.max(newer.reached),
            clean_on: newer
                .clean_on
                .filter(|_| newer.reached.revision >= self.reached.revision),
        }
    }
}

#[derive(Default)]
struct Sessions {
    open: HashMap<DocumentId, Session>,
    /// Kept so that a revision or identifier a client has seen never names a different state
    /// later in this process; a reopened copy continues after it.
    retired: HashMap<DocumentId, Retired>,
    created: ReplayLog<DocumentResponse>,
    /// Kept outside the sessions they end, so a retried close cannot end a newer session.
    closed: ReplayLog<CloseResponse>,
}

impl Sessions {
    /// Ends a working copy. Every removal goes through here so retirement is never skipped.
    fn remove(&mut self, id: &DocumentId) -> Option<Session> {
        let session = self.open.remove(id)?;
        if let Some(retired) = session.retirement() {
            let merged = match self.retired.get(id) {
                Some(known) => known.merge(retired),
                None => retired,
            };
            self.retired.insert(id.clone(), merged);
        }
        Some(session)
    }

    /// A clean session to evict when the table is full, checked before any change so a
    /// request that fails afterwards has not evicted anything.
    fn eviction(&self, keep: Option<&DocumentId>) -> Result<Option<DocumentId>, ApplicationError> {
        if self.open.len() < MAX_OPEN_SESSIONS {
            return Ok(None);
        }
        self.open
            .iter()
            .find(|(id, session)| Some(*id) != keep && !session.is_dirty())
            .map(|(id, _)| Some(id.clone()))
            .ok_or_else(|| {
                ApplicationError::new(
                    ErrorCode::ResourceLimit,
                    format!(
                        "{MAX_OPEN_SESSIONS} documents have unsaved changes; save or close one first"
                    ),
                )
            })
    }
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

    /// Open sessions are summarized under the lock; other stored documents are restored
    /// without holding it, so listing does not stall edits to open documents.
    pub(crate) fn list(&self) -> Result<DocumentListResponse, ApplicationError> {
        let stored = self.repository.list_ids().map_err(storage_error)?;
        let mut documents = Vec::new();
        let open: HashSet<DocumentId> = {
            let mut sessions = self.lock();
            for (id, session) in &mut sessions.open {
                self.refresh(id, session);
                documents.push(summary(id, &session.document, true, session.is_dirty()));
            }
            sessions.open.keys().cloned().collect()
        };
        let mut unreadable = Vec::new();
        let remaining = MAX_LISTED_DOCUMENTS.saturating_sub(documents.len());
        for id in stored
            .into_iter()
            .filter(|id| !open.contains(id))
            .take(remaining)
        {
            match self.repository.load(&id) {
                Ok(Some(document)) => documents.push(summary(&id, &document, false, false)),
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
        let id = self.repository.allocate_id().map_err(storage_error)?;
        let document =
            Document::new(id.clone(), title.unwrap_or(DEFAULT_TITLE)).map_err(|error| {
                ApplicationError::new(ErrorCode::InvalidRequest, format!("title: {error}"))
            })?;
        let eviction = sessions.eviction(None)?;
        self.repository
            .save(&document, None)
            .map_err(storage_error)?;
        if let Some(evicted) = eviction {
            sessions.remove(&evicted);
        }
        let session = Session::open(document, None);
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
                .map_err(|error| {
                    let code = match error.error {
                        EditError::RevisionLimit => ErrorCode::ResourceLimit,
                        _ => ErrorCode::InvalidCommand,
                    };
                    ApplicationError {
                        command_index: error.index,
                        ..ApplicationError::new(code, error.to_string())
                    }
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
        let response = self.mutate(id, request_id, expected_revision, mutation, |session| {
            if session.is_dirty() {
                repository
                    .save(&session.document, Some(session.stored_revision))
                    .map_err(|error| match error {
                        StorageError::Unavailable(message) => ApplicationError::new(
                            ErrorCode::StorageFailed,
                            format!(
                                "save failed; this session keeps its unsaved changes: {message}"
                            ),
                        ),
                        error => storage_error(error),
                    })?;
                session.stored_revision = session.document.revision();
                session.saved_revision = session.document.revision();
            }
            Ok(())
        })?;
        // Storage now holds a revision above everything retired copies handed out.
        let mut sessions = self.lock();
        if let Ok(id) = parse_id(id)
            && sessions
                .retired
                .get(&id)
                .is_some_and(|retired| retired.reached.revision < response.document.revision)
        {
            sessions.retired.remove(&id);
        }
        Ok(response)
    }

    /// Ends the session; the next access reopens the committed document from storage.
    ///
    /// Discarding names the revision the client agreed to lose. The session is shared by every
    /// client of this process, so without it a stale confirmation could discard edits another
    /// client made after it.
    pub(crate) fn close(
        &self,
        id: &str,
        request_id: Option<&str>,
        discard_unsaved: bool,
        expected_revision: Option<u64>,
    ) -> Result<CloseResponse, ApplicationError> {
        validate_request_id(request_id)?;
        let document_id = parse_id(id)?;
        if discard_unsaved && expected_revision.is_none() {
            return Err(ApplicationError::new(
                ErrorCode::InvalidRequest,
                "discard_unsaved requires expected_revision",
            ));
        }
        let mutation = Mutation::Close {
            document: document_id.clone(),
            discard_unsaved,
            expected_revision,
        };
        let mut sessions = self.lock();
        if let Some(response) = sessions.closed.find(request_id, &mutation)? {
            return Ok(response);
        }
        if let (Some(session), Some(expected)) =
            (sessions.open.get(&document_id), expected_revision)
        {
            session.expect_revision(expected)?;
        }
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
        sessions.remove(&document_id);
        let response = CloseResponse {
            api_version: API_VERSION,
            id: document_id.to_string(),
            discarded_changes,
        };
        sessions.closed.record(request_id, mutation, &response);
        Ok(response)
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
        if let Some(session) = sessions.open.get_mut(&id) {
            self.refresh(&id, session);
        } else {
            let document = self.load(&id)?.ok_or_else(|| not_found(&id))?;
            if let Some(evicted) = sessions.eviction(Some(&id))? {
                sessions.remove(&evicted);
            }
            let retired = sessions.retired.get(&id).copied();
            sessions
                .open
                .insert(id.clone(), Session::open(document, retired));
        }
        Ok(sessions
            .open
            .get_mut(&id)
            .expect("session was just ensured"))
    }

    /// A clean working copy follows newer commits by other writers (another process or the
    /// CLI), so it never shows stale content as saved. A session with unsaved changes keeps
    /// them and meets the other commit as a save conflict. Storage errors leave the copy as
    /// it is: it is still the last commit this session saw.
    fn refresh(&self, id: &DocumentId, session: &mut Session) {
        if session.is_dirty() {
            return;
        }
        let newer = matches!(
            self.repository.stored_revision(id),
            Ok(Some(stored)) if stored > session.stored_revision
        );
        if newer && let Ok(Some(document)) = self.repository.load(id) {
            let replays = std::mem::take(&mut session.replays);
            // The copy being replaced named the old commit; the new one must not reuse that.
            let reached = Retired {
                reached: session.document.numbering(),
                clean_on: None,
            };
            *session = Session {
                replays,
                ..Session::open(document, Some(reached))
            };
        }
    }

    fn load(&self, id: &DocumentId) -> Result<Option<Document>, ApplicationError> {
        self.repository.load(id).map_err(storage_error)
    }
}

fn summary(
    id: &DocumentId,
    document: &Document,
    open: bool,
    unsaved_changes: bool,
) -> DocumentSummary {
    DocumentSummary {
        id: id.to_string(),
        title: document.title().to_owned(),
        revision: document.revision(),
        atom_count: document.atoms().len(),
        bond_count: document.bonds().len(),
        open,
        unsaved_changes,
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
        HistoryError::RevisionLimit => ErrorCode::ResourceLimit,
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
