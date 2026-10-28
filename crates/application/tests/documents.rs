use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use chemifree_application::{
    Application, CapabilityCatalog, CommandDto, DocumentRepository, DocumentResponse, Draft,
    DraftInfo, DraftStatus, ErrorCode, ListQuery, ListSort, MAX_OPEN_SESSIONS, RecoveryState,
    StorageError, StorageReason, TURN_WAIT, TrashEntry,
};
use chemifree_domain::Capability;
use chemifree_domain::document::{Document, DocumentId, MAX_EDIT_REVISION, MAX_REVISION};
use serde_json::json;

struct NoCapabilities;

impl CapabilityCatalog for NoCapabilities {
    fn capabilities(&self) -> Vec<Capability> {
        Vec::new()
    }
}

/// In-memory store with the port's conflict and draft contracts and a switchable write
/// failure. Clones share storage; `as_process` gives a clone that owns drafts as another
/// process would, and `end_process` abandons a process's drafts as its exit would.
/// A draft with its owning process; `None` once that process ended.
type OwnedDraft = (Draft, Option<u32>);

#[derive(Clone, Default)]
struct MemoryStore {
    documents: Arc<Mutex<BTreeMap<DocumentId, Document>>>,
    drafts: Arc<Mutex<BTreeMap<DocumentId, OwnedDraft>>>,
    next: Arc<AtomicU32>,
    fail_writes: Arc<AtomicBool>,
    /// When set, writes fail with this error instead.
    fail_with: Arc<Mutex<Option<StorageError>>>,
    /// Held by a test to stall commits, as another process's save lock would.
    commit_gate: Arc<Mutex<()>>,
    /// Saves, trash moves, and restores that reached storage, counted before they wait for
    /// `commit_gate`.
    saves_started: Arc<AtomicUsize>,
    /// Trashed documents with when they were trashed.
    trash: Arc<Mutex<BTreeMap<DocumentId, (Document, u64)>>>,
    /// Commit times on a fake clock that advances by one per commit or delete.
    saved_at: Arc<Mutex<BTreeMap<DocumentId, u64>>>,
    clock: Arc<AtomicU64>,
    process: u32,
}

impl MemoryStore {
    fn as_process(&self, process: u32) -> Self {
        Self {
            process,
            ..self.clone()
        }
    }

    fn end_process(&self) {
        for (_, owner) in self.drafts.lock().unwrap().values_mut() {
            if *owner == Some(self.process) {
                *owner = None;
            }
        }
    }

    fn draft_count(&self) -> usize {
        self.drafts.lock().unwrap().len()
    }

    fn failing(&self) -> Result<(), StorageError> {
        if let Some(error) = self.fail_with.lock().unwrap().clone() {
            return Err(error);
        }
        if self.fail_writes.load(Ordering::SeqCst) {
            return Err(StorageError::Unavailable("disk unavailable".into()));
        }
        Ok(())
    }
}

impl DocumentRepository for MemoryStore {
    fn allocate_id(&self) -> Result<DocumentId, StorageError> {
        let serial = self.next.fetch_add(1, Ordering::SeqCst);
        Ok(DocumentId::parse(&format!("doc-{serial}")).unwrap())
    }

    fn list_ids(&self) -> Result<Vec<DocumentId>, StorageError> {
        Ok(self.documents.lock().unwrap().keys().cloned().collect())
    }

    fn load(&self, id: &DocumentId) -> Result<Option<Document>, StorageError> {
        Ok(self.documents.lock().unwrap().get(id).cloned())
    }

    fn save(&self, document: &Document, base: Option<u64>) -> Result<(), StorageError> {
        self.saves_started.fetch_add(1, Ordering::SeqCst);
        let _commit = self.commit_gate.lock().unwrap();
        self.failing()?;
        let mut documents = self.documents.lock().unwrap();
        let stored = documents.get(document.id());
        let matches = match (stored, base) {
            (None, None) => true,
            (Some(stored), Some(base)) => stored.revision() == base || stored == document,
            _ => false,
        };
        if !matches {
            return Err(StorageError::Conflict {
                stored_revision: stored.map(Document::revision),
            });
        }
        documents.insert(document.id().clone(), document.clone());
        let at = self.clock.fetch_add(1, Ordering::SeqCst) + 1;
        self.saved_at
            .lock()
            .unwrap()
            .insert(document.id().clone(), at);
        let mut drafts = self.drafts.lock().unwrap();
        if drafts
            .get(document.id())
            .is_some_and(|(_, owner)| *owner == Some(self.process))
        {
            drafts.remove(document.id());
        }
        Ok(())
    }

    fn draft_status(&self, id: &DocumentId) -> Result<DraftStatus, StorageError> {
        Ok(match self.drafts.lock().unwrap().get(id) {
            None => DraftStatus::None,
            Some((_, Some(owner))) if *owner == self.process => DraftStatus::Owned,
            Some((_, Some(_))) => DraftStatus::Locked,
            Some((draft, None)) => DraftStatus::Abandoned(Ok(DraftInfo {
                revision: draft.document.revision(),
                base_revision: draft.base_revision,
                saved_at_ms: draft.saved_at_ms,
            })),
        })
    }

    fn write_draft(&self, draft: &Draft) -> Result<(), StorageError> {
        let mut drafts = self.drafts.lock().unwrap();
        match drafts.get(draft.document.id()) {
            Some((_, Some(owner))) if *owner != self.process => {
                return Err(StorageError::Locked("owned by another process".into()));
            }
            Some((_, None)) => {
                return Err(StorageError::Conflict {
                    stored_revision: None,
                });
            }
            Some(_) => {}
            None if !self
                .documents
                .lock()
                .unwrap()
                .contains_key(draft.document.id()) =>
            {
                return Err(StorageError::Missing);
            }
            None => {}
        }
        self.failing()?;
        drafts.insert(
            draft.document.id().clone(),
            (draft.clone(), Some(self.process)),
        );
        Ok(())
    }

    fn claim_draft(&self, id: &DocumentId) -> Result<Option<Result<Draft, String>>, StorageError> {
        let mut drafts = self.drafts.lock().unwrap();
        match drafts.get_mut(id) {
            None => Ok(None),
            Some((_, Some(owner))) if *owner != self.process => {
                Err(StorageError::Locked("owned by another process".into()))
            }
            Some((draft, owner)) => {
                *owner = Some(self.process);
                Ok(Some(Ok(draft.clone())))
            }
        }
    }

    fn release_draft(&self, id: &DocumentId) {
        if let Some((_, owner)) = self.drafts.lock().unwrap().get_mut(id)
            && *owner == Some(self.process)
        {
            *owner = None;
        }
    }

    fn remove_draft(&self, id: &DocumentId) -> Result<(), StorageError> {
        let mut drafts = self.drafts.lock().unwrap();
        if drafts
            .get(id)
            .is_some_and(|(_, owner)| *owner == Some(self.process))
        {
            self.failing()?;
            drafts.remove(id);
        }
        Ok(())
    }

    fn saved_at_ms(&self, id: &DocumentId) -> Result<Option<u64>, StorageError> {
        Ok(self.saved_at.lock().unwrap().get(id).copied())
    }

    fn trash(&self, id: &DocumentId, base_revision: u64) -> Result<(), StorageError> {
        self.saves_started.fetch_add(1, Ordering::SeqCst);
        let _commit = self.commit_gate.lock().unwrap();
        match self.drafts.lock().unwrap().get(id) {
            Some((_, Some(owner))) if *owner != self.process => {
                return Err(StorageError::Locked("owned by another process".into()));
            }
            Some(_) => return Err(StorageError::DraftPending),
            None => {}
        }
        self.failing()?;
        let mut documents = self.documents.lock().unwrap();
        let stored = documents.get(id).ok_or(StorageError::Missing)?;
        if stored.revision() != base_revision {
            return Err(StorageError::Conflict {
                stored_revision: Some(stored.revision()),
            });
        }
        let document = documents.remove(id).expect("checked above");
        let at = self.clock.fetch_add(1, Ordering::SeqCst) + 1;
        self.trash
            .lock()
            .unwrap()
            .insert(id.clone(), (document, at));
        Ok(())
    }

    fn restore(&self, id: &DocumentId) -> Result<(), StorageError> {
        self.saves_started.fetch_add(1, Ordering::SeqCst);
        let _commit = self.commit_gate.lock().unwrap();
        self.failing()?;
        let mut documents = self.documents.lock().unwrap();
        if let Some(stored) = documents.get(id) {
            return Err(StorageError::Conflict {
                stored_revision: Some(stored.revision()),
            });
        }
        let (document, _) = self
            .trash
            .lock()
            .unwrap()
            .remove(id)
            .ok_or(StorageError::Missing)?;
        documents.insert(id.clone(), document);
        Ok(())
    }

    fn list_trash(&self) -> Result<Vec<TrashEntry>, StorageError> {
        Ok(self
            .trash
            .lock()
            .unwrap()
            .iter()
            .map(|(id, (_, at))| TrashEntry {
                id: id.clone(),
                deleted_at_ms: Some(*at),
            })
            .collect())
    }

