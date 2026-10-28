//! Conformance cases and the suite that lists them, loaded with every check that does not
//! need the application to run.

use std::collections::HashSet;
use std::fmt;
use std::fs;
use std::path::Path;

use chemifree_application::json;
use chemifree_application::{
    CloseRequest, CreateDocumentRequest, DraftRequest, RevisionRequest, TransactionRequest,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::expect::{self, ExpectationError};

/// Comparison axes a case can belong to. Each case names one.
pub const AXES: [&str; 9] = [
    "graph",
    "scene",
    "stereo",
    "numeric",
    "encoding",
    "history",
    "persistence",
    "protocol",
    "host",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// The case shows the product accepting valid input with the stated result.
    Accept,
    /// The case shows the product refusing input with the stated error.
    Reject,
}

/// A case that cannot run yet, and the roadmap item expected to make it runnable.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Pending {
    pub followup: String,
    pub reason: String,
}

#[derive(Clone, Debug)]
pub struct Case {
    pub id: String,
    pub title: String,
    pub requirements: Vec<String>,
    pub axis: String,
    pub kind: Kind,
    pub body: Body,
}

#[derive(Clone, Debug)]
pub enum Body {
    Steps(Vec<Step>),
    Pending(Pending),
}

#[derive(Clone, Debug)]
pub struct Step {
    pub call: Call,
    pub expectation: Expectation,
}

#[derive(Clone, Debug)]
pub enum Expectation {
    /// The call must succeed; its response is not compared.
    Success,
    /// The call must succeed and its response must match.
    Response(Value),
    /// The call must fail and its error envelope must match.
    Error(Value),
}

/// One call to the application, with the request body the HTTP adapter would deserialize.
/// `document` names a document bound by an earlier `create`.
#[derive(Clone, Debug)]
pub enum Call {
    Create {
        bind: Option<String>,
        body: CreateDocumentRequest,
    },
    Get {
        document: String,
    },
    Transact {
        document: String,
        body: TransactionRequest,
    },
    Undo {
        document: String,
        body: RevisionRequest,
    },
    Redo {
        document: String,
        body: RevisionRequest,
    },
    Save {
        document: String,
        body: RevisionRequest,
    },
    Autosave {
        document: String,
        body: DraftRequest,
    },
    Recover {
        document: String,
        body: DraftRequest,
    },
    Close {
        document: String,
        body: CloseRequest,
    },
    Capabilities,
    /// Ends the application without a normal stop, as a crash would, and starts a new one on
    /// the same data directory.
    Restart,
    /// A request of the operation contract, passed to `Application::execute` as the HTTP and
    /// CLI adapters pass it. With `document`, the bound document's identifier is set as
    /// `parameters.document_id`; with `bind`, the document in the result is bound to a name.
    Operation {
        request: OperationRequest,
        document: Option<String>,
        bind: Option<String>,
    },
}

/// An operation request: JSON, or raw text sent byte for byte (to send one that is not JSON).
#[derive(Clone, Debug)]
pub enum OperationRequest {
    Json(Value),
    Raw(String),
}

impl Call {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Create { .. } => "create",
            Self::Get { .. } => "get",
            Self::Transact { .. } => "transact",
            Self::Undo { .. } => "undo",
            Self::Redo { .. } => "redo",
            Self::Save { .. } => "save",
            Self::Autosave { .. } => "autosave",
            Self::Recover { .. } => "recover",
            Self::Close { .. } => "close",
            Self::Capabilities => "capabilities",
            Self::Restart => "restart",
            Self::Operation { .. } => "operation",
        }
    }
}

