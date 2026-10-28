//! Document editing sessions: working copies, revisions, idempotent replays, saves, recovery
//! drafts, and copies.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use chemifree_domain::document::{
    Command, Document, DocumentId, EditError, HistoryError, MAX_TITLE_CHARS, Numbering, Outcome,
    RevisionsExhausted, TransactionError,
};

use crate::API_VERSION;
use crate::contract::{
    self, CloseResponse, CommandDto, CopyResponse, CopySource, DeleteResponse,
    DocumentListResponse, DocumentResponse, DocumentSummary, DraftDiscardResponse, ListQuery,
    ListSort, RecoveryState, RecoveryView, RestoreResponse, TrashListResponse, TrashedSummary,
    UnreadableDocument,
};
use crate::error::{ApplicationError, ErrorCode, StorageReason};
use crate::repository::{DocumentRepository, Draft, DraftStatus, StorageError};
use crate::turns::{Refusal, Running, TURN_WAIT, Turn, Turns};

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
    Autosave,
    SaveAs {
        document: DocumentId,
        expected_revision: u64,
        title: Option<String>,
    },
    Recover {
        document: DocumentId,
    },
    RecoverAsCopy {
        document: DocumentId,
        title: Option<String>,
    },
    DiscardDraft {
        document: DocumentId,
    },
    Rename {
        expected_revision: u64,
        title: String,
    },
    Delete {
        document: DocumentId,
        expected_revision: u64,
    },
    Restore {
        document: DocumentId,
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
    /// The revision held by this session's recovery draft, which the repository owns.
    draft_revision: Option<u64>,
    /// Set while the clean copy is being renamed or deleted in storage: edits wait for the
    /// outcome instead of changing a copy that is about to be replaced or end.
    hold: Option<Hold>,
    replays: ReplayLog<DocumentResponse>,
}

/// A storage change that briefly holds a session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Hold {
    Rename,
    Delete,
}

impl Session {
    /// A working copy of a document as stored.
    fn new(document: Document) -> Self {
        Self {
            stored_revision: document.revision(),
            saved_revision: document.revision(),
            draft_revision: None,
            hold: None,
            document,
            replays: ReplayLog::default(),
        }
    }

    /// A working copy of a stored document, numbered after what earlier copies handed out.
    fn open(document: Document, retired: Option<Retired>) -> Result<Self, ApplicationError> {
        let mut session = Self::new(document);
        if let Some(retired) = retired {
            session
                .document
                .continue_after(retired.continue_after(session.stored_revision))
                .map_err(revisions_exhausted)?;
            session.saved_revision = session.document.revision();
        }
        Ok(session)
    }