    fn load_trashed(&self, id: &DocumentId) -> Result<Option<Document>, StorageError> {
        Ok(self
            .trash
            .lock()
            .unwrap()
            .get(id)
            .map(|(document, _)| document.clone()))
    }
}

fn application() -> (Application<NoCapabilities, MemoryStore>, MemoryStore) {
    let store = MemoryStore::default();
    (Application::new(NoCapabilities, store.clone()), store)
}

fn commands(value: serde_json::Value) -> Vec<CommandDto> {
    serde_json::from_value(value).unwrap()
}

/// Bonds by in-transaction references, as a client drawing on an empty canvas must.
fn draw_ethanol() -> Vec<CommandDto> {
    commands(json!([
        {"op": "add_atom", "element": "C", "position": {"x": 0, "y": 0}},
        {"op": "add_atom", "element": "C", "position": {"x": 26, "y": 15},
         "bond_to": {"atom": "$0", "order": "single"}},
        {"op": "add_atom", "element": "O", "position": {"x": 52, "y": 0}},
        {"op": "add_bond", "begin": "$1", "end": "$2", "order": "single"}
    ]))
}

fn json_of(response: &DocumentResponse) -> serde_json::Value {
    serde_json::to_value(response).unwrap()
}

#[test]
fn create_edit_undo_redo_save_and_reopen() {
    let (app, store) = application();
    let created = app.create_document(Some("c1"), Some("Ethanol")).unwrap();
    let id = created.document.id.clone();
    assert_eq!(created.document.revision, 0);
    assert!(
        !created.session.unsaved_changes,
        "creation commits before acknowledging"
    );
    assert_eq!(store.documents.lock().unwrap().len(), 1);

    let edited = app
        .apply_transaction(&id, Some("t1"), 0, draw_ethanol())
        .unwrap();
    assert_eq!(edited.document.revision, 1);
    assert_eq!(edited.document.atoms.len(), 3);
    assert_eq!(edited.document.bonds[1].begin, "a2");
    assert!(edited.session.unsaved_changes);

    let changed = app
        .apply_transaction(
            &id,
            Some("t2"),
            1,
            commands(json!([
                {"op": "update_atom", "atom": "a3", "element": "N", "charge": 1, "isotope": 15},
                {"op": "update_bond", "bond": "b2", "order": "double"}
            ])),
        )
        .unwrap();
    assert_eq!(changed.document.revision, 2);
    let undone = app.undo(&id, Some("u1"), 2).unwrap();
    assert_eq!(undone.document.revision, 3);
    assert_eq!(undone.document.atoms, edited.document.atoms);
    assert_eq!(
        (undone.session.undo_depth, undone.session.redo_depth),
        (1, 1)
    );
    let redone = app.redo(&id, Some("r1"), 3).unwrap();
    assert_eq!(redone.document.atoms, changed.document.atoms);
    assert_eq!(redone.document.atoms[2].element, "N");
    assert_eq!(redone.document.atoms[2].isotope, Some(15));

    let saved = app.save(&id, Some("s1"), 4).unwrap();
    assert_eq!(saved.session.saved_revision, 4);
    assert!(!saved.session.unsaved_changes);

    let closed = app.close(&id, Some("x1"), false, None).unwrap();
    assert!(!closed.discarded_changes);

    // A new process sees only committed storage.
    let reopened_app = Application::new(NoCapabilities, store.clone());
    let reopened = reopened_app.document(&id).unwrap();
    assert_eq!(reopened.document, saved.document);
    assert_eq!(
        reopened.session, saved.session,
        "history survives reopening"
    );
    let undone_after_reopen = reopened_app.undo(&id, None, 4).unwrap();
    assert_eq!(undone_after_reopen.document.atoms, edited.document.atoms);
}

#[test]
fn stale_revisions_are_rejected_without_changing_state() {
    let (app, _) = application();
    let id = app.create_document(None, None).unwrap().document.id;
    app.apply_transaction(&id, None, 0, draw_ethanol()).unwrap();
    let before = app.document(&id).unwrap();

    for result in [
        app.apply_transaction(&id, None, 0, draw_ethanol()),
        app.undo(&id, None, 0),
        app.redo(&id, None, 7),
        app.save(&id, None, 0),
    ] {
        let error = result.unwrap_err();
        assert_eq!(error.code, ErrorCode::RevisionConflict);
        assert_eq!(error.current_revision, Some(1));
    }
    assert_eq!(app.document(&id).unwrap(), before);
}

#[test]
fn invalid_commands_report_their_index_and_change_nothing() {
    let (app, _) = application();
    let id = app.create_document(None, None).unwrap().document.id;
    app.apply_transaction(&id, None, 0, draw_ethanol()).unwrap();
    let before = app.document(&id).unwrap();
    for (bad, index) in [
        (
            json!({"op": "add_atom", "element": "Xx", "position": {"x": 0, "y": 0}}),
            1,
        ),
        (
            json!({"op": "add_bond", "begin": "a1", "end": "a9", "order": "single"}),
            1,
        ),
        (
            json!({"op": "add_bond", "begin": "a1", "end": "a2", "order": "single"}),
            1,
        ),
        (json!({"op": "update_atom", "atom": "a1", "charge": 300}), 1),
        (
            json!({"op": "update_atom", "atom": "a1", "isotope": 70000}),
            1,
        ),
        (json!({"op": "delete_bonds", "bonds": ["a1"]}), 1),
        (
            json!({"op": "move_atoms", "atoms": ["a9"], "delta": {"x": 1, "y": 1}}),
            1,
        ),
    ] {
        let batch = commands(json!([
            {"op": "add_atom", "element": "C", "position": {"x": 1, "y": 1}},
            bad
        ]));
        let error = app.apply_transaction(&id, None, 1, batch).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidCommand, "{error}");
        assert_eq!(error.command_index, Some(index), "{error}");
    }
    assert_eq!(app.document(&id).unwrap(), before);
}

#[test]
fn request_ids_replay_completed_mutations_once() {
    let (app, _) = application();
    let first = app.create_document(Some("create-1"), Some("A")).unwrap();
    let replayed = app.create_document(Some("create-1"), Some("A")).unwrap();
    assert_eq!(
        first, replayed,
        "a retried create returns the same document"
    );
    let reused = app
        .create_document(Some("create-1"), Some("B"))
        .unwrap_err();
    assert_eq!(reused.code, ErrorCode::RequestIdReused);

    let id = first.document.id;
    let applied = app
        .apply_transaction(&id, Some("edit-1"), 0, draw_ethanol())
        .unwrap();
    let retried = app
        .apply_transaction(&id, Some("edit-1"), 0, draw_ethanol())
        .unwrap();
    assert_eq!(json_of(&applied), json_of(&retried));
    assert_eq!(
        app.document(&id).unwrap().document.atoms.len(),
        3,
        "applied once"
    );

    let different = app.undo(&id, Some("edit-1"), 1).unwrap_err();
    assert_eq!(different.code, ErrorCode::RequestIdReused);

    let invalid = app.undo(&id, Some("has space"), 1).unwrap_err();
    assert_eq!(invalid.code, ErrorCode::InvalidRequest);
}

#[test]
fn a_retried_close_replays_instead_of_closing_a_newer_session() {
    let (app, _) = application();
    let id = app.create_document(None, None).unwrap().document.id;
    app.apply_transaction(&id, None, 0, draw_ethanol()).unwrap();
    let closed = app.close(&id, Some("close-1"), true, Some(1)).unwrap();
    assert!(closed.discarded_changes);

    // Another client reopens the document and edits it before the retry arrives.
    let reopened = app.document(&id).unwrap().document.revision;
    app.apply_transaction(
        &id,
        None,
        reopened,
        commands(json!([{"op": "rename", "title": "Newer work"}])),
    )
    .unwrap();
    let retried = app.close(&id, Some("close-1"), true, Some(1)).unwrap();
    assert_eq!(retried, closed, "a retry returns the original response");
    let session = app.document(&id).unwrap();
    assert_eq!(
        session.document.title, "Newer work",
        "the newer session survives"
    );
    assert!(session.session.unsaved_changes);

    let reused = app.close(&id, Some("close-1"), false, None).unwrap_err();
    assert_eq!(reused.code, ErrorCode::RequestIdReused);
}

#[test]
fn a_stale_discard_cannot_drop_newer_edits() {
    let (app, _) = application();
    let id = app.create_document(None, None).unwrap().document.id;
    // Client A confirms discarding revision 1 while client B has moved on to revision 2.
    app.apply_transaction(&id, None, 0, draw_ethanol()).unwrap();
    app.apply_transaction(
        &id,
        None,
        1,
        commands(json!([{"op": "rename", "title": "B's work"}])),
    )
    .unwrap();

    let unconfirmed = app.close(&id, None, true, None).unwrap_err();
    assert_eq!(unconfirmed.code, ErrorCode::InvalidRequest);
    let stale = app.close(&id, None, true, Some(1)).unwrap_err();
    assert_eq!(stale.code, ErrorCode::RevisionConflict);
    assert_eq!(stale.current_revision, Some(2));
    assert_eq!(app.document(&id).unwrap().document.title, "B's work");

    assert!(
        app.close(&id, None, true, Some(2))
            .unwrap()
            .discarded_changes
    );
}

