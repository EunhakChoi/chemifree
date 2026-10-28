//! Per-document turn-taking with a bounded wait.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use chemifree_domain::document::DocumentId;

/// The longest a request waits for its turn on a document. With the storage lock wait after
/// it (at most 5 s), a request is answered within about 8 s, before the web client's 10 s
/// deadline, however many requests for the document are queued.
pub const GATE_WAIT: Duration = Duration::from_secs(3);

/// One turn at a time per document, for the operations that commit it or touch its recovery
/// draft. Waiting is bounded: a request that does not get its turn in time gives up without
/// having changed anything. A request that arrives again while it is still waiting or running
/// is told so at once instead of queuing behind itself.
#[derive(Default)]
pub(crate) struct Gates {
    gates: Mutex<HashMap<DocumentId, Arc<Gate>>>,
}

#[derive(Default)]
struct Gate {
    state: Mutex<GateState>,
    freed: Condvar,
}

#[derive(Default)]
struct GateState {
    taken: bool,
    /// `request_id`s waiting for or holding the turn.
    requests: HashSet<String>,
}

/// Why a request did not get its turn.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Refusal {
    /// Other requests held the document for longer than the wait allowed.
    Busy,
    /// The same `request_id` is already waiting or running; its outcome is not known yet.
    InProgress,
}

/// Holds a document's turn until dropped.
pub(crate) struct Turn {
    gate: Arc<Gate>,
    request_id: Option<String>,
}

impl Gates {
    /// Waits up to `wait` for the document's turn.
    pub(crate) fn enter(
        &self,
        id: &DocumentId,
        request_id: Option<&str>,
        wait: Duration,
    ) -> Result<Turn, Refusal> {
        let gate = lock(&self.gates).entry(id.clone()).or_default().clone();
        let deadline = Instant::now() + wait;
        let mut state = lock(&gate.state);
        if let Some(request_id) = request_id
            && !state.requests.insert(request_id.to_owned())
        {
            return Err(Refusal::InProgress);
        }
        while state.taken {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                if let Some(request_id) = request_id {
                    state.requests.remove(request_id);
                }
                return Err(Refusal::Busy);
            }
            state = gate
                .freed
                .wait_timeout(state, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        state.taken = true;
        drop(state);
        Ok(Turn {
            gate,
            request_id: request_id.map(str::to_owned),
        })
    }
}

impl Drop for Turn {
    fn drop(&mut self) {
        let mut state = lock(&self.gate.state);
        state.taken = false;
        if let Some(request_id) = &self.request_id {
            state.requests.remove(request_id);
        }
        drop(state);
        self.gate.freed.notify_all();
    }
}

/// The guarded values are flags, sets, and handles, valid at every step, so a panic elsewhere
/// while one was held cannot leave them inconsistent.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::thread;

    use super::*;

    fn id(name: &str) -> DocumentId {
        DocumentId::parse(name).unwrap()
    }

    #[test]
    fn a_turn_waits_for_the_previous_one_and_gives_up_at_its_deadline() {
        let gates = Gates::default();
        let first = gates.enter(&id("doc-a"), None, GATE_WAIT).unwrap();
        let started = Instant::now();
        assert_eq!(
            gates
                .enter(&id("doc-a"), Some("late"), Duration::from_millis(50))
                .err(),
            Some(Refusal::Busy),
            "the document's turn is taken"
        );
        assert!(started.elapsed() >= Duration::from_millis(50));
        assert!(
            gates.enter(&id("doc-b"), None, Duration::ZERO).is_ok(),
            "other documents are not held up"
        );
        drop(first);
        assert!(
            gates
                .enter(&id("doc-a"), Some("late"), Duration::ZERO)
                .is_ok(),
            "a request that gave up can come again"
        );
    }

    #[test]
    fn a_waiting_turn_starts_when_the_previous_one_ends() {
        let gates = Arc::new(Gates::default());
        let first = gates.enter(&id("doc-a"), None, GATE_WAIT).unwrap();
        let (sent, received) = mpsc::channel();
        let waiter = {
            let gates = Arc::clone(&gates);
            thread::spawn(move || {
                let turn = gates.enter(&id("doc-a"), None, GATE_WAIT);
                sent.send(turn.is_ok()).unwrap();
            })
        };
        assert!(
            received.recv_timeout(Duration::from_millis(100)).is_err(),
            "still waiting while the first turn is held"
        );
        drop(first);
        assert!(received.recv_timeout(GATE_WAIT).unwrap());
        waiter.join().unwrap();
    }

    #[test]
    fn a_request_arriving_again_while_it_runs_is_told_so() {
        let gates = Gates::default();
        let running = gates
            .enter(&id("doc-a"), Some("save-1"), GATE_WAIT)
            .unwrap();
        assert_eq!(
            gates.enter(&id("doc-a"), Some("save-1"), GATE_WAIT).err(),
            Some(Refusal::InProgress),
            "answered at once, without waiting for the turn"
        );
        drop(running);
        assert!(
            gates
                .enter(&id("doc-a"), Some("save-1"), Duration::ZERO)
                .is_ok(),
            "once it ended, the request can run again (or be replayed)"
        );
    }
}