/// Why a case or suite was refused.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Problem {
    Json(String),
    NotAnObject,
    UnknownField(String),
    MissingField(&'static str),
    WrongType {
        field: &'static str,
        expected: &'static str,
    },
    InvalidCaseId(String),
    InvalidRequirementId(String),
    InvalidRoadmapId(String),
    EmptyTitle,
    NoRequirements,
    DuplicateRequirement(String),
    UnknownAxis(String),
    UnknownKind(String),
    PendingWithSteps,
    EmptyReason,
    RunnableWithoutSteps,
    RejectWithoutErrorStep,
    AcceptWithoutExpectStep,
    Step {
        index: usize,
        problem: StepProblem,
    },
    InvalidSuiteVersion,
    NoCases,
    InvalidCasePath {
        path: String,
        reason: &'static str,
    },
    DuplicateCaseId(String),
    Read {
        path: String,
        message: String,
    },
    InCase {
        path: String,
        problem: Box<Problem>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StepProblem {
    NotAnObject,
    UnknownCall(String),
    UnknownField(String),
    WrongType {
        field: &'static str,
        expected: &'static str,
    },
    RequestIdNotTaken,
    DocumentNotTaken,
    MissingDocument,
    BindNotTaken,
    InvalidName(String),
    UnboundDocument(String),
    BoundTwice(String),
    BindExpectsError,
    BothExpectations,
    ExpectationOnRestart,
    OperationRequest(&'static str),
    OperationExpectsOutcome,
    OperationExpectsNoError,
    Expect(ExpectationError),
    ExpectError(ExpectationError),
    Body(String),
}

impl fmt::Display for StepProblem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAnObject => write!(formatter, "is not an object"),
            Self::UnknownCall(call) => write!(formatter, "unknown call {call:?}"),
            Self::UnknownField(field) => write!(formatter, "unknown field {field:?}"),
            Self::WrongType { field, expected } => {
                write!(formatter, "field {field:?} must be {expected}")
            }
            Self::RequestIdNotTaken => write!(formatter, "this call takes no request_id"),
            Self::DocumentNotTaken => write!(formatter, "this call takes no document"),
            Self::MissingDocument => write!(formatter, "this call needs a document"),
            Self::BindNotTaken => write!(formatter, "only create binds a document"),
            Self::InvalidName(name) => write!(
                formatter,
                "document name {name:?} must be 1-64 characters of [A-Za-z0-9_-]"
            ),
            Self::UnboundDocument(name) => {
                write!(
                    formatter,
                    "document {name:?} is used before a create binds it"
                )
            }
            Self::BoundTwice(name) => write!(formatter, "document {name:?} is bound twice"),
            Self::BindExpectsError => {
                write!(formatter, "a create that expects an error cannot bind")
            }
            Self::BothExpectations => write!(formatter, "has both expect and expect_error"),
            Self::ExpectationOnRestart => write!(formatter, "restart takes no expectation"),
            Self::OperationRequest(message) => write!(formatter, "operation request {message}"),
            Self::OperationExpectsOutcome => {
                write!(formatter, "an operation step expects the reply's outcome")
            }
            Self::OperationExpectsNoError => write!(
                formatter,
                "an operation is answered with an operation reply, never an error envelope; expect its outcome"
            ),
            Self::Expect(error) => write!(formatter, "expect {error}"),
            Self::ExpectError(error) => write!(formatter, "expect_error {error}"),
            Self::Body(message) => write!(formatter, "request body: {message}"),
        }
    }
}