#[test]
fn a_failed_save_keeps_unsaved_work_and_a_retry_commits_it() {
    let (app, store) = application();
    let id = app.create_document(None, None).unwrap().document.id;
    app.apply_transaction(&id, None, 0, draw_ethanol()).unwrap();

    store.fail_writes.store(true, Ordering::SeqCst);
    let error = app.save(&id, Some("save-1"), 1).unwrap_err();
    assert_eq!(error.code, ErrorCode::StorageFailed);
    let session = app.document(&id).unwrap();
    assert!(session.session.unsaved_changes);
    assert_eq!(session.session.saved_revision, 0);
    let stored = store.documents.lock().unwrap();
    assert_eq!(stored.values().next().unwrap().revision(), 0);
    drop(stored);

    store.fail_writes.store(false, Ordering::SeqCst);
    let saved = app.save(&id, Some("save-1"), 1).unwrap();
    assert!(
        !saved.session.unsaved_changes,
        "failed requests are not replayed"
    );
    let close = app.close(&id, None, false, None).unwrap();
    assert!(!close.discarded_changes);
}

#[test]
fn discarding_never_hands_out_a_revision_or_identifier_again() {
    let (app, _) = application();
    let id = app.create_document(None, None).unwrap().document.id;
    let drawn = app.apply_transaction(&id, None, 0, draw_ethanol()).unwrap();
    assert_eq!(drawn.document.revision, 1);
    assert!(
        app.close(&id, None, true, Some(1))
            .unwrap()
            .discarded_changes
    );

    let reopened = app.document(&id).unwrap();
    assert!(reopened.document.atoms.is_empty());
    assert!(
        reopened.document.revision > 1,
        "revision 1 already named the discarded drawing"
    );
    assert!(!reopened.session.unsaved_changes);

    // Closing and reopening the unchanged copy names the same commit the same way.
    assert!(!app.close(&id, None, false, None).unwrap().discarded_changes);
    assert_eq!(
        app.document(&id).unwrap().document.revision,
        reopened.document.revision
    );

    // A client still showing the discarded revision is refused rather than applied.
    let stale = app
        .apply_transaction(
            &id,
            None,
            1,
            commands(json!([{"op": "rename", "title": "Stale"}])),
        )
        .unwrap_err();
    assert_eq!(stale.code, ErrorCode::RevisionConflict);

    let redrawn = app
        .apply_transaction(&id, None, reopened.document.revision, draw_ethanol())
        .unwrap();
    let ids: Vec<_> = redrawn
        .document
        .atoms
        .iter()
        .map(|atom| atom.id.as_str())
        .collect();
    assert_eq!(ids, ["a4", "a5", "a6"], "a1..a3 named the discarded atoms");
    let saved = app.save(&id, None, redrawn.document.revision).unwrap();
    assert!(!saved.session.unsaved_changes);
    assert_eq!(saved.session.saved_revision, redrawn.document.revision);
}

#[test]
fn the_last_revision_is_kept_for_a_reopened_copy_and_never_named_twice() {
    let (app, store) = application();
    let id = app.create_document(None, None).unwrap().document.id;
    app.close(&id, None, false, None).unwrap();
    let key = DocumentId::parse(&id).unwrap();
    let near_limit = MAX_EDIT_REVISION - 1;
    {
        let mut documents = store.documents.lock().unwrap();
        let mut parts = documents[&key].to_parts();
        parts.revision = near_limit;
        documents.insert(key.clone(), Document::restore(parts).unwrap());
    }

    let drawn = app
        .apply_transaction(&id, None, near_limit, draw_ethanol())
        .unwrap();
    assert_eq!(drawn.document.revision, MAX_EDIT_REVISION);
    let refused = app
        .apply_transaction(
            &id,
            None,
            MAX_EDIT_REVISION,
            commands(json!([{"op": "rename", "title": "More"}])),
        )
        .unwrap_err();
    assert_eq!(refused.code, ErrorCode::ResourceLimit);
    app.close(&id, None, true, Some(MAX_EDIT_REVISION)).unwrap();

    let reopened = app.document(&id).unwrap();
    assert_eq!(reopened.document.revision, MAX_REVISION);
    assert!(reopened.document.atoms.is_empty());

    // Another writer commits different content; no revision is left to name it here.
    let other = Application::new(NoCapabilities, store.clone());
    other
        .apply_transaction(&id, None, near_limit, draw_ethanol())
        .unwrap();
    other.save(&id, None, MAX_EDIT_REVISION).unwrap();

    let exhausted = app.document(&id).unwrap_err();
    assert_eq!(exhausted.code, ErrorCode::ResourceLimit);
    let listed = app.list_documents(&ListQuery::default()).unwrap();
    assert!(listed.documents.is_empty(), "{listed:?}");
    assert_eq!(listed.unreadable.len(), 1);
    assert_eq!(
        app.document(&id).unwrap_err().code,
        ErrorCode::ResourceLimit
    );
}

#[test]
fn a_clean_session_follows_another_writers_save() {
    let (viewer, store) = application();
    let id = viewer.create_document(None, None).unwrap().document.id;

    let other = Application::new(NoCapabilities, store.clone());
    other
        .apply_transaction(&id, None, 0, draw_ethanol())
        .unwrap();
    other.save(&id, None, 1).unwrap();

    let seen = viewer.document(&id).unwrap();
    assert_eq!(seen.document.revision, 1, "the clean copy reloads");
    assert_eq!(seen.session.saved_revision, 1);
    assert!(!seen.session.unsaved_changes);
    let listed = viewer.list_documents(&ListQuery::default()).unwrap();
    assert_eq!(listed.documents[0].revision, 1);

    // An edit based on the old view is a revision conflict, not a later save conflict.
    let stale = viewer
        .apply_transaction(
            &id,
            None,
            0,
            commands(json!([{"op": "rename", "title": "Stale"}])),
        )
        .unwrap_err();
    assert_eq!(stale.code, ErrorCode::RevisionConflict);
    assert_eq!(stale.current_revision, Some(1));
}

#[test]
fn concurrent_writers_produce_a_save_conflict_instead_of_lost_updates() {
    let (editor, store) = application();
    let id = editor.create_document(None, None).unwrap().document.id;
    editor
        .apply_transaction(&id, None, 0, draw_ethanol())
        .unwrap();

    let other = Application::new(NoCapabilities, store.clone());
    other
        .apply_transaction(
            &id,
            None,
            0,
            commands(json!([{"op": "rename", "title": "Other writer"}])),
        )
        .unwrap();
    other.save(&id, None, 1).unwrap();

    let error = editor.save(&id, None, 1).unwrap_err();
    assert_eq!(error.code, ErrorCode::SaveConflict);
    assert!(editor.document(&id).unwrap().session.unsaved_changes);
    let refused = editor.close(&id, None, false, None).unwrap_err();
    assert_eq!(refused.code, ErrorCode::UnsavedChanges);
    assert!(
        editor
            .close(&id, None, true, Some(1))
            .unwrap()
            .discarded_changes
    );
    assert_eq!(editor.document(&id).unwrap().document.title, "Other writer");
}

#[test]
fn rejected_requests_do_not_evict_open_sessions() {
    let (app, _) = application();
    for _ in 0..MAX_OPEN_SESSIONS {
        app.create_document(None, None).unwrap();
    }
    let open = |app: &Application<NoCapabilities, MemoryStore>| {
        let listed = app.list_documents(&ListQuery::default()).unwrap();
        listed
            .documents
            .iter()
            .filter(|document| document.open)
            .count()
    };
    assert_eq!(open(&app), MAX_OPEN_SESSIONS);

    let missing = app.document("doc-missing").unwrap_err();
    assert_eq!(missing.code, ErrorCode::DocumentNotFound);
    let invalid = app.create_document(None, Some(" ")).unwrap_err();
    assert_eq!(invalid.code, ErrorCode::InvalidRequest);
    assert_eq!(open(&app), MAX_OPEN_SESSIONS, "nothing was evicted");
}

#[test]
fn missing_and_malformed_documents_are_not_found() {
    let (app, _) = application();
    for id in ["doc-404", "../etc/passwd", ""] {
        assert_eq!(
            app.document(id).unwrap_err().code,
            ErrorCode::DocumentNotFound
        );
        assert_eq!(
            app.close(id, None, false, None).unwrap_err().code,
            ErrorCode::DocumentNotFound
        );
    }
    let error = app.create_document(None, Some("\u{7}")).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidRequest);
}

#[test]
fn listing_merges_storage_with_open_sessions() {
    let (app, _) = application();
    let b = app.create_document(None, Some("Beta")).unwrap().document.id;
    app.create_document(None, Some("Alpha")).unwrap();
    app.apply_transaction(&b, None, 0, draw_ethanol()).unwrap();
    let list = app.list_documents(&ListQuery::default()).unwrap();
    let titles: Vec<_> = list.documents.iter().map(|d| d.title.as_str()).collect();
    assert_eq!(titles, ["Alpha", "Beta"]);
    assert_eq!(list.documents[1].atom_count, 3);
    assert!(list.documents[1].unsaved_changes);
    assert!(list.unreadable.is_empty());
}

