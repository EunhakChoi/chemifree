//! Conformance runner: loads the cases of a suite and runs them against the shared
//! application with the request types the HTTP handlers use, recording passed, failed, and not-run
//! (pending) cases separately.
//!
//! A passing case shows only what its steps compare, on this product and this run. It does
//! not show chemistry, completeness, or equivalence with any other program.

pub mod case;
pub mod expect;
pub mod json;
pub mod runner;

pub use case::{Case, Kind, Problem, StepProblem, Suite, check_case_path, load_suite};
pub use runner::{Outcome, Report, run_case, run_suite};
