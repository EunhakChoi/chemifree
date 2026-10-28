//! Versioned conformance cases run against the shared application.
//!
//! A case is an independently written JSON file (format [`CASE_FORMAT`]) that names the
//! requirements it checks, drives the application through the same calls the HTTP and CLI
//! adapters make, and states what each reply must contain. A suite file (format
//! [`SUITE_FORMAT`]) lists the cases of one suite version. Every case runs on a fresh data
//! directory and reports `passed`, `failed`, or `not_run` (a case that waits for a later
//! feature, with the plan entry that will make it runnable). A case of kind `reject` checks a
//! refusal the product makes on purpose, so it counts apart from cases that check a result.
//!
//! Expectations are subsets of the v1 JSON replies: every field given must be present and
//! equal, and an array must have exactly as many elements, each matching its counterpart.
//! `{"$contains": [...]}` in place of an array asks only that each listed element match some
//! element of it, in any order. A case therefore pins only what it is about and keeps passing
//! when unrelated fields are added within v1.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use chemifree_application::{Application, ApplicationError, CommandDto};
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const CASE_FORMAT: &str = "chemifree-conformance/1";
pub const SUITE_FORMAT: &str = "chemifree-conformance-suite/1";
pub const REPORT_FORMAT: &str = "chemifree-conformance-report/1";

type LocalApplication = Application<LocalCapabilities, FileDocumentRepository>;

/// A suite as listed in its file, with its cases loaded.
#[derive(Debug)]
pub struct Suite {
    pub version: String,
    pub cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SuiteFile {
    format: String,
    suite_version: String,
    /// Case files, relative to the suite file.
    cases: Vec<PathBuf>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub format: String,
    pub id: String,
    pub title: String,
    /// Requirement identifiers (`MODEL-01`, `OPS-02`, …) this case gives evidence for.
    pub requirements: Vec<String>,
    /// The comparison axis: what kind of outcome the case compares.
    pub axis: Axis,
    pub kind: Kind,
    /// Set when the case cannot run until a later feature exists.
    #[serde(default)]
    pub pending: Option<Pending>,
    #[serde(default)]
    pub steps: Vec<Step>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    /// Document content, identifiers and revisions.
    Document,
    /// Atomic changes, history and request replay.
    Operations,
    /// What survives a save, a restart, or a recovery.
    Persistence,
    /// The versioned request, reply and error contract.
    Api,
    /// What the service says it supports.
    Capabilities,
    /// Chemical computation results.
    Chemistry,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// The product must produce a result.
    Accept,
    /// The product must refuse, on purpose: invalid input, a conflict, or an unsupported
    /// feature reported as such.
    Reject,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Pending {
    /// The plan entry (for example `P19`) after which the case can run.
    pub followup: String,
    pub reason: String,
}

/// A step's fields are checked against its call by [`check_step_fields`]: serde cannot deny
/// unknown fields of a flattened enum.
#[derive(Clone, Debug, Deserialize)]
pub struct Step {
    #[serde(flatten)]
    pub call: Call,
    #[serde(default)]
    pub request_id: Option<String>,
    /// A subset the reply must contain; the call must succeed.
    #[serde(default)]
    pub expect: Option<Value>,
    /// A subset the error envelope must contain; the call must fail.
    #[serde(default)]
    pub expect_error: Option<Value>,
}

/// An application call. Documents are named by the `as` binding of the `create` that made
/// them, so cases never depend on generated identifiers.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Call {
    Create {
        #[serde(default)]
        title: Option<String>,
        #[serde(rename = "as")]
        bind: String,
    },
    Get {
        document: String,
    },
    Transaction {
        document: String,
        expected_revision: u64,
        commands: Vec<Value>,
    },
    Undo {
        document: String,
        expected_revision: u64,
    },
    Redo {
        document: String,
        expected_revision: u64,
    },
    Save {
        document: String,
        expected_revision: u64,
    },
    Close {
        document: String,
        #[serde(default)]
        discard_unsaved: bool,
        #[serde(default)]
        expected_revision: Option<u64>,
    },
    Capabilities,
    /// Ends the application and starts a new one on the same data directory, as a process
    /// restart would: only what storage holds remains.
    Restart,
}

/// Why a suite or case file cannot be used.
#[derive(Debug)]
pub struct SuiteError(String);

impl fmt::Display for SuiteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for SuiteError {}

impl Suite {
    /// Loads and checks a suite file and every case it lists.
    ///
    /// # Errors
    ///
    /// A file that cannot be read or parsed, an unknown format, a duplicate case ID, or a
    /// case that is not well formed (see [`Case::check`]).
    pub fn load(path: &Path) -> Result<Self, SuiteError> {
        let file: SuiteFile = read_json(path)?;
        if file.format != SUITE_FORMAT {
            return Err(SuiteError(format!(
                "{}: format {:?} is not {SUITE_FORMAT:?}",
                path.display(),
                file.format
            )));
        }
        let base = path.parent().unwrap_or(Path::new("."));
        let mut seen = HashSet::new();
        let mut cases = Vec::new();
        for relative in &file.cases {
            let case_path = base.join(relative);
            let raw: Value = read_json(&case_path)?;
            check_step_fields(&raw)
                .map_err(|message| SuiteError(format!("{}: {message}", case_path.display())))?;
            let case: Case = serde_json::from_value(raw)
                .map_err(|error| SuiteError(format!("{}: {error}", case_path.display())))?;
            case.check()
                .map_err(|message| SuiteError(format!("{}: {message}", case_path.display())))?;
            if !seen.insert(case.id.clone()) {
                return Err(SuiteError(format!("duplicate case id {:?}", case.id)));
            }
            cases.push(case);
        }
        Ok(Self {
            version: file.suite_version,
            cases,
        })
    }