#[test]
fn unknown_command_fields_are_rejected_at_the_contract_boundary() {
    for value in [
        json!([{"op": "add_atom", "element": "C", "position": {"x": 0, "y": 0}, "extra": 1}]),
        json!([{"op": "teleport"}]),
        json!([{"op": "add_bond", "begin": "a1", "end": "a2", "order": "quadruple"}]),
        json!([{"op": "move_atoms", "atoms": ["a1"], "delta": {"x": 0, "y": 0, "z": 1}}]),
    ] {
        assert!(
            serde_json::from_value::<Vec<CommandDto>>(value.clone()).is_err(),
            "{value}"
        );
    }
    let update: Vec<CommandDto> =
        commands(json!([{"op": "update_atom", "atom": "a1", "isotope": null}]));
    assert!(matches!(
        &update[0],
        CommandDto::UpdateAtom {
            isotope: Some(None),
            ..
        }
    ));
    let absent: Vec<CommandDto> = commands(json!([{"op": "update_atom", "atom": "a1"}]));
    assert!(matches!(
        &absent[0],
        CommandDto::UpdateAtom { isotope: None, .. }
    ));
}

/// An application in process 1 with an ethanol drawing at revision 3 (drawn, renamed, the
/// rename undone), unsaved and autosaved.
fn autosaved_drawing() -> (MemoryStore, String, DocumentResponse) {
    let store = MemoryStore::default().as_process(1);
    let app = Application::new(NoCapabilities, store.clone());
    let id = app
        .create_document(None, Some("Draft"))
        .unwrap()
        .document
        .id;
    app.apply_transaction(&id, None, 0, draw_ethanol()).unwrap();
    app.apply_transaction(
        &id,
        None,
        1,
        commands(json!([{"op": "rename", "title": "Renamed"}])),
    )
    .unwrap();
    app.undo(&id, None, 2).unwrap();
    let autosaved = app.autosave(&id, Some("auto-1")).unwrap();
    assert_eq!(autosaved.session.draft_revision, Some(3));
    assert!(autosaved.session.unsaved_changes);
    assert_eq!(store.draft_count(), 1);
    (store, id, autosaved)
}

#[test]
fn a_draft_left_by_an_ended_process_recovers_with_history_and_numbering() {
    let (store, id, before) = autosaved_drawing();
    let running = Application::new(NoCapabilities, store.as_process(2));
    let busy = running.document(&id).unwrap().recovery.unwrap();
    assert_eq!(busy.state, RecoveryState::InUse, "its process still runs");
    assert_eq!(
        running.recover(&id, None).unwrap_err().code,
        ErrorCode::DraftInUse
    );

    store.end_process();
    let app = Application::new(NoCapabilities, store.as_process(2));
    let opened = app.document(&id).unwrap();
    assert_eq!(
        opened.document.atoms.len(),
        0,
        "storage holds the empty commit"
    );
    let offer = opened.recovery.unwrap();
    assert_eq!(offer.state, RecoveryState::Recoverable);
    assert_eq!((offer.revision, offer.base_revision), (Some(3), Some(0)));
    let listed = app.list_documents(&ListQuery::default()).unwrap();
    assert_eq!(
        listed.documents[0].recovery.as_ref().unwrap().state,
        RecoveryState::Recoverable
    );

    let recovered = app.recover(&id, Some("rec-1")).unwrap();
    assert_eq!(recovered.document.atoms, before.document.atoms);
    assert_eq!(recovered.document.revision, 3);
    assert_eq!(recovered.session.saved_revision, 0);
    assert!(recovered.session.unsaved_changes);
    assert_eq!(
        (recovered.session.undo_depth, recovered.session.redo_depth),
        (1, 1)
    );
    assert_eq!(recovered.session.draft_revision, Some(3));
    assert!(
        recovered.recovery.is_none(),
        "the session now owns the draft"
    );
    assert_eq!(
        app.recover(&id, Some("rec-1")).unwrap(),
        recovered,
        "replayed"
    );

    let redone = app.redo(&id, None, 3).unwrap();
    assert_eq!(redone.document.title, "Renamed");
    let added = app
        .apply_transaction(
            &id,
            None,
            4,
            commands(json!([{"op": "add_atom", "element": "N", "position": {"x": 0, "y": 40}}])),
        )
        .unwrap();
    assert_eq!(added.document.atoms.last().unwrap().id, "a4");
    app.save(&id, None, 5).unwrap();
    assert_eq!(store.draft_count(), 0, "the save removed the draft");

    let later = Application::new(NoCapabilities, store.as_process(3));
    let reopened = later.document(&id).unwrap();
    assert!(reopened.recovery.is_none());
    assert_eq!(reopened.document.atoms.len(), 4);
}

#[test]
fn a_draft_whose_base_was_saved_over_is_recovered_only_as_a_copy() {
    let (store, id, before) = autosaved_drawing();
    store.end_process();
    // Another writer commits different content on the draft's base.
    let other = Application::new(NoCapabilities, store.as_process(2));
    other
        .apply_transaction(
            &id,
            None,
            0,
            commands(json!([{"op": "rename", "title": "Other"}])),
        )
        .unwrap();
    other.save(&id, None, 1).unwrap();

    let app = Application::new(NoCapabilities, store.as_process(3));
    let offer = app.document(&id).unwrap().recovery.unwrap();
    assert_eq!(offer.state, RecoveryState::Outdated);
    let refused = app.recover(&id, None).unwrap_err();
    assert_eq!(refused.code, ErrorCode::DraftOutdated);
    assert_eq!(refused.current_revision, Some(1));
    assert_eq!(store.draft_count(), 1, "a refused recovery keeps the draft");

    let copied = app.recover_as_copy(&id, Some("copy-1"), None).unwrap();
    assert_ne!(copied.copy.document.id, id);
    assert_eq!(copied.copy.document.title, "Draft (recovered)");
    assert_eq!(copied.copy.document.atoms, before.document.atoms);
    assert!(
        !copied.copy.session.unsaved_changes,
        "the copy is committed"
    );
    assert!(!copied.source.closed);
    assert_eq!(store.draft_count(), 0);
    assert_eq!(
        app.recover_as_copy(&id, Some("copy-1"), None).unwrap(),
        copied
    );
    assert_eq!(app.document(&id).unwrap().document.title, "Other");
    assert_eq!(
        app.recover_as_copy(&id, Some("copy-2"), None)
            .unwrap_err()
            .code,
        ErrorCode::DraftNotFound
    );
}

#[test]
fn discarded_work_never_comes_back_as_a_draft() {
    // Discarding the session's changes removes its draft before the session ends.
    let (store, id, _) = autosaved_drawing();
    let app = Application::new(NoCapabilities, store.clone());
    app.close(&id, None, true, Some(3)).unwrap();
    assert_eq!(store.draft_count(), 0);
    store.end_process();
    let later = Application::new(NoCapabilities, store.as_process(2));
    assert!(later.document(&id).unwrap().recovery.is_none());

    // An abandoned draft blocks autosave of new work until it is recovered or discarded.
    let (store, id, _) = autosaved_drawing();
    store.end_process();
    let app = Application::new(NoCapabilities, store.as_process(2));
    app.apply_transaction(
        &id,
        None,
        0,
        commands(json!([{"op": "rename", "title": "New"}])),
    )
    .unwrap();
    assert_eq!(
        app.autosave(&id, None).unwrap_err().code,
        ErrorCode::RecoveryPending
    );
    assert_eq!(
        app.discard_draft(&id, Some("d1")).unwrap().id,
        id,
        "an abandoned draft can be discarded while the session has its own changes"
    );
    assert_eq!(
        app.discard_draft(&id, Some("d1")).unwrap().id,
        id,
        "replayed"
    );
    assert_eq!(store.draft_count(), 0);
    let autosaved = app.autosave(&id, None).unwrap();
    assert_eq!(autosaved.session.draft_revision, Some(1));
    assert_eq!(
        app.discard_draft(&id, None).unwrap_err().code,
        ErrorCode::DraftInUse,
        "the session's own draft is dropped by discarding the session"
    );
    app.save(&id, None, 1).unwrap();
    assert_eq!(store.draft_count(), 0);
    assert_eq!(
        app.discard_draft(&id, None).unwrap_err().code,
        ErrorCode::DraftNotFound
    );
}

#[test]
fn autosave_failures_keep_the_session_and_succeed_on_retry() {
    let (app, store) = application();
    let id = app.create_document(None, None).unwrap().document.id;
    assert_eq!(
        app.autosave(&id, None).unwrap().session.draft_revision,
        None,
        "nothing unsaved"
    );
    app.apply_transaction(&id, None, 0, draw_ethanol()).unwrap();
    store.fail_writes.store(true, Ordering::SeqCst);
    let failed = app.autosave(&id, Some("auto-1")).unwrap_err();
    assert_eq!(failed.code, ErrorCode::StorageFailed);
    let session = app.document(&id).unwrap();
    assert!(session.session.unsaved_changes);
    assert_eq!(session.session.draft_revision, None);
    store.fail_writes.store(false, Ordering::SeqCst);
    assert_eq!(
        app.autosave(&id, Some("auto-1"))
            .unwrap()
            .session
            .draft_revision,
        Some(1)
    );
    // Undoing back to the saved content is still unsaved by revision, and keeps a draft.
    app.undo(&id, None, 1).unwrap();
    assert_eq!(
        app.autosave(&id, None).unwrap().session.draft_revision,
        Some(2)
    );
}

