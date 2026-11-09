//! How many computations run at once. A computation takes a [`Permit`] before it runs and gives
//! it back when it ends. While every permit is taken it waits, asking its own interrupt at each
//! poll, so its time limit and any cancel still end the wait.
//!
//! The gate is not first-in, first-out, and a waiter that stops holds nothing. A computation
//! that gets its permit at once runs exactly as it would without the gate.

use std::num::NonZeroUsize;
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

use chemifree_domain::budget::{CancelFlag, Interrupt, Interrupted};

/// How long a waiter sleeps before it asks its interrupt again. A disconnect's [`CancelFlag`]
/// wakes nobody, so waiters poll; [`ComputeGate::close`] wakes them at once.
const WAIT_SLICE: Duration = Duration::from_millis(10);

/// The computations that may run at once, counted in [`Permit`]s, and the flag that stops them
/// when the host shuts down.
pub(crate) struct ComputeGate {
    /// Permits in use.
    state: Mutex<usize>,
    /// Signalled whenever a permit is given back.
    freed: Condvar,
    cap: usize,
    shutdown: CancelFlag,
}

impl ComputeGate {
    /// The largest cap a host may set, so a configured value cannot allow more computations at
    /// once than this.
    const MAX: usize = 64;

    /// The largest default cap, however many cores the machine has.
    const DEFAULT_MAX: usize = 4;

    /// A gate that lets `cap` computations run at once. A `cap` outside `1..=MAX` is clamped
    /// into that range, because zero would leave every computation waiting for a permit that
    /// never comes.
    #[must_use]
    pub(crate) fn new(cap: usize) -> Self {
        Self {
            state: Mutex::new(0),
            freed: Condvar::new(),
            cap: cap.clamp(1, Self::MAX),
            shutdown: CancelFlag::new(),
        }
    }

    /// The default cap: the machine's available parallelism, at most [`Self::DEFAULT_MAX`], and
    /// one when the parallelism cannot be read.
    #[must_use]
    pub(crate) fn machine_default() -> usize {
        thread::available_parallelism()
            .map_or(1, NonZeroUsize::get)
            .min(Self::DEFAULT_MAX)
    }

    /// The flag that [`Self::close`] sets. Each computation's budget asks it after the caller's
    /// own interrupt, so a closed gate stops running computations at their next checkpoint.
    #[must_use]
    pub(crate) fn shutdown_flag(&self) -> &CancelFlag {
        &self.shutdown
    }

    /// Ends every wait and refuses every later [`Self::enter`] with `Interrupted::Cancelled`.
    /// Running computations stop at their next checkpoint, because their budgets ask
    /// [`Self::shutdown_flag`].
    pub(crate) fn close(&self) {
        // The flag is set under the lock. A waiter checks the flag under the same lock before it
        // waits, so none can miss this wake-up.
        let _state = self.lock();
        self.shutdown.cancel();
        self.freed.notify_all();
    }

    /// Takes a permit, waiting while every permit is taken. While it waits, `interrupt` is asked
    /// at each poll. When a permit is free, `interrupt` is not asked at all.
    ///
    /// # Errors
    ///
    /// `Interrupted::Cancelled` once the gate is closed, or when `interrupt` reports a cancel
    /// during the wait. `Interrupted::TimeLimit` when `interrupt` reports the deadline during the
    /// wait.
    pub(crate) fn enter(&self, interrupt: &dyn Interrupt) -> Result<Permit<'_>, Interrupted> {
        let mut running = self.lock();
        loop {
            self.shutdown.check()?;
            if *running < self.cap {
                *running += 1;
                return Ok(Permit { gate: self });
            }
            // Asked under the lock, so it must be cheap, as `Interrupt::check` requires.
            interrupt.check()?;
            let (guard, _) = self
                .freed
                .wait_timeout(running, WAIT_SLICE)
                .unwrap_or_else(PoisonError::into_inner);
            running = guard;
        }
    }

    /// The count changes in single steps under this lock, so a panic while the lock is held
    /// cannot leave it half-updated. The guard is recovered from a poisoned lock because a permit
    /// is given back while its holder unwinds, and a second panic during unwinding aborts.
    fn lock(&self) -> MutexGuard<'_, usize> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A permit to run one computation, given back when it is dropped.
