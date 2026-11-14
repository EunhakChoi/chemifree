//! Document editing sessions: working copies, revisions, idempotent replays, saves, recovery
//! drafts, and copies.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::{panic, thread};

use chemifree_domain::budget::{Interrupt, Interrupted};
use chemifree_domain::document::{
    Applied, AtomId, Command, Document, DocumentId, EditError, HistoryError, MAX_COMMANDS,
    MAX_TITLE_CHARS, Numbering, ObjectId, Outcome, Point, RevisionsExhausted, TransactionError,
    validated_title,
};
use chemifree_domain::scene::{
    ArrangementPlan, ObjectArrangement, Selection, SelectionScope, SelectionTransform,
    TransactionSelection, TransformPlan,
};
use chemifree_domain::settings::Settings;
use chemifree_domain::stereo::StereoKey;
use chemifree_domain::stereo_drawing::DeriveError;
use chemifree_domain::stereo_group::{GroupKind, GroupNote, LeaveCause, StereoGroupTable};
use chemifree_domain::{aromaticity, graph::MoleculeGraph};

use crate::API_VERSION;
use crate::compute_gate::ComputeGate;
use crate::contract::{
    self, CloseResponse, CommandDto, CopyResponse, CopySource, DeleteResponse,
    DocumentListResponse, DocumentResponse, DocumentSummary, DocumentView, DraftDiscardResponse,
    DraftFailureView, ListQuery, ListSort, RecoveryState, RecoveryView, RestoreResponse,
    SelectionDto, SettingsSource, TransactionSelections, TrashListResponse, TrashedSummary,
    UnreadableDocument,
};
use crate::draft_schedule::DraftSchedule;
use crate::error::{ApplicationError, ErrorCode, RingRefusal, StorageReason};
use crate::operations::{Diagnostic, Loss, LossConsent, LossDisposition, SemanticImpact};
use crate::repository::{
    DocumentRepository, Draft, DraftRecoveryTarget, DraftStatus, StorageError,
};
use crate::turns::{Refusal, Running, TURN_WAIT, Turn, Turns};

pub const DEFAULT_TITLE: &str = "Untitled";
pub const MAX_OPEN_SESSIONS: usize = 64;
/// Successful mutations remembered per session (and per process for creates and closes)
/// for `request_id` replay.
pub const REPLAY_LIMIT: usize = 64;
pub const MAX_REQUEST_ID_CHARS: usize = 64;
pub const MAX_LISTED_DOCUMENTS: usize = 1_000;

pub(crate) type GraphSnapshot = (DocumentResponse, MoleculeGraph);

/// What a request asked for, compared when a `request_id` is seen again.
#[derive(Clone, Debug, PartialEq)]
enum Mutation {
    Create {
        title: Option<String>,
    },
    CreateInterchange {
        fingerprint: String,
    },
    Transaction {
        expected_revision: u64,
        commands: Vec<CommandDto>,
        selection: TransactionSelections,
    },
    ImportCtfile {
        fingerprint: String,
    },
    TemplateReplace {
        fingerprint: String,
    },
    TemplateAttach {
        fingerprint: String,
    },
    ClipboardPaste {
        expected_revision: u64,
        payload: String,
        offset: crate::contract::PointDto,
        selection_before: Option<SelectionDto>,
    },
    ClipboardScenePaste {
        expected_revision: u64,
        payload: String,
        // Signed zero changes translated graphic coordinates and binds the replay request.
        offset_bits: [u64; 2],
        selection_before: Option<SelectionDto>,
    },
    AromaticityConversion {
        expected_revision: u64,
        conversion: AromaticityConversion,
        selection: TransactionSelections,
        /// The time limit as the request stated it (not the resolved one), so that the same
        /// request ID with another limit is a different request.
        time_limit_ms: Option<u64>,
    },
    LayoutApply {
        expected_revision: u64,
        selection: SelectionDto,
        scope: Option<String>,
        instance: Option<String>,
        selections: TransactionSelections,
        /// The time limit as the request stated it (not the resolved one), so that the same
        /// request ID with another limit is a different request.
        time_limit_ms: Option<u64>,
    },
    ResetSettings {
        expected_revision: u64,
        to: SettingsSource,
        selection: TransactionSelections,
    },
    ImportSettings {
        expected_revision: u64,
        source: String,
        selection: TransactionSelections,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AromaticityConversion {
    Aromatic,
    Kekule,
}

/// Where replaced settings come from, as a request names it.
pub(crate) enum SettingsMutation {
    Reset(SettingsSource),
    /// The identifier of the document whose settings are imported, as sent.
    Import(String),
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
    /// When the service drafts unsaved work this draft does not hold yet.
    drafting: DraftSchedule,
    /// The document's recovery state as probed when the draft write in progress started,
    /// reported until it ends: probing again would wait for that write to take the draft's
    /// ownership in storage.
    recovery_at_write: Option<RecoveryView>,
    replays: ReplayLog<DocumentResponse>,
}

/// Who asked for a draft: a client's autosave may open the session from storage, while the
/// service drafts only sessions that are open.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Drafter {
    Client,
    /// The service's periodic pass over due drafts.
    Service,
    /// The service's last pass before the process stops.
    Stop,
}

impl Drafter {
    /// How long the attempt waits for the document's turn. The periodic pass does not wait:
    /// a document whose turn is taken (by a save, say) must not hold up the others, and it is
    /// simply tried again on the next pass. At a stop no request is running any more.
    fn turn_wait(self) -> Duration {
        match self {
            Self::Client | Self::Stop => TURN_WAIT,
            Self::Service => Duration::ZERO,
        }
    }
}

/// A storage change that briefly holds a session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Hold {
    Rename,
    Delete,
}

/// The storage change that makes a session's recovery draft hold its unsaved work, taken
/// from the session so it can be carried out without holding the session table.
enum DraftTask {
    /// Nothing is unsaved: remove the draft, if the session has one.
    Remove(DocumentId),
    /// Write the working copy as it was when the task was taken. Boxed: a document is much
    /// larger than an identifier.
    Write(Box<Draft>),
}

impl DraftTask {
    /// `None` when the session's draft already holds its unsaved work.
    fn for_session(session: &Session) -> Option<Self> {
        if !session.is_dirty() {
            Some(Self::Remove(session.document.id().clone()))
        } else if session.is_drafted() {
            None
        } else {
            Some(Self::Write(Box::new(Draft {
                document: session.document.clone(),
                base_revision: session.stored_revision,
                saved_at_ms: now_ms(),
            })))
        }
    }

    fn run<R: DocumentRepository>(&self, repository: &R) -> Result<(), ApplicationError> {
        match self {
            Self::Remove(id) => repository.remove_draft(id).map_err(storage_error),
            Self::Write(draft) => repository.write_draft(draft).map_err(|error| match error {
                StorageError::Conflict { .. } => ApplicationError::new(
                    ErrorCode::RecoveryPending,
                    "unsaved work from an earlier session is waiting; recover or discard it \
                     before new changes are autosaved",
                ),
                StorageError::Locked(message) => {
                    ApplicationError::new(ErrorCode::DraftInUse, message)
                }
                error => storage_failure(error, "autosave failed; the session keeps its changes"),
            }),
        }
    }

    /// The revision the session's draft holds once the task succeeded.
    fn drafted_revision(&self) -> Option<u64> {
        match self {
            Self::Remove(_) => None,
            Self::Write(draft) => Some(draft.document.revision()),
        }
    }
}