impl fmt::Display for Problem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(message) => write!(formatter, "invalid JSON: {message}"),
            Self::NotAnObject => write!(formatter, "not a JSON object"),
            Self::UnknownField(field) => write!(formatter, "unknown field {field:?}"),
            Self::MissingField(field) => write!(formatter, "missing field {field:?}"),
            Self::WrongType { field, expected } => {
                write!(formatter, "field {field:?} must be {expected}")
            }
            Self::InvalidCaseId(id) => write!(
                formatter,
                "case id {id:?} must be lowercase words of [a-z0-9] joined by '-'"
            ),
            Self::InvalidRequirementId(id) => {
                write!(formatter, "requirement id {id:?} must look like OPS-02")
            }
            Self::InvalidRoadmapId(id) => {
                write!(formatter, "roadmap id {id:?} must look like P13 or A1")
            }
            Self::EmptyTitle => write!(formatter, "the title is empty"),
            Self::NoRequirements => write!(formatter, "no requirements are named"),
            Self::DuplicateRequirement(id) => write!(formatter, "requirement {id} is repeated"),
            Self::UnknownAxis(axis) => write!(
                formatter,
                "unknown axis {axis:?}; expected one of {}",
                AXES.join(", ")
            ),
            Self::UnknownKind(kind) => {
                write!(
                    formatter,
                    "unknown kind {kind:?}; expected accept or reject"
                )
            }
            Self::PendingWithSteps => write!(formatter, "a pending case cannot have steps"),
            Self::EmptyReason => write!(formatter, "pending.reason is empty"),
            Self::RunnableWithoutSteps => {
                write!(formatter, "a case needs steps or a pending entry")
            }
            Self::RejectWithoutErrorStep => {
                write!(formatter, "a reject case needs a step with expect_error")
            }
            Self::AcceptWithoutExpectStep => {
                write!(formatter, "an accept case needs a step with expect")
            }
            Self::Step { index, problem } => write!(formatter, "step {index}: {problem}"),
            Self::InvalidSuiteVersion => {
                write!(formatter, "suite_version must be a positive integer")
            }
            Self::NoCases => write!(formatter, "the suite lists no cases"),
            Self::InvalidCasePath { path, reason } => {
                write!(formatter, "case path {path:?} {reason}")
            }
            Self::DuplicateCaseId(id) => write!(formatter, "case id {id:?} is repeated"),
            Self::Read { path, message } => write!(formatter, "cannot read {path}: {message}"),
            Self::InCase { path, problem } => write!(formatter, "{path}: {problem}"),
        }
    }
}

impl std::error::Error for Problem {}

/// A case together with its file's identity.
#[derive(Clone, Debug)]
pub struct LoadedCase {
    pub case: Case,
    /// SHA-256 of the case file's bytes, lowercase hex.
    pub sha256: String,
}

#[derive(Clone, Debug)]
pub struct Suite {
    pub version: u64,
    pub cases: Vec<LoadedCase>,
}

/// Checks a case path from the suite file: relative to it, without empty, `.`, or `..`
/// segments (split on both `/` and `\`), not absolute and without a drive prefix.
/// The traceability check applies the same rule to the same raw string.
///
/// # Errors
///
/// Returns why the path is refused.
pub fn check_case_path(raw: &str) -> Result<(), &'static str> {
    if raw.starts_with('/') || raw.starts_with('\\') {
        return Err("is absolute");
    }
    let bytes = raw.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return Err("has a drive prefix");
    }
    for segment in raw.split(['/', '\\']) {
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err("has an empty, '.', or '..' segment");
        }
    }
    Ok(())
}

/// Loads the suite at `path` and every case it lists.
///
/// # Errors
///
/// Returns the first problem found in the suite or a case.
pub fn load_suite(path: &Path) -> Result<Suite, Problem> {
    let bytes = read(path)?;
    let value = json::parse_strict(&bytes).map_err(|error| Problem::Json(error.to_string()))?;
    let Value::Object(fields) = value else {
        return Err(Problem::NotAnObject);
    };
    only_fields(&fields, &["suite_version", "cases"])?;
    let version = match fields.get("suite_version") {
        Some(Value::Number(number)) => number
            .as_u64()
            .filter(|version| *version > 0)
            .ok_or(Problem::InvalidSuiteVersion)?,
        _ => return Err(Problem::InvalidSuiteVersion),
    };
    let Some(Value::Array(entries)) = fields.get("cases") else {
        return Err(Problem::WrongType {
            field: "cases",
            expected: "a list of case paths",
        });
    };
    if entries.is_empty() {
        return Err(Problem::NoCases);
    }
    let base = path.parent().unwrap_or_else(|| Path::new(""));
    let mut ids = HashSet::new();
    let mut cases = Vec::with_capacity(entries.len());
    for entry in entries {
        let Value::String(raw) = entry else {
            return Err(Problem::WrongType {
                field: "cases",
                expected: "a list of case paths",
            });
        };
        check_case_path(raw).map_err(|reason| Problem::InvalidCasePath {
            path: raw.clone(),
            reason,
        })?;
        let in_case = |problem| Problem::InCase {
            path: raw.clone(),
            problem: Box::new(problem),
        };
        let bytes = read(&base.join(raw)).map_err(in_case)?;
        let case = parse_case_bytes(&bytes).map_err(in_case)?;
        if !ids.insert(case.id.clone()) {
            return Err(Problem::DuplicateCaseId(case.id));
        }
        cases.push(LoadedCase {
            case,
            sha256: sha256_hex(&bytes),
        });
    }
    Ok(Suite { version, cases })
}

