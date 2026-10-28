//! When the service writes a working copy's recovery draft on its own.
//!
//! The service drafts unsaved work whether or not a client calls autosave: a quiet period after
//! the last change, bounded by a maximum wait so continuous editing cannot postpone it forever.
//! A failed draft is retried on a fixed schedule that further edits do not push back. Clients
//! that autosave sooner (the web editor) simply find nothing left to do.

use std::time::{Duration, Instant};

/// A draft is written this long after the last change…
pub const DRAFT_QUIET: Duration = Duration::from_secs(2);
/// …and at most this long after the first change it does not hold yet.
pub const DRAFT_MAX_WAIT: Duration = Duration::from_secs(10);
/// Waits before retrying a failed draft; the last repeats for as long as it keeps failing.
pub const DRAFT_RETRY: [Duration; 4] = [
    Duration::from_secs(2),
    Duration::from_secs(5),
    Duration::from_secs(15),
    Duration::from_secs(30),
];

/// The drafting state of one working copy whose unsaved work the draft may not hold yet.
#[derive(Clone, Debug, Default)]
pub(crate) struct DraftSchedule {
    pending: Option<Pending>,
    /// Set while a draft write is in progress, holding the first change noted since it
    /// started: the write holds the working copy as it was before that change.
    writing: Option<Option<Instant>>,
}

#[derive(Clone, Debug)]
struct Pending {
    /// The first change the draft does not hold.
    since: Instant,
    /// The latest change.
    last: Instant,
    failure: Option<Failure>,
}

/// A failed draft and when it is tried again.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Failure {
    pub(crate) attempts: u32,
    pub(crate) retry_at: Instant,
    pub(crate) message: String,
}

impl DraftSchedule {
    /// Notes a change at `now` that the draft does not hold.
    pub(crate) fn changed(&mut self, now: Instant) {
        if let Some(first) = &mut self.writing {
            first.get_or_insert(now);
        }
        match &mut self.pending {
            Some(pending) => pending.last = pending.last.max(now),
            None => {
                self.pending = Some(Pending {
                    since: now,
                    last: now,
                    failure: None,
                });
            }
        }
    }

    /// The draft now holds the working copy, or there is nothing unsaved to hold.
    pub(crate) fn settled(&mut self) {
        self.pending = None;
        self.writing = None;
    }

    /// Notes that a write of the working copy as it is now has started.
    pub(crate) fn writing(&mut self) {
        self.writing = Some(None);
    }

    /// Whether a write started by [`Self::writing`] has not finished yet.
    pub(crate) fn is_writing(&self) -> bool {
        self.writing.is_some()
    }

    /// The write started by [`Self::writing`] succeeded. Only changes noted since it started
    /// are left to draft, as if the first of them was the first change the draft lacks.
    pub(crate) fn written(&mut self) {
        let first_since = self.writing.take().flatten();
        self.pending = match (first_since, self.pending.take()) {
            (Some(since), Some(pending)) => Some(Pending {
                since,
                last: pending.last,
                failure: None,
            }),
            _ => None,
        };
    }

    /// Notes a failed attempt at `now`; the next one waits per [`DRAFT_RETRY`].
    pub(crate) fn failed(&mut self, now: Instant, message: String) {
        self.writing = None;
        let pending = self.pending.get_or_insert(Pending {
            since: now,
            last: now,
            failure: None,
        });
        let attempts = pending
            .failure
            .as_ref()
            .map_or(0, |failure| failure.attempts)
            + 1;
        let step = usize::try_from(attempts - 1).unwrap_or(usize::MAX);
        let wait = DRAFT_RETRY[step.min(DRAFT_RETRY.len() - 1)];
        pending.failure = Some(Failure {
            attempts,
            retry_at: now + wait,
            message,
        });
    }

    /// When the draft should be written next, if there is work it does not hold.
    pub(crate) fn due_at(&self) -> Option<Instant> {
        let pending = self.pending.as_ref()?;
        Some(match &pending.failure {
            // A retry keeps its time; edits made meanwhile do not push it back.
            Some(failure) => failure.retry_at,
            None => (pending.last + DRAFT_QUIET).min(pending.since + DRAFT_MAX_WAIT),
        })
    }

    pub(crate) fn failure(&self) -> Option<&Failure> {
        self.pending.as_ref()?.failure.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECOND: Duration = Duration::from_secs(1);

    #[test]
    fn nothing_is_due_without_unsaved_work() {
        let mut schedule = DraftSchedule::default();
        assert_eq!(schedule.due_at(), None);
        schedule.changed(Instant::now());
        schedule.settled();
        assert_eq!(schedule.due_at(), None);
    }

    #[test]
    fn a_draft_follows_a_quiet_period_after_the_last_change() {
        let start = Instant::now();
        let mut schedule = DraftSchedule::default();
        schedule.changed(start);
        assert_eq!(schedule.due_at(), Some(start + DRAFT_QUIET));
        schedule.changed(start + SECOND);
        assert_eq!(schedule.due_at(), Some(start + SECOND + DRAFT_QUIET));
    }

    #[test]
    fn continuous_changes_cannot_postpone_a_draft_past_the_maximum_wait() {
        let start = Instant::now();
        let mut schedule = DraftSchedule::default();
        for tenth in 0..200 {
            schedule.changed(start + SECOND * tenth / 10);
        }
        assert_eq!(schedule.due_at(), Some(start + DRAFT_MAX_WAIT));
    }

    #[test]
    fn a_finished_write_leaves_only_the_changes_made_while_it_ran() {
        let start = Instant::now();
        let mut schedule = DraftSchedule::default();
        schedule.changed(start);
        schedule.writing();
        assert!(schedule.is_writing());
        schedule.written();
        assert_eq!(schedule.due_at(), None, "the write held every change");

        schedule.changed(start);
        schedule.writing();
        schedule.changed(start + SECOND * 3);
        schedule.changed(start + SECOND * 4);
        schedule.written();
        assert!(!schedule.is_writing());
        assert_eq!(
            schedule.due_at(),
            Some(start + SECOND * 4 + DRAFT_QUIET),
            "the later changes are due after their own quiet period"
        );
        schedule.changed(start + SECOND * 20);
        assert_eq!(
            schedule.due_at(),
            Some(start + SECOND * 3 + DRAFT_MAX_WAIT),
            "and within the maximum wait of the first of them"
        );
    }

    #[test]
    fn failures_back_off_and_edits_do_not_push_a_retry_back() {
        let start = Instant::now();
        let mut schedule = DraftSchedule::default();
        schedule.changed(start);
        let mut at = start + DRAFT_QUIET;
        let mut waits = Vec::new();
        for _ in 0..6 {
            schedule.failed(at, "disk full".into());
            let retry = schedule.due_at().unwrap();
            waits.push(retry - at);
            schedule.changed(retry - SECOND / 2);
            assert_eq!(
                schedule.due_at(),
                Some(retry),
                "an edit does not postpone the retry"
            );
            at = retry;
        }
        assert_eq!(
            waits,
            [2, 5, 15, 30, 30, 30].map(Duration::from_secs),
            "backs off, then keeps retrying every 30 s"
        );
        assert_eq!(schedule.failure().unwrap().attempts, 6);
        schedule.settled();
        assert_eq!(schedule.failure(), None);
    }
}