impl Session {
    /// A working copy of a document as stored.
    fn new(document: Document) -> Self {
        Self {
            stored_revision: document.revision(),
            saved_revision: document.revision(),
            draft_revision: None,
            hold: None,
            drafting: DraftSchedule::default(),
            recovery_at_write: None,
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
        let mut response =
            DocumentResponse::new(&self.document, self.saved_revision, self.draft_revision);
        response.session.draft_failure = self.drafting.failure().map(|failure| DraftFailureView {
            attempts: failure.attempts,
            retry_in_ms: u64::try_from(
                failure
                    .retry_at
                    .saturating_duration_since(Instant::now())
                    .as_millis(),
            )
            .unwrap_or(u64::MAX),
            code: failure.error.code.as_str(),
            reason: failure.error.reason.map(StorageReason::as_str),
            message: failure.error.message.clone(),
        });
        response
    }

    fn is_dirty(&self) -> bool {
        self.document.revision() != self.saved_revision
    }

    /// Whether the session's draft holds everything unsaved (trivially, when nothing is).
    fn is_drafted(&self) -> bool {
        !self.is_dirty() || self.draft_revision == Some(self.document.revision())
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
            // A session held by a rename or delete in progress, or whose draft is being
            // written, is not evicted under it.
            .filter(|(_, session)| {
                !session.is_dirty() && session.hold.is_none() && !session.drafting.is_writing()
            })
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
/// same one, are not held up. Edits (transactions, undo, redo, imports, aromaticity
/// conversion) take no turn, only `sessions`; an aromaticity conversion's computation holds
/// neither: it works on a copy and applies its result under `sessions` after checking the
/// revision again.
pub(crate) struct Documents<R> {
    pub(crate) image_decoder: Option<Arc<dyn chemifree_domain::image::PngDecoder>>,
    image_cache: Mutex<chemifree_domain::image::PngCache>,
    repository: R,
    sessions: Mutex<Sessions>,
    turns: Turns,
}

impl<R: DocumentRepository> Documents<R> {
    pub(crate) fn new(repository: R) -> Self {
        Self {
            repository,
            image_decoder: None,
            image_cache: Mutex::new(Default::default()),
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
        // Recovery states of open sessions whose draft is being written, taken as reported.
        let mut writing = HashMap::new();
        let (open, retired): (HashSet<DocumentId>, HashMap<DocumentId, Retired>) = {
            let mut sessions = self.lock();
            let mut stale = Vec::new();
            for (id, session) in &mut sessions.open {
                if self.refresh(id, session).is_err() {
                    stale.push(id.clone());
                    continue;
                }
                documents.push(summary(id, &session.document, true, session.is_dirty()));
                if session.drafting.is_writing() {
                    writing.insert(id.clone(), session.recovery_at_write.clone());
                }
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
                summary.recovery = match writing.remove(&id) {
                    Some(recovery) => recovery,
                    None => self.recovery(&id),
                };
                summary.saved_at_ms = self.repository.saved_at_ms(&id).ok().flatten();
            }
        }
        let mut unreadable = Vec::new();
        let remaining = MAX_LISTED_DOCUMENTS.saturating_sub(documents.len());
        let unopened: Vec<DocumentId> =
            stored.into_iter().filter(|id| !open.contains(id)).collect();
        // Counted before the cap, so truncation is known without reading past it.
        let truncated = unopened.len() > remaining;
        for id in unopened.into_iter().take(remaining) {
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
            truncated,
        })
    }

    /// Creates an empty document starting from the settings `settings` reads and commits it to
    /// storage before acknowledging. A replayed create returns the document as it was created
    /// without calling `settings`.
    pub(crate) fn create(
        &self,
        request_id: Option<&str>,
        title: Option<&str>,
        settings: impl FnOnce() -> Result<Settings, ApplicationError>,
    ) -> Result<DocumentResponse, ApplicationError> {
        validate_request_id(request_id)?;
        let mutation = Mutation::Create {
            title: title.map(str::to_owned),
        };
        let _running = self.start(None, request_id)?;
        {
            let sessions = self.lock();
            if let Some(response) = sessions.created.find(request_id, &mutation)? {
                return Ok(response);
            }
        }
        // The running-request guard prevents another create with this ID while preferences
        // are read. Slow preference storage must not hold the shared session table.
        let settings = settings()?;
        let mut sessions = self.lock();
        let id = self.repository.allocate_id().map_err(storage_error)?;
        let document =
            Document::new_with_settings(id.clone(), title.unwrap_or(DEFAULT_TITLE), settings)
                .map_err(|error| {
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

    /// A previously acknowledged format creation is replayed before reading preferences.
    pub(crate) fn replay_interchange_create(
        &self,
        request_id: Option<&str>,
        fingerprint: &str,
    ) -> Result<Option<DocumentResponse>, ApplicationError> {
        validate_request_id(request_id)?;
        let mutation = Mutation::CreateInterchange {
            fingerprint: fingerprint.to_owned(),
        };
        self.replayed(request_id, |sessions| {
            sessions.created.find(request_id, &mutation)
        })
    }

    /// Build the same validated graph as creation, without allocating or saving a document.
    pub(crate) fn preview_interchange_create(
        title: &str,
        settings: Settings,
        commands: &[Command],
    ) -> Result<DocumentResponse, ApplicationError> {
        let id = DocumentId::parse("preview").expect("preview identifier is valid");
        let mut candidate = Document::new_with_settings(id, title, settings).map_err(|error| {
            ApplicationError::new(ErrorCode::InvalidRequest, format!("title: {error}"))
        })?;
        if !commands.is_empty() {
            candidate.apply(commands).map_err(rejected_edit)?;
        }
        Ok(DocumentResponse::new(&candidate, 0, None))
    }

    /// Validate, verify consent and durably create the complete graph before acknowledging it.
    pub(crate) fn create_interchange(
        &self,
        request_id: Option<&str>,
        fingerprint: String,
        title: &str,
        commands: &[Command],
        settings: impl FnOnce() -> Result<Settings, ApplicationError>,
        verify: impl FnOnce(&DocumentView) -> Result<(), ApplicationError>,
    ) -> Result<DocumentResponse, ApplicationError> {
        validate_request_id(request_id)?;
        let mutation = Mutation::CreateInterchange { fingerprint };
        let _running = self.start(None, request_id)?;
        {
            let sessions = self.lock();
            if let Some(response) = sessions.created.find(request_id, &mutation)? {
                return Ok(response);
            }
        }
        let settings = settings()?;
        let mut sessions = self.lock();
        let id = self.repository.allocate_id().map_err(storage_error)?;
        let mut candidate =
            Document::new_with_settings(id.clone(), title, settings).map_err(|error| {
                ApplicationError::new(ErrorCode::InvalidRequest, format!("title: {error}"))
            })?;
        if !commands.is_empty() {
            candidate.apply(commands).map_err(rejected_edit)?;
        }
        let projected = DocumentResponse::new(&candidate, 0, None);
        verify(&projected.document)?;
        let eviction = sessions.eviction(None)?;
        self.repository
            .save(&candidate, None)
            .map_err(storage_error)?;
        if let Some(evicted) = eviction {
            sessions.remove(&evicted);
        }
        let session = Session::new(candidate);
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

    /// Read the graph and its ordinary document view under one lock. The view lets clients
    /// verify the actual analyzed content even when a revision number is reused after restart.
    pub(crate) fn inspect_valence(
        &self,
        id: &str,
    ) -> Result<
        (
            DocumentResponse,
            Vec<chemifree_domain::chemistry::AtomAssessment>,
        ),
        ApplicationError,
    > {
        let mut sessions = self.lock();
        let session = self.session(&mut sessions, id)?;
        let findings = chemifree_domain::chemistry::inspect_valence(&session.document);
        Ok((self.respond(session), findings))
    }

    /// Resolves movement geometry and its exact response snapshot under the same session lock.
    /// Neither history nor document content is copied or edited to compute the scope.
    pub(crate) fn selection_scope(
        &self,
        id: &str,
        selection: &Selection,
        instance: Option<ObjectId>,
    ) -> Result<(DocumentResponse, SelectionScope), ApplicationError> {
        let mut sessions = self.lock();
        let session = self.session(&mut sessions, id)?;
        let scope = session
            .document
            .selection_scope(selection, instance)
            .map_err(|error| ApplicationError {
                current_revision: Some(session.document.revision()),
                ..rejected_edit(TransactionError { index: None, error })
            })?;
        Ok((self.respond(session), scope))
    }

    /// Captures a checked rigid arrangement and its exact view without editing or copying history.
    pub(crate) fn arrangement_plan(
        &self,
        id: &str,
        objects: &BTreeSet<ObjectId>,
        arrangement: ObjectArrangement,
    ) -> Result<(DocumentResponse, ArrangementPlan), ApplicationError> {
        let mut sessions = self.lock();
        let session = self.session(&mut sessions, id)?;
        let plan = session
            .document
            .object_arrangement(objects, arrangement)
            .map_err(|error| ApplicationError {
                current_revision: Some(session.document.revision()),
                ..rejected_edit(TransactionError { index: None, error })
            })?;
        Ok((self.respond(session), plan))
    }

    /// Captures checked transformed geometry and its exact snapshot under one session lock.
    pub(crate) fn transform_plan(
        &self,
        id: &str,
        selection: &Selection,
        instance: Option<ObjectId>,
        pivot: Point,
        transform: SelectionTransform,
    ) -> Result<(DocumentResponse, TransformPlan), ApplicationError> {
        let mut sessions = self.lock();
        let session = self.session(&mut sessions, id)?;
        let plan = session
            .document
            .selection_transform(selection, instance, pivot, transform)
            .map_err(|error| ApplicationError {
                current_revision: Some(session.document.revision()),
                ..rejected_edit(TransactionError { index: None, error })
            })?;
        Ok((self.respond(session), plan))
    }

    /// The view and a history-free copy of the document are taken under the same lock, so later
    /// computation can be checked against the exact analyzed snapshot even after a service restart
    /// or concurrent edit. The copy leaves out undo and redo history, which no analysis reads; the
    /// view still reports the session's history.
    pub(crate) fn analysis_snapshot(
        &self,
        id: &str,
    ) -> Result<(DocumentResponse, Document), ApplicationError> {
        let mut sessions = self.lock();
        let session = self.session(&mut sessions, id)?;
        Ok((self.respond(session), session.document.without_history()))
    }

    /// Captures graph and document view under the same lock; the read-only interpretation
    /// runs after the copy is made so its result remains tied to these exact atoms and bonds.
    pub(crate) fn graph_snapshot(
        &self,
        id: &str,
    ) -> Result<(DocumentResponse, MoleculeGraph), ApplicationError> {
        let mut sessions = self.lock();
        let session = self.session(&mut sessions, id)?;
        Ok((
            self.respond(session),
            MoleculeGraph::from_document(&session.document),
        ))
    }

    /// Captures the rich export's graph, stereo and scene together with its response snapshot.
    /// Neither capture includes undo or redo history.
    pub(crate) fn interchange_export_snapshot(
        &self,
        id: &str,
    ) -> Result<(DocumentResponse, crate::interchange_export::ExportSnapshot), ApplicationError>
    {
        let mut sessions = self.lock();
        let session = self.session(&mut sessions, id)?;
        Ok((
            self.respond(session),
            crate::interchange_export::ExportSnapshot::capture(&session.document),
        ))
    }

    /// Captures the graph and enhanced stereo groups at the same revision without cloning the
    /// document's undo and redo histories.
    pub(crate) fn graph_and_groups_snapshot(
        &self,
        id: &str,
    ) -> Result<(DocumentResponse, MoleculeGraph, StereoGroupTable), ApplicationError> {
        let mut sessions = self.lock();
        let session = self.session(&mut sessions, id)?;
        Ok((
            self.respond(session),
            MoleculeGraph::from_document(&session.document),
            session.document.stereo_groups().clone(),
        ))
    }

    /// Capture both working copies while holding the session lock. IDs and revisions in the
    /// returned views therefore name the exact graphs used by a comparison.
    pub(crate) fn graph_pair_snapshot(
        &self,
        id: &str,
        other_id: Option<&str>,
    ) -> Result<(GraphSnapshot, Option<GraphSnapshot>), ApplicationError> {
        let mut sessions = self.lock();
        // Validate both IDs before either read. Closed graphs are inspected without opening a
        // session, so even a refused comparison cannot evict an unrelated working copy.
        parse_id(id)?;
        if let Some(other_id) = other_id {
            parse_id(other_id)?;
        }
        let first = self.graph_readonly(&mut sessions, id)?;
        let other = match other_id {
            None => None,
            Some(other_id) => Some(self.graph_second(&mut sessions, &first, id, other_id)?),
        };
        Ok((first, other))
    }

    /// Both graphs of a comparison that always has two sides, which may be one document, read
    /// like [`Self::graph_pair_snapshot`].
    pub(crate) fn graph_pair(
        &self,
        id: &str,
        other_id: &str,
    ) -> Result<(GraphSnapshot, GraphSnapshot), ApplicationError> {
        let mut sessions = self.lock();
        parse_id(id)?;
        parse_id(other_id)?;
        let first = self.graph_readonly(&mut sessions, id)?;
        let other = self.graph_second(&mut sessions, &first, id, other_id)?;
        Ok((first, other))
    }

    /// The second graph of a pair: the first itself when it is the same document.
    fn graph_second(
        &self,
        sessions: &mut Sessions,
        first: &GraphSnapshot,
        id: &str,
        other_id: &str,
    ) -> Result<GraphSnapshot, ApplicationError> {
        if other_id == id {
            Ok(first.clone())
        } else {
            self.graph_readonly(sessions, other_id)
        }
    }

    fn graph_readonly(
        &self,
        sessions: &mut Sessions,
        id: &str,
    ) -> Result<GraphSnapshot, ApplicationError> {
        let parsed = parse_id(id)?;
        if sessions.open.contains_key(&parsed) {
            let session = self.session(sessions, id)?;
            Ok((
                self.respond(session),
                MoleculeGraph::from_document(&session.document),
            ))
        } else {
            // Use the same revision continuation that opening the saved copy would use,
            // without publishing a session or consuming a slot. A returned revision must
            // still be retired so a later different commit cannot reuse it.
            let document = self.load(&parsed)?.ok_or_else(|| not_found(&parsed))?;
            let session = Session::open(document, sessions.retired.get(&parsed).copied())?;
            let snapshot = (
                self.respond(&session),
                MoleculeGraph::from_document(&session.document),
            );
            if let Some(retired) = session.retirement() {
                let merged = match sessions.retired.get(&parsed) {
                    Some(known) => known.merge(retired),
                    None => retired,
                };
                sessions.retired.insert(parsed, merged);
            }
            Ok(snapshot)
        }
    }

    /// Convert an entire graph as one ordinary undoable edit. A replay is checked before
    /// interpreting again, and a failed assignment never reaches the document's history.
    ///
    /// The conversion is computed on a copy of the graph while no lock is held, so other
    /// requests, including edits to this very document, go on meanwhile. Its result is applied
    /// only if the document is still at `expected_revision`; otherwise nothing is applied and
    /// the request is refused with `revision_conflict`. `compute` can stop the computation,
    /// including while it waits for a free slot.
    pub(crate) fn convert_aromaticity(
        &self,
        id: &str,
        request_id: Option<&str>,
        expected_revision: u64,
        conversion: AromaticityConversion,
        selection: TransactionSelections,
        compute: Compute<'_>,
    ) -> Result<DocumentResponse, ApplicationError> {
        let domain_selection = contract::to_selection(&selection)?;
        let mutation = Mutation::AromaticityConversion {
            expected_revision,
            conversion,
            selection,
            time_limit_ms: compute.requested_ms,
        };
        validate_request_id(request_id)?;
        let document_id = parse_id(id)?;
        let _running = self.start(Some(&document_id), request_id)?;
        // Under the lock: answer a completed request, refuse early if the document is already
        // not at the expected revision, and copy the graph. Nothing else happens there.
        let graph = {
            let mut sessions = self.lock();
            let session = self.session(&mut sessions, id)?;
            if let Some(response) = session.replays.find(request_id, &mutation)? {
                return Ok(response);
            }
            if let Some(hold) = session.hold {
                return Err(held(id, hold));
            }
            session.expect_revision(expected_revision)?;
            MoleculeGraph::from_document(&session.document)
        };
        let permit = compute
            .gate
            .enter(compute.interrupt)
            .map_err(|interrupted| interruption(interrupted, compute.limit_ms))?;
        let target = match conversion {
            AromaticityConversion::Aromatic => aromaticity::Target::Aromatic,
            AromaticityConversion::Kekule => aromaticity::Target::Kekule,
        };
        let commands = aromaticity::conversion_commands_with(&graph, target, compute.interrupt)
            .map_err(|halt| match halt {
                aromaticity::Halt::Reason(reason) => aromaticity_refusal(reason),
                aromaticity::Halt::Interrupted(interrupted) => {
                    interruption(interrupted, compute.limit_ms)
                }
            })?;
        // The slot is given back before the session lock is taken again, so that waiting for
        // the lock does not keep another computation out.
        drop(permit);
        // The lock is taken again, which checks the replay, the hold and the revision again:
        // an edit made while the conversion was computed refuses its result. A stop that came
        // after the last checkpoint is honoured here too, immediately before the edit is
        // committed; once it is committed it stands.
        self.mutate_running(id, request_id, expected_revision, mutation, |session| {
            compute
                .interrupt
                .check()
                .map_err(|interrupted| interruption(interrupted, compute.limit_ms))?;
            if commands.is_empty() {
                return Ok(Vec::new());
            }
            let result = session
                .document
                .apply_reporting(&commands, &domain_selection);
            conversion_notes(&session.document, result)
        })
    }

    /// Applies a chain layout as one ordinary undoable edit (`layout.structure` with
    /// `mode: "apply"`), like [`Self::convert_aromaticity`]. Under one lock a completed request is
    /// answered from the replay log, a held document is refused, and a document that is not at
    /// `expected_revision` is refused with `revision_conflict`, before any layout refusal; then the
    /// document is copied without its history. `prepare` reads the copy with no lock held, and its
    /// refusal is the caller's own reply, so a deterministic refusal never waits for a slot. A
    /// compute slot is then taken with the request's budget, and the budget is asked once; `place`
    /// computes the commands and diagnostics, and the slot is given back. The edit is committed
    /// under the lock only if the replay, the hold and the revision still allow it and no stop came
    /// at the last question. A refused edit names no command index, because its commands are
    /// generated. An edit with no commands changes nothing and is still recorded for replay.
    pub(crate) fn apply_layout<P, E>(
        &self,
        id: &str,
        request_id: Option<&str>,
        request: LayoutApply,
        compute: Compute<'_>,
        prepare: impl FnOnce(&Document) -> Result<P, E>,
        place: impl FnOnce(P, &dyn Interrupt) -> Result<Result<LayoutEdit, E>, Interrupted>,
    ) -> Result<DocumentResponse, LayoutHalt<E>> {
        let domain_selection = contract::to_selection(&request.selections)?;
        let expected_revision = request.expected_revision;
        let mutation = Mutation::LayoutApply {
            expected_revision,
            selection: request.selection,
            scope: request.scope,
            instance: request.instance,
            selections: request.selections,
            time_limit_ms: compute.requested_ms,
        };
        validate_request_id(request_id)?;
        let document_id = parse_id(id)?;
        let _running = self.start(Some(&document_id), request_id)?;
        // Under the lock: answer a completed request, refuse a held document or a stale revision
        // before any layout refusal, and copy the document. Nothing else happens there.
        let document = {
            let mut sessions = self.lock();
            let session = self.session(&mut sessions, id)?;
            if let Some(response) = session.replays.find(request_id, &mutation)? {
                return Ok(response);
            }
            if let Some(hold) = session.hold {
                return Err(held(id, hold).into());
            }
            session.expect_revision(expected_revision)?;
            session.document.without_history()
        };
        let prepared = prepare(&document).map_err(LayoutHalt::Refused)?;
        let stopped = |interrupted: Interrupted| interruption(interrupted, compute.limit_ms);
        let permit = compute.gate.enter(compute.interrupt).map_err(stopped)?;
        compute.interrupt.check().map_err(stopped)?;
        let LayoutEdit {
            commands,
            diagnostics,
        } = place(prepared, compute.interrupt)
            .map_err(stopped)?
            .map_err(LayoutHalt::Refused)?;
        // The slot is given back before the session lock is taken again, so that waiting for
        // the lock does not keep another computation out.
        drop(permit);
        // The lock is taken again, which checks the replay, the hold and the revision again: an
        // edit made while the layout was computed refuses its result. A stop that came after the
        // last checkpoint is honoured here, immediately before the edit is committed.
        let response = self.mutate_running_reporting(
            id,
            request_id,
            expected_revision,
            mutation,
            diagnostics,
            |session| {
                compute.interrupt.check().map_err(stopped)?;
                if commands.is_empty() {
                    return Ok(Vec::new());
                }
                let result = session
                    .document
                    .apply_reporting(&commands, &domain_selection);
                edit_notes(&session.document, result).map_err(|refused| ApplicationError {
                    command_index: None,
                    ..refused
                })
            },
        )?;
        Ok(response)
    }

    pub(crate) fn transact(
        &self,
        id: &str,
        request_id: Option<&str>,
        expected_revision: u64,
        commands: Vec<CommandDto>,
        selection: TransactionSelections,
    ) -> Result<DocumentResponse, ApplicationError> {
        let domain_commands = if commands
            .iter()
            .any(|command| matches!(command, CommandDto::AddImage { .. }))
        {
            let mut cache = self.image_cache.lock().map_err(|_| {
                ApplicationError::new(
                    ErrorCode::ResourceLimit,
                    "Image validation cache is unavailable",
                )
            })?;
            contract::to_commands(&commands, self.image_decoder.as_deref(), &mut cache)?
        } else {
            contract::to_commands(
                &commands,
                self.image_decoder.as_deref(),
                &mut Default::default(),
            )?
        };
        let domain_selection = contract::to_selection(&selection)?;
        let mutation = Mutation::Transaction {
            expected_revision,
            commands,
            selection,
        };
        self.mutate(id, request_id, expected_revision, mutation, |session| {
            let result = session
                .document
                .apply_reporting(&domain_commands, &domain_selection);
            edit_notes(&session.document, result)
        })
    }

    /// Preview validates on a clone under the session lock; it reserves no identifiers.
    pub(crate) fn template_plan(
        &self,
        parameters: &crate::operations::template::AttachParameters,
        fragment: &chemifree_domain::clipboard::Fragment,
    ) -> Result<(DocumentResponse, DocumentResponse), ApplicationError> {
        let mut sessions = self.lock();
        let session = self.session(&mut sessions, &parameters.document_id)?;
        session.expect_revision(parameters.expected_revision)?;
        let (instance, source, target) = crate::operations::template::targets(parameters)?;
        let plan = chemifree_domain::template::attach(
            fragment,
            &session.document,
            instance,
            source,
            target,
            chemifree_domain::document::Vector {
                dx: parameters.direction.dx,
                dy: parameters.direction.dy,
            },
        )
        .map_err(crate::operations::template::refusal)?;
        let selection = contract::to_selection(&TransactionSelections {
            before: parameters.selection_before.clone(),
            after: None,
        })?;
        let mut candidate = session.document.clone();
        let applied = candidate.apply_reporting(
            &plan.paste.commands,
            &TransactionSelection {
                before: selection.before,
                after: Some(plan.paste.selection),
            },
        );
        let notes = edit_notes(&candidate, applied)?;
        let mut after =
            DocumentResponse::new(&candidate, session.saved_revision, session.draft_revision);
        after.notes = notes;
        Ok((self.respond(session), after))
    }

    /// Replay binds the exact native bytes, explicit anchors, direction bits and captured selection.
    pub(crate) fn template_attach(
        &self,
        request_id: &str,
        parameters: &crate::operations::template::AttachParameters,
        fragment: &chemifree_domain::clipboard::Fragment,
    ) -> Result<DocumentResponse, ApplicationError> {
        let fingerprint = serde_json::to_string(parameters)
            .map_err(|error| ApplicationError::new(ErrorCode::InvalidRequest, error.to_string()))?;
        let (instance, source, target) = crate::operations::template::targets(parameters)?;
        let selection = contract::to_selection(&TransactionSelections {
            before: parameters.selection_before.clone(),
            after: None,
        })?;
        self.mutate(
            &parameters.document_id,
            Some(request_id),
            parameters.expected_revision,
            Mutation::TemplateAttach { fingerprint },
            |session| {
                let plan = chemifree_domain::template::attach(
                    fragment,
                    &session.document,
                    instance,
                    source,
                    target,
                    chemifree_domain::document::Vector {
                        dx: parameters.direction.dx,
                        dy: parameters.direction.dy,
                    },
                )
                .map_err(crate::operations::template::refusal)?;
                let result = session.document.apply_reporting(
                    &plan.paste.commands,
                    &TransactionSelection {
                        before: selection.before,
                        after: Some(plan.paste.selection),
                    },
                );
                edit_notes(&session.document, result)
            },
        )
    }

    /// Replacement preview runs on a clone under the exact destination revision lock.
    pub(crate) fn template_replace_plan(
        &self,
        parameters: &crate::operations::template::ReplaceParameters,
        fragment: &chemifree_domain::clipboard::Fragment,
    ) -> Result<(DocumentResponse, DocumentResponse), ApplicationError> {
        let mut sessions = self.lock();
        let session = self.session(&mut sessions, &parameters.document_id)?;
        session.expect_revision(parameters.expected_revision)?;
        let (instance, source, target) =
            crate::operations::template::replacement_targets(parameters)?;
        let plan = chemifree_domain::template::replace(
            fragment,
            &session.document,
            instance,
            source,
            target,
        )
        .map_err(crate::operations::template::refusal)?;
        let selection = contract::to_selection(&TransactionSelections {
            before: parameters.selection_before.clone(),
            after: None,
        })?;
        let mut candidate = session.document.clone();
        let applied = candidate.apply_reporting(
            &plan.paste.commands,
            &TransactionSelection {
                before: selection.before,
                after: Some(plan.paste.selection),
            },
        );
        let notes = edit_notes(&candidate, applied)?;
        let mut after =
            DocumentResponse::new(&candidate, session.saved_revision, session.draft_revision);
        after.notes = notes;
        Ok((self.respond(session), after))
    }

    /// Replacement replay includes the exact file, bound atom and captured selection.
    pub(crate) fn template_replace(
        &self,
        request_id: &str,
        parameters: &crate::operations::template::ReplaceParameters,
        fragment: &chemifree_domain::clipboard::Fragment,
    ) -> Result<DocumentResponse, ApplicationError> {
        let fingerprint = serde_json::to_string(parameters)
            .map_err(|error| ApplicationError::new(ErrorCode::InvalidRequest, error.to_string()))?;
        let (instance, source, target) =
            crate::operations::template::replacement_targets(parameters)?;
        let selection = contract::to_selection(&TransactionSelections {
            before: parameters.selection_before.clone(),
            after: None,
        })?;
        self.mutate(
            &parameters.document_id,
            Some(request_id),
            parameters.expected_revision,
            Mutation::TemplateReplace { fingerprint },
            |session| {
                let plan = chemifree_domain::template::replace(
                    fragment,
                    &session.document,
                    instance,
                    source,
                    target,
                )
                .map_err(crate::operations::template::refusal)?;
                let result = session.document.apply_reporting(
                    &plan.paste.commands,
                    &TransactionSelection {
                        before: selection.before,
                        after: Some(plan.paste.selection),
                    },
                );
                edit_notes(&session.document, result)
            },
        )
    }

    /// Native copy captures its source graph and snapshot under one lock.
    pub(crate) fn clipboard_copy(
        &self,
        id: &str,
        selection: &Selection,
        instance: ObjectId,
    ) -> Result<
        (
            DocumentResponse,
            chemifree_domain::clipboard::CopiedFragment,
        ),
        ApplicationError,
    > {
        let mut sessions = self.lock();
        let session = self.session(&mut sessions, id)?;
        let copied =
            chemifree_domain::clipboard::copy_fragment(&session.document, selection, instance)
                .map_err(|error| ApplicationError {
                    current_revision: Some(session.document.revision()),
                    ..crate::clipboard::domain_refusal(error)
                })?;
        Ok((self.respond(session), copied))
    }

    /// Validate and apply the complete native fragment within the ordinary mutation/replay gate.
    pub(crate) fn clipboard_paste(
        &self,
        id: &str,
        request_id: Option<&str>,
        request: crate::clipboard::Paste<'_>,
    ) -> Result<DocumentResponse, ApplicationError> {
        let selection = contract::to_selection(&TransactionSelections {
            before: request.selection_before.clone(),
            after: None,
        })?;
        let mutation = Mutation::ClipboardPaste {
            expected_revision: request.expected_revision,
            payload: request.payload.into(),
            offset: request.offset,
            selection_before: request.selection_before,
        };
        self.mutate(
            id,
            request_id,
            request.expected_revision,
            mutation,
            |session| {
                let plan = request
                    .fragment
                    .paste_commands(
                        &session.document,
                        chemifree_domain::document::Vector {
                            dx: request.offset.x,
                            dy: request.offset.y,
                        },
                    )
                    .map_err(|error| ApplicationError {
                        current_revision: Some(session.document.revision()),
                        ..crate::clipboard::domain_refusal(error)
                    })?;
                if plan.selection.atoms.len() != request.fragment.atoms.len()
                    || plan.selection.bonds.len() != request.fragment.bonds.len()
                {
                    return Err(crate::clipboard::refusal(
                        "clipboard_mapping_invalid",
                        "Paste plan does not identify the complete new fragment",
                    ));
                }
                let result = session.document.apply_reporting(
                    &plan.commands,
                    &TransactionSelection {
                        before: selection.before,
                        after: Some(plan.selection),
                    },
                );
                edit_notes(&session.document, result)
            },
        )
    }

    /// Capture a graphic-only forest and its exact source snapshot under one lock.
    pub(crate) fn clipboard_scene_copy(
        &self,
        id: &str,
        selection: &Selection,
    ) -> Result<
        (
            DocumentResponse,
            chemifree_domain::scene_clipboard::Fragment,
        ),
        ApplicationError,
    > {
        let mut sessions = self.lock();
        let session = self.session(&mut sessions, id)?;
        let fragment =
            chemifree_domain::scene_clipboard::copy_fragment(&session.document, selection)
                .map_err(|error| ApplicationError {
                    current_revision: Some(session.document.revision()),
                    ..crate::scene_clipboard::domain_refusal(error)
                })?;
        Ok((self.respond(session), fragment))
    }

    /// Preflight every object/reference and replay one immutable scene paste atomically.
    pub(crate) fn clipboard_scene_paste(
        &self,
        id: &str,
        request_id: Option<&str>,
        request: crate::scene_clipboard::Paste<'_>,
    ) -> Result<DocumentResponse, ApplicationError> {
        let selection = contract::to_selection(&TransactionSelections {
            before: request.selection_before.clone(),
            after: None,
        })?;
        let mutation = Mutation::ClipboardScenePaste {
            expected_revision: request.expected_revision,
            payload: request.payload.into(),
            offset_bits: [request.offset.x.to_bits(), request.offset.y.to_bits()],
            selection_before: request.selection_before,
        };
        self.mutate(
            id,
            request_id,
            request.expected_revision,
            mutation,
            |session| {
                let plan = request
                    .fragment
                    .paste_commands(
                        &session.document,
                        chemifree_domain::document::Vector {
                            dx: request.offset.x,
                            dy: request.offset.y,
                        },
                    )
                    .map_err(|error| ApplicationError {
                        current_revision: Some(session.document.revision()),
                        ..crate::scene_clipboard::domain_refusal(error)
                    })?;
                let mut candidate = session.document.clone();
                let result = candidate.apply_reporting(
                    &plan.commands,
                    &TransactionSelection {
                        before: selection.before,
                        after: Some(plan.selection),
                    },
                );
                let notes = edit_notes(&candidate, result)?;
                let projected = DocumentResponse::new(
                    &candidate,
                    session.saved_revision,
                    session.draft_revision,
                );
                let mapping = crate::operations::scene_clipboard_mapping(
                    request.fragment,
                    &projected.document,
                )?;
                if mapping != plan.objects || !notes.is_empty() {
                    return Err(crate::clipboard::refusal(
                        "clipboard_scene_mapping_invalid",
                        "Scene paste must preserve the complete graphic forest without chemical changes",
                    ));
                }
                session.document = candidate;
                Ok(notes)
            },
        )
    }

    /// Check a previously completed import before looking at the document's current revision.
    pub(crate) fn replay_import(
        &self,
        id: &str,
        request_id: Option<&str>,
        fingerprint: &str,
    ) -> Result<Option<DocumentResponse>, ApplicationError> {
        validate_request_id(request_id)?;
        let document_id = parse_id(id)?;
        let mutation = Mutation::ImportCtfile {
            fingerprint: fingerprint.to_owned(),
        };
        self.replayed(request_id, |sessions| {
            sessions.open.get(&document_id).map_or(Ok(None), |session| {
                session.replays.find(request_id, &mutation)
            })
        })
    }

    /// Apply the complete graph to a copy for preview. This performs the very same domain
    /// validation as a real transaction without changing the working document or its history.
    pub(crate) fn preview_import(
        &self,
        id: &str,
        expected_revision: u64,
        commands: &[Command],
    ) -> Result<(DocumentResponse, DocumentResponse), ApplicationError> {
        let mut sessions = self.lock();
        let session = self.session(&mut sessions, id)?;
        session.expect_revision(expected_revision)?;
        let before = self.respond(session);
        let mut candidate = session.document.clone();
        if !commands.is_empty() {
            candidate.apply(commands).map_err(rejected_edit)?;
        }
        let after =
            DocumentResponse::new(&candidate, session.saved_revision, session.draft_revision);
        Ok((before, after))
    }

    /// Recheck the consented candidate under the session lock and commit it atomically.
    pub(crate) fn import(
        &self,
        id: &str,
        request_id: Option<&str>,
        expected_revision: u64,
        fingerprint: String,
        commands: &[Command],
        verify: impl FnOnce(&DocumentView, &DocumentView) -> Result<(), ApplicationError>,
    ) -> Result<DocumentResponse, ApplicationError> {
        let mutation = Mutation::ImportCtfile { fingerprint };
        self.mutate(id, request_id, expected_revision, mutation, |session| {
            let before = session.response();
            let mut candidate = session.document.clone();
            let mut notes = Vec::new();
            if !commands.is_empty() {
                let result = candidate.apply_reporting(commands, &TransactionSelection::default());
                notes = edit_notes(&candidate, result)?;
            }
            let projected =
                DocumentResponse::new(&candidate, session.saved_revision, session.draft_revision);
            verify(&before.document, &projected.document)?;
            session.document = candidate;
            // An import adds atoms and bonds between new atoms only, so `notes` is empty; if
            // that ever stops being true the reply is `partial` instead of silently `ok`.
            Ok(notes)
        })
    }

    /// The settings of the document as this process shows it: its working copy, opened from
    /// storage if needed.
    pub(crate) fn settings(&self, id: &str) -> Result<Settings, ApplicationError> {
        let mut sessions = self.lock();
        let session = self.session(&mut sessions, id)?;
        Ok(session.document.settings().clone())
    }

    /// Replaces the document's settings with what `target` reads, as one `update_settings`
    /// edit: one revision and history entry, or nothing when they are the same. A completed
    /// request with the same `request_id` is replayed before `target` is read, so a replay does
    /// not depend on where the settings came from.
    pub(crate) fn replace_settings(
        &self,
        id: &str,
        request_id: Option<&str>,
        expected_revision: u64,
        mutation: SettingsMutation,
        selection: TransactionSelections,
        target: impl FnOnce() -> Result<Settings, ApplicationError>,
    ) -> Result<DocumentResponse, ApplicationError> {
        let domain_selection = contract::to_selection(&selection)?;
        validate_request_id(request_id)?;
        let document_id = parse_id(id)?;
        let _running = self.start(Some(&document_id), request_id)?;
        let mutation = match mutation {
            SettingsMutation::Reset(to) => Mutation::ResetSettings {
                expected_revision,
                to,
                selection,
            },
            SettingsMutation::Import(source) => Mutation::ImportSettings {
                expected_revision,
                source,
                selection,
            },
        };
        {
            let mut sessions = self.lock();
            let session = self.session(&mut sessions, id)?;
            if let Some(response) = session.replays.find(request_id, &mutation)? {
                return Ok(response);
            }
            if let Some(hold) = session.hold {
                return Err(held(id, hold));
            }
            session.expect_revision(expected_revision)?;
        }
        let command = Command::UpdateSettings(target()?.to_edit());
        self.mutate_running(id, request_id, expected_revision, mutation, |session| {
            let result = session
                .document
                .apply_reporting(&[command], &domain_selection);
            edit_notes(&session.document, result)
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
            session
                .document
                .undo()
                .map(|()| Vec::new())
                .map_err(history_error)
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
            session
                .document
                .redo()
                .map(|()| Vec::new())
                .map_err(history_error)
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
        if !session.is_dirty() {
            session.drafting.settled();
        }
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
        self.draft_at(id, request_id, Instant::now(), Drafter::Client)
    }

    /// Writes the draft for `drafter`. `now` is when the service found it due; a failure's
    /// retry counts from when the failure happened, which may be a turn wait later.
    fn draft_at(
        &self,
        id: &str,
        request_id: Option<&str>,
        now: Instant,
        drafter: Drafter,
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
        let _turn = match self
            .turns
            .take(&document_id, drafter.turn_wait())
            .map_err(refused)
        {
            Ok(turn) => turn,
            Err(error) => {
                // A document that stayed busy is a failed attempt for undrafted work only; the
                // periodic pass, which does not wait, just tries again next time.
                if drafter != Drafter::Service
                    && let Some(session) = self.lock().open.get_mut(&document_id)
                    && !session.is_drafted()
                {
                    session
                        .drafting
                        .failed(now.max(Instant::now()), error.clone());
                }
                return Err(error);
            }
        };
        self.write_draft(&document_id, id, request_id, now, drafter)
    }

    /// The draft write itself, with the document's turn held. Every attempt, whoever asked for
    /// it, moves the service's own schedule: a success leaves it only the changes made while
    /// the write ran, and a failure is retried on its backoff, not sooner.
    ///
    /// The recovery lookup and the write run with the turn held but without the session table,
    /// so a stalled disk there holds up this document's drafting and other writers waiting for
    /// its turn, not other documents or edits to this one. The table is still held for the
    /// session's load or refresh and to record the outcome, and a reply recorded there reads the
    /// draft's status under it. Responses for this document are not isolated from the lookup:
    /// while it holds the draft's ownership, a response waits for it with the table held. While
    /// the write runs, responses show the lookup's result without reading the draft. Edits made
    /// meanwhile stay unsaved and undrafted: the outcome names the revision written.
    fn write_draft(
        &self,
        document_id: &DocumentId,
        id: &str,
        request_id: Option<&str>,
        now: Instant,
        drafter: Drafter,
    ) -> Result<DocumentResponse, ApplicationError> {
        // Looked up with this document's turn held but not the table: the lookup can read a
        // whole abandoned draft or wait for the draft's ownership. Changes this process makes to
        // the draft take the turn, except settling an interrupted save, which the store
        // serializes and makes idempotent. This lookup may settle one; nothing else in this
        // method changes the draft before the snapshot below. Another process could change it at
        // any time, with or without the table.
        let recovery = self.recovery(document_id);
        let task = {
            let mut sessions = self.lock();
            if drafter != Drafter::Client && !sessions.open.contains_key(document_id) {
                // Closed since the service found it due: there is nothing left to draft, and
                // opening it again would only make a session nobody asked for.
                return Err(no_longer_open(id));
            }
            let session = self.session(&mut sessions, id)?;
            if let Some(response) = session.replays.find(request_id, &Mutation::Autosave)? {
                return Ok(response);
            }
            match DraftTask::for_session(session) {
                Some(task) => {
                    session.recovery_at_write = recovery;
                    session.drafting.writing();
                    task
                }
                None => {
                    session.drafting.settled();
                    return Ok(self.record_autosave(session, request_id));
                }
            }
        };
        let outcome = task.run(&self.repository);
        let mut sessions = self.lock();
        let Some(session) = sessions.open.get_mut(document_id) else {
            // Only a clean copy can end while its turn is held (its stored document was
            // deleted elsewhere), and a clean copy's task removes the draft. A draft written
            // for a copy that is gone is left for recovery rather than owned by nobody.
            if matches!(task, DraftTask::Write(_)) && outcome.is_ok() {
                self.repository.release_draft(document_id);
            }
            return Err(no_longer_open(id));
        };
        session.recovery_at_write = None;
        if let Err(error) = outcome {
            session
                .drafting
                .failed(now.max(Instant::now()), error.clone());
            return Err(error);
        }
        session.draft_revision = task.drafted_revision();
        session.drafting.written();
        Ok(self.record_autosave(session, request_id))
    }

    fn record_autosave(&self, session: &mut Session, request_id: Option<&str>) -> DocumentResponse {
        let response = self.respond(session);
        session
            .replays
            .record(request_id, Mutation::Autosave, &response);
        response
    }

    /// Documents whose drafts are due at `now` (see [`DraftSchedule`]), except those whose
    /// draft is being written already and those whose turn is taken (by a save, say): the
    /// service does not wait for a turn, so they are simply due again on a later pass.
    pub(crate) fn due_drafts(&self, now: Instant) -> Vec<String> {
        let due: Vec<DocumentId> = self
            .lock()
            .open
            .iter()
            .filter(|(_, session)| {
                !session.drafting.is_writing()
                    && session.drafting.due_at().is_some_and(|at| at <= now)
            })
            .map(|(id, _)| id.clone())
            .collect();
        due.into_iter()
            .filter(|id| !self.turns.is_taken(id))
            .map(|id| id.to_string())
            .collect()
    }

    /// The service's attempt at a due draft. It does not wait for a turn that is taken: the
    /// document is simply due again on a later pass. A failure is kept on the session's
    /// schedule and retried there.
    pub(crate) fn write_due_draft(
        &self,
        id: &str,
        now: Instant,
    ) -> Result<DocumentResponse, ApplicationError> {
        self.draft_at(id, None, now, Drafter::Service)
    }

    /// When the next draft is due, if any work is undrafted.
    pub(crate) fn next_draft_due(&self) -> Option<Instant> {
        self.lock()
            .open
            .values()
            .filter_map(|session| session.drafting.due_at())
            .min()
    }

    /// Drafts every working copy whose draft does not hold its unsaved work, now, as before the
    /// process stops. Returns the documents that could not be drafted, with why.
    pub(crate) fn draft_all(&self) -> Vec<(String, ApplicationError)> {
        let mut undrafted: Vec<String> = self
            .lock()
            .open
            .iter()
            .filter(|(_, session)| !session.is_drafted())
            .map(|(id, _)| id.to_string())
            .collect();
        // In identifier order, so failures are reported in the same order every time.
        undrafted.sort_unstable();
        // One thread per document, so a write that stalls does not keep the others from being
        // drafted before the stop.
        thread::scope(|scope| {
            let attempts: Vec<_> = undrafted
                .into_iter()
                .map(|id| {
                    let document = id.clone();
                    let attempt = scope.spawn(move || {
                        self.draft_at(&document, None, Instant::now(), Drafter::Stop)
                    });
                    (id, attempt)
                })
                .collect();
            attempts
                .into_iter()
                .filter_map(|(id, attempt)| match attempt.join() {
                    Ok(drafted) => drafted.err().map(|error| (id, error)),
                    Err(panic) => panic::resume_unwind(panic),
                })
                .collect()
        })
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
        let mut turn = self.turn(&document_id)?;
        let draft_io = turn.draft_io();
        let check_clean = |sessions: &Sessions| {
            if sessions
                .open
                .get(&document_id)
                .is_some_and(Session::is_dirty)
            {
                Err(ApplicationError::new(
                    ErrorCode::UnsavedChanges,
                    "the open session has unsaved changes; save or discard them before recovering",
                ))
            } else {
                Ok(())
            }
        };
        {
            let sessions = self.lock();
            if let Some(response) = sessions.recovered.find(request_id, &mutation)? {
                return Ok(response);
            }
            check_clean(&sessions)?;
        }
        let stored = self
            .load(&document_id)?
            .ok_or_else(|| not_found(&document_id))?;
        // Claiming and preserving a draft may wait on storage; no session lock is held.
        let draft = self.claim_abandoned(&document_id)?;
        if stored.revision() != draft.base_revision {
            self.repository.release_draft(&document_id);
            return Err(outdated_draft(stored.revision(), draft.base_revision));
        }
        {
            let sessions = self.lock();
            let ready = check_clean(&sessions)
                .and_then(|()| self.recovery_numbering(&sessions, &stored).map(|_| ()))
                .and_then(|()| {
                    if sessions.open.contains_key(&document_id) {
                        Ok(())
                    } else {
                        sessions.eviction(None).map(|_| ())
                    }
                });
            if let Err(error) = ready {
                self.repository.release_draft(&document_id);
                return Err(error);
            }
        }
        self.prepare_claimed_draft(
            &document_id,
            DraftRecoveryTarget::Original {
                base_revision: draft.base_revision,
            },
        )?;
        drop(draft_io);
        let mut sessions = self.lock();
        // Edits can proceed while storage is busy. Never replace work made in that interval.
        if let Err(error) = check_clean(&sessions) {
            self.repository.release_draft(&document_id);
            return Err(error);
        }
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

    /// The working copy a draft becomes after its base was checked, if this process has not
    /// handed out identifiers the draft may also use.
    fn recovered_session(
        &self,
        sessions: &Sessions,
        stored: &Document,
        draft: Draft,
    ) -> Result<Session, ApplicationError> {
        let (reached, saved_revision) = self.recovery_numbering(sessions, stored)?;
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
            drafting: DraftSchedule::default(),
            recovery_at_write: None,
            replays: ReplayLog::default(),
        })
    }

    /// Numbering the restored session must continue after, provided it cannot reuse IDs this
    /// process handed out since the stored commit.
    fn recovery_numbering(
        &self,
        sessions: &Sessions,
        stored: &Document,
    ) -> Result<(Numbering, u64), ApplicationError> {
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
        if has_issued_identifiers_beyond(reached, counters) {
            return Err(ApplicationError::new(
                ErrorCode::DraftOutdated,
                "this process already used atom, bond, scene instance, or stereo group identifiers \
                 the draft may reuse; recover the draft as a copy",
            ));
        }
        Ok((reached, saved_revision))
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
        let mut turn = self.turn(&document_id)?;
        if let Some(response) = self.replayed(request_id, |sessions| {
            sessions.copied.find(request_id, &mutation)
        })? {
            return Ok(response);
        }
        let draft_io = turn.draft_io();
        // The abandoned draft and its original bytes are read without holding the session table.
        let draft = self.claim_abandoned(&document_id)?;
        let title = title.map_or_else(
            || suffixed(draft.document.title(), " (recovered)"),
            str::to_owned,
        );
        if let Err(error) = validated_title(&title) {
            self.repository.release_draft(&document_id);
            return Err(ApplicationError::new(
                ErrorCode::InvalidRequest,
                format!("title: {error}"),
            ));
        }
        if let Err(error) = self.lock().eviction(Some(&document_id)) {
            self.repository.release_draft(&document_id);
            return Err(error);
        }
        self.prepare_claimed_draft(&document_id, DraftRecoveryTarget::Copy)?;
        let mut sessions = self.lock();
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
        drop(sessions);
        // The copy is committed. If the draft cannot be removed it is abandoned again and can
        // still be discarded; the copy is not undone. Removal does not hold the session table.
        if self.repository.remove_draft(&document_id).is_err() {
            self.repository.release_draft(&document_id);
        }
        drop(draft_io);
        // The copy or another request may have evicted the source. Only clean sessions are
        // evicted, so no changes were discarded.
        let mut sessions = self.lock();
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
        let mut turn = self.turn(&document_id)?;
        if let Some(response) = self.replayed(request_id, |sessions| {
            sessions.drafts_discarded.find(request_id, &mutation)
        })? {
            return Ok(response);
        }
        let draft_io = turn.draft_io();
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
        drop(draft_io);
        let response = DraftDiscardResponse {
            api_version: API_VERSION,
            id: document_id.to_string(),
        };
        self.lock()
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

    /// Trashed documents, most recently trashed first; equal stamps by identifier.
    pub(crate) fn list_trash(&self) -> Result<TrashListResponse, ApplicationError> {
        let mut entries = self.repository.list_trash().map_err(storage_error)?;
        // The cap keeps the most recent deletions, which the stamps give without reading the
        // documents.
        entries.sort_by(|a, b| {
            b.deleted_at_ms
                .cmp(&a.deleted_at_ms)
                .then_with(|| a.id.cmp(&b.id))
        });
        let mut documents = Vec::new();
        let mut unreadable = Vec::new();
        let truncated = entries.len() > MAX_LISTED_DOCUMENTS;
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
            truncated,
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
        // The copy starts with no history (copy_as), so the source's history is not copied
        // under the table.
        let source = session.document.without_history();
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

    /// Preserves a claimed draft before using it. A failed preservation releases ownership so
    /// the unchanged draft can be recovered or discarded later.
    fn prepare_claimed_draft(
        &self,
        id: &DocumentId,
        target: DraftRecoveryTarget,
    ) -> Result<(), ApplicationError> {
        if let Err(error) = self.repository.prepare_draft_for_recovery(id, target) {
            self.repository.release_draft(id);
            return Err(match (target, error) {
                (
                    DraftRecoveryTarget::Original { base_revision },
                    StorageError::Conflict {
                        stored_revision: Some(current),
                    },
                ) => outdated_draft(current, base_revision),
                (
                    DraftRecoveryTarget::Original { .. },
                    StorageError::Conflict {
                        stored_revision: None,
                    },
                ) => not_found(id),
                // The draft is already owned. A lock refusal here is the document's save
                // lock, so report storage busy rather than another owner of the draft.
                (_, error) => storage_error(error),
            });
        }
        Ok(())
    }

    /// The session's response with any abandoned draft of the same document.
    fn respond(&self, session: &Session) -> DocumentResponse {
        DocumentResponse {
            recovery: self.session_recovery(session),
            ..session.response()
        }
    }

    /// Describes the session document's draft as [`Self::recovery`] does, without probing
    /// storage while the session's own draft write is in progress.
    fn session_recovery(&self, session: &Session) -> Option<RecoveryView> {
        if session.drafting.is_writing() {
            session.recovery_at_write.clone()
        } else if self.turns.draft_io_in_progress(session.document.id()) {
            // The draft is being acted on by this process, like one it already owns. A later
            // response can report it as abandoned again if that operation did not consume it.
            None
        } else {
            self.recovery(session.document.id())
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
        change: impl FnOnce(&mut Session) -> Result<Vec<Loss>, ApplicationError>,
    ) -> Result<DocumentResponse, ApplicationError> {
        validate_request_id(request_id)?;
        let document_id = parse_id(id)?;
        let _running = self.start(Some(&document_id), request_id)?;
        self.mutate_running(id, request_id, expected_revision, mutation, change)
    }

    /// The request is already marked as running; recheck replay and revision after any I/O.
    fn mutate_running(
        &self,
        id: &str,
        request_id: Option<&str>,
        expected_revision: u64,
        mutation: Mutation,
        change: impl FnOnce(&mut Session) -> Result<Vec<Loss>, ApplicationError>,
    ) -> Result<DocumentResponse, ApplicationError> {
        self.mutate_running_reporting(
            id,
            request_id,
            expected_revision,
            mutation,
            Vec::new(),
            change,
        )
    }

    /// Like [`Self::mutate_running`], and records `diagnostics` with the reply, so that a replay
    /// returns them too.
    fn mutate_running_reporting(
        &self,
        id: &str,
        request_id: Option<&str>,
        expected_revision: u64,
        mutation: Mutation,
        diagnostics: Vec<Diagnostic>,
        change: impl FnOnce(&mut Session) -> Result<Vec<Loss>, ApplicationError>,
    ) -> Result<DocumentResponse, ApplicationError> {
        let mut sessions = self.lock();
        let session = self.session(&mut sessions, id)?;
        if let Some(response) = session.replays.find(request_id, &mutation)? {
            return Ok(response);
        }
        if let Some(hold) = session.hold {
            return Err(held(id, hold));
        }
        session.expect_revision(expected_revision)?;
        let before = session.document.revision();
        let notes = change(session)?;
        if session.document.revision() != before && !session.is_drafted() {
            session.drafting.changed(Instant::now());
        }
        // The notes belong to this request alone: they are recorded with its reply, so a
        // replay returns them, and no later reply repeats them.
        let response = DocumentResponse {
            notes,
            diagnostics,
            ..self.respond(session)
        };
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

pub(crate) fn validate_request_id(request_id: Option<&str>) -> Result<(), ApplicationError> {
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

fn no_longer_open(id: &str) -> ApplicationError {
    ApplicationError::new(
        ErrorCode::DocumentNotFound,
        format!("document {id} is no longer open; nothing to draft"),
    )
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

/// The refusal of an aromaticity conversion, with the model's reason. The message says what
/// the caller can act on; the reason carries the detail.
fn aromaticity_refusal(reason: &'static str) -> ApplicationError {
    // Two reasons are not limits of the model, so their messages name the cause: an internal
    // defect, and the transaction bound that the conversion would exceed.
    let message = match reason {
        "invalid_graph" => {
            "Aromaticity conversion found an internal inconsistency in the graph; nothing was changed."
                .to_owned()
        }
        "conversion_command_limit" => format!(
            "Aromaticity conversion needs more than {MAX_COMMANDS} commands and a transaction accepts at most {MAX_COMMANDS}; nothing was changed."
        ),
        _ => "Aromaticity conversion could not be completed under the selected model.".to_owned(),
    };
    ApplicationError {
        invalid_aromaticity: Some(reason),
        ..ApplicationError::new(ErrorCode::InvalidCommand, message)
    }
}

/// What bounds a computation that runs outside the session lock.
#[derive(Clone, Copy)]
pub(crate) struct Compute<'a> {
    /// Asked at the computation's checkpoints.
    pub interrupt: &'a dyn Interrupt,
    /// The time limit in force, in milliseconds, reported when it stops the computation.
    pub limit_ms: u64,
    /// The limit the request asked for, if it asked for one.
    pub requested_ms: Option<u64>,
    /// The gate the computation takes a slot from before it runs.
    pub gate: &'a ComputeGate,
}

/// What a `layout.structure` apply asked for, as sent. With the time limit the request stated it
/// is the replay key, so another `selection`, `scope` or `instance` is another request.
pub(crate) struct LayoutApply {
    pub expected_revision: u64,
    /// The selection the layout reads; it alone decides which atoms move.
    pub selection: SelectionDto,
    pub scope: Option<String>,
    pub instance: Option<String>,
    /// The history entry's selections, with ordinary transaction meaning.
    pub selections: TransactionSelections,
}

/// The edit a layout apply commits and the diagnostics its reply carries. The diagnostics are
/// recorded with the reply, so a replay returns them.
pub(crate) struct LayoutEdit {
    pub commands: Vec<Command>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Why a layout apply ended without an edit.
pub(crate) enum LayoutHalt<R> {
    /// A refusal or stop of the session layer: an unreadable selection, the request ID, the
    /// document, its hold or revision, the slot wait, a stop, or the refused edit.
    Failed(ApplicationError),
    /// A refusal of the layout itself, as the caller's complete reply.
    Refused(R),
}

impl<R> From<ApplicationError> for LayoutHalt<R> {
    fn from(error: ApplicationError) -> Self {
        Self::Failed(error)
    }
}

/// A computation stopped before it finished: `limit_ms` is the time limit that was in force.
/// Nothing was changed.
pub(crate) fn interruption(interrupted: Interrupted, limit_ms: u64) -> ApplicationError {
    match interrupted {
        Interrupted::TimeLimit => ApplicationError {
            limit_ms: Some(limit_ms),
            ..ApplicationError::new(
                ErrorCode::TimeLimit,
                format!(
                    "The computation did not finish within the time limit of {limit_ms} ms; nothing was changed."
                ),
            )
        },
        Interrupted::Cancelled => ApplicationError::new(
            ErrorCode::Cancelled,
            "The computation was cancelled before it finished; nothing was changed.",
        ),
    }
}

/// One loss entry for each stereo record a transaction cleared, in key order (centers, then
/// double bonds). The reason says why, judged from the document after the edit: the atom or
/// bond is gone, or it is still there and the bonded neighbors it was about changed.
fn cleared_stereo_notes(document: &Document, cleared: &[StereoKey]) -> Vec<Loss> {
    cleared
        .iter()
        .map(|key| {
            let (source_path, subject, why) = match key {
                StereoKey::Center(atom) => {
                    let kept = document
                        .position(document.first_instance(), *atom)
                        .is_some();
                    (
                        format!("atoms[{atom}]"),
                        format!("atom {atom}"),
                        if kept {
                            "its bonded neighbors changed"
                        } else {
                            "the atom was deleted"
                        },
                    )
                }
                StereoKey::DoubleBond(bond) => {
                    let kept = document.bond(*bond).is_some();
                    (
                        format!("bonds[{bond}]"),
                        format!("bond {bond}"),
                        if kept {
                            "the bonded neighbors of one of its ends changed"
                        } else {
                            "the bond was deleted"
                        },
                    )
                }
            };
            Loss {
                consent: LossConsent::NotRequired,
                feature: "stereo_configuration".into(),
                source_path,
                disposition: LossDisposition::Dropped,
                reason: format!(
                    "The stereo configuration of {subject} was cleared because {why}. Undoing \
                     this edit, while it is still in the history, brings it back."
                ),
                semantic_impact: SemanticImpact::Stereo,
            }
        })
        .collect()
}

/// One loss entry for each change of meaning a transaction made to a stereo group that existed
/// before it (the rule is in the domain's `stereo_group` module). The features are `stereo_group_member` (a member left
/// because it stopped being a stored tetrahedral center), `stereo_group` (a group disappeared)
/// and `stereo_group_relation` (the statement about which centers belong together changed). A
/// change of a group's number, adding members, and a group made and changed within the
/// transaction are not reported.
fn group_notes(document: &Document, notes: &[GroupNote]) -> Vec<Loss> {
    let atoms = |atoms: &[AtomId]| {
        atoms
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    };
    let note =
        |feature: &str, source_path: String, disposition: LossDisposition, reason: String| Loss {
            consent: LossConsent::NotRequired,
            feature: feature.into(),
            source_path,
            disposition,
            reason,
            semantic_impact: SemanticImpact::Stereo,
        };
    notes
        .iter()
        .map(|change| match change {
            GroupNote::MemberLeft {
                group,
                atom,
                cause,
                relation_lost,
            } => {
                let why = match cause {
                    LeaveCause::ConfigurationUnknown => "its stereo configuration is now unknown",
                    LeaveCause::ConfigurationCleared
                        if document
                            .position(document.first_instance(), *atom)
                            .is_none() =>
                    {
                        "the atom was deleted"
                    }
                    LeaveCause::ConfigurationCleared => "its stereo configuration was cleared",
                };
                let relation = if *relation_lost {
                    " The relation between it and the other members of the group, which were \
                     stated to belong together, is no longer stated."
                } else {
                    ""
                };
                note(
                    "stereo_group_member",
                    format!("atoms[{atom}]"),
                    LossDisposition::Dropped,
                    format!(
                        "Atom {atom} left stereo group {group} because {why}.{relation} Undoing \
                         this edit, while it is still in the history, brings it back."
                    ),
                )
            }
            GroupNote::GroupRemoved { group } => note(
                "stereo_group",
                format!("groups[{group}]"),
                LossDisposition::Dropped,
                format!(
                    "Stereo group {group} was removed because it has no members left. Undoing \
                     this edit, while it is still in the history, brings it back."
                ),
            ),
            GroupNote::RelationLost {
                group,
                together,
                ungrouped,
            } => {
                let all: Vec<AtomId> = together
                    .iter()
                    .flatten()
                    .chain(ungrouped)
                    .copied()
                    .collect();
                let mut now: Vec<String> = together
                    .iter()
                    .enumerate()
                    .map(|(index, part)| {
                        let place = if index == 0 {
                            "one group"
                        } else {
                            "another group"
                        };
                        format!("{} in {place}", atoms(part))
                    })
                    .collect();
                if !ungrouped.is_empty() {
                    now.push(format!("{} in no group", atoms(ungrouped)));
                }
                note(
                    "stereo_group_relation",
                    format!("groups[{group}]"),
                    LossDisposition::Dropped,
                    format!(
                        "Stereo group {group} no longer states that its remaining members {} belong \
                         together: now {}.",
                        atoms(&all),
                        now.join("; ")
                    ),
                )
            }
            GroupNote::KindChanged {
                group,
                atoms: moved,
                from,
                to,
            } => note(
                "stereo_group_relation",
                format!("groups[{group}]"),
                if *to == GroupKind::Absolute {
                    LossDisposition::Dropped
                } else {
                    LossDisposition::Normalized
                },
                format!(
                    "The remaining members {} of stereo group {group} were stated {} together and \
                     are now in a group of kind {}: what is stated about how they relate to \
                     each other changed.",
                    atoms(moved),
                    from.as_str(),
                    to.as_str()
                ),
            ),
            GroupNote::Merged {
                group,
                number,
                kind,
                sources,
                intact,
            } => {
                let former = sources
                    .iter()
                    .map(|(id, _, _)| id.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                let dropped: String = sources
                    .iter()
                    .filter(|(_, former_kind, former_number)| {
                        former_number != number || former_kind != kind
                    })
                    .filter_map(|(id, _, former_number)| {
                        former_number.map(|n| format!(" The number {n} of group {id} is not kept."))
                    })
                    .collect();
                let changed: String = sources
                    .iter()
                    .filter(|(_, former_kind, _)| former_kind != kind)
                    .map(|(id, former_kind, _)| {
                        format!(
                            " Group {id} was of kind {} and is now of kind {}.",
                            former_kind.as_str(),
                            kind.as_str()
                        )
                    })
                    .collect();
                let (disposition, together) = if *intact {
                    (
                        LossDisposition::Normalized,
                        "all their surviving members now belong together, a relation that was not \
                         stated before.",
                    )
                } else {
                    (
                        LossDisposition::Dropped,
                        "the surviving members that are in it now belong together, a relation \
                         that was not stated before; not all surviving members of the former \
                         groups stayed together.",
                    )
                };
                note(
                    "stereo_group_relation",
                    format!("groups[{group}]"),
                    disposition,
                    format!(
                        "Stereo groups {former} are now in group {group}: {together}{changed}{dropped}"
                    ),
                )
            }
        })
        .collect()
}

/// Whether this process has handed out an atom, bond, scene instance or stereo group
/// identifier that the stored commit's counters would issue again for different content.
fn has_issued_identifiers_beyond(reached: Numbering, stored: Numbering) -> bool {
    reached.next_atom > stored.next_atom
        || reached.next_bond > stored.next_bond
        || reached.next_object > stored.next_object
        || reached.next_group > stored.next_group
}

/// The notes of an applied edit, or its refusal: what a transaction, a settings change and an
/// import all answer, so that no path can clear a stated record and still be `ok`.
fn edit_notes(
    document: &Document,
    result: Result<Applied, TransactionError>,
) -> Result<Vec<Loss>, ApplicationError> {
    result
        .map(|applied| {
            let mut notes = cleared_stereo_notes(document, &applied.cleared);
            notes.extend(group_notes(document, &applied.group_notes));
            notes
        })
        .map_err(rejected_edit)
}

/// Like [`edit_notes`] for an aromaticity conversion. Its commands are generated, so an index
/// into them means nothing to the caller: a stereo refusal is of the conversion as a whole,
/// with the rule it breaks and no command index. Every other refusal keeps its index.
fn conversion_notes(
    document: &Document,
    result: Result<Applied, TransactionError>,
) -> Result<Vec<Loss>, ApplicationError> {
    edit_notes(document, result).map_err(|refused| {
        if refused.invalid_stereo.is_some() {
            ApplicationError {
                command_index: None,
                ..refused
            }
        } else {
            refused
        }
    })
}

fn rejected_edit(error: TransactionError) -> ApplicationError {
    let code = match error.error {
        EditError::RevisionLimit => ErrorCode::ResourceLimit,
        _ => ErrorCode::InvalidCommand,
    };
    let invalid_state = match error.error {
        EditError::InvalidState(state) => Some(state),
        _ => None,
    };
    let invalid_settings = match error.error {
        EditError::Settings(settings) => Some(settings),
        _ => None,
    };
    let (invalid_stereo, invalid_derivation) = match error.error {
        EditError::Stereo(stereo)
        | EditError::FromDrawing {
            cause: DeriveError::Structure(stereo),
            ..
        } => (Some(stereo), None),
        EditError::FromDrawing {
            cause: DeriveError::NotDerivable(reason),
            ..
        } => (None, Some(reason)),
        _ => (None, None),
    };
    ApplicationError {
        command_index: error.index,
        invalid_state,
        invalid_settings,
        invalid_stereo,
        invalid_derivation,
        invalid_stereo_group: match error.error {
            EditError::StereoGroup(group) => Some(group.code()),
            _ => None,
        },
        invalid_scene: error.error.scene_error(),
        invalid_ring: match error.error {
            EditError::RingPatternInvalid => Some(RingRefusal::Pattern),
            EditError::RingDirectionInvalid => Some(RingRefusal::Direction),
            _ => None,
        },
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

fn outdated_draft(current: u64, base: u64) -> ApplicationError {
    ApplicationError {
        current_revision: Some(current),
        ..ApplicationError::new(
            ErrorCode::DraftOutdated,
            format!(
                "the document was saved at revision {current} after this draft's base {base}; \
                 recover the draft as a copy"
            ),
        )
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

#[cfg(test)]
mod tests {
    use super::*;
    use chemifree_domain::document::{BondId, GroupId, Point};
    use chemifree_domain::states::StateError;
    use chemifree_domain::stereo::StereoError;

    fn drawing() -> Document {
        let id = DocumentId::parse("notes").expect("valid identifier");
        let mut document = Document::new(id, "Notes").expect("valid title");
        document
            .apply(&[Command::AddAtom {
                atom: chemifree_domain::document::Atom::new(
                    chemifree_domain::element::Element::from_symbol("C")
                        .expect("carbon")
                        .into(),
                ),
                positions: chemifree_domain::document::NewAtomPositions::Single(
                    Point::new(0.0, 0.0).unwrap(),
                ),
                bond_to: None,
            }])
            .expect("an atom");
        document
    }

    fn forged(cleared: Vec<StereoKey>) -> Result<Applied, TransactionError> {
        Ok(Applied {
            outcome: Outcome::Changed,
            cleared,
            group_notes: Vec::new(),
        })
    }

    fn refused(index: usize, error: EditError) -> Result<Applied, TransactionError> {
        Err(TransactionError {
            index: Some(index),
            error,
        })
    }

    #[test]
    fn a_conversion_that_cleared_a_record_reports_it_like_a_transaction() {
        // No conversion clears a record today (it only changes bond orders), so the list is
        // forced: the point is that the path maps it to notes, which make the reply `partial`.
        let document = drawing();
        let a1 = AtomId::from_serial(1).unwrap();
        let bond = BondId::from_serial(1).unwrap();
        let cleared = vec![StereoKey::Center(a1), StereoKey::DoubleBond(bond)];
        let via_transaction = edit_notes(&document, forged(cleared.clone())).unwrap();
        let via_conversion = conversion_notes(&document, forged(cleared)).unwrap();
        assert_eq!(via_transaction.len(), 2);
        assert_eq!(via_conversion, via_transaction);
        assert_eq!(via_conversion[0].source_path, "atoms[a1]");
        assert_eq!(via_conversion[1].source_path, "bonds[b1]");
        assert!(
            via_conversion[1].reason.contains("the bond was deleted"),
            "{}",
            via_conversion[1].reason
        );
        assert!(
            conversion_notes(&document, forged(Vec::new()))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn group_changes_of_meaning_are_notes_after_the_cleared_records() {
        let document = drawing();
        let (a1, a2) = (
            AtomId::from_serial(1).unwrap(),
            AtomId::from_serial(2).unwrap(),
        );
        let (e1, e2) = (
            GroupId::from_serial(1).unwrap(),
            GroupId::from_serial(2).unwrap(),
        );
        let applied = Applied {
            outcome: Outcome::Changed,
            cleared: vec![StereoKey::Center(a1)],
            group_notes: vec![
                GroupNote::MemberLeft {
                    group: e1,
                    atom: a1,
                    cause: LeaveCause::ConfigurationUnknown,
                    relation_lost: true,
                },
                GroupNote::GroupRemoved { group: e2 },
                GroupNote::RelationLost {
                    group: e1,
                    together: vec![vec![a1]],
                    ungrouped: vec![a2],
                },
                GroupNote::KindChanged {
                    group: e1,
                    atoms: vec![a1, a2],
                    from: GroupKind::Or,
                    to: GroupKind::And,
                },
                GroupNote::KindChanged {
                    group: e1,
                    atoms: vec![a1, a2],
                    from: GroupKind::And,
                    to: GroupKind::Absolute,
                },
                GroupNote::Merged {
                    group: e1,
                    number: Some(0),
                    kind: GroupKind::Or,
                    sources: vec![
                        (e1, GroupKind::Or, Some(0)),
                        (e2, GroupKind::Or, Some(0)),
                        (GroupId::from_serial(3).unwrap(), GroupKind::Or, Some(2)),
                    ],
                    intact: true,
                },
            ],
        };
        let notes = edit_notes(&document, Ok(applied)).unwrap();
        let summary: Vec<(&str, &str, LossDisposition)> = notes
            .iter()
            .map(|n| (n.feature.as_str(), n.source_path.as_str(), n.disposition))
            .collect();
        assert_eq!(
            summary,
            [
                (
                    "stereo_configuration",
                    "atoms[a1]",
                    LossDisposition::Dropped
                ),
                ("stereo_group_member", "atoms[a1]", LossDisposition::Dropped),
                ("stereo_group", "groups[e2]", LossDisposition::Dropped),
                (
                    "stereo_group_relation",
                    "groups[e1]",
                    LossDisposition::Dropped
                ),
                (
                    "stereo_group_relation",
                    "groups[e1]",
                    LossDisposition::Normalized
                ),
                (
                    "stereo_group_relation",
                    "groups[e1]",
                    LossDisposition::Dropped
                ),
                (
                    "stereo_group_relation",
                    "groups[e1]",
                    LossDisposition::Normalized
                ),
            ]
        );
        assert!(
            notes
                .iter()
                .all(|n| n.semantic_impact == SemanticImpact::Stereo
                    && n.consent == LossConsent::NotRequired)
        );
        assert!(
            notes[1].reason.contains("now unknown"),
            "{}",
            notes[1].reason
        );
        assert!(
            notes[1].reason.contains("no longer stated"),
            "{}",
            notes[1].reason
        );
        assert!(
            notes[6].reason.contains("number 2 of group e3 is not kept"),
            "{}",
            notes[6].reason
        );
        let text: String = notes.iter().map(|n| n.reason.as_str()).collect();
        assert!(!text.contains("1:1") && !text.contains(" ratio"), "{text}");
    }

    #[test]
    fn a_group_identifier_issued_since_the_stored_commit_makes_a_draft_outdated() {
        let stored = drawing().numbering();
        let same = |reached: Numbering| has_issued_identifiers_beyond(reached, stored);
        assert!(!same(stored));
        for (name, reached) in [
            (
                "atom",
                Numbering {
                    next_atom: stored.next_atom + 1,
                    ..stored
                },
            ),
            (
                "bond",
                Numbering {
                    next_bond: stored.next_bond + 1,
                    ..stored
                },
            ),
            (
                "object",
                Numbering {
                    next_object: stored.next_object + 1,
                    ..stored
                },
            ),
            (
                "group",
                Numbering {
                    next_group: stored.next_group + 1,
                    ..stored
                },
            ),
        ] {
            assert!(same(reached), "{name}");
        }
    }

    #[test]
    fn a_merge_with_a_member_that_left_speaks_of_the_surviving_members_only() {
        let document = drawing();
        let (a1, a2) = (
            AtomId::from_serial(1).unwrap(),
            AtomId::from_serial(2).unwrap(),
        );
        let (e1, e2) = (
            GroupId::from_serial(1).unwrap(),
            GroupId::from_serial(2).unwrap(),
        );
        let applied = Applied {
            outcome: Outcome::Changed,
            cleared: vec![StereoKey::Center(a2)],
            group_notes: vec![
                GroupNote::MemberLeft {
                    group: e2,
                    atom: a2,
                    cause: LeaveCause::ConfigurationCleared,
                    relation_lost: true,
                },
                GroupNote::Merged {
                    group: e1,
                    number: Some(1),
                    kind: GroupKind::Or,
                    sources: vec![(e1, GroupKind::Or, Some(1)), (e2, GroupKind::Or, Some(2))],
                    intact: true,
                },
                GroupNote::RelationLost {
                    group: e1,
                    together: vec![vec![a1]],
                    ungrouped: vec![],
                },
                GroupNote::KindChanged {
                    group: e1,
                    atoms: vec![a1],
                    from: GroupKind::Or,
                    to: GroupKind::And,
                },
            ],
        };
        let notes = edit_notes(&document, Ok(applied)).unwrap();
        let merge = notes
            .iter()
            .find(|n| n.reason.contains("are now in group"))
            .unwrap();
        assert!(
            merge.reason.contains("all their surviving members"),
            "{}",
            merge.reason
        );
        assert!(
            !merge.reason.contains("all their members"),
            "{}",
            merge.reason
        );
        let text: Vec<&str> = notes.iter().map(|n| n.reason.as_str()).collect();
        assert!(
            text.iter().any(|t| t.contains("its remaining members")),
            "{text:?}"
        );
        assert!(
            text.iter().any(|t| t.contains("The remaining members")),
            "{text:?}"
        );
    }

    #[test]
    fn a_merge_that_also_changes_the_kind_says_so_in_the_same_note() {
        let document = drawing();
        let (e1, e2) = (
            GroupId::from_serial(1).unwrap(),
            GroupId::from_serial(2).unwrap(),
        );
        let merged = GroupNote::Merged {
            group: e1,
            number: Some(1),
            kind: GroupKind::And,
            sources: vec![(e1, GroupKind::Or, Some(1)), (e2, GroupKind::And, Some(1))],
            intact: true,
        };
        let notes = group_notes(&document, &[merged]);
        assert_eq!(notes.len(), 1);
        let reason = &notes[0].reason;
        assert!(
            reason.contains("Group e1 was of kind or and is now of kind and"),
            "{reason}"
        );
        assert!(!reason.contains("Group e2 was of kind"), "{reason}");
        assert!(
            reason.contains("number 1 of group e1 is not kept"),
            "{reason}"
        );
        assert!(!reason.contains("of group e2"), "{reason}");
    }

    #[test]
    fn a_merge_names_a_dropped_number_only_when_the_interpreted_number_differs() {
        let document = drawing();
        let (e1, e2) = (
            GroupId::from_serial(1).unwrap(),
            GroupId::from_serial(2).unwrap(),
        );
        let reason = |number: Option<i32>, former: Option<i32>, intact: bool| {
            let merged = GroupNote::Merged {
                group: e1,
                number,
                kind: GroupKind::Or,
                sources: vec![(e1, GroupKind::Or, number), (e2, GroupKind::Or, former)],
                intact,
            };
            let notes = group_notes(&document, &[merged]);
            (notes[0].disposition, notes[0].reason.clone())
        };
        // A group without a stored number reads as 0 (the same as a stored 0): nothing is lost.
        let (_, same) = reason(Some(0), Some(0), true);
        assert!(!same.contains("not kept"), "{same}");
        // A different interpreted number is dropped, including the 0 of an unnumbered group.
        let (_, other) = reason(Some(3), Some(0), true);
        assert!(
            other.contains("number 0 of group e2 is not kept"),
            "{other}"
        );
        let (_, positive) = reason(Some(0), Some(4), true);
        assert!(
            positive.contains("number 4 of group e2 is not kept"),
            "{positive}"
        );
        // The disposition says whether everything stayed together.
        assert_eq!(
            reason(Some(0), Some(4), true).0,
            LossDisposition::Normalized
        );
        let (disposition, apart) = reason(Some(0), Some(4), false);
        assert_eq!(disposition, LossDisposition::Dropped);
        assert!(apart.contains("not all surviving members"), "{apart}");
    }

    #[test]
    fn only_a_stereo_refusal_of_a_conversion_loses_its_command_index() {
        let document = drawing();
        let stereo = conversion_notes(
            &document,
            refused(3, EditError::Stereo(StereoError::MarkBondOrder)),
        )
        .unwrap_err();
        assert_eq!(stereo.command_index, None);
        assert_eq!(stereo.reason_code(), Some("stereo_mark_bond_order"));
        let other = conversion_notes(
            &document,
            refused(3, EditError::InvalidState(StateError::QueryAtomCharge)),
        )
        .unwrap_err();
        assert_eq!(
            other.command_index,
            Some(3),
            "as before the stereo commands"
        );
        // A transaction keeps the index of a stereo refusal.
        let transaction = edit_notes(
            &document,
            refused(3, EditError::Stereo(StereoError::MarkBondOrder)),
        )
        .unwrap_err();
        assert_eq!(transaction.command_index, Some(3));
    }

    #[test]
    fn rejected_ring_edits_keep_the_domain_reason_in_the_v1_envelope() {
        for (cause, reason) in [
            (EditError::RingPatternInvalid, "ring_pattern_invalid"),
            (EditError::RingDirectionInvalid, "ring_direction_invalid"),
        ] {
            assert_eq!(cause.reason(), Some(reason));
            let error = rejected_edit(TransactionError {
                index: Some(1),
                error: cause,
            });
            assert_eq!(error.code, ErrorCode::InvalidCommand);
            assert_eq!(error.reason_code(), Some(reason));
            let envelope = serde_json::to_value(error.to_response()).unwrap();
            assert_eq!(envelope["code"], "invalid_command");
            assert_eq!(envelope["command_index"], 1);
            assert_eq!(envelope["reason"], reason);
        }
    }

    #[test]
    fn aromaticity_refusals_that_are_not_model_limits_name_their_cause() {
        let error = aromaticity_refusal("invalid_graph");
        assert_eq!(error.code, ErrorCode::InvalidCommand);
        assert_eq!(error.reason_code(), Some("invalid_graph"));
        assert!(
            error.message.contains("internal") && error.message.contains("nothing was changed"),
            "the message names an internal defect and that nothing changed: {}",
            error.message
        );
        // The command bound names its cause and the shared limit.
        let limit = aromaticity_refusal("conversion_command_limit");
        assert_eq!(limit.reason_code(), Some("conversion_command_limit"));
        assert_eq!(
            limit.message,
            format!(
                "Aromaticity conversion needs more than {MAX_COMMANDS} commands and a transaction accepts at most {MAX_COMMANDS}; nothing was changed."
            )
        );
        // Every other reason keeps the model message.
        assert!(
            aromaticity_refusal("adjacent_donors")
                .message
                .contains("selected model")
        );
    }
}
