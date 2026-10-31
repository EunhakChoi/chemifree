//! Runs cases against the shared [`Application`] with the request types and arguments the HTTP
//! handlers use (not through HTTP), each case on its own fresh data directory.

use std::collections::HashMap;
use std::path::Path;

use chemifree_application::{
    Application, ApplicationError, DataRegistry, ProfileRecords, phase4_nist,
};
use chemifree_providers::{
    FileDocumentRepository, FilePreferenceStore, LocalCapabilities, V2000Codec,
};
use serde::Serialize;
use serde_json::Value;

use crate::case::{
    Body, Call, Case, DataFixture, Expectation, Kind, LoadedCase, OperationRequest, Pending, Step,
    Suite,
};
use crate::expect;

/// The record format version.
pub const REPORT_VERSION: u32 = 1;

type LocalApplication = Application<LocalCapabilities, FileDocumentRepository>;

/// A run record. It holds no times, and runs in which no case fails write identical records.
/// A failure message quotes the reply or error as is, so it may contain run-specific values,
/// including data-directory paths.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    pub report_version: u32,
    pub suite_version: u64,
    pub results: Vec<CaseResult>,
    pub counts: Counts,
}

#[derive(Clone, Debug, Serialize)]
pub struct CaseResult {
    pub id: String,
    pub title: String,
    pub requirements: Vec<String>,
    pub axis: String,
    pub kind: Kind,
    pub outcome: Outcome,
    pub pending: Option<Pending>,
    pub failure: Option<String>,
    pub case_sha256: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Passed,
    Failed,
    /// Pending: the case did not run.
    NotRun,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct Tally {
    pub passed: usize,
    pub failed: usize,
    pub not_run: usize,
}

impl Tally {
    fn add(&mut self, outcome: Outcome) {
        match outcome {
            Outcome::Passed => self.passed += 1,
            Outcome::Failed => self.failed += 1,
            Outcome::NotRun => self.not_run += 1,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct Counts {
    pub accept: Tally,
    pub reject: Tally,
    pub total: Tally,
}

impl Report {
    pub fn failed(&self) -> bool {
        self.counts.total.failed > 0
    }
}

/// Runs every case of `suite` in order.
pub fn run_suite(suite: &Suite) -> Report {
    let mut counts = Counts::default();
    let results: Vec<CaseResult> = suite
        .cases
        .iter()
        .map(|loaded| {
            let result = run_loaded(loaded);
            counts.total.add(result.outcome);
            match result.kind {
                Kind::Accept => counts.accept.add(result.outcome),
                Kind::Reject => counts.reject.add(result.outcome),
            }
            result
        })
        .collect();
    Report {
        report_version: REPORT_VERSION,
        suite_version: suite.version,
        results,
        counts,
    }
}

fn run_loaded(loaded: &LoadedCase) -> CaseResult {
    let case = &loaded.case;
    let (outcome, pending, failure) = match &case.body {
        Body::Pending(pending) => (Outcome::NotRun, Some(pending.clone()), None),
        Body::Steps(steps) => match run_steps(steps, case.data_fixture.as_ref()) {
            Ok(()) => (Outcome::Passed, None, None),
            Err(failure) => (Outcome::Failed, None, Some(failure)),
        },
    };
    CaseResult {
        id: case.id.clone(),
        title: case.title.clone(),
        requirements: case.requirements.clone(),
        axis: case.axis.clone(),
        kind: case.kind,
        outcome,
        pending,
        failure,
        case_sha256: loaded.sha256.clone(),
    }
}

/// Runs a case's steps on a new temporary data directory; returns the first failure.
///
/// # Errors
///
/// Returns a message naming the failing step.
pub fn run_case(case: &Case) -> Result<(), String> {
    match &case.body {
        Body::Steps(steps) => run_steps(steps, case.data_fixture.as_ref()),
        Body::Pending(_) => Err("the case is pending".to_owned()),
    }
}

fn run_steps(steps: &[Step], fixture: Option<&DataFixture>) -> Result<(), String> {
    let directory = tempfile::tempdir()
        .map_err(|error| format!("cannot create a temporary data directory: {error}"))?;
    let mut application = Some(start(directory.path(), fixture)?);
    let mut documents: HashMap<String, String> = HashMap::new();
    let mut captured: HashMap<String, Value> = HashMap::new();
    for (index, step) in steps.iter().enumerate() {
        let fail = |message: String| format!("step {index} ({}): {message}", step.call.name());
        if let Call::Restart = step.call {
            // Dropping the application without `draft_all` is what a crash leaves behind:
            // nothing more is written, and its file handles (and so its locks) are released.
            drop(application.take());
            application = Some(start(directory.path(), fixture)?);
            continue;
        }
        let Some(running) = application.as_ref() else {
            return Err(fail("no application is running".to_owned()));
        };
        let outcome = call(running, &step.call, &documents, &captured).map_err(fail)?;
        check(&step.expectation, &outcome).map_err(fail)?;
        let bound = match (&step.call, &outcome) {
            (
                Call::Create {
                    bind: Some(name), ..
                },
                Ok(response),
            ) => Some((name, response.pointer("/document/id"))),
            (
                Call::Operation {
                    bind: Some(name), ..
                },
                Ok(reply),
            ) => Some((name, reply.pointer("/result/document/id"))),
            _ => None,
        };
        if let (Call::Operation { capture, .. }, Ok(response)) = (&step.call, &outcome) {
            for (name, pointer) in capture {
                let value = response
                    .pointer(pointer)
                    .ok_or_else(|| fail(format!("capture {name:?}: no value at {pointer:?}")))?;
                captured.insert(name.clone(), value.clone());
            }
        }
        if let Some((name, id)) = bound {
            let id = id
                .and_then(Value::as_str)
                .ok_or_else(|| fail("the response names no document id".to_owned()))?;
            documents.insert(name.clone(), id.to_owned());
        }
    }
    Ok(())
}

/// Starts the application on `directory` as the API process does at startup.
fn start(directory: &Path, fixture: Option<&DataFixture>) -> Result<LocalApplication, String> {
    let repository = FileDocumentRepository::new(directory);
    repository.prune_stale_temporaries();
    let preferences = FilePreferenceStore::new(directory);
    preferences.prune_stale_temporaries();
    let application = Application::new(LocalCapabilities, repository)
        .with_ctfile_codec(V2000Codec)
        .with_preferences(preferences);
    let Some(fixture) = fixture else {
        return Ok(application);
    };
    let bytes = include_bytes!("../../../conformance/fixtures/synthetic-carbon-nist.json");
    if fixture.id != "synthetic-carbon-nist-1" || crate::case::sha256_hex(bytes) != fixture.sha256 {
        return Err("synthetic data fixture identity does not match its pinned hash".into());
    }
    let snapshot = phase4_nist::decode_dataset(&[(phase4_nist::FILE_ID, bytes)])
        .map_err(|error| format!("invalid synthetic data fixture: {error}"))?;
    Ok(
        application.with_data_registry(DataRegistry::synthetic_conformance(
            ProfileRecords::Phase4Nist(Box::new(snapshot)),
        )),
    )
}

/// The response, or the v1 error envelope, as JSON.
type CallOutcome = Result<Value, Value>;

fn call(
    application: &LocalApplication,
    call: &Call,
    documents: &HashMap<String, String>,
    captured: &HashMap<String, Value>,
) -> Result<CallOutcome, String> {
    let id = |name: &String| {
        documents
            .get(name)
            .map(String::as_str)
            .ok_or_else(|| format!("document {name:?} was not bound by a successful create"))
    };
    let outcome = match call {
        Call::Create { body, .. } => {
            json(application.create_document(Some(&body.request_id), body.title.as_deref()))
        }
        Call::Get { document } => json(application.document(id(document)?)),
        Call::Transact { document, body } => json(application.apply_transaction_selecting(
            id(document)?,
            Some(&body.request_id),
            body.expected_revision,
            body.commands.clone(),
            chemifree_application::TransactionSelections {
                before: body.selection_before.clone(),
                after: body.selection_after.clone(),
            },
        )),
        Call::Undo { document, body } => json(application.undo(
            id(document)?,
            Some(&body.request_id),
            body.expected_revision,
        )),
        Call::Redo { document, body } => json(application.redo(
            id(document)?,
            Some(&body.request_id),
            body.expected_revision,
        )),
        Call::Save { document, body } => json(application.save(
            id(document)?,
            Some(&body.request_id),
            body.expected_revision,
        )),
        Call::Autosave { document, body } => {
            json(application.autosave(id(document)?, Some(&body.request_id)))
        }
        Call::Recover { document, body } => {
            json(application.recover(id(document)?, Some(&body.request_id)))
        }
        Call::Close { document, body } => json(application.close(
            id(document)?,
            Some(&body.request_id),
            body.discard_unsaved,
            body.expected_revision,
        )),
        Call::ResetSettings { document, body } => json(application.reset_settings(
            id(document)?,
            Some(&body.request_id),
            body.expected_revision,
            body.to,
        )),
        Call::Capabilities => json(Ok(application.capabilities())),
        Call::Preferences => json(application.preferences()),
        Call::SetPreferences { body } => json(application.set_preferences(
            Some(&body.request_id),
            body.expected_revision,
            body.change(),
        )),
        Call::Restart => return Err("restart is not a call".to_owned()),
        Call::Operation {
            request,
            document,
            substitute,
            ..
        } => {
            let body = match request {
                OperationRequest::Raw(raw) => raw.clone().into_bytes(),
                OperationRequest::Json(request) => {
                    let mut request = request.clone();
                    if let Some(name) = document {
                        let parameters = request
                            .get_mut("parameters")
                            .and_then(Value::as_object_mut)
                            .ok_or_else(|| "operation has no parameters object".to_owned())?;
                        parameters.insert("document_id".to_owned(), Value::from(id(name)?));
                    }
                    for (pointer, name) in substitute {
                        let value = captured
                            .get(name)
                            .ok_or_else(|| format!("value {name:?} was not captured"))?;
                        let slot = request
                            .pointer_mut(pointer)
                            .ok_or_else(|| format!("request has no value at {pointer:?}"))?;
                        *slot = value.clone();
                    }
                    request.to_string().into_bytes()
                }
            };
            // An operation answers every request with an operation reply, never an error.
            json(Ok(application.execute(&body)))
        }
    };
    outcome.map_err(|error| format!("cannot encode the outcome: {error}"))
}

fn json<T: Serialize>(
    result: Result<T, ApplicationError>,
) -> Result<CallOutcome, serde_json::Error> {
    Ok(match result {
        Ok(response) => Ok(serde_json::to_value(response)?),
        Err(error) => Err(serde_json::to_value(error.to_response())?),
    })
}

fn check(expectation: &Expectation, outcome: &CallOutcome) -> Result<(), String> {
    match (expectation, outcome) {
        (Expectation::Success, Ok(_)) => Ok(()),
        (Expectation::Response(expected), Ok(actual)) => {
            expect::matches(expected, actual).map_err(|mismatch| format!("response {mismatch}"))
        }
        (Expectation::Error(expected), Err(actual)) => {
            expect::matches(expected, actual).map_err(|mismatch| format!("error {mismatch}"))
        }
        (Expectation::Success | Expectation::Response(_), Err(error)) => {
            Err(format!("expected success, got error {error}"))
        }
        (Expectation::Error(_), Ok(_)) => Err("expected an error, got success".to_owned()),
    }
}