fn read(path: &Path) -> Result<Vec<u8>, Problem> {
    fs::read(path).map_err(|error| Problem::Read {
        path: path.display().to_string(),
        message: error.to_string(),
    })
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Parses and checks one case file's bytes.
///
/// # Errors
///
/// Returns the first problem found.
pub fn parse_case_bytes(bytes: &[u8]) -> Result<Case, Problem> {
    let value = json::parse_strict(bytes).map_err(|error| Problem::Json(error.to_string()))?;
    parse_case(&value)
}

/// Checks one case.
///
/// # Errors
///
/// Returns the first problem found.
pub fn parse_case(value: &Value) -> Result<Case, Problem> {
    let Value::Object(fields) = value else {
        return Err(Problem::NotAnObject);
    };
    only_fields(
        fields,
        &[
            "id",
            "title",
            "requirements",
            "axis",
            "kind",
            "steps",
            "pending",
        ],
    )?;
    let id = required_string(fields, "id")?;
    if !is_case_id(id) {
        return Err(Problem::InvalidCaseId(id.to_owned()));
    }
    let title = required_string(fields, "title")?;
    if title.trim().is_empty() {
        return Err(Problem::EmptyTitle);
    }
    let requirements = requirements(fields)?;
    let axis = required_string(fields, "axis")?;
    if !AXES.contains(&axis) {
        return Err(Problem::UnknownAxis(axis.to_owned()));
    }
    let kind = match required_string(fields, "kind")? {
        "accept" => Kind::Accept,
        "reject" => Kind::Reject,
        other => return Err(Problem::UnknownKind(other.to_owned())),
    };
    let body = match (fields.get("pending"), fields.get("steps")) {
        (Some(_), Some(_)) => return Err(Problem::PendingWithSteps),
        (Some(pending), None) => Body::Pending(parse_pending(pending)?),
        (None, Some(Value::Array(steps))) if !steps.is_empty() => {
            let steps = parse_steps(steps)?;
            check_kind(kind, &steps)?;
            Body::Steps(steps)
        }
        (None, Some(Value::Array(_)) | None) => return Err(Problem::RunnableWithoutSteps),
        (None, Some(_)) => {
            return Err(Problem::WrongType {
                field: "steps",
                expected: "a list",
            });
        }
    };
    Ok(Case {
        id: id.to_owned(),
        title: title.to_owned(),
        requirements,
        axis: axis.to_owned(),
        kind,
        body,
    })
}

fn only_fields(fields: &Map<String, Value>, allowed: &[&str]) -> Result<(), Problem> {
    match fields.keys().find(|key| !allowed.contains(&key.as_str())) {
        Some(key) => Err(Problem::UnknownField(key.clone())),
        None => Ok(()),
    }
}

fn required_string<'a>(
    fields: &'a Map<String, Value>,
    field: &'static str,
) -> Result<&'a str, Problem> {
    match fields.get(field) {
        Some(Value::String(text)) => Ok(text),
        Some(_) => Err(Problem::WrongType {
            field,
            expected: "a string",
        }),
        None => Err(Problem::MissingField(field)),
    }
}