#[test]
fn save_as_commits_an_independent_copy_and_leaves_the_original_as_saved() {
    let (app, store) = application();
    let id = app
        .create_document(None, Some("Original"))
        .unwrap()
        .document
        .id;
    app.apply_transaction(&id, None, 0, draw_ethanol()).unwrap();
    app.save(&id, None, 1).unwrap();
    app.apply_transaction(
        &id,
        None,
        1,
        commands(json!([{"op": "delete_atoms", "atoms": ["a3"]}])),
    )
    .unwrap();
    app.autosave(&id, None).unwrap();
    assert_eq!(store.draft_count(), 1);

    let stale = app.save_as(&id, Some("as-0"), 1, None).unwrap_err();
    assert_eq!(stale.code, ErrorCode::RevisionConflict);

    store.fail_writes.store(true, Ordering::SeqCst);
    let failed = app.save_as(&id, Some("as-1"), 2, None).unwrap_err();
    assert_eq!(failed.code, ErrorCode::StorageFailed);
    assert!(
        app.document(&id).unwrap().session.unsaved_changes,
        "nothing changed"
    );
    assert_eq!(store.documents.lock().unwrap().len(), 1);
    store.fail_writes.store(false, Ordering::SeqCst);

    let copied = app.save_as(&id, Some("as-1"), 2, None).unwrap();
    let copy_id = copied.copy.document.id.clone();
    assert_ne!(copy_id, id);
    assert_eq!(copied.copy.document.title, "Original (copy)");
    assert_eq!(copied.copy.document.revision, 0);
    assert_eq!(
        copied.copy.document.atoms.len(),
        2,
        "the unsaved deletion is in the copy"
    );
    assert_eq!(
        (
            copied.copy.session.undo_depth,
            copied.copy.session.redo_depth
        ),
        (0, 0)
    );
    assert!(copied.source.closed && copied.source.discarded_changes);
    assert_eq!(
        store.draft_count(),
        0,
        "the source's draft went with its session"
    );
    assert_eq!(
        app.save_as(&id, Some("as-1"), 2, None).unwrap(),
        copied,
        "replayed"
    );

    // The original reopens from its last save; the two documents change independently.
    let original = app.document(&id).unwrap();
    assert_eq!(original.document.atoms.len(), 3);
    assert!(!original.session.unsaved_changes);
    let edited_copy = app
        .apply_transaction(
            &copy_id,
            None,
            0,
            commands(
                json!([{"op": "add_atom", "element": "N", "position": {"x": 0, "y": 40},
                             "bond_to": {"atom": "a1", "order": "single"}}]),
            ),
        )
        .unwrap();
    assert_eq!(
        edited_copy.document.atoms.last().unwrap().id,
        "a4",
        "counters are kept"
    );
    app.save(&copy_id, None, 1).unwrap();
    assert_eq!(app.document(&id).unwrap().document.atoms.len(), 3);
    let renamed = app
        .apply_transaction(
            &id,
            None,
            original.document.revision,
            commands(json!([{"op": "rename", "title": "Changed"}])),
        )
        .unwrap();
    app.save(&id, None, renamed.document.revision).unwrap();
    assert_eq!(
        app.document(&copy_id).unwrap().document.title,
        "Original (copy)"
    );
}

#[test]
fn a_save_waiting_on_storage_does_not_hold_up_other_requests() {
    let (app, store) = application();
    let app = Arc::new(app);
    let id = app.create_document(None, None).unwrap().document.id;
    let other = app.create_document(None, None).unwrap().document.id;
    app.apply_transaction(&id, None, 0, draw_ethanol()).unwrap();

    let stalled = store.commit_gate.lock().unwrap();
    let saving = {
        let (app, id) = (Arc::clone(&app), id.clone());
        std::thread::spawn(move || app.save(&id, Some("save-1"), 1))
    };
    std::thread::sleep(std::time::Duration::from_millis(100));
    let (done, finished) = std::sync::mpsc::channel();
    {
        let (app, id, other) = (Arc::clone(&app), id.clone(), other.clone());
        std::thread::spawn(move || {
            app.document(&other).unwrap();
            let edited = app
                .apply_transaction(
                    &id,
                    None,
                    1,
                    commands(json!([{"op": "rename", "title": "During"}])),
                )
                .unwrap();
            done.send(edited.document.revision).unwrap();
        });
    }
    let during = finished
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("other requests proceed while a save waits for storage");
    assert_eq!(during, 2);
    drop(stalled);

    let saved = saving.join().unwrap().unwrap();
    assert_eq!(
        saved.session.saved_revision, 1,
        "the checked revision was committed"
    );
    assert!(
        saved.session.unsaved_changes,
        "the edit made meanwhile is still unsaved"
    );
    assert_eq!(
        store.documents.lock().unwrap()[&DocumentId::parse(&id).unwrap()].revision(),
        1
    );
    assert_eq!(app.save(&id, Some("save-1"), 1).unwrap(), saved, "replayed");
    let resaved = app.save(&id, None, 2).unwrap();
    assert!(!resaved.session.unsaved_changes);
}

#[test]
fn recovery_and_copies_reuse_their_session_slot_when_every_other_is_unsaved() {
    // A draft abandoned by process 1, then 63 unsaved documents in process 2.
    let (store, id, _) = autosaved_drawing();
    store.end_process();
    let app = Application::new(NoCapabilities, store.as_process(2));
    for _ in 1..MAX_OPEN_SESSIONS {
        let busy = app.create_document(None, None).unwrap().document.id;
        app.apply_transaction(&busy, None, 0, draw_ethanol())
            .unwrap();
    }
    assert!(
        app.document(&id).unwrap().recovery.is_some(),
        "the 64th session is clean"
    );
    let recovered = app.recover(&id, None).unwrap();
    assert!(recovered.session.unsaved_changes);

    let copied = app
        .save_as(&id, None, recovered.document.revision, None)
        .unwrap();
    assert!(copied.source.closed);
    let open = app
        .list_documents(&ListQuery::default())
        .unwrap()
        .documents
        .iter()
        .filter(|document| document.open)
        .count();
    assert_eq!(open, MAX_OPEN_SESSIONS, "the copy took the source's slot");
}

/// An application in process 2 holding the abandoned draft of an autosaved drawing (process 1
/// ended), with the drawing's own session open and clean, beside `others` sessions with
/// unsaved work and `clean_others` clean ones.
fn full_table_beside_an_abandoned_draft(
    others: usize,
    clean_others: usize,
) -> (
    Application<NoCapabilities, MemoryStore>,
    MemoryStore,
    String,
    DocumentResponse,
) {
    let (store, id, autosaved) = autosaved_drawing();
    store.end_process();
    let app = Application::new(NoCapabilities, store.as_process(2));
    for _ in 0..others {
        let busy = app.create_document(None, None).unwrap().document.id;
        app.apply_transaction(&busy, None, 0, draw_ethanol())
            .unwrap();
    }
    for _ in 0..clean_others {
        app.create_document(None, None).unwrap();
    }
    assert_eq!(
        app.document(&id).unwrap().recovery.unwrap().state,
        RecoveryState::Recoverable
    );
    (app, store, id, autosaved)
}

fn open_ids(app: &Application<NoCapabilities, MemoryStore>) -> Vec<String> {
    let listed = app.list_documents(&ListQuery::default()).unwrap();
    listed
        .documents
        .into_iter()
        .filter(|document| document.open)
        .map(|document| document.id)
        .collect()
}

#[test]
fn recovering_as_a_copy_at_capacity_reports_closing_the_source_it_evicted() {
    // Every other session has unsaved work: the clean source is the only one that can go.
    let (app, store, id, autosaved) =
        full_table_beside_an_abandoned_draft(MAX_OPEN_SESSIONS - 1, 0);
    let stored_before = store.load(&DocumentId::parse(&id).unwrap()).unwrap();

    let copied = app.recover_as_copy(&id, Some("copy-full"), None).unwrap();

    assert!(
        copied.source.closed,
        "the source session was evicted for the copy, so the reply must say it closed"
    );
    assert!(
        !copied.source.discarded_changes,
        "only a clean session is evicted"
    );
    let open = open_ids(&app);
    assert_eq!(open.len(), MAX_OPEN_SESSIONS);
    assert!(!open.contains(&id), "the source is no longer open");
    assert!(open.contains(&copied.copy.document.id));
    assert_eq!(copied.copy.document.atoms, autosaved.document.atoms);
    assert_eq!(store.draft_count(), 0, "the draft went into the copy");
    assert_eq!(
        store.load(&DocumentId::parse(&id).unwrap()).unwrap(),
        stored_before,
        "the source's commit is untouched"
    );
    assert_eq!(
        app.document(&id).unwrap().document.revision,
        0,
        "the source reopens from its last save"
    );
}

