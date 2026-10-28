//! The service's own drafting, run in the background by a long-running host.

use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::{Application, ApplicationError, CapabilityCatalog, DocumentRepository, DraftAttempt};

/// Writes recovery drafts in the background, so unsaved work is drafted whether or not a client
/// autosaves. [`DraftWriter::finish`] stops it and drafts whatever is still undrafted, as a
/// host does before it exits.
pub struct DraftWriter<C, R> {
    application: Arc<Application<C, R>>,
    /// Dropping it stops the loop.
    stop: mpsc::Sender<()>,
    /// The loop, which returns the draft writes still in progress when it stops.
    thread: JoinHandle<Vec<DraftAttempt>>,
}

impl<C, R> DraftWriter<C, R>
where
    C: CapabilityCatalog + 'static,
    R: DocumentRepository + 'static,
{
    /// Starts looking for due drafts every `tick`; each one is written on its own thread (see
    /// [`Application::start_due_drafts`]).
    pub fn start(application: Arc<Application<C, R>>, tick: Duration) -> Self {
        let (stop, stopped) = mpsc::channel();
        let thread = {
            let application = Arc::clone(&application);
            thread::spawn(move || {
                let mut running: Vec<DraftAttempt> = Vec::new();
                while let Err(RecvTimeoutError::Timeout) = stopped.recv_timeout(tick) {
                    running.retain(|attempt| !attempt.is_finished());
                    let started = application.start_due_drafts(Instant::now());
                    running.extend(started.into_iter().map(|(_, attempt)| attempt));
                }
                running
            })
        };
        Self {
            application,
            stop,
            thread,
        }
    }

    /// Stops looking for due drafts, waits for the draft writes still in progress, then drafts
    /// every working copy with undrafted changes. Returns the documents that could not be
    /// drafted, with why.
    ///
    /// Waiting first means no write is cut off when the host exits, and the last pass does not
    /// compete with a slow write for its document's turn; a write that never ends keeps the
    /// host from stopping, as it would keep that document's work undrafted.
    pub fn finish(self) -> Vec<(String, ApplicationError)> {
        drop(self.stop);
        // A panic in the loop has already been reported on standard error, and its writes in
        // progress cannot be waited for; drafting below still runs.
        let running = self.thread.join().unwrap_or_default();
        for attempt in running {
            // The outcome is kept on the document's session; a panic was reported.
            let _ended = attempt.join();
        }
        self.application.draft_all()
    }
}