#[must_use = "the permit is given back when this is dropped"]
pub(crate) struct Permit<'a> {
    gate: &'a ComputeGate,
}

impl Drop for Permit<'_> {
    fn drop(&mut self) {
        // Only `enter` counts up, once per permit, so the count cannot go below zero here.
        *self.gate.lock() -= 1;
        self.gate.freed.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chemifree_domain::budget::NEVER;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;

    /// Long enough for any wait that a correct gate ends; short enough to end a failing run. A
    /// wait that reaches it fails its test by the assertion that names it.
    const WAIT: Duration = Duration::from_secs(30);

    /// Refuses with `TimeLimit` when it is asked. A test in which the gate must not ask fails at
    /// its outcome, and a waiter on a full gate returns this refusal as its outcome.
    struct RefusesWhenAsked;

    impl Interrupt for RefusesWhenAsked {
        fn check(&self) -> Result<(), Interrupted> {
            Err(Interrupted::TimeLimit)
        }
    }

    /// Reports its first check on `asked` and answers `first` to it, then answers `later` to every
    /// later check. The gate asks under its lock, so a check only reports and answers.
    struct Latch {
        asked: mpsc::Sender<()>,
        checks: AtomicUsize,
        first: Result<(), Interrupted>,
        later: Result<(), Interrupted>,
    }

    impl Latch {
        fn new(
            asked: mpsc::Sender<()>,
            first: Result<(), Interrupted>,
            later: Result<(), Interrupted>,
        ) -> Self {
            Self {
                asked,
                checks: AtomicUsize::new(0),
                first,
                later,
            }
        }
    }

    impl Interrupt for Latch {
        fn check(&self) -> Result<(), Interrupted> {
            if self.checks.fetch_add(1, Ordering::SeqCst) == 0 {
                // The send fails only after the test has stopped listening, so nothing is lost.
                self.asked.send(()).ok();
                self.first
            } else {
                self.later
            }
        }
    }

    /// Enters `gate` on a new thread with `interrupt` and returns the channel that carries the
    /// outcome. The thread is not joined: a broken gate leaves it waiting, and the test then ends
    /// at its `WAIT` bound instead of hanging.
    fn spawn_waiter<I>(
        gate: &Arc<ComputeGate>,
        interrupt: I,
    ) -> mpsc::Receiver<Result<(), Interrupted>>
    where
        I: Interrupt + Send + 'static,
    {
        let (outcome, outcome_rx) = mpsc::channel();
        let waiter = Arc::clone(gate);
        thread::spawn(move || {
            // A permit the waiter gets is given back at once: the outcome is all a test reads.
            outcome
                .send(waiter.enter(&interrupt).map(|_permit| ()))
                .ok();
        });
        outcome_rx
    }

    /// Enters `gate` on a new thread with an interrupt that refuses when asked, and returns the
    /// outcome. On a full gate the outcome is that refusal. The wait is bounded by `WAIT`, so a
    /// gate that never asks fails the test instead of hanging it.
    fn enter_refusing(gate: &Arc<ComputeGate>) -> Result<(), Interrupted> {
        spawn_waiter(gate, RefusesWhenAsked)
            .recv_timeout(WAIT)
            .expect("a full gate must answer within WAIT")
    }

    #[test]
    fn a_free_slot_is_entered_without_asking_the_interrupt() {
        let gate = ComputeGate::new(2);
        let _first = gate
            .enter(&RefusesWhenAsked)
            .expect("a free slot must be entered without asking the interrupt");
        let _second = gate
            .enter(&RefusesWhenAsked)
            .expect("the second free slot must be entered without asking the interrupt");
    }

    #[test]
    fn a_full_gate_asks_the_waiters_interrupt() {
        let gate = Arc::new(ComputeGate::new(1));
        let _held = gate
            .enter(&RefusesWhenAsked)
            .expect("the only slot is free");
        let (asked, asked_rx) = mpsc::channel();
        let outcomes = spawn_waiter(
            &gate,
            Latch::new(
                asked,
                Err(Interrupted::TimeLimit),
                Err(Interrupted::TimeLimit),
            ),
        );
        let outcome = outcomes
            .recv_timeout(WAIT)
            .expect("the waiter must answer within WAIT");
        assert_eq!(
            outcome,
            Err(Interrupted::TimeLimit),
            "a waiter on a full gate must answer with its own interrupt's refusal"
        );
        assert!(
            asked_rx.try_recv().is_ok(),
            "a waiter on a full gate must ask its interrupt before it answers"
        );
    }

    #[test]
    fn dropping_a_permit_lets_the_waiter_in() {
        let gate = Arc::new(ComputeGate::new(1));
        let held = gate
            .enter(&RefusesWhenAsked)
            .expect("the only slot is free");
        let (asked, asked_rx) = mpsc::channel();
        // Every check answers Ok, so the waiter keeps waiting until a permit is given back.
        let outcomes = spawn_waiter(&gate, Latch::new(asked, Ok(()), Ok(())));
        asked_rx
            .recv_timeout(WAIT)
            .expect("the waiter must ask its interrupt before it finishes, within WAIT");
        drop(held);
        let outcome = outcomes
            .recv_timeout(WAIT)
            .expect("the waiter must be let in within WAIT once a permit is given back");
        assert_eq!(
            outcome,
            Ok(()),
            "the waiter must take the permit that was given back"
        );
    }

    #[test]
    fn a_cancelled_waiter_leaves_and_takes_no_slot() {
        let gate = Arc::new(ComputeGate::new(1));
        let held = gate
            .enter(&RefusesWhenAsked)
            .expect("the only slot is free");
        let (asked, asked_rx) = mpsc::channel();
        // The first check lets the waiter wait; its second check, after one wait slice, cancels.
        let outcomes = spawn_waiter(
            &gate,
            Latch::new(asked, Ok(()), Err(Interrupted::Cancelled)),
        );
        asked_rx
            .recv_timeout(WAIT)
            .expect("the waiter must ask its interrupt before it finishes, within WAIT");
        let outcome = outcomes
            .recv_timeout(WAIT)
            .expect("the cancelled waiter must answer within WAIT");
        assert_eq!(
            outcome,
            Err(Interrupted::Cancelled),
            "a waiter whose interrupt cancels must leave with Cancelled"
        );
        drop(held);
        let _next = gate
            .enter(&RefusesWhenAsked)
            .expect("a waiter that left took no slot, so the slot given back is free");
        assert_eq!(
            enter_refusing(&gate),
            Err(Interrupted::TimeLimit),
            "the gate has one slot, so a second computation waits and its interrupt is asked"
        );
    }

    /// Signals that it was asked, then panics. The gate asks it under its lock while the caller
    /// holds a permit, so the panic poisons the lock with that permit still held.
    struct PanicsWhenAsked {
        asked: mpsc::Sender<()>,
    }

    impl Interrupt for PanicsWhenAsked {
        fn check(&self) -> Result<(), Interrupted> {
            self.asked.send(()).ok();
            panic!("the holder's interrupt panics on purpose");
        }
    }

    #[test]
    fn a_panic_while_holding_a_permit_releases_it() {
        let gate = Arc::new(ComputeGate::new(1));
        let (asked, asked_rx) = mpsc::channel();
        let holder = Arc::clone(&gate);
        let handle = thread::spawn(move || {
            let _permit = holder
                .enter(&RefusesWhenAsked)
                .expect("the only slot is free");
            // The gate is full, so it asks the interrupt under its lock and the interrupt panics.
            // `_permit` is given back while this thread unwinds, on a lock that the panic poisoned.
            let _second = holder
                .enter(&PanicsWhenAsked { asked })
                .expect("the interrupt panics before the gate answers");
        });
        asked_rx
            .recv_timeout(WAIT)
            .expect("the holder must ask its interrupt before it finishes, within WAIT");
        let payload = handle
            .join()
            .expect_err("the holder must panic while it holds its permit");
        assert_eq!(
            payload.downcast_ref::<&str>(),
            Some(&"the holder's interrupt panics on purpose"),
            "the holder must panic in its interrupt, not somewhere else"
        );
        let _next = gate
            .enter(&RefusesWhenAsked)
            .expect("a permit given back while its holder unwinds must free its slot");
    }

    #[test]
    fn close_wakes_every_waiter_with_cancelled_and_refuses_new_entries_even_when_a_slot_is_free() {
        const WAITERS: usize = 3;
        let gate = Arc::new(ComputeGate::new(1));
        let held = gate
            .enter(&RefusesWhenAsked)
            .expect("the only slot is free");
        let (asked, asked_rx) = mpsc::channel();
        // Every check answers Ok, so only the gate's own close can end a wait.
        let outcomes: Vec<_> = (0..WAITERS)
            .map(|_| spawn_waiter(&gate, Latch::new(asked.clone(), Ok(()), Ok(()))))
            .collect();
        for _ in 0..WAITERS {
            asked_rx
                .recv_timeout(WAIT)
                .expect("every waiter must ask its interrupt before it finishes, within WAIT");
        }
        gate.close();
        for outcome_rx in &outcomes {
            let outcome = outcome_rx
                .recv_timeout(WAIT)
                .expect("every waiter must leave within WAIT once the gate closes");
            assert_eq!(
                outcome,
                Err(Interrupted::Cancelled),
                "a waiter that the gate closes on must leave with Cancelled"
            );
        }
        drop(held);
        assert_eq!(
            gate.enter(&RefusesWhenAsked).map(|_permit| ()),
            Err(Interrupted::Cancelled),
            "a closed gate must refuse with Cancelled even when a slot is free, without asking"
        );
    }

    #[test]
    fn a_cap_of_zero_is_one_and_a_cap_above_the_maximum_is_the_maximum() {
        // Zero is raised to one: the one slot is taken at once, and a second computation waits.
        let zero = Arc::new(ComputeGate::new(0));
        let worker = Arc::clone(&zero);
        let (admitted_tx, admitted_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        thread::spawn(move || {
            match worker.enter(&NEVER) {
                Ok(_permit) => {
                    admitted_tx.send(Ok(())).ok();
                    // Holds the one slot until the test has checked that no second slot exists.
                    release_rx.recv().ok();
                }
                Err(interrupted) => {
                    admitted_tx.send(Err(interrupted)).ok();
                }
            }
        });
        let admitted = admitted_rx
            .recv_timeout(WAIT)
            .expect("a cap of zero must admit one computation within WAIT");
        assert_eq!(admitted, Ok(()), "a cap of zero must be raised to one");
        assert_eq!(
            enter_refusing(&zero),
            Err(Interrupted::TimeLimit),
            "a cap of one must make a second computation wait, so its interrupt is asked"
        );
        release_tx.send(()).ok();

        // Above the maximum is the maximum: MAX computations are admitted, and the next one waits.
        let high = Arc::new(ComputeGate::new(ComputeGate::MAX + 1));
        let permits: Vec<_> = (0..ComputeGate::MAX)
            .map(|_| {
                high.enter(&RefusesWhenAsked)
                    .expect("every one of the MAX slots must be free")
            })
            .collect();
        assert_eq!(
            enter_refusing(&high),
            Err(Interrupted::TimeLimit),
            "a cap above the maximum must admit exactly MAX computations"
        );
        drop(permits);
    }
}
