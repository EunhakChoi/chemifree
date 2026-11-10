//! A draft write looks up the document's recovery state before it takes the session table. These
//! tests stall that lookup, and then the write, with one-shot gates in a repository that delegates
//! every call to a file store. A gate sits before the file store runs its call, so the file store
//! holds no lock while the call is stalled.
//!
//! Each outcome is decided by the order of the calls: a gate is released only after the other
//! requests have answered. `HANG_GUARD` only turns a deadlock into a failure. The first test shows
//! that other documents, a create and a list are answered during a stalled lookup. The second shows
//! that an edit made during the lookup is the one drafted. The third shows that a response during
//! the write reports the lookup's result, and that the write then fails because the abandoned draft
//! is still there.

use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use chemifree_application::{
    Application, ApplicationError, CommandDto, DocumentRepository, DocumentResponse, Draft,
    DraftRecoveryTarget, DraftStatus, ListQuery, RecoveryState, StorageError, TrashEntry,
};
use chemifree_domain::document::{Document, DocumentId};
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::json;

type App = Application<LocalCapabilities, GatedRepository>;

/// Only a guard against a deadlock: a call that never answers fails its test instead of hanging
/// it. No outcome depends on how long anything takes.
const HANG_GUARD: Duration = Duration::from_secs(30);

/// One-shot gate on one document's calls to one repository method. Armed for a document, its
/// first matching call reports that it entered the gate and then waits for the release. Later
/// calls pass through.
#[derive(Default)]
struct Gate {
    armed: Mutex<Option<Armed>>,
}

struct Armed {
    id: DocumentId,
    entered: Sender<()>,
    release: Receiver<()>,
}

/// The test's side of an armed gate.
struct GateControl {
    entered: Receiver<()>,
    release: Sender<()>,
}

impl Gate {
    fn arm(&self, id: &DocumentId) -> GateControl {
        let (entered, entered_rx) = mpsc::channel();
        let (release_tx, release) = mpsc::channel();
        let mut armed = self.armed.lock().expect("the gate lock is not poisoned");
        assert!(
            armed.is_none(),
            "a gate is armed while an earlier one has not been passed"
        );
        *armed = Some(Armed {
            id: id.clone(),
            entered,
            release,
        });
        GateControl {
            entered: entered_rx,
            release: release_tx,
        }
    }

    fn pass(&self, id: &DocumentId) {
        let armed = self
            .armed
            .lock()
            .expect("the gate lock is not poisoned")
            .take_if(|armed| armed.id == *id);
        if let Some(armed) = armed {
            armed
                .entered
                .send(())
                .expect("the test waits for the gated call to enter");
            armed
                .release
                .recv_timeout(HANG_GUARD)
                .expect("the test releases the gated call (hang guard)");
        }
    }
}

impl GateControl {
    /// Returns once the armed call has entered its gate, which it does before any storage work.
    fn wait_until_entered(&self) {
        self.entered
            .recv_timeout(HANG_GUARD)
            .expect("the gated call entered its gate (hang guard)");
    }

    /// Lets the gated call go on.
    fn release(self) {
        self.release
            .send(())
            .expect("the gated call waits to be released");
    }
}

/// The gates of one repository: the status of a document's draft, and its draft write.
#[derive(Default)]
struct Gates {
    status_gate: Gate,
    write_gate: Gate,
}

/// Delegates every call of the port to a file store, passing `draft_status` and `write_draft`
/// through their gates first. Every method is delegated, `stored_revision` included, so the double
/// changes no storage behavior.
struct GatedRepository {
    inner: FileDocumentRepository,
    gates: Arc<Gates>,
}

impl DocumentRepository for GatedRepository {
    fn allocate_id(&self) -> Result<DocumentId, StorageError> {
        self.inner.allocate_id()
    }

    fn list_ids(&self) -> Result<Vec<DocumentId>, StorageError> {
        self.inner.list_ids()
    }

    fn load(&self, id: &DocumentId) -> Result<Option<Document>, StorageError> {
        self.inner.load(id)
    }

    fn stored_revision(&self, id: &DocumentId) -> Result<Option<u64>, StorageError> {
        self.inner.stored_revision(id)
    }

    fn save(&self, document: &Document, base_revision: Option<u64>) -> Result<(), StorageError> {
        self.inner.save(document, base_revision)
    }

    fn draft_status(&self, id: &DocumentId) -> Result<DraftStatus, StorageError> {
        self.gates.status_gate.pass(id);
        self.inner.draft_status(id)
    }

    fn write_draft(&self, draft: &Draft) -> Result<(), StorageError> {
        self.gates.write_gate.pass(draft.document.id());
        self.inner.write_draft(draft)
    }

    fn claim_draft(&self, id: &DocumentId) -> Result<Option<Result<Draft, String>>, StorageError> {
        self.inner.claim_draft(id)
    }

    fn prepare_draft_for_recovery(
        &self,
        id: &DocumentId,
        target: DraftRecoveryTarget,
    ) -> Result<(), StorageError> {
        self.inner.prepare_draft_for_recovery(id, target)
    }