#[test]
fn recovering_as_a_copy_at_capacity_keeps_the_source_open_when_another_session_is_clean() {
    // The source was created first and has the lowest identifier, which eviction would pick
    // first if it did not spare the source.
    let (app, _store, id, _) = full_table_beside_an_abandoned_draft(MAX_OPEN_SESSIONS - 2, 1);

    let copied = app.recover_as_copy(&id, None, None).unwrap();

    assert!(!copied.source.closed);
    let open = open_ids(&app);
    assert_eq!(open.len(), MAX_OPEN_SESSIONS);
    assert!(
        open.contains(&id),
        "the other clean session made room instead"
    );
    assert!(open.contains(&copied.copy.document.id));
}

#[test]
fn recovering_as_a_copy_with_no_session_to_evict_changes_nothing() {
    let (app, store, id, _) = full_table_beside_an_abandoned_draft(MAX_OPEN_SESSIONS - 1, 0);
    // The source's own session takes unsaved work too, so no session is clean.
    let edited = app
        .apply_transaction(
            &id,
            None,
            0,
            commands(json!([{"op": "rename", "title": "Edited here"}])),
        )
        .unwrap();
    let stored = store.list_ids().unwrap().len();

    let refused = app.recover_as_copy(&id, None, None).unwrap_err();

    assert_eq!(refused.code, ErrorCode::ResourceLimit);
    assert_eq!(
        store.list_ids().unwrap().len(),
        stored,
        "no copy was committed"
    );
    assert_eq!(
        store
            .draft_status(&DocumentId::parse(&id).unwrap())
            .unwrap(),
        DraftStatus::Abandoned(Ok(DraftInfo {
            revision: 3,
            base_revision: 0,
            saved_at_ms: store.drafts.lock().unwrap()[&DocumentId::parse(&id).unwrap()]
                .0
                .saved_at_ms,
        })),
        "the draft is kept and abandoned again"
    );
    assert_eq!(
        app.document(&id).unwrap(),
        edited,
        "the source session is as it was"
    );
}

/// A document with unsaved changes at revision 1.
fn edited(app: &Application<NoCapabilities, MemoryStore>) -> String {
    let id = app.create_document(None, None).unwrap().document.id;
    app.apply_transaction(&id, None, 0, draw_ethanol()).unwrap();
    id
}

/// Runs `request` on another thread and returns once its save has reached storage, where it
/// waits while the test holds `store.commit_gate`.
fn stalled<T: Send + 'static>(
    store: &MemoryStore,
    request: impl FnOnce() -> T + Send + 'static,
) -> std::thread::JoinHandle<T> {
    let before = store.saves_started.load(Ordering::SeqCst);
    let running = std::thread::spawn(request);
    let started = std::time::Instant::now();
    while store.saves_started.load(Ordering::SeqCst) == before {
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "the stalled request reached storage"
        );
        std::thread::yield_now();
    }
    running
}

/// Runs `request` on another thread and gives up on it after `limit`, so a request that
/// blocks fails the test instead of hanging it.
fn within<T: Send + 'static>(
    limit: std::time::Duration,
    request: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    let (answered, answer) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = answered.send(request());
    });
    answer.recv_timeout(limit).ok()
}

#[test]
fn storage_failures_say_whether_retrying_can_help() {
    let (app, store) = application();
    let id = edited(&app);
    let cases = [
        (
            StorageError::Denied("read-only file system".into()),
            StorageReason::ReadOnly,
        ),
        (
            StorageError::Locked("another process is saving".into()),
            StorageReason::Busy,
        ),
        (
            StorageError::Unavailable("disk unavailable".into()),
            StorageReason::Io,
        ),
    ];
    for (error, reason) in cases {
        *store.fail_with.lock().unwrap() = Some(error.clone());
        let saved = app.save(&id, None, 1).unwrap_err();
        assert_eq!(
            (saved.code, saved.reason),
            (ErrorCode::StorageFailed, Some(reason)),
            "save failing with {error:?}"
        );
        assert_eq!(
            saved.to_response().reason,
            Some(reason.as_str()),
            "the reason is in the v1 envelope"
        );
        // For a draft, a lock held elsewhere means another process owns it.
        let expected = match reason {
            StorageReason::Busy => (ErrorCode::DraftInUse, None),
            reason => (ErrorCode::StorageFailed, Some(reason)),
        };
        let autosaved = app.autosave(&id, None).unwrap_err();
        assert_eq!(
            (autosaved.code, autosaved.reason),
            expected,
            "autosave failing with {error:?}"
        );
    }
    *store.fail_with.lock().unwrap() = None;
    let document = app.document(&id).unwrap();
    assert!(
        document.session.unsaved_changes,
        "the session kept its changes"
    );
    assert_eq!(store.draft_count(), 0, "no draft was written");
    assert!(!app.save(&id, None, 1).unwrap().session.unsaved_changes);
}

#[test]
fn requests_queued_behind_a_stalled_save_are_answered_busy_without_changes() {
    let (app, store) = application();
    let app = Arc::new(app);
    let id = edited(&app);
    let documents = store.documents.lock().unwrap().len();
    let held = store.commit_gate.lock().unwrap();
    let saving = stalled(&store, {
        let (app, id) = (Arc::clone(&app), id.clone());
        move || app.save(&id, Some("save-1"), 1)
    });

    let started = std::time::Instant::now();
    let queued: Vec<_> = (0..3)
        .map(|which| {
            let (app, id) = (Arc::clone(&app), id.clone());
            std::thread::spawn(move || match which {
                0 => app.autosave(&id, Some("autosave-1")).map(|_| ()),
                1 => app.save_as(&id, Some("copy-1"), 1, None).map(|_| ()),
                _ => app.close(&id, Some("close-1"), true, Some(1)).map(|_| ()),
            })
        })
        .collect();
    for request in queued {
        let refused = request.join().unwrap().unwrap_err();
        assert_eq!(
            (refused.code, refused.reason),
            (ErrorCode::StorageFailed, Some(StorageReason::Busy)),
            "{refused}"
        );
    }
    let waited = started.elapsed();
    assert!(
        waited >= TURN_WAIT && waited < TURN_WAIT + std::time::Duration::from_secs(2),
        "each waited for the turn, side by side ({waited:?})"
    );

    drop(held);
    let saved = saving.join().unwrap().unwrap();
    assert_eq!(saved.session.saved_revision, 1);
    assert_eq!(
        store.documents.lock().unwrap().len(),
        documents,
        "no copy was made"
    );
    assert_eq!(store.draft_count(), 0, "no draft was written");
    assert_eq!(
        app.document(&id).unwrap().document.revision,
        1,
        "the session was not closed"
    );
}

#[test]
fn a_save_whose_reply_was_lost_is_reported_by_asking_again() {
    let (app, store) = application();
    let app = Arc::new(app);
    let id = edited(&app);
    let held = store.commit_gate.lock().unwrap();
    let saving = stalled(&store, {
        let (app, id) = (Arc::clone(&app), id.clone());
        move || app.save(&id, Some("save-1"), 1)
    });

    let asked = within(std::time::Duration::from_millis(500), {
        let (app, id) = (Arc::clone(&app), id.clone());
        move || app.save(&id, Some("save-1"), 1)
    })
    .expect("answered at once, not after waiting for its own turn");
    assert_eq!(asked.unwrap_err().code, ErrorCode::RequestInProgress);

    drop(held);
    let committed = saving.join().unwrap().unwrap();
    let saves = store.saves_started.load(Ordering::SeqCst);
    assert_eq!(
        app.save(&id, Some("save-1"), 1).unwrap(),
        committed,
        "once done, asking again returns the committed save"
    );
    assert_eq!(
        store.saves_started.load(Ordering::SeqCst),
        saves,
        "storage was not written again"
    );
}

#[test]
fn a_finished_request_asked_about_again_is_answered_while_another_holds_the_turn() {
    let (app, store) = application();
    let app = Arc::new(app);
    let id = edited(&app);
    let committed = app.save(&id, Some("save-1"), 1).unwrap();
    app.apply_transaction(
        &id,
        None,
        1,
        commands(json!([{"op": "rename", "title": "Second"}])),
    )
    .unwrap();
    let held = store.commit_gate.lock().unwrap();
    // Another tab's save holds the document's turn.
    let saving = stalled(&store, {
        let (app, id) = (Arc::clone(&app), id.clone());
        move || app.save(&id, Some("save-2"), 2)
    });

    let saves = store.saves_started.load(Ordering::SeqCst);
    let asked = within(std::time::Duration::from_millis(500), {
        let (app, id) = (Arc::clone(&app), id.clone());
        move || app.save(&id, Some("save-1"), 1)
    })
    .expect("answered without waiting for the turn");
    assert_eq!(asked.unwrap(), committed, "the recorded reply, not busy");
    assert_eq!(store.saves_started.load(Ordering::SeqCst), saves);

    drop(held);
    saving.join().unwrap().unwrap();
}

