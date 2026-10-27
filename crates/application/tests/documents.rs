use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use chemifree_application::{
    Application, CapabilityCatalog, CommandDto, DocumentRepository, DocumentResponse, Draft,
    DraftInfo, DraftStatus, ErrorCode, MAX_OPEN_SESSIONS, RecoveryState, StorageError,
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
    /// Held by a test to stall commits, as another process's save lock would.
    commit_gate: Arc<Mutex<()>>,
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
            _ => {}
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
    let listed = app.list_documents().unwrap();
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
    let listed = viewer.list_documents().unwrap();
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
        let listed = app.list_documents().unwrap();
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
    let list = app.list_documents().unwrap();
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
    let listed = app.list_documents().unwrap();
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
        .list_documents()
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
    let listed = app.list_documents().unwrap();
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