    fn release_draft(&self, id: &DocumentId) {
        self.inner.release_draft(id);
    }

    fn remove_draft(&self, id: &DocumentId) -> Result<(), StorageError> {
        self.inner.remove_draft(id)
    }

    fn saved_at_ms(&self, id: &DocumentId) -> Result<Option<u64>, StorageError> {
        self.inner.saved_at_ms(id)
    }

    fn trash(&self, id: &DocumentId, base_revision: u64) -> Result<(), StorageError> {
        self.inner.trash(id, base_revision)
    }

    fn restore(&self, id: &DocumentId) -> Result<(), StorageError> {
        self.inner.restore(id)
    }

    fn list_trash(&self) -> Result<Vec<TrashEntry>, StorageError> {
        self.inner.list_trash()
    }

    fn load_trashed(&self, id: &DocumentId) -> Result<Option<Document>, StorageError> {
        self.inner.load_trashed(id)
    }
}

/// An application over the file store in `directory`, with the gates it shares with the test.
fn app_over(directory: &Path) -> (Arc<App>, Arc<Gates>) {
    let gates = Arc::new(Gates::default());
    let repository = GatedRepository {
        inner: FileDocumentRepository::new(directory),
        gates: Arc::clone(&gates),
    };
    (
        Arc::new(Application::new(LocalCapabilities, repository)),
        gates,
    )
}

fn document_id(id: &str) -> DocumentId {
    DocumentId::parse(id).expect("a document identifier the store issued")
}

/// The reply of a request that is expected to succeed.
fn answered<T>(reply: Result<T, ApplicationError>, request: &str) -> T {
    reply.unwrap_or_else(|error| panic!("{request} failed: {error:?}"))
}

/// The state, draft revision and base revision that a response reports for its document's
/// recovery, or `None` when it reports no recovery.
fn recovery_of(response: &DocumentResponse) -> Option<(RecoveryState, Option<u64>, Option<u64>)> {
    response
        .recovery
        .as_ref()
        .map(|recovery| (recovery.state, recovery.revision, recovery.base_revision))
}

fn create(app: &App, title: &str) -> DocumentResponse {
    answered(
        app.create_document(None, Some(title)),
        "creating a document",
    )
}

/// The commands of one transaction that adds a carbon atom at horizontal position `x`.
fn carbon_at(x: i64) -> Vec<CommandDto> {
    serde_json::from_value(json!([
        {"op": "add_atom", "element": "C", "position": {"x": x, "y": 0}}
    ]))
    .expect("the carbon command is well formed")
}

/// Adds one carbon atom to the document, which must be at `expected_revision`.
fn add_carbon(app: &App, id: &str, expected_revision: u64, x: i64) -> DocumentResponse {
    answered(
        app.apply_transaction(id, None, expected_revision, carbon_at(x)),
        "an edit",
    )
}

/// Autosaves `id` on its own thread, as a client's request does.
fn autosave_on_thread(
    app: &Arc<App>,
    id: &str,
) -> JoinHandle<Result<DocumentResponse, ApplicationError>> {
    let (app, id) = (Arc::clone(app), id.to_owned());
    thread::spawn(move || app.autosave(&id, None))
}

/// The reply of an autosave that ran on its own thread.
fn finish_autosave(
    autosave: JoinHandle<Result<DocumentResponse, ApplicationError>>,
) -> Result<DocumentResponse, ApplicationError> {
    autosave.join().expect("the autosave thread did not panic")
}

/// Runs `work` on its own thread. The returned channel delivers what `work` returns.
fn answer_on_thread<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Receiver<T> {
    let (answer, delivered) = mpsc::channel();
    thread::spawn(move || {
        // The test stops listening only after it has failed, so nobody needs this answer then.
        let _ = answer.send(work());
    });
    delivered
}

/// The answer that `answer_on_thread` delivers, which must arrive within `HANG_GUARD`.
fn received<T>(answer: &Receiver<T>, what: &str) -> T {
    answer
        .recv_timeout(HANG_GUARD)
        .unwrap_or_else(|error| panic!("{what} did not answer (hang guard): {error:?}"))
}

