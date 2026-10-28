//! A versioned conformance runner for Chemifree.
//!
//! A suite lists case files. Each case names requirement IDs, an axis, and a kind (`accept`:
//! the case shows a result; `reject`: it shows an intentional refusal), and calls the shared
//! `Application` with the arguments the HTTP adapter passes, on a fresh temporary data
//! directory. HTTP sends a `request_id` with every write, so a write step without one sends one
//! generated from its index. A `restart` step drops the application without a normal stop, as
//! killing the process would (a normal stop drafts unsaved edits first), and opens a new one on
//! the same directory. A case that cannot run yet is pending: it names the roadmap item it
//! waits for and is reported as not run, never as passed.
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