fn requirements(fields: &Map<String, Value>) -> Result<Vec<String>, Problem> {
    let wrong = Problem::WrongType {
        field: "requirements",
        expected: "a list of requirement ids",
    };
    let Some(Value::Array(items)) = fields.get("requirements") else {
        return Err(if fields.contains_key("requirements") {
            wrong
        } else {
            Problem::MissingField("requirements")
        });
    };
    if items.is_empty() {
        return Err(Problem::NoRequirements);
    }
    let mut ids = Vec::with_capacity(items.len());
    for item in items {
        let Value::String(id) = item else {
            return Err(wrong);
        };
        if !is_requirement_id(id) {
            return Err(Problem::InvalidRequirementId(id.clone()));
        }
        if ids.contains(id) {
            return Err(Problem::DuplicateRequirement(id.clone()));
        }
        ids.push(id.clone());
    }
    Ok(ids)
}

fn parse_pending(value: &Value) -> Result<Pending, Problem> {
    let Value::Object(fields) = value else {
        return Err(Problem::WrongType {
            field: "pending",
            expected: "an object with followup and reason",
        });
    };
    only_fields(fields, &["followup", "reason"])?;
    let followup = required_string(fields, "followup")?;
    if !is_roadmap_id(followup) {
        return Err(Problem::InvalidRoadmapId(followup.to_owned()));
    }
    let reason = required_string(fields, "reason")?;
    if reason.trim().is_empty() {
        return Err(Problem::EmptyReason);
    }
    Ok(Pending {
        followup: followup.to_owned(),
        reason: reason.to_owned(),
    })
}

/// A runnable case must show its kind in some step. An operation step shows it by the outcome
/// it expects: `ok` or `partial` as a result, `error` or `unsupported` as a refusal.
fn check_kind(kind: Kind, steps: &[Step]) -> Result<(), Problem> {
    let found = steps
        .iter()
        .any(|step| match (kind, operation_outcome(step)) {
            (Kind::Reject, Some(outcome)) => matches!(outcome, "error" | "unsupported"),
            (Kind::Accept, Some(outcome)) => matches!(outcome, "ok" | "partial"),
            (Kind::Reject, None) => matches!(step.expectation, Expectation::Error(_)),
            (Kind::Accept, None) => {
                matches!(step.expectation, Expectation::Response(_))
                    && !matches!(step.call, Call::Restart)
            }
        });
    match (found, kind) {
        (true, _) => Ok(()),
        (false, Kind::Reject) => Err(Problem::RejectWithoutErrorStep),
        (false, Kind::Accept) => Err(Problem::AcceptWithoutExpectStep),
    }
}

/// The outcome an operation step expects, if the step is one.
fn operation_outcome(step: &Step) -> Option<&str> {
    match (&step.call, &step.expectation) {
        (Call::Operation { .. }, Expectation::Response(expected)) => {
            expected.get("outcome").and_then(Value::as_str)
        }
        _ => None,
    }
}

