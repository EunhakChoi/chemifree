//! Runs cases against the shared `Application` and reports passed, failed, and not run.

use std::collections::HashMap;
use std::path::Path;

use chemifree_application::{Application, ApplicationError};
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde::Serialize;
use serde_json::Value;
use tempfile::TempDir;

use crate::case::{Axis, Body, Call, Case, Expectation, Kind, Pending, Step, Suite};
use crate::expect;

/// The report format's version; bump it when its shape changes.
pub const REPORT_VERSION: u32 = 1;

type LocalApplication = Application<LocalCapabilities, FileDocumentRepository>;

/// A suite run. It holds no times or paths, so two runs of the same suite and product give
/// the same report.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Report {
    pub report_version: u32,
    pub suite_version: String,
    pub counts: Counts,
    pub results: Vec<CaseResult>,
}

/// Outcomes per kind, so expected refusals are never added to results.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct Counts {
    pub accept: KindCounts,
    pub reject: KindCounts,
    pub total: KindCounts,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct KindCounts {
    pub passed: usize,
    pub failed: usize,
    pub not_run: usize,
}

impl KindCounts {
    fn add(&mut self, outcome: Outcome) {
        match outcome {
            Outcome::Passed => self.passed += 1,
            Outcome::Failed => self.failed += 1,
            Outcome::NotRun => self.not_run += 1,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Passed,
    Failed,
    NotRun,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CaseResult {
    pub id: String,
    pub title: String,
    pub requirements: Vec<String>,
    pub axis: Axis,
    pub kind: Kind,
    pub outcome: Outcome,
    /// Set for a case that cannot run yet.
    pub pending: Option<Pending>,
    /// Set for a failed case.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<Failure>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Failure {
    /// The failing step's index, or `null` when the case could not start.
    pub step: Option<usize>,
    pub message: String,
}

impl Report {
    /// True when no case failed; not-run cases do not fail a run.
    pub fn succeeded(&self) -> bool {
        self.counts.total.failed == 0
    }
}

/// Runs every case in order, each on a fresh temporary data directory.
pub fn run_suite(suite: &Suite) -> Report {
    let results: Vec<CaseResult> = suite.cases.iter().map(run_case).collect();
    let mut counts = Counts::default();
    for result in &results {
        match result.kind {
            Kind::Accept => counts.accept.add(result.outcome),
            Kind::Reject => counts.reject.add(result.outcome),
        }
        counts.total.add(result.outcome);
    }
    Report {
        report_version: REPORT_VERSION,
        suite_version: suite.version.clone(),
        counts,
        results,
    }
}

pub fn run_case(case: &Case) -> CaseResult {
    let (outcome, pending, failure) = match &case.body {
        Body::Pending(pending) => (Outcome::NotRun, Some(pending.clone()), None),
        Body::Runnable(steps) => match run_steps(steps) {
            Ok(()) => (Outcome::Passed, None, None),
            Err(failure) => (Outcome::Failed, None, Some(failure)),
        },
    };
    CaseResult {
        id: case.id.clone(),
        title: case.title.clone(),
        requirements: case.requirements.clone(),
        axis: case.axis,
        kind: case.kind,
        outcome,
        pending,
        failure,
    }
}

fn run_steps(steps: &[Step]) -> Result<(), Failure> {
    let directory = TempDir::new().map_err(|error| Failure {
        step: None,
        message: format!("could not create a data directory: {error}"),
    })?;
    let mut application = open(directory.path());
    let mut documents: HashMap<&str, String> = HashMap::new();
    for (index, step) in steps.iter().enumerate() {
        let fail = |message: String| Failure {
            step: Some(index),
            message: format!("{}: {message}", step.call.name()),
        };
        if let Call::Restart {} = step.call {
            // Only storage survives: the old application, with its sessions, is dropped first.
            drop(application);
            application = open(directory.path());
        }
        let reply = call(&application, &step.call, &documents).map_err(&fail)?;
        if let (Call::Create { doc, .. }, Ok(created)) = (&step.call, &reply) {
            let id = created["document"]["id"]
                .as_str()
                .ok_or_else(|| fail("the reply has no document id".to_owned()))?;
            documents.insert(doc, id.to_owned());
        }
        check(&step.expectation, &reply).map_err(fail)?;
    }
    Ok(())
}

fn open(directory: &Path) -> LocalApplication {
    Application::new(LocalCapabilities, FileDocumentRepository::new(directory))
}

/// The v1 JSON reply of a call, or its v1 error envelope. The outer error is a problem of the
/// case itself, such as a document whose `create` failed.
fn call(
    application: &LocalApplication,
    call: &Call,
    documents: &HashMap<&str, String>,
) -> Result<Result<Value, Value>, String> {
    let id = |doc: &str| {
        documents
            .get(doc)
            .map(String::as_str)
            .ok_or_else(|| format!("document {doc:?} was not created"))
    };
    let reply = match call {
        Call::Create {
            title, request_id, ..
        } => to_json(application.create_document(request_id.as_deref(), title.as_deref())),
        Call::Get { doc } => to_json(application.document(id(doc)?)),
        Call::Transact {
            doc,
            expected_revision,
            commands,
            request_id,
        } => to_json(application.apply_transaction(
            id(doc)?,
            request_id.as_deref(),
            *expected_revision,
            commands.clone(),
        )),
        Call::Undo {
            doc,
            expected_revision,
            request_id,
        } => to_json(application.undo(id(doc)?, request_id.as_deref(), *expected_revision)),
        Call::Redo {
            doc,
            expected_revision,
            request_id,
        } => to_json(application.redo(id(doc)?, request_id.as_deref(), *expected_revision)),
        Call::Save {
            doc,
            expected_revision,
            request_id,
        } => to_json(application.save(id(doc)?, request_id.as_deref(), *expected_revision)),
        Call::Autosave { doc, request_id } => {
            to_json(application.autosave(id(doc)?, request_id.as_deref()))
        }
        Call::Recover { doc, request_id } => {
            to_json(application.recover(id(doc)?, request_id.as_deref()))
        }
        Call::Close {
            doc,
            discard_unsaved,
            expected_revision,
            request_id,
        } => to_json(application.close(
            id(doc)?,
            request_id.as_deref(),
            *discard_unsaved,
            *expected_revision,
        )),
        Call::Capabilities {} => Ok(json(&application.capabilities())),
        Call::Restart {} => Ok(Value::Object(serde_json::Map::new())),
    };
    Ok(reply)
}

fn to_json<T: Serialize>(result: Result<T, ApplicationError>) -> Result<Value, Value> {
    result
        .map(|reply| json(&reply))
        .map_err(|error| json(&error.to_response()))
}

fn json<T: Serialize>(value: &T) -> Value {
    serde_json::to_value(value).expect("v1 replies serialize to JSON")
}

fn check(expectation: &Expectation, reply: &Result<Value, Value>) -> Result<(), String> {
    match (expectation, reply) {
        (Expectation::Success, Ok(_)) => Ok(()),
        (Expectation::Reply(expected), Ok(actual)) => {
            expect::matches(expected, actual).map_err(|mismatch| mismatch.to_string())
        }
        (Expectation::Error(expected), Err(actual)) => expect::matches(expected, actual)
            .map_err(|mismatch| format!("error envelope {mismatch}")),
        (Expectation::Success | Expectation::Reply(_), Err(error)) => {
            Err(format!("expected success, got error {error}"))
        }
        (Expectation::Error(_), Ok(_)) => {
            Err("expected an error, but the call succeeded".to_owned())
        }
    }
}