#[test]
fn a_copy_asked_about_while_it_writes_is_answered_at_once() {
    let (app, store) = application();
    let app = Arc::new(app);
    let id = edited(&app);
    let held = store.commit_gate.lock().unwrap();
    // The copy's commit stalls while the copy holds the session table.
    let copying = stalled(&store, {
        let (app, id) = (Arc::clone(&app), id.clone());
        move || app.save_as(&id, Some("copy-1"), 1, None)
    });

    let asked = within(std::time::Duration::from_millis(500), {
        let (app, id) = (Arc::clone(&app), id.clone());
        move || app.save_as(&id, Some("copy-1"), 1, None)
    })
    .expect("answered without waiting for the session table");
    assert_eq!(asked.unwrap_err().code, ErrorCode::RequestInProgress);

    drop(held);
    let copied = copying.join().unwrap().unwrap();
    let documents = store.documents.lock().unwrap().len();
    assert_eq!(
        app.save_as(&id, Some("copy-1"), 1, None).unwrap(),
        copied,
        "once done, asking again returns the same copy"
    );
    assert_eq!(
        store.documents.lock().unwrap().len(),
        documents,
        "no second copy"
    );
}

#[test]
fn a_request_id_kept_per_process_is_in_progress_for_every_document_while_it_runs() {
    let (app, store) = application();
    let app = Arc::new(app);
    let id = edited(&app);
    let other = app.create_document(None, None).unwrap().document.id;
    let held = store.commit_gate.lock().unwrap();
    let copying = stalled(&store, {
        let (app, id) = (Arc::clone(&app), id.clone());
        move || app.save_as(&id, Some("shared-1"), 1, None)
    });

    // Copies are remembered per process, so the same request_id names the running copy even
    // for another document.
    let asked = within(std::time::Duration::from_millis(500), {
        let (app, other) = (Arc::clone(&app), other.clone());
        move || app.recover_as_copy(&other, Some("shared-1"), None)
    })
    .expect("answered without waiting for the session table");
    assert_eq!(asked.unwrap_err().code, ErrorCode::RequestInProgress);

    drop(held);
    copying.join().unwrap().unwrap();
    assert_eq!(
        app.recover_as_copy(&other, Some("shared-1"), None)
            .unwrap_err()
            .code,
        ErrorCode::RequestIdReused,
        "once done, the ID names the copy"
    );
}

/// A document titled `title`, saved at revision 1 and still open in this process.
fn stored(app: &Application<NoCapabilities, MemoryStore>, title: &str) -> String {
    let id = app.create_document(None, Some(title)).unwrap().document.id;
    app.apply_transaction(&id, None, 0, draw_ethanol()).unwrap();
    app.save(&id, None, 1).unwrap();
    id
}

fn listed(
    app: &Application<NoCapabilities, MemoryStore>,
    query: Option<&str>,
    sort: ListSort,
) -> Vec<String> {
    app.list_documents(&ListQuery {
        query: query.map(str::to_owned),
        sort,
    })
    .unwrap()
    .documents
    .into_iter()
    .map(|document| document.title)
    .collect()
}

#[test]
fn the_list_is_searched_by_title_and_sorted_by_title_or_last_save() {
    let (app, _store) = application();
    stored(&app, "Methane");
    let ethanol = stored(&app, "Ethanol");
    stored(&app, "benzene");
    assert_eq!(
        listed(&app, None, ListSort::Title),
        ["Ethanol", "Methane", "benzene"],
        "by title (code point order), then identifier"
    );
    assert_eq!(
        listed(&app, Some("  ETH "), ListSort::Title),
        ["Ethanol", "Methane"]
    );
    assert_eq!(
        listed(&app, Some("xyz"), ListSort::Title),
        Vec::<String>::new()
    );
    assert_eq!(
        listed(&app, Some(""), ListSort::Title).len(),
        3,
        "empty lists all"
    );

    app.rename(&ethanol, None, 1, "Ethanol, renamed").unwrap();
    assert_eq!(
        listed(&app, None, ListSort::Recent),
        ["Ethanol, renamed", "benzene", "Methane"],
        "most recently saved first"
    );
    let long = "x".repeat(1_000);
    let refused = app
        .list_documents(&ListQuery {
            query: Some(long),
            sort: ListSort::Title,
        })
        .unwrap_err();
    assert_eq!(refused.code, ErrorCode::InvalidRequest);
}

#[test]
fn renaming_commits_at_once_and_needs_a_clean_copy() {
    let (app, store) = application();
    let id = stored(&app, "Ethanol");
    let renamed = app
        .rename(&id, Some("rename-1"), 1, "Ethyl alcohol")
        .unwrap();
    assert_eq!(renamed.document.title, "Ethyl alcohol");
    assert_eq!(renamed.document.revision, 2);
    assert!(
        !renamed.session.unsaved_changes,
        "committed, not just edited"
    );
    let key = DocumentId::parse(&id).unwrap();
    assert_eq!(store.load(&key).unwrap().unwrap().title(), "Ethyl alcohol");
    assert_eq!(
        app.rename(&id, Some("rename-1"), 1, "Ethyl alcohol")
            .unwrap(),
        renamed,
        "a retried request replays"
    );
    let same = app.rename(&id, None, 2, "Ethyl alcohol").unwrap();
    assert_eq!(
        same.document.revision, 2,
        "an unchanged title commits nothing"
    );

    let stale = app.rename(&id, None, 1, "Other").unwrap_err();
    assert_eq!(stale.code, ErrorCode::RevisionConflict);
    app.apply_transaction(
        &id,
        None,
        2,
        commands(json!([{"op": "delete_atoms", "atoms": ["a1"]}])),
    )
    .unwrap();
    let unsaved = app.rename(&id, None, 3, "Other").unwrap_err();
    assert_eq!(unsaved.code, ErrorCode::UnsavedChanges);
    assert_eq!(store.load(&key).unwrap().unwrap().title(), "Ethyl alcohol");
    assert_eq!(
        app.document(&id).unwrap().document.atoms.len(),
        2,
        "the edit is kept"
    );
}

#[test]
fn renaming_or_deleting_waits_for_an_earlier_sessions_draft() {
    let (store, id, _) = autosaved_drawing();
    store.end_process();
    let app = Application::new(NoCapabilities, store.as_process(2));
    let revision = app.document(&id).unwrap().document.revision;
    let rename = app
        .rename(&id, None, revision, "Renamed over a draft")
        .unwrap_err();
    assert_eq!(rename.code, ErrorCode::RecoveryPending);
    let delete = app.delete(&id, None, revision).unwrap_err();
    assert_eq!(delete.code, ErrorCode::RecoveryPending);
    let key = DocumentId::parse(&id).unwrap();
    assert_eq!(
        store.load(&key).unwrap().unwrap().title(),
        "Draft",
        "nothing changed"
    );
    assert_eq!(store.draft_count(), 1, "the draft is kept");
}

#[test]
fn deleting_moves_a_clean_document_to_the_trash_and_restoring_brings_it_back() {
    let (app, store) = application();
    let id = stored(&app, "Ethanol");
    let deleted = app.delete(&id, Some("delete-1"), 1).unwrap();
    assert_eq!(
        (deleted.id.as_str(), deleted.title.as_str()),
        (id.as_str(), "Ethanol")
    );
    assert_eq!(
        app.delete(&id, Some("delete-1"), 1).unwrap(),
        deleted,
        "replayed"
    );
    assert_eq!(listed(&app, None, ListSort::Title), Vec::<String>::new());
    assert_eq!(
        app.document(&id).unwrap_err().code,
        ErrorCode::DocumentNotFound
    );
    let trash = app.list_trash().unwrap();
    assert_eq!(trash.documents.len(), 1);
    assert_eq!(
        (
            trash.documents[0].title.as_str(),
            trash.documents[0].revision
        ),
        ("Ethanol", 1)
    );

    let restored = app.restore(&id, Some("restore-1")).unwrap();
    assert_eq!(restored.title, "Ethanol");
    assert_eq!(
        app.restore(&id, Some("restore-1")).unwrap(),
        restored,
        "replayed"
    );
    assert_eq!(
        app.restore(&id, Some("restore-2")).unwrap_err().code,
        ErrorCode::DocumentNotFound,
        "not in the trash any more"
    );
    assert!(app.list_trash().unwrap().documents.is_empty());
    let reopened = app.document(&id).unwrap();
    assert_eq!(reopened.document.atoms.len(), 3, "content as trashed");
    assert!(
        store
            .load_trashed(&DocumentId::parse(&id).unwrap())
            .unwrap()
            .is_none()
    );
}

#[test]
fn deleting_refuses_to_lose_unsaved_work_anywhere() {
    let (app, store) = application();
    let id = stored(&app, "Ethanol");
    assert_eq!(
        app.delete(&id, None, 0).unwrap_err().code,
        ErrorCode::RevisionConflict
    );
    app.apply_transaction(
        &id,
        None,
        1,
        commands(json!([{"op": "rename", "title": "Edited"}])),
    )
    .unwrap();
    assert_eq!(
        app.delete(&id, None, 2).unwrap_err().code,
        ErrorCode::UnsavedChanges
    );
    app.autosave(&id, None).unwrap();

    // Another process sees the first one's draft as in use.
    let other = Application::new(NoCapabilities, store.as_process(2));
    let refused = other.delete(&id, None, 1).unwrap_err();
    assert_eq!(refused.code, ErrorCode::DraftInUse);
    assert!(other.list_trash().unwrap().documents.is_empty());
    assert_eq!(
        app.document(&id).unwrap().document.title,
        "Edited",
        "still unsaved, not lost"
    );
}

