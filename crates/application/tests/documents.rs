use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use chemifree_application::{
    Application, CapabilityCatalog, CommandDto, DocumentRepository, DocumentResponse, ErrorCode,
    MAX_OPEN_SESSIONS, StorageError,
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

/// In-memory store with the port's conflict contract and a switchable write failure.
#[derive(Clone, Default)]
struct MemoryStore {
    documents: Arc<Mutex<BTreeMap<DocumentId, Document>>>,
    next: Arc<AtomicU32>,
    fail_writes: Arc<AtomicBool>,
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
        if self.fail_writes.load(Ordering::SeqCst) {
            return Err(StorageError::Unavailable("disk unavailable".into()));
        }
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