#[test]
fn other_documents_are_answered_while_a_recovery_lookup_is_stalled() {
    let directory = tempfile::tempdir().unwrap();
    let (app, gates) = app_over(directory.path());
    let x = create(&app, "stalled");
    let y = create(&app, "other");
    let x_edited = add_carbon(&app, &x.document.id, x.document.revision, 0);
    let y_edited = add_carbon(&app, &y.document.id, y.document.revision, 0);

    let status = gates.status_gate.arm(&document_id(&x.document.id));
    let autosave = autosave_on_thread(&app, &x.document.id);
    status.wait_until_entered();

    // Requests for Y, a create, a list and X, all made while X's lookup is stalled.
    let answers = answer_on_thread({
        let app = Arc::clone(&app);
        let (x_id, y_id) = (x.document.id.clone(), y.document.id.clone());
        let y_revision = y_edited.document.revision;
        move || {
            (
                app.document(&y_id),
                app.apply_transaction(&y_id, None, y_revision, carbon_at(20)),
                app.create_document(None, Some("created meanwhile")),
                app.list_documents(&ListQuery::default()),
                app.document(&x_id),
            )
        }
    });
    let (document_y, edit_y, created, listed, document_x) =
        received(&answers, "the requests made while the lookup is stalled");
    answered(document_y, "document(Y)");
    let edit_y = answered(edit_y, "the edit of Y");
    answered(created, "the create");
    let listed = answered(listed, "the list");
    let document_x = answered(document_x, "document(X)");

    assert_eq!(
        edit_y.document.revision,
        y_edited.document.revision + 1,
        "the edit of Y applies while X's lookup is stalled"
    );
    assert_eq!(
        listed.documents.len(),
        3,
        "the list names X, Y and the document created meanwhile"
    );
    assert_eq!(
        document_x.session.draft_revision, None,
        "X has no draft yet, because its write waits behind the lookup"
    );

    status.release();
    let autosaved = answered(finish_autosave(autosave), "the autosave of X");
    assert_eq!(
        autosaved.session.draft_revision,
        Some(x_edited.document.revision),
        "the autosave drafts X at the revision it had when its lookup began"
    );
}

#[test]
fn an_edit_made_during_a_recovery_lookup_is_the_one_drafted() {
    let directory = tempfile::tempdir().unwrap();
    let (app, gates) = app_over(directory.path());
    let created = create(&app, "edited");
    let id = created.document.id.clone();
    let edited = add_carbon(&app, &id, created.document.revision, 0);
    let recorded = edited.document.revision;

    let status = gates.status_gate.arm(&document_id(&id));
    let autosave = autosave_on_thread(&app, &id);
    status.wait_until_entered();

    let edit_answer = answer_on_thread({
        let (app, id) = (Arc::clone(&app), id.clone());
        move || app.apply_transaction(&id, None, recorded, carbon_at(20))
    });
    let edit = answered(
        received(&edit_answer, "the edit made during the lookup"),
        "the edit made during the lookup",
    );
    assert_eq!(
        edit.document.revision,
        recorded + 1,
        "the edit moves X on from revision {recorded} while its lookup is stalled"
    );
    status.release();

    let autosaved = answered(finish_autosave(autosave), "the autosave of X");
    assert_eq!(
        autosaved.session.draft_revision,
        Some(recorded + 1),
        "the draft holds the edit made during the lookup, not the revision the lookup began with"
    );
    let viewed = answered(app.document(&id), "document(X) after the draft");
    assert_eq!(
        viewed.session.draft_revision,
        Some(recorded + 1),
        "the session keeps reporting the draft of the edit"
    );
}

#[test]
fn a_response_during_a_draft_write_reports_the_lookup_it_made() {
    let directory = tempfile::tempdir().unwrap();
    // Process A drafts its unsaved work and then ends, so the draft is abandoned.
    let (id, created_revision, draft_revision) = {
        let (app, _gates) = app_over(directory.path());
        let created = create(&app, "abandoned");
        let id = created.document.id.clone();
        let once = add_carbon(&app, &id, created.document.revision, 0);
        let twice = add_carbon(&app, &id, once.document.revision, 20);
        let drafted = answered(app.autosave(&id, None), "the autosave in process A");
        assert_eq!(
            drafted.session.draft_revision,
            Some(twice.document.revision),
            "process A drafts its working copy"
        );
        (id, created.document.revision, twice.document.revision)
    };

    // Process B finds the abandoned draft, then makes an edit of its own.
    let (app, gates) = app_over(directory.path());
    let found = answered(app.document(&id), "document(X) in process B");
    assert_eq!(
        recovery_of(&found),
        Some((
            RecoveryState::Recoverable,
            Some(draft_revision),
            Some(created_revision),
        )),
        "process B finds the draft recoverable, at its revision, on its base"
    );
    let edited = add_carbon(&app, &id, found.document.revision, 40);
    assert_ne!(
        edited.document.revision, draft_revision,
        "B's revision must differ from the draft's, or the check below proves nothing"
    );

    let write = gates.write_gate.arm(&document_id(&id));
    let autosave = autosave_on_thread(&app, &id);
    write.wait_until_entered();

    let during_answer = answer_on_thread({
        let (app, id) = (Arc::clone(&app), id.clone());
        move || app.document(&id)
    });
    let during = answered(
        received(&during_answer, "document(X) during the draft write"),
        "document(X) during the draft write",
    );
    assert_eq!(
        recovery_of(&during),
        Some((
            RecoveryState::Recoverable,
            Some(draft_revision),
            Some(created_revision),
        )),
        "during the write, the response reports the recovery state its lookup found"
    );

    write.release();
    let refused = finish_autosave(autosave).expect_err("the abandoned draft refuses the write");
    assert_eq!(
        refused.code.as_str(),
        "recovery_pending",
        "the write is refused because an abandoned draft is waiting"
    );
}
