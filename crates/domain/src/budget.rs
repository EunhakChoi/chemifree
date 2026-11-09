//! Cooperative interruption of a bounded computation.
//!
//! A computation that accepts an [`Interrupt`] asks it at checkpoints whether to stop. A stop
//! is only ever an `Err(Interrupted)`: a function either returns exactly what the same call
//! without an interrupt returns, or returns `Interrupted` and nothing else. No interrupt can
//! produce a partial or different result.
//!
//! Work limits such as step counts stay the deterministic bound of a computation. A time limit
//! or a cancel is a property of the machine and the moment, so it is a separate outcome that
//! callers must not mix with the deterministic refusals.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

/// Why a computation was stopped before it finished.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Interrupted {
    /// The caller asked for the computation to stop.
    Cancelled,
    /// The time allowed for the computation ran out.
    TimeLimit,
}

/// Asked at a computation's checkpoints; `Err` stops it.
pub trait Interrupt {
    /// Whether the computation may go on. Cheap enough to call in a hot loop.
    ///
    /// # Errors
    ///
    /// Returns why the computation must stop.
    fn check(&self) -> Result<(), Interrupted>;
}

/// An [`Interrupt`] that never stops anything: the computation runs to its own limits.
#[derive(Clone, Copy, Debug, Default)]
pub struct Never;

/// The value of [`Never`].
pub const NEVER: Never = Never;

impl Interrupt for Never {
    fn check(&self) -> Result<(), Interrupted> {
        Ok(())
    }
}

/// A flag another thread sets to cancel a computation that checks it.
#[derive(Debug, Default)]
pub struct CancelFlag(AtomicBool);

impl CancelFlag {
    /// A flag that has not been set.
    #[must_use]
    pub const fn new() -> Self {
        Self(AtomicBool::new(false))
    }

    /// Asks every computation checking this flag to stop at its next checkpoint.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

impl Interrupt for CancelFlag {
    fn check(&self) -> Result<(), Interrupted> {
        if self.0.load(Ordering::Relaxed) {
            Err(Interrupted::Cancelled)
        } else {
            Ok(())
        }
    }
}

/// A caller's interrupt, an optional second interrupt set by [`Budget::also`], and a deadline.
/// Each check asks the interrupts in that order and then reads the clock, so a cancel is
/// reported as a cancel even after the deadline.
#[derive(Clone, Copy)]
pub struct Budget<'a> {
    external: &'a dyn Interrupt,
    also: Option<&'a dyn Interrupt>,
    deadline: Option<Instant>,
}

impl<'a> Budget<'a> {
    /// `external` and, when given, a `deadline` after which every check reports
    /// [`Interrupted::TimeLimit`].
    #[must_use]
    pub fn new(external: &'a dyn Interrupt, deadline: Option<Instant>) -> Self {
        Self {
            external,
            also: None,
            deadline,
        }
    }

    /// Also asks `second` at every check, after the caller's interrupt and before the deadline.
    /// Use it for a host-wide stop such as shutdown. The caller's interrupt is still asked
    /// first, once per check. A later call replaces an earlier `second`.
    #[must_use]
    pub fn also(mut self, second: &'a dyn Interrupt) -> Self {
        self.also = Some(second);
        self
    }
}

impl Interrupt for Budget<'_> {
    fn check(&self) -> Result<(), Interrupted> {
        self.external.check()?;
        if let Some(second) = self.also {
            second.check()?;
        }
        match self.deadline {
            Some(deadline) if Instant::now() >= deadline => Err(Interrupted::TimeLimit),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::time::Duration;

    #[test]
    fn never_does_not_stop() {
        assert_eq!(NEVER.check(), Ok(()));
    }

    #[test]
    fn a_set_flag_cancels_and_an_unset_one_does_not() {
        let flag = CancelFlag::new();
        assert_eq!(flag.check(), Ok(()));
        flag.cancel();
        assert_eq!(flag.check(), Err(Interrupted::Cancelled));
    }

    #[test]
    fn a_deadline_in_the_past_is_a_time_limit() {
        let now = Instant::now();
        let expired = Budget::new(&NEVER, Some(now));
        assert_eq!(expired.check(), Err(Interrupted::TimeLimit));
        let later = Budget::new(&NEVER, now.checked_add(Duration::from_secs(3600)));
        assert_eq!(later.check(), Ok(()));
        assert_eq!(Budget::new(&NEVER, None).check(), Ok(()));
    }

    #[test]
    fn a_cancel_is_reported_before_an_expired_deadline() {
        let flag = CancelFlag::new();
        flag.cancel();
        let budget = Budget::new(&flag, Some(Instant::now()));
        assert_eq!(budget.check(), Err(Interrupted::Cancelled));
    }

    /// An interrupt that always stops with the same reason.
    struct Stops(Interrupted);

    impl Interrupt for Stops {
        fn check(&self) -> Result<(), Interrupted> {
            Err(self.0)
        }
    }

    /// An interrupt that never stops and counts how many times it was asked.
    #[derive(Default)]
    struct Counting {
        asked: Cell<u32>,
    }

    impl Interrupt for Counting {
        fn check(&self) -> Result<(), Interrupted> {
            self.asked.set(self.asked.get() + 1);
            Ok(())
        }
    }

    #[test]
    fn a_second_interrupt_stops_a_budget_whose_caller_did_not() {
        let second = CancelFlag::new();
        let budget = Budget::new(&NEVER, None).also(&second);
        assert_eq!(
            budget.check(),
            Ok(()),
            "an unset second interrupt must not stop a budget whose caller never stops"
        );
        second.cancel();
        assert_eq!(
            budget.check(),
            Err(Interrupted::Cancelled),
            "a set second interrupt must stop a budget whose caller never stops"
        );
    }

    #[test]
    fn the_callers_cancel_is_reported_before_the_second_interrupt_and_the_second_before_an_expired_deadline()
     {
        // The second interrupt answers TimeLimit, unlike the caller's Cancelled, so the reported
        // outcome shows which of the two was asked first.
        let caller = CancelFlag::new();
        caller.cancel();
        let second = Stops(Interrupted::TimeLimit);
        assert_eq!(
            Budget::new(&caller, None).also(&second).check(),
            Err(Interrupted::Cancelled),
            "the caller's cancel must be reported before the second interrupt is asked"
        );

        let host_stop = CancelFlag::new();
        host_stop.cancel();
        let deadline = Some(Instant::now());
        assert_eq!(
            Budget::new(&NEVER, deadline).also(&host_stop).check(),
            Err(Interrupted::Cancelled),
            "the second interrupt must be reported before an expired deadline"
        );
        assert_eq!(
            Budget::new(&NEVER, deadline).check(),
            Err(Interrupted::TimeLimit),
            "control: the same expired deadline without a second interrupt is a time limit"
        );
    }

    #[test]
    fn a_budget_with_an_also_asks_the_callers_interrupt_exactly_once_per_check() {
        let caller = Counting::default();
        let second = CancelFlag::new();
        let budget = Budget::new(&caller, None).also(&second);
        for check in 1..=3 {
            assert_eq!(
                budget.check(),
                Ok(()),
                "check {check} with an unset second interrupt"
            );
            assert_eq!(
                caller.asked.get(),
                check,
                "check {check} must ask the caller's interrupt exactly once"
            );
        }
        second.cancel();
        assert_eq!(
            budget.check(),
            Err(Interrupted::Cancelled),
            "a set second interrupt must stop check 4"
        );
        assert_eq!(
            caller.asked.get(),
            4,
            "check 4 must ask the caller's interrupt once, before the second stops it"
        );
    }
}