#[test]
fn copies_in_other_processes_never_bring_a_deleted_document_back() {
    let (first, store) = application();
    let id = stored(&first, "Ethanol");
    let clean = Application::new(NoCapabilities, store.as_process(2));
    let dirty = Application::new(NoCapabilities, store.as_process(3));
    assert_eq!(clean.document(&id).unwrap().document.revision, 1);
    dirty
        .apply_transaction(
            &id,
            None,
            1,
            commands(json!([{"op": "rename", "title": "Late"}])),
        )
        .unwrap();

    first.delete(&id, None, 1).unwrap();
    // A clean copy closes on its next access instead of showing the document as stored.
    assert_eq!(
        clean.document(&id).unwrap_err().code,
        ErrorCode::DocumentNotFound
    );
    assert_eq!(listed(&clean, None, ListSort::Title), Vec::<String>::new());
    // A copy with unsaved work keeps it, but cannot save or draft it as the deleted document.
    assert_eq!(
        dirty.save(&id, None, 2).unwrap_err().code,
        ErrorCode::SaveConflict
    );
    assert_eq!(
        dirty.autosave(&id, None).unwrap_err().code,
        ErrorCode::DocumentNotFound
    );
    let key = DocumentId::parse(&id).unwrap();
    assert!(store.load(&key).unwrap().is_none(), "not re-created");
    assert_eq!(store.draft_count(), 0, "no draft outlives it");
    assert_eq!(
        dirty.document(&id).unwrap().document.title,
        "Late",
        "still held in memory"
    );
    // Its work can still be kept as a new document.
    assert_eq!(
        dirty
            .save_as(&id, None, 2, None)
            .unwrap()
            .copy
            .document
            .title,
        "Late (copy)"
    );
}

#[test]
fn edits_wait_while_a_delete_is_in_progress() {
    let (app, store) = application();
    let app = Arc::new(app);
    let id = stored(&app, "Ethanol");
    let stalled = store.commit_gate.lock().unwrap();
    let before = store.saves_started.load(Ordering::SeqCst);
    let deleting = {
        let (app, id) = (Arc::clone(&app), id.clone());
        std::thread::spawn(move || app.delete(&id, None, 1))
    };
    while store.saves_started.load(Ordering::SeqCst) == before {
        std::thread::yield_now();
    }
    let edit = app
        .apply_transaction(
            &id,
            None,
            1,
            commands(json!([{"op": "rename", "title": "Lost?"}])),
        )
        .unwrap_err();
    assert_eq!(
        (edit.code, edit.reason),
        (ErrorCode::StorageFailed, Some(StorageReason::Busy)),
        "{}",
        edit.message
    );
    drop(stalled);
    deleting.join().unwrap().unwrap();
    let trashed = store
        .load_trashed(&DocumentId::parse(&id).unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(
        trashed.title(),
        "Ethanol",
        "trashed as confirmed, without the refused edit"
    );
}

#[test]
fn renames_and_deletes_queued_behind_a_stalled_save_are_answered_busy_without_changes() {
    let (app, store) = application();
    let app = Arc::new(app);
    let id = edited(&app);
    let title = app.document(&id).unwrap().document.title;
    let held = store.commit_gate.lock().unwrap();
    let saving = stalled(&store, {
        let (app, id) = (Arc::clone(&app), id.clone());
        move || app.save(&id, Some("save-1"), 1)
    });

    let started = std::time::Instant::now();
    let renaming = std::thread::spawn({
        let (app, id) = (Arc::clone(&app), id.clone());
        move || app.rename(&id, Some("rename-1"), 1, "Renamed").map(|_| ())
    });
    let deleting = std::thread::spawn({
        let (app, id) = (Arc::clone(&app), id.clone());
        move || app.delete(&id, Some("delete-1"), 1).map(|_| ())
    });
    for request in [renaming, deleting] {
        let refused = request.join().unwrap().unwrap_err();
        assert_eq!(
            (refused.code, refused.reason),
            (ErrorCode::StorageFailed, Some(StorageReason::Busy)),
            "{refused}"
        );
    }
    let waited = started.elapsed();
    assert!(
        waited >= TURN_WAIT && waited < TURN_WAIT + std::time::Duration::from_secs(2),
        "each waited for the turn, side by side ({waited:?})"
    );

    drop(held);
    saving.join().unwrap().unwrap();
    let shown = app.document(&id).unwrap();
    assert_eq!(
        (shown.document.title, shown.document.revision),
        (title, 1),
        "neither the rename nor the delete took effect"
    );
    assert!(
        store.trash.lock().unwrap().is_empty(),
        "nothing was trashed"
    );
}

#[test]
fn a_delete_or_restore_asked_about_while_it_writes_is_answered_at_once_and_not_repeated() {
    let (app, store) = application();
    let app = Arc::new(app);
    let id = stored(&app, "Moved");

    for step in ["delete", "restore"] {
        let request = {
            let (app, id) = (Arc::clone(&app), id.clone());
            move || match step {
                "delete" => app.delete(&id, Some("move-1"), 1).map(|moved| moved.title),
                _ => app.restore(&id, Some("move-2")).map(|moved| moved.title),
            }
        };
        let held = store.commit_gate.lock().unwrap();
        let moving = stalled(&store, request.clone());
        let asked = within(std::time::Duration::from_millis(500), request.clone())
            .expect("answered without waiting for the running request");
        assert_eq!(
            asked.unwrap_err().code,
            ErrorCode::RequestInProgress,
            "{step}"
        );

        drop(held);
        let moved = moving.join().unwrap().unwrap();
        let writes = store.saves_started.load(Ordering::SeqCst);
        assert_eq!(request().unwrap(), moved, "{step}: the recorded reply");
        assert_eq!(
            store.saves_started.load(Ordering::SeqCst),
            writes,
            "{step}: storage was not written again"
        );
    }
    assert_eq!(app.document(&id).unwrap().document.title, "Moved");
}

#[test]
fn a_rename_asked_about_while_it_commits_is_answered_at_once() {
    let (app, store) = application();
    let app = Arc::new(app);
    let id = stored(&app, "Before");
    let held = store.commit_gate.lock().unwrap();
    let renaming = stalled(&store, {
        let (app, id) = (Arc::clone(&app), id.clone());
        move || app.rename(&id, Some("rename-1"), 1, "After")
    });

    let asked = within(std::time::Duration::from_millis(500), {
        let (app, id) = (Arc::clone(&app), id.clone());
        move || app.rename(&id, Some("rename-1"), 1, "After")
    })
    .expect("answered without waiting for the turn");
    assert_eq!(asked.unwrap_err().code, ErrorCode::RequestInProgress);

    drop(held);
    let renamed = renaming.join().unwrap().unwrap();
    assert_eq!(
        app.rename(&id, Some("rename-1"), 1, "After").unwrap(),
        renamed
    );
}

#[test]
fn rename_delete_and_restore_failures_say_whether_retrying_can_help() {
    let (app, store) = application();
    let renamed = stored(&app, "Renamed");
    let deleted = stored(&app, "Deleted");
    let restored = stored(&app, "Restored");
    app.delete(&restored, None, 1).unwrap();
    let cases = [
        (
            StorageError::Denied("read-only file system".into()),
            StorageReason::ReadOnly,
        ),
        (
            StorageError::Locked("another process is saving".into()),
            StorageReason::Busy,
        ),
        (
            StorageError::Unavailable("disk unavailable".into()),
            StorageReason::Io,
        ),
    ];
    for (error, reason) in cases {
        *store.fail_with.lock().unwrap() = Some(error);
        let failures = [
            app.rename(&renamed, None, 1, "Other").map(|_| ()),
            app.delete(&deleted, None, 1).map(|_| ()),
            app.restore(&restored, None).map(|_| ()),
        ];
        for failure in failures {
            let failure = failure.unwrap_err();
            assert_eq!(
                (failure.code, failure.reason),
                (ErrorCode::StorageFailed, Some(reason)),
                "{failure}"
            );
        }
    }
    *store.fail_with.lock().unwrap() = None;
    assert_eq!(app.document(&renamed).unwrap().document.title, "Renamed");
    assert_eq!(app.document(&deleted).unwrap().document.title, "Deleted");
    assert!(app.document(&restored).is_err(), "still in the trash");
}

#[test]
fn the_most_recently_trashed_documents_are_listed_however_full_the_trash_is() {
    let (app, _store) = application();
    let ids: Vec<String> = (0..1001)
        .map(|_| app.create_document(None, None).unwrap().document.id)
        .collect();
    // Deleted last, and last in identifier order ("doc-999" sorts after "doc-1000").
    let newest = "doc-999".to_owned();
    assert!(ids.contains(&newest));
    for id in ids.iter().filter(|id| **id != newest) {
        app.delete(id, None, 0).unwrap();
    }
    app.delete(&newest, None, 0).unwrap();

    let trash = app.list_trash().unwrap();
    assert_eq!(trash.documents.len(), 1000, "the list is capped");
    assert_eq!(
        trash.documents[0].id, newest,
        "the newest deletion is listed first"
    );
}