/// `^[a-z0-9]+(-[a-z0-9]+)*$`, at most 80 characters.
pub fn is_case_id(id: &str) -> bool {
    id.len() <= 80
        && id.split('-').all(|word| {
            !word.is_empty()
                && word
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}

/// `^[A-Z]+-[0-9]{2}$`, such as `OPS-02`.
pub fn is_requirement_id(id: &str) -> bool {
    id.split_once('-').is_some_and(|(prefix, number)| {
        !prefix.is_empty()
            && prefix.bytes().all(|b| b.is_ascii_uppercase())
            && number.len() == 2
            && number.bytes().all(|b| b.is_ascii_digit())
    })
}

/// `^[A-Z][0-9]{1,3}$`, such as `P13`, `A1`, or `W01`.
pub fn is_roadmap_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    (2..=4).contains(&bytes.len())
        && bytes[0].is_ascii_uppercase()
        && bytes[1..].iter().all(u8::is_ascii_digit)
}

fn is_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Fields every step may carry, checked one by one below; the rest belong to the request body.
const STEP_FIELDS: [&str; 6] = [
    "call",
    "request_id",
    "document",
    "bind",
    "expect",
    "expect_error",
];

fn parse_steps(steps: &[Value]) -> Result<Vec<Step>, Problem> {
    let mut bound = HashSet::new();
    let mut parsed = Vec::with_capacity(steps.len());
    for (index, step) in steps.iter().enumerate() {
        let step = parse_step(index, step, &mut bound)
            .map_err(|problem| Problem::Step { index, problem })?;
        parsed.push(step);
    }
    Ok(parsed)
}

fn parse_step(
    index: usize,
    value: &Value,
    bound: &mut HashSet<String>,
) -> Result<Step, StepProblem> {
    let Value::Object(fields) = value else {
        return Err(StepProblem::NotAnObject);
    };
    let call = match fields.get("call") {
        Some(Value::String(call)) => call.as_str(),
        _ => {
            return Err(StepProblem::WrongType {
                field: "call",
                expected: "a string",
            });
        }
    };
    if call == "operation" {
        return parse_operation_step(fields, bound);
    }
    let (body_fields, writes, takes_document): (&[&str], bool, bool) = match call {
        "create" => (&["title"], true, false),
        "get" => (&[], false, true),
        "transact" => (&["expected_revision", "commands"], true, true),
        "undo" | "redo" | "save" => (&["expected_revision"], true, true),
        "autosave" | "recover" => (&[], true, true),
        "close" => (&["discard_unsaved", "expected_revision"], true, true),
        "capabilities" | "restart" => (&[], false, false),
        other => return Err(StepProblem::UnknownCall(other.to_owned())),
    };
    if let Some(field) = fields
        .keys()
        .find(|key| !STEP_FIELDS.contains(&key.as_str()) && !body_fields.contains(&key.as_str()))
    {
        return Err(StepProblem::UnknownField(field.clone()));
    }

    let request_id = match fields.get("request_id") {
        None => format!("step-{index}"),
        Some(_) if !writes => return Err(StepProblem::RequestIdNotTaken),
        Some(Value::String(id)) => id.clone(),
        Some(_) => {
            return Err(StepProblem::WrongType {
                field: "request_id",
                expected: "a string",
            });
        }
    };

    let expectation = match (fields.get("expect"), fields.get("expect_error")) {
        (Some(_), Some(_)) => return Err(StepProblem::BothExpectations),
        (Some(_), None) | (None, Some(_)) if call == "restart" => {
            return Err(StepProblem::ExpectationOnRestart);
        }
        (Some(expected), None) => {
            expect::validate_expect(expected).map_err(StepProblem::Expect)?;
            Expectation::Response(expected.clone())
        }
        (None, Some(expected)) => {
            expect::validate_expect_error(expected).map_err(StepProblem::ExpectError)?;
            Expectation::Error(expected.clone())
        }
        (None, None) => Expectation::Success,
    };

    let document = match fields.get("document") {
        None if takes_document => return Err(StepProblem::MissingDocument),
        None => None,
        Some(_) if !takes_document => return Err(StepProblem::DocumentNotTaken),
        Some(Value::String(name)) => {
            if !bound.contains(name) {
                return Err(StepProblem::UnboundDocument(name.clone()));
            }
            Some(name.clone())
        }
        Some(_) => {
            return Err(StepProblem::WrongType {
                field: "document",
                expected: "a string",
            });
        }
    };

    let bind = match fields.get("bind") {
        None => None,
        Some(_) if call != "create" => return Err(StepProblem::BindNotTaken),
        Some(_) if matches!(expectation, Expectation::Error(_)) => {
            return Err(StepProblem::BindExpectsError);
        }
        Some(Value::String(name)) => {
            if !is_name(name) {
                return Err(StepProblem::InvalidName(name.clone()));
            }
            if !bound.insert(name.clone()) {
                return Err(StepProblem::BoundTwice(name.clone()));
            }
            Some(name.clone())
        }
        Some(_) => {
            return Err(StepProblem::WrongType {
                field: "bind",
                expected: "a string",
            });
        }
    };

    let mut body = Map::new();
    if writes {
        body.insert("request_id".to_owned(), Value::String(request_id));
    }
    for field in body_fields {
        if let Some(value) = fields.get(*field) {
            body.insert((*field).to_owned(), value.clone());
        }
    }
    let body = Value::Object(body);
    // `document` is always Some for calls that take one, as checked above.
    let document = || document.clone().unwrap_or_default();
    let call = match call {
        "create" => Call::Create {
            bind,
            body: request(body)?,
        },
        "get" => Call::Get {
            document: document(),
        },
        "transact" => Call::Transact {
            document: document(),
            body: request(body)?,
        },
        "undo" => Call::Undo {
            document: document(),
            body: request(body)?,
        },
        "redo" => Call::Redo {
            document: document(),
            body: request(body)?,
        },
        "save" => Call::Save {
            document: document(),
            body: request(body)?,
        },
        "autosave" => Call::Autosave {
            document: document(),
            body: request(body)?,
        },
        "recover" => Call::Recover {
            document: document(),
            body: request(body)?,
        },
        "close" => Call::Close {
            document: document(),
            body: request(body)?,
        },
        "capabilities" => Call::Capabilities,
        "restart" => Call::Restart,
        other => return Err(StepProblem::UnknownCall(other.to_owned())),
    };
    Ok(Step { call, expectation })
}

/// An `operation` step: `request`, an optional bound `document` and `bind`, and an `expect`
/// naming the reply's `outcome`. It takes no `request_id` (the request carries its own) and no
/// `expect_error`, since every request is answered with an operation reply.
fn parse_operation_step(
    fields: &Map<String, Value>,
    bound: &mut HashSet<String>,
) -> Result<Step, StepProblem> {
    const FIELDS: [&str; 6] = [
        "call",
        "request",
        "document",
        "bind",
        "expect",
        "expect_error",
    ];
    if let Some(field) = fields.keys().find(|key| !FIELDS.contains(&key.as_str())) {
        return Err(if field == "request_id" {
            StepProblem::RequestIdNotTaken
        } else {
            StepProblem::UnknownField(field.clone())
        });
    }
    if fields.contains_key("expect_error") {
        return Err(StepProblem::OperationExpectsNoError);
    }
    let request = match fields.get("request") {
        Some(Value::Object(_)) => OperationRequest::Json(fields["request"].clone()),
        Some(Value::String(raw)) => OperationRequest::Raw(raw.clone()),
        Some(_) => {
            return Err(StepProblem::OperationRequest(
                "must be an object or a string",
            ));
        }
        None => return Err(StepProblem::OperationRequest("is missing")),
    };
    let expected = fields
        .get("expect")
        .ok_or(StepProblem::OperationExpectsOutcome)?;
    expect::validate_expect(expected).map_err(StepProblem::Expect)?;
    if !expected.get("outcome").is_some_and(Value::is_string) {
        return Err(StepProblem::OperationExpectsOutcome);
    }
    let document = match fields.get("document") {
        None => None,
        Some(Value::String(name)) => {
            if !bound.contains(name) {
                return Err(StepProblem::UnboundDocument(name.clone()));
            }
            let has_parameters = matches!(&request, OperationRequest::Json(json)
                if json.get("parameters").is_some_and(Value::is_object));
            if !has_parameters {
                return Err(StepProblem::OperationRequest(
                    "needs a parameters object to receive the document",
                ));
            }
            Some(name.clone())
        }
        Some(_) => {
            return Err(StepProblem::WrongType {
                field: "document",
                expected: "a string",
            });
        }
    };
    let bind = match fields.get("bind") {
        None => None,
        Some(Value::String(name)) => {
            if !is_name(name) {
                return Err(StepProblem::InvalidName(name.clone()));
            }
            if !bound.insert(name.clone()) {
                return Err(StepProblem::BoundTwice(name.clone()));
            }
            Some(name.clone())
        }
        Some(_) => {
            return Err(StepProblem::WrongType {
                field: "bind",
                expected: "a string",
            });
        }
    };
    Ok(Step {
        call: Call::Operation {
            request,
            document,
            bind,
        },
        expectation: Expectation::Response(expected.clone()),
    })
}

/// Deserializes a request body with the same types (and unknown-field refusal) as the HTTP
/// adapter.
fn request<T: DeserializeOwned>(body: Value) -> Result<T, StepProblem> {
    serde_json::from_value(body).map_err(|error| StepProblem::Body(error.to_string()))
}
