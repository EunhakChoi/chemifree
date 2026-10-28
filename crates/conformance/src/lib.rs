//! A versioned conformance runner for Chemifree.
//!
//! A suite lists case files. Each case names requirement IDs, an axis, and a kind (`accept`:
//! the case shows a result; `reject`: it shows an intentional refusal), and drives the shared
//! `Application` with the same calls the HTTP and CLI adapters make, on a fresh temporary data
//! directory. A case that cannot run yet is pending: it names the roadmap item it waits for
//! and is reported as not run, never as passed.
//!
//! Passing cases show only what they check. They are not evidence of chemistry, file formats,
//! or equivalence with any other program.

mod case;
mod expect;
mod runner;

pub use case::{
    Axis, Body, Call, Case, Expectation, Kind, LoadError, Pending, Step, Suite, is_requirement_id,
    load_suite, parse_case, parse_step,
};
pub use expect::{CONTAINS, Mismatch, matches, validate};
pub use runner::{
    CaseResult, Counts, Failure, KindCounts, Outcome, REPORT_VERSION, Report, run_case, run_suite,
};