    /// Runs every case, each on its own fresh data directory.
    pub fn run(&self) -> Report {
        let results = self.cases.iter().map(run_case).collect();
        Report::new(&self.version, results)
    }
}

impl Case {
    /// Checks what loading cannot: the format, requirement identifiers, that a pending case
    /// has no steps and a runnable one has some, that a `reject` case expects an error, and
    /// that documents are bound before use.
    ///
    /// # Errors
    ///
    /// Describes the first problem found.
    pub fn check(&self) -> Result<(), String> {
        if self.format != CASE_FORMAT {
            return Err(format!("format {:?} is not {CASE_FORMAT:?}", self.format));
        }
        if self.requirements.is_empty() {
            return Err("names no requirement".into());
        }
        if let Some(bad) = self.requirements.iter().find(|id| !is_requirement_id(id)) {
            return Err(format!("{bad:?} is not a requirement identifier"));
        }
        match (&self.pending, self.steps.is_empty()) {
            (Some(_), false) => return Err("a pending case has no steps to run".into()),
            (None, true) => return Err("has no steps".into()),
            _ => {}
        }
        if self.pending.is_none()
            && self.kind == Kind::Reject
            && self.steps.iter().all(|step| step.expect_error.is_none())
        {
            return Err("a reject case must expect an error".into());
        }
        let mut bound = HashSet::new();
        for (index, step) in self.steps.iter().enumerate() {
            if step.expect.is_some() && step.expect_error.is_some() {
                return Err(format!("step {index} expects both a reply and an error"));
            }
            match &step.call {
                Call::Create { bind, .. } => {
                    bound.insert(bind.as_str());
                }
                call => {
                    if let Some(name) = call.document()
                        && !bound.contains(name)
                    {
                        return Err(format!("step {index} uses unbound document {name:?}"));
                    }
                }
            }
        }
        Ok(())
    }
}

impl Call {
    fn document(&self) -> Option<&str> {
        match self {
            Self::Get { document }
            | Self::Transaction { document, .. }
            | Self::Undo { document, .. }
            | Self::Redo { document, .. }
            | Self::Save { document, .. }
            | Self::Close { document, .. } => Some(document),
            Self::Create { .. } | Self::Capabilities | Self::Restart => None,
        }
    }
}

/// Rejects step fields that its call does not take, so a misspelled expectation cannot pass
/// by being ignored.
fn check_step_fields(case: &Value) -> Result<(), String> {
    const COMMON: [&str; 4] = ["op", "request_id", "expect", "expect_error"];
    let steps = case.get("steps").and_then(Value::as_array);
    for (index, step) in steps.into_iter().flatten().enumerate() {
        let Some(fields) = step.as_object() else {
            return Err(format!("step {index} is not an object"));
        };
        let op = fields.get("op").and_then(Value::as_str).unwrap_or_default();
        let call: &[&str] = match op {
            "create" => &["title", "as"],
            "get" => &["document"],
            "transaction" => &["document", "expected_revision", "commands"],
            "undo" | "redo" | "save" => &["document", "expected_revision"],
            "close" => &["document", "discard_unsaved", "expected_revision"],
            "capabilities" | "restart" => &[],
            op => return Err(format!("step {index} has unknown op {op:?}")),
        };
        if let Some(field) = fields
            .keys()
            .find(|key| !COMMON.contains(&key.as_str()) && !call.contains(&key.as_str()))
        {
            return Err(format!("step {index} ({op}) has unknown field {field:?}"));
        }
    }
    Ok(())
}

fn is_requirement_id(id: &str) -> bool {
    let Some((domain, number)) = id.split_once('-') else {
        return false;
    };
    !domain.is_empty()
        && domain.bytes().all(|byte| byte.is_ascii_uppercase())
        && number.len() == 2
        && number.bytes().all(|byte| byte.is_ascii_digit())
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, SuiteError> {
    let text = fs::read_to_string(path)
        .map_err(|error| SuiteError(format!("{}: {error}", path.display())))?;
    serde_json::from_str(&text).map_err(|error| SuiteError(format!("{}: {error}", path.display())))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Passed,
    Failed,
    NotRun,
}

#[derive(Debug, Serialize)]
pub struct CaseResult {
    pub id: String,
    pub requirements: Vec<String>,
    pub axis: Axis,
    pub kind: Kind,
    pub status: Status,
    /// Why a case failed, or what a case that did not run waits for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub followup: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub format: &'static str,
    pub suite_version: String,
    pub runner_version: &'static str,
    /// Cases by status and kind, for example `passed/accept`.
    pub counts: BTreeMap<String, usize>,
    pub results: Vec<CaseResult>,
}

impl Report {
    fn new(suite_version: &str, results: Vec<CaseResult>) -> Self {
        let mut counts = BTreeMap::new();
        for result in &results {
            let status = serde_json::to_value(result.status).expect("a status serializes");
            let kind = serde_json::to_value(result.kind).expect("a kind serializes");
            let key = format!(
                "{}/{}",
                status.as_str().unwrap_or_default(),
                kind.as_str().unwrap_or_default()
            );
            *counts.entry(key).or_default() += 1;
        }
        Self {
            format: REPORT_FORMAT,
            suite_version: suite_version.to_owned(),
            runner_version: env!("CARGO_PKG_VERSION"),
            counts,
            results,
        }
    }

    pub fn failed(&self) -> usize {
        self.results
            .iter()
            .filter(|result| result.status == Status::Failed)
            .count()
    }
}

fn run_case(case: &Case) -> CaseResult {
    let result = |status, detail, followup| CaseResult {
        id: case.id.clone(),
        requirements: case.requirements.clone(),
        axis: case.axis,
        kind: case.kind,
        status,
        detail,
        followup,
    };
    if let Some(pending) = &case.pending {
        return result(
            Status::NotRun,
            Some(pending.reason.clone()),
            Some(pending.followup.clone()),
        );
    }
    match Run::new().and_then(|mut run| run.steps(&case.steps)) {
        Ok(()) => result(Status::Passed, None, None),
        Err(message) => result(Status::Failed, Some(message), None),
    }
}

/// One case's application and data directory.
struct Run {
    directory: tempfile::TempDir,
    application: LocalApplication,
    documents: HashMap<String, String>,
}

impl Run {
    fn new() -> Result<Self, String> {
        let directory = tempfile::tempdir().map_err(|error| format!("temporary data: {error}"))?;
        let application = open(directory.path());
        Ok(Self {
            directory,
            application,
            documents: HashMap::new(),
        })
    }

    fn steps(&mut self, steps: &[Step]) -> Result<(), String> {
        for (index, step) in steps.iter().enumerate() {
            self.step(step)
                .map_err(|message| format!("step {index}: {message}"))?;
        }
        Ok(())
    }

    fn step(&mut self, step: &Step) -> Result<(), String> {
        let outcome = self.call(&step.call, step.request_id.as_deref())?;
        match (outcome, &step.expect_error) {
            (Ok(reply), None) => match &step.expect {
                Some(expected) => matches(expected, &reply, "reply"),
                None => Ok(()),
            },
            (Ok(reply), Some(_)) => Err(format!("expected an error, got {reply}")),
            (Err(error), Some(expected)) => {
                let envelope = serde_json::to_value(error.to_response())
                    .map_err(|error| format!("error envelope: {error}"))?;
                matches(expected, &envelope, "error")
            }
            (Err(error), None) => Err(format!("unexpected error: {error}")),
        }
    }

    /// Makes the call. The outer error is a broken case (an unknown binding, a malformed
    /// command list); the inner one is the application's answer.
    fn call(
        &mut self,
        call: &Call,
        request_id: Option<&str>,
    ) -> Result<Result<Value, ApplicationError>, String> {
        let app = &self.application;
        let reply = match call {
            Call::Create { title, bind } => {
                let created = app.create_document(request_id, title.as_deref());
                if let Ok(created) = &created {
                    self.documents
                        .insert(bind.clone(), created.document.id.clone());
                }
                created.map(to_value)
            }
            Call::Get { document } => app.document(self.id(document)?).map(to_value),
            Call::Transaction {
                document,
                expected_revision,
                commands,
            } => {
                let commands: Vec<CommandDto> =
                    serde_json::from_value(Value::Array(commands.clone()))
                        .map_err(|error| format!("commands: {error}"))?;
                app.apply_transaction(self.id(document)?, request_id, *expected_revision, commands)
                    .map(to_value)
            }
            Call::Undo {
                document,
                expected_revision,
            } => app
                .undo(self.id(document)?, request_id, *expected_revision)
                .map(to_value),
            Call::Redo {
                document,
                expected_revision,
            } => app
                .redo(self.id(document)?, request_id, *expected_revision)
                .map(to_value),
            Call::Save {
                document,
                expected_revision,
            } => app
                .save(self.id(document)?, request_id, *expected_revision)
                .map(to_value),
            Call::Close {
                document,
                discard_unsaved,
                expected_revision,
            } => app
                .close(
                    self.id(document)?,
                    request_id,
                    *discard_unsaved,
                    *expected_revision,
                )
                .map(to_value),
            Call::Capabilities => Ok(to_value(app.capabilities())),
            Call::Restart => {
                self.application = open(self.directory.path());
                Ok(Value::Null)
            }
        };
        Ok(reply)
    }

    fn id(&self, name: &str) -> Result<&str, String> {
        self.documents
            .get(name)
            .map(String::as_str)
            .ok_or_else(|| format!("no document was created as {name:?}"))
    }
}

fn open(directory: &Path) -> LocalApplication {
    Application::new(LocalCapabilities, FileDocumentRepository::new(directory))
}

fn to_value(reply: impl Serialize) -> Value {
    serde_json::to_value(reply).expect("v1 replies serialize to JSON")
}

/// Checks that `actual` contains `expected` as described in the module documentation.
///
/// # Errors
///
/// Names the first path where they differ.
pub fn matches(expected: &Value, actual: &Value, path: &str) -> Result<(), String> {
    if let Some(wanted) = expected.get("$contains") {
        let (Some(wanted), Some(actual)) = (wanted.as_array(), actual.as_array()) else {
            return Err(format!(
                "{path}: $contains needs a list and an array, found {actual}"
            ));
        };
        return wanted.iter().enumerate().try_for_each(|(index, wanted)| {
            if actual
                .iter()
                .any(|element| matches(wanted, element, path).is_ok())
            {
                Ok(())
            } else {
                Err(format!(
                    "{path} has no element matching $contains[{index}] {wanted}"
                ))
            }
        });
    }
    match (expected, actual) {
        (Value::Object(expected), Value::Object(actual)) => {
            for (key, value) in expected {
                let at = format!("{path}.{key}");
                let found = actual.get(key).ok_or_else(|| format!("{at} is missing"))?;
                matches(value, found, &at)?;
            }
            Ok(())
        }
        (Value::Array(expected), Value::Array(actual)) => {
            if expected.len() != actual.len() {
                return Err(format!(
                    "{path} has {} elements, expected {}",
                    actual.len(),
                    expected.len()
                ));
            }
            expected
                .iter()
                .zip(actual)
                .enumerate()
                .try_for_each(|(index, (expected, actual))| {
                    matches(expected, actual, &format!("{path}[{index}]"))
                })
        }
        // A float compares by value, so a stored 0.0 matches a written 0; two integers compare
        // exactly, so revisions beyond 2^53 are not rounded together.
        (Value::Number(expected), Value::Number(actual))
            if (expected.is_f64() || actual.is_f64()) && expected.as_f64() == actual.as_f64() =>
        {
            Ok(())
        }
        (expected, actual) if expected == actual => Ok(()),
        (expected, actual) => Err(format!("{path} is {actual}, expected {expected}")),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn case(value: Value) -> Case {
        serde_json::from_value(value).expect("a well-formed case")
    }

    fn create_and_get(expect: Value) -> Value {
        json!({
            "format": CASE_FORMAT, "id": "t", "title": "t", "requirements": ["OPS-02"],
            "axis": "document", "kind": "accept",
            "steps": [
                { "op": "create", "title": "Kept", "as": "doc" },
                { "op": "get", "document": "doc", "expect": expect }
            ]
        })
    }

    #[test]
    fn a_subset_matches_and_names_the_first_difference() {
        let actual = json!({ "a": 1, "b": { "c": [1.0, 2], "d": "x" } });
        assert_eq!(
            matches(&json!({ "b": { "c": [1, 2] } }), &actual, "r"),
            Ok(())
        );
        assert_eq!(
            matches(&json!({ "b": { "d": "y" } }), &actual, "r"),
            Err("r.b.d is \"x\", expected \"y\"".into())
        );
        assert_eq!(
            matches(&json!({ "b": { "c": [1] } }), &actual, "r"),
            Err("r.b.c has 2 elements, expected 1".into())
        );
        assert_eq!(
            matches(&json!({ "e": null }), &actual, "r"),
            Err("r.e is missing".into())
        );
        assert_eq!(
            matches(&json!({ "b": { "c": { "$contains": [2] } } }), &actual, "r"),
            Ok(())
        );
        assert!(matches(&json!({ "b": { "c": { "$contains": [3] } } }), &actual, "r").is_err());
    }

    #[test]
    fn integers_compare_exactly_and_floats_by_value() {
        let big = json!(9_007_199_254_740_993_u64);
        assert!(matches(&json!(9_007_199_254_740_992_u64), &big, "r").is_err());
        assert_eq!(matches(&json!(0), &json!(0.0), "r"), Ok(()));
        assert!(matches(&json!(1), &json!(1.5), "r").is_err());
    }

    #[test]
    fn a_wrong_expectation_fails_the_case() {
        let passing = run_case(&case(create_and_get(
            json!({ "document": { "title": "Kept" } }),
        )));
        assert_eq!(passing.status, Status::Passed, "{:?}", passing.detail);
        let failing = run_case(&case(create_and_get(
            json!({ "document": { "title": "Other" } }),
        )));
        assert_eq!(failing.status, Status::Failed);
        assert_eq!(
            failing.detail.as_deref(),
            Some("step 1: reply.document.title is \"Kept\", expected \"Other\"")
        );
    }

    #[test]
    fn a_reject_case_fails_when_the_call_succeeds() {
        let mut value = create_and_get(json!(null));
        value["kind"] = json!("reject");
        value["steps"][1] = json!({ "op": "get", "document": "doc", "expect_error": { "code": "document_not_found" } });
        let result = run_case(&case(value));
        assert_eq!(result.status, Status::Failed);
        assert!(
            result
                .detail
                .as_deref()
                .unwrap_or_default()
                .starts_with("step 1: expected an error"),
            "{:?}",
            result.detail
        );
    }

    #[test]
    fn a_pending_case_is_not_run_and_names_its_followup() {
        let result = run_case(&case(json!({
            "format": CASE_FORMAT, "id": "t", "title": "t", "requirements": ["CHEM-01"],
            "axis": "chemistry", "kind": "accept",
            "pending": { "followup": "P19", "reason": "not implemented" }
        })));
        assert_eq!(result.status, Status::NotRun);
        assert_eq!(result.followup.as_deref(), Some("P19"));
    }

    #[test]
    fn malformed_cases_are_refused_before_running() {
        let base = create_and_get(json!(null));
        let mut reject_without_error = base.clone();
        reject_without_error["kind"] = json!("reject");
        let mut pending_with_steps = base.clone();
        pending_with_steps["pending"] = json!({ "followup": "P19", "reason": "later" });
        let mut unbound = base.clone();
        unbound["steps"][1]["document"] = json!("other");
        let mut bad_requirement = base.clone();
        bad_requirement["requirements"] = json!(["OPS-2"]);
        for (value, problem) in [
            (reject_without_error, "a reject case must expect an error"),
            (pending_with_steps, "a pending case has no steps to run"),
            (unbound, "step 1 uses unbound document \"other\""),
            (bad_requirement, "\"OPS-2\" is not a requirement identifier"),
        ] {
            assert_eq!(case(value).check(), Err(problem.to_owned()));
        }
    }

    #[test]
    fn a_misspelled_step_field_is_refused() {
        let mut value = create_and_get(json!(null));
        value["steps"][1] =
            json!({ "op": "get", "document": "doc", "expected": { "document": {} } });
        assert_eq!(
            check_step_fields(&value),
            Err("step 1 (get) has unknown field \"expected\"".into())
        );
    }

    #[test]
    fn a_suite_with_a_duplicate_case_id_is_refused() {
        let directory = tempfile::tempdir().unwrap();
        let case = create_and_get(json!(null));
        fs::write(directory.path().join("a.json"), case.to_string()).unwrap();
        fs::write(directory.path().join("b.json"), case.to_string()).unwrap();
        let suite =
            json!({ "format": SUITE_FORMAT, "suite_version": "t", "cases": ["a.json", "b.json"] });
        fs::write(directory.path().join("suite.json"), suite.to_string()).unwrap();
        let error = Suite::load(&directory.path().join("suite.json")).unwrap_err();
        assert_eq!(error.to_string(), "duplicate case id \"t\"");
    }
}