    fn response(&self) -> DocumentResponse {
        DocumentResponse::new(&self.document, self.saved_revision, self.draft_revision)
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
    recovered: ReplayLog<DocumentResponse>,
    copied: ReplayLog<CopyResponse>,
    drafts_discarded: ReplayLog<DraftDiscardResponse>,
    /// Kept outside the sessions they end, so a retried close cannot end a newer session.
    closed: ReplayLog<CloseResponse>,
    deleted: ReplayLog<DeleteResponse>,
    restored: ReplayLog<RestoreResponse>,
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
    /// request that fails afterwards has not evicted anything. `spare` is chosen only when no
    /// other session is clean.
    fn eviction(&self, spare: Option<&DocumentId>) -> Result<Option<DocumentId>, ApplicationError> {
        if self.open.len() < MAX_OPEN_SESSIONS {
            return Ok(None);
        }
        // The lowest identifier, so the choice does not depend on hash order.
        let clean = self
            .open
            .iter()
            // A session held by a rename or delete in progress is not evicted under it.
            .filter(|(_, session)| !session.is_dirty() && session.hold.is_none())
            .map(|(id, _)| id);
        clean
            .clone()
            .filter(|id| Some(*id) != spare)
            .min()
            .or_else(|| clean.min())
            .map(|id| Some(id.clone()))
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

/// Where the session of a new copy goes.
enum CopySlot<'a> {
    /// The open source session's, which the copy replaces.
    Source,
    /// A clean session's, evicted if the table is full; `spare`'s only if no other is clean.
    Evicted { spare: &'a DocumentId },
}

/// Owns open sessions. One working copy exists per document per process; a session is
/// opened from storage on first access and replaced only after it is closed.
///
/// Writes that commit a document or touch its recovery draft first mark their `request_id`
/// as running, then look for a completed request with that ID, and only then wait (at most
/// `TURN_WAIT`) for the document's turn, which serializes them per document. Lock order: the
/// document's turn, then `sessions`. A save waits for storage (which may wait for another
/// process's save) holding only its turn, so requests for other documents, and edits to the
/// same one, are not held up.
pub(crate) struct Documents<R> {
    repository: R,
    sessions: Mutex<Sessions>,
    turns: Turns,
}

impl<R: DocumentRepository> Documents<R> {
    pub(crate) fn new(repository: R) -> Self {
        Self {
            repository,
            sessions: Mutex::new(Sessions::default()),
            turns: Turns::default(),
        }
    }

    /// Marks the request as running. A repeated `request_id` that is still running is
    /// answered at once, without waiting for any other lock. `scope` is where the request's
    /// reply is remembered: its document for saves and autosaves (kept with the session),
    /// `None` for requests kept per process (creates, closes, recoveries, draft discards,
    /// copies).
    fn start(
        &self,
        scope: Option<&DocumentId>,
        request_id: Option<&str>,
    ) -> Result<Running<'_>, ApplicationError> {
        self.turns.start(scope, request_id).map_err(refused)
    }

    /// Waits for the document's turn, at most `TURN_WAIT`.
    fn turn(&self, document: &DocumentId) -> Result<Turn<'_>, ApplicationError> {
        self.turns.take(document, TURN_WAIT).map_err(refused)
    }

    /// The reply of a completed request with this `request_id`, looked up before waiting for
    /// the document's turn, so a request asked about again is not held up by others.
    fn replayed<T>(
        &self,
        request_id: Option<&str>,
        find: impl FnOnce(&Sessions) -> Result<Option<T>, ApplicationError>,
    ) -> Result<Option<T>, ApplicationError> {
        if request_id.is_none() {
            return Ok(None);
        }
        find(&self.lock())
    }

    /// Every mutation is applied to a copy or validated first, so a panic while the lock
    /// is held cannot leave a half-applied document behind; recovering the guard is safe.
    fn lock(&self) -> MutexGuard<'_, Sessions> {
        self.sessions.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Open sessions are summarized under the lock; other stored documents are restored
    /// without holding it, so listing does not stall edits to open documents.
    /// Stored and open documents whose title contains `query.query` (ignoring case), in
    /// `query.sort` order. Unreadable files are always reported.
    pub(crate) fn list(&self, query: &ListQuery) -> Result<DocumentListResponse, ApplicationError> {
        let needle = match query.query.as_deref().map(str::trim) {
            Some(text) if text.chars().count() > MAX_TITLE_CHARS => {
                return Err(ApplicationError::new(
                    ErrorCode::InvalidRequest,
                    format!("query is longer than {MAX_TITLE_CHARS} characters"),
                ));
            }
            Some(text) if !text.is_empty() => Some(text.to_lowercase()),
            _ => None,
        };
        let stored = self.repository.list_ids().map_err(storage_error)?;
        let mut documents = Vec::new();
        let (open, retired): (HashSet<DocumentId>, HashMap<DocumentId, Retired>) = {
            let mut sessions = self.lock();
            let mut stale = Vec::new();
            for (id, session) in &mut sessions.open {
                if self.refresh(id, session).is_err() {
                    stale.push(id.clone());
                    continue;
                }
                documents.push(summary(id, &session.document, true, session.is_dirty()));
            }
            // These clean copies cannot follow a newer commit; they close and it is listed.
            for id in &stale {
                sessions.remove(id);
            }
            (
                sessions.open.keys().cloned().collect(),
                sessions.retired.clone(),
            )
        };
        for summary in &mut documents {
            if let Ok(id) = parse_id(&summary.id) {
                summary.recovery = self.recovery(&id);
                summary.saved_at_ms = self.repository.saved_at_ms(&id).ok().flatten();
            }
        }
        let mut unreadable = Vec::new();
        let remaining = MAX_LISTED_DOCUMENTS.saturating_sub(documents.len());
        for id in stored
            .into_iter()
            .filter(|id| !open.contains(id))
            .take(remaining)
        {
            match self.repository.load(&id) {
                // Numbered as opening it would number it, so no listed revision is reused.
                Ok(Some(document)) => match Session::open(document, retired.get(&id).copied()) {
                    Ok(session) => {
                        let mut listed = summary(&id, &session.document, false, false);
                        listed.recovery = self.recovery(&id);
                        listed.saved_at_ms = self.repository.saved_at_ms(&id).ok().flatten();
                        documents.push(listed);
                    }
                    Err(error) => unreadable.push(UnreadableDocument {
                        id: id.to_string(),
                        message: error.message,
                    }),
                },
                Ok(None) => {}
                Err(error) => unreadable.push(UnreadableDocument {
                    id: id.to_string(),
                    message: error.to_string(),
                }),
            }
        }
        if let Some(needle) = needle {
            documents.retain(|document| document.title.to_lowercase().contains(&needle));
        }
        let by_title = |a: &DocumentSummary, b: &DocumentSummary| {
            a.title.cmp(&b.title).then_with(|| a.id.cmp(&b.id))
        };
        match query.sort {
            ListSort::Title => documents.sort_by(by_title),
            // Newest first; documents without a known time last.
            ListSort::Recent => documents.sort_by(|a, b| {
                b.saved_at_ms
                    .cmp(&a.saved_at_ms)
                    .then_with(|| by_title(a, b))
            }),
        }
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
        let _running = self.start(None, request_id)?;
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
        let session = Session::new(document);
        let response = session.response();
        sessions.created.record(request_id, mutation, &response);
        sessions.open.insert(id, session);
        Ok(response)
    }

    /// Returns the working copy, opening it from storage if needed.
    pub(crate) fn get(&self, id: &str) -> Result<DocumentResponse, ApplicationError> {
        let mut sessions = self.lock();
        let session = self.session(&mut sessions, id)?;
        Ok(self.respond(session))
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
                .map_err(rejected_edit)
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
    ///
    /// The commit is of the revision checked against `expected_revision`, made without
    /// holding the session table: edits that arrive meanwhile stay unsaved after it.
    pub(crate) fn save(
        &self,
        id: &str,
        request_id: Option<&str>,
        expected_revision: u64,
    ) -> Result<DocumentResponse, ApplicationError> {
        validate_request_id(request_id)?;
        let document_id = parse_id(id)?;
        let mutation = Mutation::Save { expected_revision };
        let _running = self.start(Some(&document_id), request_id)?;
        if let Some(response) = self.replayed(request_id, |sessions| {
            sessions.open.get(&document_id).map_or(Ok(None), |session| {
                session.replays.find(request_id, &mutation)
            })
        })? {
            return Ok(response);
        }
        let _turn = self.turn(&document_id)?;
        let (snapshot, base) = {
            let mut sessions = self.lock();
            let session = self.session(&mut sessions, id)?;
            if let Some(response) = session.replays.find(request_id, &mutation)? {
                return Ok(response);
            }
            session.expect_revision(expected_revision)?;
            if !session.is_dirty() {
                let response = self.respond(session);
                session.replays.record(request_id, mutation, &response);
                return Ok(response);
            }
            (session.document.clone(), session.stored_revision)
        };
        self.repository
            .save(&snapshot, Some(base))
            .map_err(|error| {
                storage_failure(error, "save failed; this session keeps its unsaved changes")
            })?;
        let committed = snapshot.revision();
        let mut sessions = self.lock();
        // Storage now holds a revision above everything retired copies handed out.
        if sessions
            .retired
            .get(&document_id)
            .is_some_and(|retired| retired.reached.revision < committed)
        {
            sessions.retired.remove(&document_id);
        }
        let Some(session) = sessions.open.get_mut(&document_id) else {
            // Closed meanwhile; the commit stands and the next access reopens it.
            return Ok(DocumentResponse::new(&snapshot, committed, None));
        };
        session.stored_revision = committed;
        session.saved_revision = committed;
        // The repository removed the session's draft with the commit.
        session.draft_revision = None;
        let response = self.respond(session);
        session.replays.record(request_id, mutation, &response);
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
        let _running = self.start(None, request_id)?;
        if let Some(response) = self.replayed(request_id, |sessions| {
            sessions.closed.find(request_id, &mutation)
        })? {
            return Ok(response);
        }
        let _turn = self.turn(&document_id)?;
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
        // Discarded work must not come back as an abandoned draft, so the session ends only
        // after its draft is gone.
        self.repository
            .remove_draft(&document_id)
            .map_err(storage_error)?;
        sessions.remove(&document_id);
        let response = CloseResponse {
            api_version: API_VERSION,
            id: document_id.to_string(),
            discarded_changes,
        };
        sessions.closed.record(request_id, mutation, &response);
        Ok(response)
    }

    /// Writes the working copy's recovery draft when it has unsaved changes not yet in it, or
    /// removes the session's draft when nothing is unsaved.
    pub(crate) fn autosave(
        &self,
        id: &str,
        request_id: Option<&str>,
    ) -> Result<DocumentResponse, ApplicationError> {
        validate_request_id(request_id)?;
        let document_id = parse_id(id)?;
        let _running = self.start(Some(&document_id), request_id)?;
        if let Some(response) = self.replayed(request_id, |sessions| {
            sessions.open.get(&document_id).map_or(Ok(None), |session| {
                session.replays.find(request_id, &Mutation::Autosave)
            })
        })? {
            return Ok(response);
        }
        let _turn = self.turn(&document_id)?;
        let mut sessions = self.lock();
        let session = self.session(&mut sessions, id)?;
        if let Some(response) = session.replays.find(request_id, &Mutation::Autosave)? {
            return Ok(response);
        }
        let revision = session.document.revision();
        if !session.is_dirty() {
            self.repository
                .remove_draft(session.document.id())
                .map_err(storage_error)?;
            session.draft_revision = None;
        } else if session.draft_revision != Some(revision) {
            let draft = Draft {
                document: session.document.clone(),
                base_revision: session.stored_revision,
                saved_at_ms: now_ms(),
            };
            self.repository
                .write_draft(&draft)
                .map_err(|error| match error {
                    StorageError::Conflict { .. } => ApplicationError::new(
                        ErrorCode::RecoveryPending,
                        "unsaved work from an earlier session is waiting; recover or discard it \
                         before new changes are autosaved",
                    ),
                    StorageError::Locked(message) => {
                        ApplicationError::new(ErrorCode::DraftInUse, message)
                    }
                    error => {
                        storage_failure(error, "autosave failed; the session keeps its changes")
                    }
                })?;
            session.draft_revision = Some(revision);
        }
        let response = self.respond(session);
        session
            .replays
            .record(request_id, Mutation::Autosave, &response);
        Ok(response)
    }

    /// Makes an abandoned draft the working copy: content, history, and identifier counters as
    /// the draft holds them, unsaved, numbered after everything this process handed out. The
    /// session owns the draft from then on.
    pub(crate) fn recover(
        &self,
        id: &str,
        request_id: Option<&str>,
    ) -> Result<DocumentResponse, ApplicationError> {
        validate_request_id(request_id)?;
        let document_id = parse_id(id)?;
        let mutation = Mutation::Recover {
            document: document_id.clone(),
        };
        let _running = self.start(None, request_id)?;
        if let Some(response) = self.replayed(request_id, |sessions| {
            sessions.recovered.find(request_id, &mutation)
        })? {
            return Ok(response);
        }
        let _turn = self.turn(&document_id)?;
        let mut sessions = self.lock();
        if let Some(response) = sessions.recovered.find(request_id, &mutation)? {
            return Ok(response);
        }
        if sessions
            .open
            .get(&document_id)
            .is_some_and(Session::is_dirty)
        {
            return Err(ApplicationError::new(
                ErrorCode::UnsavedChanges,
                "the open session has unsaved changes; save or discard them before recovering",
            ));
        }
        let stored = self
            .load(&document_id)?
            .ok_or_else(|| not_found(&document_id))?;
        let draft = self.claim_abandoned(&document_id)?;
        let recovered = self.recovered_session(&sessions, &stored, draft);
        // An open session is replaced in its own slot.
        let eviction = recovered.and_then(|session| {
            let eviction = if sessions.open.contains_key(&document_id) {
                None
            } else {
                sessions.eviction(None)?
            };
            Ok((session, eviction))
        });
        let (session, eviction) = match eviction {
            Ok(ready) => ready,
            Err(error) => {
                self.repository.release_draft(&document_id);
                return Err(error);
            }
        };
        if let Some(evicted) = eviction {
            sessions.remove(&evicted);
        }
        let replays = sessions
            .open
            .remove(&document_id)
            .map(|replaced| replaced.replays)
            .unwrap_or_default();
        let session = sessions
            .open
            .entry(document_id)
            .insert_entry(Session { replays, ..session })
            .into_mut();
        let response = self.respond(session);
        sessions.recovered.record(request_id, mutation, &response);
        Ok(response)
    }

    /// The working copy a draft becomes, if storage still holds its base and this process has
    /// not handed out identifiers the draft may also use.
    fn recovered_session(
        &self,
        sessions: &Sessions,
        stored: &Document,
        draft: Draft,
    ) -> Result<Session, ApplicationError> {
        if stored.revision() != draft.base_revision {
            return Err(ApplicationError {
                current_revision: Some(stored.revision()),
                ..ApplicationError::new(
                    ErrorCode::DraftOutdated,
                    format!(
                        "the document was saved at revision {} after this draft's base {}; \
                         recover the draft as a copy",
                        stored.revision(),
                        draft.base_revision
                    ),
                )
            });
        }
        let id = stored.id();
        // How this process names the stored commit, and what it has handed out.
        let (reached, saved_revision) = match sessions.open.get(id) {
            Some(session) => (session.document.numbering(), session.saved_revision),
            None => {
                let session = Session::open(stored.clone(), sessions.retired.get(id).copied())?;
                (session.document.numbering(), session.saved_revision)
            }
        };
        let counters = stored.numbering();
        if reached.next_atom > counters.next_atom || reached.next_bond > counters.next_bond {
            return Err(ApplicationError::new(
                ErrorCode::DraftOutdated,
                "this process already used atom or bond identifiers the draft may reuse; \
                 recover the draft as a copy",
            ));
        }
        let mut document = draft.document;
        document
            .continue_after(reached)
            .map_err(revisions_exhausted)?;
        Ok(Session {
            draft_revision: Some(document.revision()),
            document,
            stored_revision: draft.base_revision,
            saved_revision,
            hold: None,
            replays: ReplayLog::default(),
        })
    }

    /// Saves an abandoned draft as a new document, then removes the draft.
    pub(crate) fn recover_as_copy(
        &self,
        id: &str,
        request_id: Option<&str>,
        title: Option<&str>,
    ) -> Result<CopyResponse, ApplicationError> {
        validate_request_id(request_id)?;
        let document_id = parse_id(id)?;
        let mutation = Mutation::RecoverAsCopy {
            document: document_id.clone(),
            title: title.map(str::to_owned),
        };
        let _running = self.start(None, request_id)?;
        if let Some(response) = self.replayed(request_id, |sessions| {
            sessions.copied.find(request_id, &mutation)
        })? {
            return Ok(response);
        }
        let _turn = self.turn(&document_id)?;
        let mut sessions = self.lock();
        if let Some(response) = sessions.copied.find(request_id, &mutation)? {
            return Ok(response);
        }
        let draft = self.claim_abandoned(&document_id)?;
        let title = title.map_or_else(
            || suffixed(draft.document.title(), " (recovered)"),
            str::to_owned,
        );
        let source_was_open = sessions.open.contains_key(&document_id);
        let slot = CopySlot::Evicted {
            spare: &document_id,
        };
        let copy = match self.copy(&mut sessions, &draft.document, &title, slot) {
            Ok(copy) => self.open_copy(&mut sessions, copy),
            Err(error) => {
                self.repository.release_draft(&document_id);
                return Err(error);
            }
        };
        // The copy is committed. If the draft cannot be removed it is abandoned again and can
        // still be discarded; the copy is not undone.
        if self.repository.remove_draft(&document_id).is_err() {
            self.repository.release_draft(&document_id);
        }
        // The source's session stays open unless it was the only clean one left to make room
        // for the copy. Only a clean session is evicted, so no changes were discarded.
        let response = CopyResponse {
            copy,
            source: CopySource {
                id: document_id.to_string(),
                closed: source_was_open && !sessions.open.contains_key(&document_id),
                discarded_changes: false,
            },
        };
        sessions.copied.record(request_id, mutation, &response);
        Ok(response)
    }

    pub(crate) fn discard_draft(
        &self,
        id: &str,
        request_id: Option<&str>,
    ) -> Result<DraftDiscardResponse, ApplicationError> {
        validate_request_id(request_id)?;
        let document_id = parse_id(id)?;
        let mutation = Mutation::DiscardDraft {
            document: document_id.clone(),
        };
        let _running = self.start(None, request_id)?;
        if let Some(response) = self.replayed(request_id, |sessions| {
            sessions.drafts_discarded.find(request_id, &mutation)
        })? {
            return Ok(response);
        }
        let _turn = self.turn(&document_id)?;
        let mut sessions = self.lock();
        if let Some(response) = sessions.drafts_discarded.find(request_id, &mutation)? {
            return Ok(response);
        }
        if self
            .repository
            .draft_status(&document_id)
            .map_err(storage_error)?
            == DraftStatus::Owned
        {
            return Err(own_draft(&document_id));
        }
        // An unreadable draft is claimed and removed like any other.
        match self.repository.claim_draft(&document_id) {
            Ok(Some(_)) => {}
            Ok(None) => return Err(draft_not_found(&document_id)),
            Err(error) => return Err(draft_error(error)),
        }
        if let Err(error) = self.repository.remove_draft(&document_id) {
            self.repository.release_draft(&document_id);
            return Err(storage_error(error));
        }
        let response = DraftDiscardResponse {
            api_version: API_VERSION,
            id: document_id.to_string(),
        };
        sessions
            .drafts_discarded
            .record(request_id, mutation, &response);
        Ok(response)
    }

    /// Renames the stored document in one commit. The open copy must have no unsaved changes:
    /// a rename from the document list is not a way to save other work. An abandoned draft
    /// would be left behind by the commit, so it must be recovered or discarded first.
    pub(crate) fn rename(
        &self,
        id: &str,
        request_id: Option<&str>,
        expected_revision: u64,
        title: &str,
    ) -> Result<DocumentResponse, ApplicationError> {
        validate_request_id(request_id)?;
        let document_id = parse_id(id)?;
        let mutation = Mutation::Rename {
            expected_revision,
            title: title.to_owned(),
        };
        let _running = self.start(Some(&document_id), request_id)?;
        if let Some(response) = self.replayed(request_id, |sessions| {
            sessions.open.get(&document_id).map_or(Ok(None), |session| {
                session.replays.find(request_id, &mutation)
            })
        })? {
            return Ok(response);
        }
        let _turn = self.turn(&document_id)?;
        let pending = self.pending_draft(&document_id)?;
        let (renamed, base) = {
            let mut sessions = self.lock();
            let session = self.session(&mut sessions, id)?;
            if let Some(response) = session.replays.find(request_id, &mutation)? {
                return Ok(response);
            }
            session.expect_revision(expected_revision)?;
            if session.is_dirty() {
                return Err(unsaved_before("renaming it from the list", session));
            }
            if let Some(error) = pending {
                return Err(error);
            }
            let mut renamed = session.document.clone();
            let command = Command::Rename {
                title: title.to_owned(),
            };
            if renamed.apply(&[command]).map_err(rejected_edit)? == Outcome::Unchanged {
                let response = self.respond(session);
                session.replays.record(request_id, mutation, &response);
                return Ok(response);
            }
            session.hold = Some(Hold::Rename);
            (renamed, session.stored_revision)
        };
        let committed = self.repository.save(&renamed, Some(base));
        let mut sessions = self.lock();
        let mut session = sessions.open.get_mut(&document_id);
        if let Some(session) = session.as_deref_mut() {
            session.hold = None;
        }
        committed.map_err(|error| storage_failure(error, "rename failed; nothing was changed"))?;
        let Some(session) = session else {
            // Held sessions are neither evicted nor refreshed; the commit stands regardless.
            return Ok(DocumentResponse::new(&renamed, renamed.revision(), None));
        };
        session.stored_revision = renamed.revision();
        session.saved_revision = renamed.revision();
        session.document = renamed;
        let response = self.respond(session);
        session.replays.record(request_id, mutation, &response);
        Ok(response)
    }

    /// Moves the document to the trash. Nothing unsaved may be lost by it: the open copy must
    /// be clean at `expected_revision`, and no recovery draft may hold unsaved work, whether
    /// abandoned (`recovery_pending`) or another running process's (`draft_in_use`). The
    /// session ends; other processes' copies close on their next access, and their saves meet
    /// the missing document as a conflict instead of re-creating it.
    pub(crate) fn delete(
        &self,
        id: &str,
        request_id: Option<&str>,
        expected_revision: u64,
    ) -> Result<DeleteResponse, ApplicationError> {
        validate_request_id(request_id)?;
        let document_id = parse_id(id)?;
        let mutation = Mutation::Delete {
            document: document_id.clone(),
            expected_revision,
        };
        let _running = self.start(None, request_id)?;
        if let Some(response) = self.replayed(request_id, |sessions| {
            sessions.deleted.find(request_id, &mutation)
        })? {
            return Ok(response);
        }
        let _turn = self.turn(&document_id)?;
        let pending = self.pending_draft(&document_id)?;
        let (base, title) = {
            let mut sessions = self.lock();
            let session = self.session(&mut sessions, id)?;
            session.expect_revision(expected_revision)?;
            if session.is_dirty() {
                return Err(unsaved_before("deleting it", session));
            }
            if let Some(error) = pending {
                return Err(error);
            }
            session.hold = Some(Hold::Delete);
            (session.stored_revision, session.document.title().to_owned())
        };
        // A draft that appears meanwhile is refused by the repository: it checks under the
        // draft's owner lock. A refusal there is reported as busy; a retry names the draft.
        let trashed = self.repository.trash(&document_id, base);
        let mut sessions = self.lock();
        if let Err(error) = trashed {
            if let Some(session) = sessions.open.get_mut(&document_id) {
                session.hold = None;
            }
            return Err(storage_failure(
                error,
                "delete failed; the document is still stored",
            ));
        }
        sessions.remove(&document_id);
        let response = DeleteResponse {
            api_version: API_VERSION,
            id: document_id.to_string(),
            title,
        };
        sessions.deleted.record(request_id, mutation, &response);
        Ok(response)
    }

    /// Trashed documents, most recently trashed first.
    pub(crate) fn list_trash(&self) -> Result<TrashListResponse, ApplicationError> {
        let entries = self.repository.list_trash().map_err(storage_error)?;
        let mut documents = Vec::new();
        let mut unreadable = Vec::new();
        for entry in entries.into_iter().take(MAX_LISTED_DOCUMENTS) {
            match self.repository.load_trashed(&entry.id) {
                Ok(Some(document)) => documents.push(TrashedSummary {
                    id: entry.id.to_string(),
                    title: document.title().to_owned(),
                    revision: document.revision(),
                    atom_count: document.atoms().len(),
                    bond_count: document.bonds().len(),
                    deleted_at_ms: entry.deleted_at_ms,
                }),
                Ok(None) => {}
                Err(error) => unreadable.push(UnreadableDocument {
                    id: entry.id.to_string(),
                    message: error.to_string(),
                }),
            }
        }
        documents.sort_by(|a, b| {
            b.deleted_at_ms
                .cmp(&a.deleted_at_ms)
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(TrashListResponse {
            api_version: API_VERSION,
            documents,
            unreadable,
        })
    }

    /// Moves a trashed document back among the stored documents, as it was trashed.
    pub(crate) fn restore(
        &self,
        id: &str,
        request_id: Option<&str>,
    ) -> Result<RestoreResponse, ApplicationError> {
        validate_request_id(request_id)?;
        let document_id = parse_id(id)?;
        let mutation = Mutation::Restore {
            document: document_id.clone(),
        };
        let _running = self.start(None, request_id)?;
        if let Some(response) = self.replayed(request_id, |sessions| {
            sessions.restored.find(request_id, &mutation)
        })? {
            return Ok(response);
        }
        let _turn = self.turn(&document_id)?;
        let not_trashed = || {
            ApplicationError::new(
                ErrorCode::DocumentNotFound,
                format!("document {document_id} is not in the trash"),
            )
        };
        let document = self
            .repository
            .load_trashed(&document_id)
            .map_err(storage_error)?
            .ok_or_else(not_trashed)?;
        self.repository
            .restore(&document_id)
            .map_err(|error| match error {
                StorageError::Missing => not_trashed(),
                error => storage_failure(error, "restore failed; the document stays in the trash"),
            })?;
        let response = RestoreResponse {
            api_version: API_VERSION,
            id: document_id.to_string(),
            title: document.title().to_owned(),
        };
        self.lock().restored.record(request_id, mutation, &response);
        Ok(response)
    }

    /// The error for a recovery draft that holds unsaved work this process does not own.
    fn pending_draft(&self, id: &DocumentId) -> Result<Option<ApplicationError>, ApplicationError> {
        Ok(
            match self.repository.draft_status(id).map_err(storage_error)? {
                DraftStatus::None | DraftStatus::Owned => None,
                DraftStatus::Locked => Some(ApplicationError::new(
                    ErrorCode::DraftInUse,
                    format!(
                        "another running process has unsaved work in document {id}'s recovery draft"
                    ),
                )),
                DraftStatus::Abandoned(_) => Some(storage_error(StorageError::DraftPending)),
            },
        )
    }

    /// Commits the working copy as a new document. The source session closes: its working
    /// copy, saved or not, lives on in the copy, and the source reopens from its last save.
    pub(crate) fn save_as(
        &self,
        id: &str,
        request_id: Option<&str>,
        expected_revision: u64,
        title: Option<&str>,
    ) -> Result<CopyResponse, ApplicationError> {
        validate_request_id(request_id)?;
        let document_id = parse_id(id)?;
        let mutation = Mutation::SaveAs {
            document: document_id.clone(),
            expected_revision,
            title: title.map(str::to_owned),
        };
        let _running = self.start(None, request_id)?;
        if let Some(response) = self.replayed(request_id, |sessions| {
            sessions.copied.find(request_id, &mutation)
        })? {
            return Ok(response);
        }
        let _turn = self.turn(&document_id)?;
        let mut sessions = self.lock();
        if let Some(response) = sessions.copied.find(request_id, &mutation)? {
            return Ok(response);
        }
        let session = self.session(&mut sessions, id)?;
        session.expect_revision(expected_revision)?;
        let source = session.document.clone();
        let discarded_changes = session.is_dirty();
        let title = title.map_or_else(|| suffixed(source.title(), " (copy)"), str::to_owned);
        let copy = self.copy(&mut sessions, &source, &title, CopySlot::Source)?;
        // The copy takes the source's slot. If the source's draft cannot be removed, the
        // source stays open with its changes rather than leave a draft that would be offered
        // for recovery later, and the copy is opened on its next access instead.
        let closed = self.repository.remove_draft(&document_id).is_ok();
        let copy = if closed {
            sessions.remove(&document_id);
            self.open_copy(&mut sessions, copy)
        } else {
            copy.response()
        };
        let response = CopyResponse {
            copy,
            source: CopySource {
                id: document_id.to_string(),
                closed,
                discarded_changes: closed && discarded_changes,
            },
        };
        sessions.copied.record(request_id, mutation, &response);
        Ok(response)
    }

    /// Commits `source`'s content as a new document, in a clean session for the caller to
    /// open, making room for it as `slot` says.
    fn copy(
        &self,
        sessions: &mut Sessions,
        source: &Document,
        title: &str,
        slot: CopySlot<'_>,
    ) -> Result<Session, ApplicationError> {
        let id = self.repository.allocate_id().map_err(storage_error)?;
        let copy = source.copy_as(id, title).map_err(|error| {
            ApplicationError::new(ErrorCode::InvalidRequest, format!("title: {error}"))
        })?;
        let mut eviction = match slot {
            CopySlot::Source => None,
            CopySlot::Evicted { spare } => sessions.eviction(Some(spare))?,
        };
        self.repository.save(&copy, None).map_err(storage_error)?;
        if let Some(evicted) = eviction.take() {
            sessions.remove(&evicted);
        }
        Ok(Session::new(copy))
    }

    fn open_copy(&self, sessions: &mut Sessions, session: Session) -> DocumentResponse {
        let response = self.respond(&session);
        sessions.open.insert(session.document.id().clone(), session);
        response
    }

    /// Takes over the document's abandoned draft. The draft of a session open in this process
    /// is that session's, not abandoned.
    fn claim_abandoned(&self, id: &DocumentId) -> Result<Draft, ApplicationError> {
        if self.repository.draft_status(id).map_err(storage_error)? == DraftStatus::Owned {
            return Err(own_draft(id));
        }
        match self.repository.claim_draft(id) {
            Ok(Some(Ok(draft))) => Ok(draft),
            Ok(Some(Err(message))) => {
                self.repository.release_draft(id);
                Err(ApplicationError::new(
                    ErrorCode::DocumentUnreadable,
                    format!("the recovery draft is unreadable: {message}; discard it"),
                ))
            }
            Ok(None) => Err(draft_not_found(id)),
            Err(error) => Err(draft_error(error)),
        }
    }

    /// The session's response with any abandoned draft of the same document.
    fn respond(&self, session: &Session) -> DocumentResponse {
        DocumentResponse {
            recovery: self.recovery(session.document.id()),
            ..session.response()
        }
    }

    /// Describes the document's draft when it is not the draft of a session in this process.
    fn recovery(&self, id: &DocumentId) -> Option<RecoveryView> {
        let view = |state, info: Option<&crate::repository::DraftInfo>, message| RecoveryView {
            state,
            revision: info.map(|info| info.revision),
            base_revision: info.map(|info| info.base_revision),
            saved_at_ms: info.map(|info| info.saved_at_ms),
            message,
        };
        match self.repository.draft_status(id) {
            Ok(DraftStatus::None | DraftStatus::Owned) => None,
            Ok(DraftStatus::Locked) => Some(view(RecoveryState::InUse, None, None)),
            Ok(DraftStatus::Abandoned(Err(message))) => {
                Some(view(RecoveryState::Unreadable, None, Some(message)))
            }
            Ok(DraftStatus::Abandoned(Ok(info))) => {
                let stored = self.repository.stored_revision(id).ok().flatten();
                Some(if stored == Some(info.base_revision) {
                    view(RecoveryState::Recoverable, Some(&info), None)
                } else {
                    view(
                        RecoveryState::Outdated,
                        Some(&info),
                        Some("the document was saved elsewhere after this draft".to_owned()),
                    )
                })
            }
            Err(error) => Some(view(
                RecoveryState::Unreadable,
                None,
                Some(error.to_string()),
            )),
        }
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
        if let Some(hold) = session.hold {
            return Err(held(id, hold));
        }
        session.expect_revision(expected_revision)?;
        change(session)?;
        let response = self.respond(session);
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
            if let Err(error) = self.refresh(&id, session) {
                sessions.remove(&id);
                return Err(error);
            }
        } else {
            let document = self.load(&id)?.ok_or_else(|| not_found(&id))?;
            let retired = sessions.retired.get(&id).copied();
            let session = Session::open(document, retired)?;
            if let Some(evicted) = sessions.eviction(None)? {
                sessions.remove(&evicted);
            }
            sessions.open.insert(id.clone(), session);
        }
        Ok(sessions
            .open
            .get_mut(&id)
            .expect("session was just ensured"))
    }

    /// A clean working copy follows newer commits by other writers (another process or the
    /// CLI), so it never shows stale content as saved. A session with unsaved changes keeps
    /// them and meets the other commit as a save conflict. Storage errors leave the copy as
    /// it is: it is still the last commit this session saw. Fails when no revision remains to
    /// name the newer commit; the caller then closes the clean copy.
    fn refresh(&self, id: &DocumentId, session: &mut Session) -> Result<(), ApplicationError> {
        if session.is_dirty() || session.hold.is_some() {
            return Ok(());
        }
        let stored = self.repository.stored_revision(id);
        // Deleted elsewhere: a clean copy has nothing to keep, and must not show it as stored.
        if matches!(stored, Ok(None)) {
            return Err(not_found(id));
        }
        let newer = matches!(stored, Ok(Some(stored)) if stored > session.stored_revision);
        if newer && let Ok(Some(document)) = self.repository.load(id) {
            // The copy being replaced named the old commit; the new one must not reuse that.
            let reached = Retired {
                reached: session.document.numbering(),
                clean_on: None,
            };
            let reopened = Session::open(document, Some(reached))?;
            *session = Session {
                replays: std::mem::take(&mut session.replays),
                ..reopened
            };
        }
        Ok(())
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
        saved_at_ms: None,
        id: id.to_string(),
        title: document.title().to_owned(),
        revision: document.revision(),
        atom_count: document.atoms().len(),
        bond_count: document.bonds().len(),
        open,
        unsaved_changes,
        recovery: None,
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

fn draft_not_found(id: &DocumentId) -> ApplicationError {
    ApplicationError::new(
        ErrorCode::DraftNotFound,
        format!("document {id} has no abandoned recovery draft"),
    )
}

fn own_draft(id: &DocumentId) -> ApplicationError {
    ApplicationError::new(
        ErrorCode::DraftInUse,
        format!(
            "the recovery draft of document {id} belongs to its open session; save it, save it \
             as a copy, or close it with discard_unsaved"
        ),
    )
}

fn unsaved_before(action: &str, session: &Session) -> ApplicationError {
    ApplicationError::new(
        ErrorCode::UnsavedChanges,
        format!(
            "revision {} is not saved; save or discard it before {action}",
            session.document.revision()
        ),
    )
}

fn rejected_edit(error: TransactionError) -> ApplicationError {
    let code = match error.error {
        EditError::RevisionLimit => ErrorCode::ResourceLimit,
        _ => ErrorCode::InvalidCommand,
    };
    ApplicationError {
        command_index: error.index,
        ..ApplicationError::new(code, error.to_string())
    }
}

fn held(id: &str, hold: Hold) -> ApplicationError {
    let action = match hold {
        Hold::Rename => "renamed",
        Hold::Delete => "deleted",
    };
    ApplicationError::storage_failed(
        StorageReason::Busy,
        format!("document {id} is being {action}; nothing was changed, retry"),
    )
}

fn draft_error(error: StorageError) -> ApplicationError {
    match error {
        StorageError::Locked(message) => ApplicationError::new(ErrorCode::DraftInUse, message),
        error => storage_error(error),
    }
}

/// `title` followed by `suffix`, shortened to fit the title limit.
fn suffixed(title: &str, suffix: &str) -> String {
    let room = MAX_TITLE_CHARS.saturating_sub(suffix.chars().count());
    let mut result: String = title.chars().take(room).collect();
    result.truncate(result.trim_end().len());
    result.push_str(suffix);
    result
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        })
}

fn revisions_exhausted(error: RevisionsExhausted) -> ApplicationError {
    ApplicationError::new(ErrorCode::ResourceLimit, error.to_string())
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
        StorageError::Missing => {
            ApplicationError::new(ErrorCode::DocumentNotFound, "no such document is stored")
        }
        StorageError::DraftPending => ApplicationError::new(
            ErrorCode::RecoveryPending,
            "an earlier session's recovery draft holds unsaved work for this document; recover or discard it first",
        ),
        error => storage_failure(error, "storage failed"),
    }
}

/// `storage_failed` for a failure that is not a conflict or corrupt data, with `context` in
/// front of the provider's message.
fn storage_failure(error: StorageError, context: &str) -> ApplicationError {
    let (reason, message) = match error {
        StorageError::Locked(message) => (StorageReason::Busy, message),
        StorageError::Denied(message) => (StorageReason::ReadOnly, message),
        StorageError::Unavailable(message) => (StorageReason::Io, message),
        error @ (StorageError::Conflict { .. }
        | StorageError::Corrupt(_)
        | StorageError::Missing
        | StorageError::DraftPending) => return storage_error(error),
    };
    ApplicationError::storage_failed(reason, format!("{context}: {message}"))
}

fn refused(refusal: Refusal) -> ApplicationError {
    match refusal {
        Refusal::InProgress => ApplicationError::new(
            ErrorCode::RequestInProgress,
            "a request with this request_id is still running; ask again for its outcome",
        ),
        Refusal::Busy => ApplicationError::storage_failed(
            StorageReason::Busy,
            format!(
                "the document stayed busy with other requests for {} seconds; nothing was \
                 changed, and retrying later can succeed",
                TURN_WAIT.as_secs()
            ),
        ),
    }
}
