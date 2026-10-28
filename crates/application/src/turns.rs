//! Which requests are running, and whose turn it is to write each document.
//!
//! Both are kept in one small table whose lock is held only to read or change it, never
//! across storage I/O or while waiting on another lock. A repeated `request_id` is therefore
//! answered at once, even while the session table or the document's turn is held by a slow
//! write.

use std::collections::HashSet;
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use chemifree_domain::document::DocumentId;

/// How long a request waits for its document's turn before it is refused as busy.
pub const TURN_WAIT: Duration = Duration::from_secs(3);

/// A request as its `request_id` names it, within the scope where its reply is remembered.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct RequestKey {
    /// The document, for requests remembered per document; `None` for requests remembered
    /// per process.
    scope: Option<DocumentId>,
    request_id: String,
}

#[derive(Default)]
struct Table {
    running: HashSet<RequestKey>,
    /// Documents whose turn is taken.
    taken: HashSet<DocumentId>,
    /// Requests waiting for a turn.
    waiting: usize,
}

/// Why a request was not let in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Refusal {
    /// A request with the same `request_id` is still running.
    InProgress,
    /// The document's turn stayed taken for the whole wait.
    Busy,
}

#[derive(Default)]
pub struct Turns {
    table: Mutex<Table>,
    /// Signalled whenever a turn is given back.
    returned: Condvar,
}

impl Turns {
    /// The table holds only sets of keys, changed in single steps, so a panic elsewhere cannot
    /// leave it half-updated; recovering the guard is safe.
    fn lock(&self) -> MutexGuard<'_, Table> {
        self.table.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Marks the request as running until the returned guard is dropped. Requests without a
    /// `request_id` cannot be asked about again and are not tracked.
    ///
    /// # Errors
    ///
    /// `Refusal::InProgress` when a request with the same `request_id` in the same scope is
    /// still running.
    pub fn start(
        &self,
        scope: Option<&DocumentId>,
        request_id: Option<&str>,
    ) -> Result<Running<'_>, Refusal> {
        let key = request_id.map(|request_id| RequestKey {
            scope: scope.cloned(),
            request_id: request_id.to_owned(),
        });
        if let Some(key) = &key
            && !self.lock().running.insert(key.clone())
        {
            return Err(Refusal::InProgress);
        }
        Ok(Running { turns: self, key })
    }

    /// Waits up to `wait` for the document's turn, which is held until the returned guard is
    /// dropped.
    ///
    /// # Errors
    ///
    /// `Refusal::Busy` when the turn stayed taken for the whole wait.
    pub fn take(&self, document: &DocumentId, wait: Duration) -> Result<Turn<'_>, Refusal> {
        let mut table = self.lock();
        table.waiting += 1;
        let (mut table, waited) = self
            .returned
            .wait_timeout_while(table, wait, |table| table.taken.contains(document))
            .unwrap_or_else(PoisonError::into_inner);
        table.waiting -= 1;
        if waited.timed_out() && table.taken.contains(document) {
            return Err(Refusal::Busy);
        }
        table.taken.insert(document.clone());
        Ok(Turn {
            turns: self,
            document: document.clone(),
        })
    }
}

impl Turns {
    /// Whether the document's turn is taken at this moment.
    pub fn is_taken(&self, document: &DocumentId) -> bool {
        self.lock().taken.contains(document)
    }
}

/// A request marked as running.
#[must_use = "the request stops counting as running when this is dropped"]
pub struct Running<'a> {
    turns: &'a Turns,
    key: Option<RequestKey>,
}

impl Drop for Running<'_> {
    fn drop(&mut self) {
        if let Some(key) = &self.key {
            self.turns.lock().running.remove(key);
        }
    }
}

/// A document's turn.
#[must_use = "the turn is given back when this is dropped"]
pub struct Turn<'a> {
    turns: &'a Turns,
    document: DocumentId,
}

impl Drop for Turn<'_> {
    fn drop(&mut self) {
        self.turns.lock().taken.remove(&self.document);
        self.turns.returned.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::thread;
    use std::time::Instant;

    use super::*;

    fn document(name: &str) -> DocumentId {
        DocumentId::parse(name).expect("a valid test identifier")
    }

    #[test]
    fn a_request_id_still_running_is_refused_until_it_ends() {
        let turns = Turns::default();
        let id = document("doc-1");
        let running = turns.start(Some(&id), Some("save-1")).unwrap();
        assert_eq!(
            turns.start(Some(&id), Some("save-1")).err(),
            Some(Refusal::InProgress)
        );
        assert!(
            turns
                .start(Some(&document("doc-2")), Some("save-1"))
                .is_ok(),
            "the same request_id for another document is another request"
        );
        assert!(
            turns.start(Some(&id), None).is_ok() && turns.start(Some(&id), None).is_ok(),
            "requests without a request_id are not tracked"
        );
        drop(running);
        assert!(turns.start(Some(&id), Some("save-1")).is_ok());
    }

    #[test]
    fn a_taken_turn_refuses_after_the_wait_and_changes_nothing() {
        let turns = Turns::default();
        let id = document("doc-1");
        let held = turns.take(&id, TURN_WAIT).unwrap();
        let started = Instant::now();
        assert_eq!(
            turns.take(&id, Duration::from_millis(100)).err(),
            Some(Refusal::Busy)
        );
        assert!(
            started.elapsed() >= Duration::from_millis(100),
            "waited before refusing ({:?})",
            started.elapsed()
        );
        assert!(
            turns.take(&document("doc-2"), Duration::ZERO).is_ok(),
            "other documents have their own turn"
        );
        drop(held);
        assert!(
            turns.take(&id, Duration::ZERO).is_ok(),
            "the turn was given back"
        );
    }

    #[test]
    fn a_waiting_request_gets_the_turn_as_soon_as_it_is_given_back() {
        let turns = Arc::new(Turns::default());
        let id = document("doc-1");
        let held = turns.take(&id, TURN_WAIT).unwrap();
        let waiter = {
            let (turns, id) = (Arc::clone(&turns), id.clone());
            thread::spawn(move || turns.take(&id, TURN_WAIT).map(|_| Instant::now()))
        };
        // The count changes under the table lock, which the waiter gives up only while it
        // waits, so a count of one means it is waiting.
        let deadline = Instant::now() + Duration::from_secs(5);
        while turns.lock().waiting == 0 {
            assert!(Instant::now() < deadline, "the waiter started waiting");
            thread::yield_now();
        }
        let given_back = Instant::now();
        drop(held);
        let got = waiter.join().unwrap().expect("the waiter took the turn");
        assert!(
            got.duration_since(given_back) < Duration::from_secs(1),
            "woken when the turn was given back, not at the end of its wait ({:?})",
            got.duration_since(given_back)
        );
    }

    #[test]
    fn checking_a_request_id_does_not_wait_for_a_taken_turn() {
        let turns = Turns::default();
        let id = document("doc-1");
        let _running = turns.start(Some(&id), Some("save-1")).unwrap();
        let _held = turns.take(&id, TURN_WAIT).unwrap();
        let started = Instant::now();
        assert_eq!(
            turns.start(Some(&id), Some("save-1")).err(),
            Some(Refusal::InProgress)
        );
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "answered at once ({:?})",
            started.elapsed()
        );
    }
}
