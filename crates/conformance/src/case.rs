//! Suites and cases: loading and the rules a case must meet before it runs.

use std::collections::{BTreeSet, HashSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use chemifree_application::CommandDto;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::expect;

/// A versioned list of cases, in run order.
#[derive(Clone, Debug, PartialEq)]
pub struct Suite {
    pub version: String,
    pub cases: Vec<Case>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Case {
    pub id: String,
    pub title: String,
    /// Requirement IDs such as `OPS-02`.
    pub requirements: Vec<String>,
    pub axis: Axis,
    pub kind: Kind,
    pub body: Body,
    /// SHA-256 of the case file's bytes, so a run record shows which case text it ran.
    pub sha256: String,
}

/// What a case compares. Axes are kept apart so that, for example, a coordinate match is
/// never counted as a graph match.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    Graph,
    Scene,
    Stereo,
    Numeric,
    Encoding,
    Host,
    History,
    Revision,
    Idempotency,
    Persistence,
    Validation,
    Capability,
    Contract,
}

/// Whether the case shows a result (`accept`) or an intentional refusal (`reject`). Results
/// are counted per kind, so refusals never count as results.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Accept,
    Reject,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Body {
    Runnable(Vec<Step>),
    /// Cannot run until the named roadmap item exists; reported as not run.
    Pending(Pending),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Pending {
    /// The roadmap item (e.g. `P19`) that makes the case runnable.
    pub followup: String,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    pub call: Call,
    pub expectation: Expectation,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Expectation {
    /// The call must succeed; its reply is not compared.
    Success,
    /// The call must succeed with a reply matching this subset.
    Reply(Value),
    /// The call must fail with an error envelope matching this subset.
    Error(Value),
}

/// One call on the shared `Application`, with the arguments the HTTP adapter passes. `doc`
/// names a document bound by an earlier `create` in the same case. A write without a
/// `request_id` is sent one generated from its step index, as HTTP always sends one.
///
/// `restart` drops the application without a normal stop, as killing the process would, and
/// opens a new one on the same data directory: only what storage holds survives. (A normal
/// stop of the HTTP service first drafts unsaved edits; `restart` does not.)
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(tag = "call", rename_all = "snake_case")]
pub enum Call {
    Create {
        doc: String,
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        request_id: Option<String>,
    },
    Get {
        doc: String,
    },
    Transact {
        doc: String,
        expected_revision: u64,
        commands: Vec<CommandDto>,
        #[serde(default)]
        request_id: Option<String>,
    },
    Undo {
        doc: String,
        expected_revision: u64,
        #[serde(default)]
        request_id: Option<String>,
    },
    Redo {
        doc: String,
        expected_revision: u64,
        #[serde(default)]
        request_id: Option<String>,
    },
    Save {
        doc: String,
        expected_revision: u64,
        #[serde(default)]
        request_id: Option<String>,
    },
    Autosave {
        doc: String,
        #[serde(default)]
        request_id: Option<String>,
    },
    Recover {
        doc: String,
        #[serde(default)]
        request_id: Option<String>,
    },
    Close {
        doc: String,
        #[serde(default)]
        discard_unsaved: bool,
        #[serde(default)]
        expected_revision: Option<u64>,
        #[serde(default)]
        request_id: Option<String>,
    },
    Capabilities {},
    Restart {},
}

/// Keys every step may carry besides its call's own fields.
const STEP_KEYS: [&str; 4] = ["call", "note", "expect", "expect_error"];

/// The fields each call takes. Steps flatten the call into one object, which serde cannot
/// check for unknown fields, so loading checks them against this table.
const CALL_FIELDS: [(&str, &[&str]); 11] = [
    ("create", &["doc", "title", "request_id"]),
    ("get", &["doc"]),
    (
        "transact",
        &["doc", "expected_revision", "commands", "request_id"],
    ),
    ("undo", &["doc", "expected_revision", "request_id"]),
    ("redo", &["doc", "expected_revision", "request_id"]),
    ("save", &["doc", "expected_revision", "request_id"]),
    ("autosave", &["doc", "request_id"]),
    ("recover", &["doc", "request_id"]),
    (
        "close",
        &["doc", "discard_unsaved", "expected_revision", "request_id"],
    ),
    ("capabilities", &[]),
    ("restart", &[]),
];

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
            Self::Capabilities {} => "capabilities",
            Self::Restart {} => "restart",
        }
    }

    /// The document name this call uses, if any.
    pub fn doc(&self) -> Option<&str> {
        match self {
            Self::Create { doc, .. }
            | Self::Get { doc }
            | Self::Transact { doc, .. }
            | Self::Undo { doc, .. }
            | Self::Redo { doc, .. }
            | Self::Save { doc, .. }
            | Self::Autosave { doc, .. }
            | Self::Recover { doc, .. }
            | Self::Close { doc, .. } => Some(doc),
            Self::Capabilities {} | Self::Restart {} => None,
        }
    }
}

/// Why a suite or case could not be loaded.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadError {
    pub file: PathBuf,
    pub case: Option<String>,
    pub step: Option<usize>,
    pub message: String,
}

impl fmt::Display for LoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.file.display())?;
        if let Some(case) = &self.case {
            write!(formatter, " case {case}")?;
        }
        if let Some(step) = self.step {
            write!(formatter, " step {step}")?;
        }
        write!(formatter, ": {}", self.message)
    }
}

impl std::error::Error for LoadError {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SuiteFile {
    suite_version: String,
    /// Case files, relative to the suite file, in run order.
    cases: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CaseFile {
    id: String,
    title: String,
    requirements: Vec<String>,
    axis: Axis,
    kind: Kind,
    #[serde(default)]
    steps: Option<Vec<Value>>,
    #[serde(default)]
    pending: Option<Pending>,
}

/// Loads a suite and every case it lists.
///
/// # Errors
///
/// Returns the first unreadable or malformed file, or the first case that breaks a rule
/// (see [`parse_case`]); case IDs must also be unique across the suite.
pub fn load_suite(path: &Path) -> Result<Suite, LoadError> {
    let error = |message: String| LoadError {
        file: path.to_owned(),
        case: None,
        step: None,
        message,
    };
    let text = fs::read_to_string(path).map_err(|failure| error(failure.to_string()))?;
    let file: SuiteFile =
        serde_json::from_str(&text).map_err(|failure| error(failure.to_string()))?;
    if file.suite_version.trim().is_empty() {
        return Err(error("suite_version is empty".to_owned()));
    }
    let base = path.parent().unwrap_or(Path::new(""));
    let mut cases = Vec::with_capacity(file.cases.len());
    for relative in &file.cases {
        if !is_plain_relative(relative) {
            return Err(error(format!(
                "case path {relative:?} must be relative to the suite file, without empty, '.', or '..' segments"
            )));
        }
        let case_path = base.join(relative);
        let text = fs::read_to_string(&case_path).map_err(|failure| LoadError {
            file: case_path.clone(),
            case: None,
            step: None,
            message: failure.to_string(),
        })?;
        cases.push(parse_case(&text).map_err(|failure| LoadError {
            file: case_path.clone(),
            ..failure
        })?);
    }
    check_unique_ids(&cases).map_err(|id| error(format!("case id {id:?} is used twice")))?;
    Ok(Suite {
        version: file.suite_version,
        cases,
    })
}

/// A suite entry made of plain segments, checked on the text as written (as the JS check does):
/// not absolute, no drive prefix, and no empty, `.`, or `..` segment between `/` or backslashes.
fn is_plain_relative(entry: &str) -> bool {
    let drive = entry.as_bytes().get(1) == Some(&b':') && entry.as_bytes()[0].is_ascii_alphabetic();
    !drive
        && entry
            .split(['/', '\\'])
            .all(|segment| !matches!(segment, "" | "." | ".."))
}

/// The first case ID used more than once.
fn check_unique_ids(cases: &[Case]) -> Result<(), String> {
    let mut seen = HashSet::new();
    for case in cases {
        if !seen.insert(case.id.as_str()) {
            return Err(case.id.clone());
        }
    }
    Ok(())
}

/// Parses one case and checks it:
///
/// - the ID is lowercase words joined by `-`, and requirement IDs look like `OPS-02`;
/// - a pending case has no steps, and a runnable case has steps;
/// - each step's fields are the ones its call takes (`request_id` only where the call takes
///   one), it expects at most one of a reply or an error, and its expectation uses operators
///   correctly; an expected reply is an object and an expected error names its `code`;
///   neither may hold an empty object or an empty `$contains` at any depth, and each must
///   compare at least one value other than `api_version` (see [`expect::validate`]);
///   `restart` expects nothing;
/// - a document is used only after a `create` binds its name, and is bound once;
/// - a runnable `reject` case expects an error in some step, and a runnable `accept` case
///   expects a successful reply in some step. Because of the rules above, those expectations
///   always check something.
///
/// The returned error leaves `file` empty for the caller to fill in.
///
/// # Errors
///
/// Returns the first broken rule.
pub fn parse_case(text: &str) -> Result<Case, LoadError> {
    let file: CaseFile = serde_json::from_str(text).map_err(|failure| LoadError {
        file: PathBuf::new(),
        case: None,
        step: None,
        message: failure.to_string(),
    })?;
    let CaseFile {
        id,
        title,
        requirements,
        axis,
        kind,
        steps,
        pending,
    } = file;
    let case_error = |step: Option<usize>, message: String| LoadError {
        file: PathBuf::new(),
        case: Some(id.clone()),
        step,
        message,
    };
    if !is_case_id(&id) {
        return Err(case_error(
            None,
            "a case id is lowercase letters and digits in words joined by '-'".to_owned(),
        ));
    }
    if title.trim().is_empty() {
        return Err(case_error(None, "title is empty".to_owned()));
    }
    if requirements.is_empty() {
        return Err(case_error(
            None,
            "a case names at least one requirement".to_owned(),
        ));
    }
    let mut seen = BTreeSet::new();
    for requirement in &requirements {
        if !is_requirement_id(requirement) {
            return Err(case_error(
                None,
                format!("{requirement:?} is not a requirement id like \"OPS-02\""),
            ));
        }
        if !seen.insert(requirement.as_str()) {
            return Err(case_error(None, format!("{requirement} is listed twice")));
        }
    }
    let body = match (steps, pending) {
        (Some(_), Some(_)) => {
            return Err(case_error(
                None,
                "a pending case cannot have steps".to_owned(),
            ));
        }
        (None, Some(pending)) => {
            if !is_roadmap_id(&pending.followup) {
                return Err(case_error(
                    None,
                    format!("{:?} is not a roadmap id like \"P19\"", pending.followup),
                ));
            }
            if pending.reason.trim().is_empty() {
                return Err(case_error(None, "a pending case gives a reason".to_owned()));
            }
            Body::Pending(pending)
        }
        (Some(steps), None) if !steps.is_empty() => {
            let steps = steps_of(&steps, &case_error)?;
            check_kind(kind, &steps).map_err(|message| case_error(None, message))?;
            Body::Runnable(steps)
        }
        (Some(_) | None, None) => {
            return Err(case_error(
                None,
                "a runnable case needs steps; a case that cannot run yet is pending".to_owned(),
            ));
        }
    };
    Ok(Case {
        id,
        title,
        requirements,
        axis,
        kind,
        body,
        sha256: hex(&Sha256::digest(text.as_bytes())),
    })
}

fn steps_of(
    raw: &[Value],
    case_error: &impl Fn(Option<usize>, String) -> LoadError,
) -> Result<Vec<Step>, LoadError> {
    let mut bound = HashSet::new();
    let mut steps = Vec::with_capacity(raw.len());
    for (index, value) in raw.iter().enumerate() {
        let step = parse_step(value).map_err(|message| case_error(Some(index), message))?;
        if let Some(doc) = step.call.doc() {
            let creates = matches!(step.call, Call::Create { .. });
            if creates && !bound.insert(doc.to_owned()) {
                return Err(case_error(
                    Some(index),
                    format!("document {doc:?} is already bound by an earlier create"),
                ));
            }
            if !creates && !bound.contains(doc) {
                return Err(case_error(
                    Some(index),
                    format!("document {doc:?} is used before a create binds it"),
                ));
            }
        }
        steps.push(step);
    }
    Ok(steps)
}

/// Parses one step, checking its fields explicitly (see [`CALL_FIELDS`]).
///
/// # Errors
///
/// Returns why the step is refused: not an object, an unknown call, a field its call does not
/// take, both `expect` and `expect_error`, a misused operator, or a malformed call field.
pub fn parse_step(value: &Value) -> Result<Step, String> {
    let Value::Object(object) = value else {
        return Err("a step is an object".to_owned());
    };
    let Some(Value::String(name)) = object.get("call") else {
        return Err("a step names its call in \"call\"".to_owned());
    };
    let Some((_, fields)) = CALL_FIELDS.iter().find(|(call, _)| call == name) else {
        return Err(format!("unknown call {name:?}"));
    };
    for key in object.keys() {
        if STEP_KEYS.contains(&key.as_str()) || fields.contains(&key.as_str()) {
            continue;
        }
        return Err(if key == "request_id" {
            format!("{name} does not take a request_id")
        } else {
            format!("unknown field {key:?} for {name}")
        });
    }
    let expectation = match (object.get("expect"), object.get("expect_error")) {
        (Some(_), Some(_)) => {
            return Err("a step expects a reply or an error, not both".to_owned());
        }
        (Some(_), None) | (None, Some(_)) if name == "restart" => {
            return Err("restart has no reply to expect".to_owned());
        }
        (Some(expected), None) => {
            if !expected.is_object() {
                return Err("expect is an object of reply fields".to_owned());
            }
            Expectation::Reply(expected.clone())
        }
        (None, Some(expected)) => {
            if !expected.get("code").is_some_and(|code| code.is_string()) {
                return Err("expect_error names the error's code".to_owned());
            }
            Expectation::Error(expected.clone())
        }
        (None, None) => Expectation::Success,
    };
    if let Expectation::Reply(expected) | Expectation::Error(expected) = &expectation {
        expect::validate(expected).map_err(|mismatch| format!("expectation {mismatch}"))?;
    }
    let call_fields: Map<String, Value> = object
        .iter()
        .filter(|(key, _)| !matches!(key.as_str(), "note" | "expect" | "expect_error"))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    let call: Call = serde_json::from_value(Value::Object(call_fields))
        .map_err(|failure| format!("{name}: {failure}"))?;
    Ok(Step { call, expectation })
}

/// A runnable case of either kind must show what it claims in at least one step.
fn check_kind(kind: Kind, steps: &[Step]) -> Result<(), String> {
    let expects =
        |wanted: fn(&Expectation) -> bool| steps.iter().any(|step| wanted(&step.expectation));
    match kind {
        Kind::Reject if !expects(|expectation| matches!(expectation, Expectation::Error(_))) => {
            Err("a reject case expects an error (expect_error) in some step".to_owned())
        }
        Kind::Accept if !expects(|expectation| matches!(expectation, Expectation::Reply(_))) => {
            Err("an accept case expects a successful reply (expect) in some step".to_owned())
        }
        Kind::Accept | Kind::Reject => Ok(()),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn is_case_id(id: &str) -> bool {
    !id.is_empty()
        && id.split('-').all(|word| {
            !word.is_empty()
                && word
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        })
}

/// `MODEL-01`, `CLEANROOM-01`: an uppercase word, `-`, two digits.
pub fn is_requirement_id(id: &str) -> bool {
    let Some((word, number)) = id.split_once('-') else {
        return false;
    };
    word.bytes()
        .next()
        .is_some_and(|byte| byte.is_ascii_uppercase())
        && word
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
        && number.len() == 2
        && number.bytes().all(|byte| byte.is_ascii_digit())
}

/// `P19`, `A1`, `W01`, `S07`: an uppercase letter and digits.
fn is_roadmap_id(id: &str) -> bool {
    let mut bytes = id.bytes();
    bytes.next().is_some_and(|byte| byte.is_ascii_uppercase())
        && id.len() > 1
        && bytes.all(|byte| byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn create() -> Value {
        json!({ "call": "create", "doc": "d", "expect": { "document": { "revision": 0 } } })
    }

    fn case(kind: &str, steps: Value) -> String {
        json!({
            "id": "a-case",
            "title": "A case",
            "requirements": ["OPS-02"],
            "axis": "graph",
            "kind": kind,
            "steps": steps,
        })
        .to_string()
    }

    fn refusal(text: &str) -> LoadError {
        parse_case(text).expect_err("the case must be refused")
    }

    #[test]
    fn a_well_formed_case_loads() {
        let loaded = parse_case(&case(
            "reject",
            json!([
                create(),
                { "call": "get", "doc": "d" },
                { "call": "undo", "doc": "d", "expected_revision": 0, "request_id": "u",
                  "expect_error": { "code": "nothing_to_undo" } },
                { "call": "restart", "note": "only storage survives" },
                { "call": "capabilities" },
            ]),
        ))
        .unwrap();
        let Body::Runnable(steps) = &loaded.body else {
            panic!("expected a runnable case, got {:?}", loaded.body);
        };
        assert_eq!(steps.len(), 5);
        assert_eq!(
            steps[2],
            Step {
                call: Call::Undo {
                    doc: "d".to_owned(),
                    expected_revision: 0,
                    request_id: Some("u".to_owned()),
                },
                expectation: Expectation::Error(json!({ "code": "nothing_to_undo" })),
            }
        );
        assert_eq!(steps[1].expectation, Expectation::Success);
    }

    #[test]
    fn unknown_step_fields_are_refused_for_each_call() {
        let cases = [
            json!({ "call": "create", "doc": "d", "titel": "typo" }),
            json!({ "call": "get", "doc": "d", "expected_revision": 0 }),
            json!({ "call": "save", "doc": "d", "expected_revision": 0, "commands": [] }),
            json!({ "call": "restart", "doc": "d" }),
        ];
        for step in cases {
            let message = parse_step(&step).expect_err(&format!("{step} must be refused"));
            assert!(
                message.starts_with("unknown field"),
                "{step}: unexpected message {message:?}"
            );
        }
    }

    #[test]
    fn request_id_is_refused_on_calls_that_take_none() {
        for call in ["get", "capabilities", "restart"] {
            let mut step = json!({ "call": call, "request_id": "r" });
            if call == "get" {
                step["doc"] = json!("d");
            }
            assert_eq!(
                parse_step(&step),
                Err(format!("{call} does not take a request_id")),
                "{step}"
            );
        }
    }

    #[test]
    fn unknown_calls_and_steps_without_a_call_are_refused() {
        assert_eq!(
            parse_step(&json!({ "call": "delete", "doc": "d" })),
            Err("unknown call \"delete\"".to_owned())
        );
        assert!(parse_step(&json!({ "doc": "d" })).is_err());
        assert!(parse_step(&json!("get")).is_err());
    }

    #[test]
    fn a_step_cannot_expect_both_a_reply_and_an_error() {
        let step = json!({ "call": "get", "doc": "d", "expect": {}, "expect_error": {} });
        assert_eq!(
            parse_step(&step),
            Err("a step expects a reply or an error, not both".to_owned())
        );
    }

    #[test]
    fn restart_expects_nothing() {
        for step in [
            json!({ "call": "restart", "expect": {} }),
            json!({ "call": "restart", "expect": { "api_version": "v1" } }),
            json!({ "call": "restart", "expect_error": { "code": "x" } }),
        ] {
            assert_eq!(
                parse_step(&step),
                Err("restart has no reply to expect".to_owned()),
                "{step}"
            );
        }
    }

    #[test]
    fn an_expected_reply_is_an_object() {
        for expected in [json!([]), json!(null), json!(1), json!("v1")] {
            let step = json!({ "call": "get", "doc": "d", "expect": expected });
            assert_eq!(
                parse_step(&step),
                Err("expect is an object of reply fields".to_owned()),
                "{step}"
            );
        }
    }

    #[test]
    fn vacuous_expectations_are_refused_at_any_depth() {
        let cases = [
            (json!({}), "expectation $: an empty object checks nothing"),
            (
                json!({ "document": {} }),
                "expectation $.document: an empty object checks nothing",
            ),
            (
                json!({ "document": { "atoms": { "$contains": [] } } }),
                "expectation $.document.atoms: $contains needs at least one item",
            ),
            (
                json!({ "document": { "atoms": { "$contains": [{}] } } }),
                "expectation $.document.atoms.$contains[0]: an empty object checks nothing",
            ),
            (
                json!({ "api_version": "v1" }),
                "expectation $: an expectation names at least one value besides api_version",
            ),
        ];
        for (expected, message) in cases {
            let step = json!({ "call": "get", "doc": "d", "expect": expected });
            assert_eq!(parse_step(&step), Err(message.to_owned()), "{step}");
        }
        let error =
            json!({ "call": "get", "doc": "d", "expect_error": { "code": "x", "detail": {} } });
        assert_eq!(
            parse_step(&error),
            Err("expectation $.detail: an empty object checks nothing".to_owned())
        );
    }

    #[test]
    fn an_expected_error_names_its_code() {
        for expected in [
            json!({}),
            json!({ "message": "x" }),
            json!({ "code": 1 }),
            json!("revision_conflict"),
        ] {
            let step = json!({ "call": "get", "doc": "d", "expect_error": expected });
            assert_eq!(
                parse_step(&step),
                Err("expect_error names the error's code".to_owned()),
                "{step}"
            );
        }
    }

    #[test]
    fn kind_rules_cannot_be_met_by_expectations_that_check_nothing() {
        let restart_only = case("accept", json!([{ "call": "restart", "expect": {} }]));
        assert_eq!(
            refusal(&restart_only).message,
            "restart has no reply to expect"
        );
        let empty_error = case(
            "reject",
            json!([
                { "call": "create", "doc": "d" },
                { "call": "undo", "doc": "d", "expected_revision": 7, "expect_error": {} },
            ]),
        );
        assert_eq!(
            refusal(&empty_error).message,
            "expect_error names the error's code"
        );
    }

    #[test]
    fn a_case_records_the_sha256_of_its_file() {
        // Digest computed with sha256sum over exactly this text.
        let text = r#"{"id":"a-case","title":"T","requirements":["CHEM-01"],"axis":"numeric","kind":"accept","pending":{"followup":"P19","reason":"not built"}}"#;
        assert_eq!(
            parse_case(text).unwrap().sha256,
            "c95baeab6a4319051b2aa9369c6cf66c69f6e64c66050ba35d09b4481efb8b5d"
        );
    }

    #[test]
    fn a_misused_operator_is_refused_when_loading() {
        let step = json!({ "call": "get", "doc": "d", "expect": { "document": { "atoms": { "$contains": {} } } } });
        let message = parse_step(&step).expect_err("misuse must be refused");
        assert!(message.contains("$contains needs a list"), "{message}");
        let siblings = json!({ "call": "capabilities",
            "expect": { "capabilities": { "$contains": [], "id": "x" } } });
        let message = parse_step(&siblings).expect_err("siblings must be refused");
        assert!(message.contains("cannot have sibling keys"), "{message}");
    }

    #[test]
    fn a_document_is_used_only_after_a_create_binds_it() {
        let used_first = refusal(&case(
            "accept",
            json!([{ "call": "get", "doc": "d" }, create()]),
        ));
        assert_eq!(used_first.step, Some(0));
        assert_eq!(
            used_first.message,
            "document \"d\" is used before a create binds it"
        );
        let other_name = refusal(&case(
            "accept",
            json!([create(), { "call": "get", "doc": "e" }]),
        ));
        assert_eq!(other_name.step, Some(1));
    }

    #[test]
    fn a_document_name_is_bound_once() {
        let twice = refusal(&case("accept", json!([create(), create()])));
        assert_eq!(twice.step, Some(1));
        assert!(twice.message.contains("already bound"), "{}", twice.message);
    }

    #[test]
    fn malformed_requirement_ids_are_refused() {
        for id in [
            "ops-02", "OPS-2", "OPS02", "-01", "OPS-012", "OPS-0a", "1OPS-01", "",
        ] {
            let text = json!({ "id": "a-case", "title": "T", "requirements": [id],
                "axis": "graph", "kind": "accept", "steps": [create()] })
            .to_string();
            let message = refusal(&text).message;
            assert!(
                message.contains("is not a requirement id"),
                "{id:?}: {message}"
            );
        }
        for id in ["OPS-02", "CLEANROOM-01", "WIN-02"] {
            assert!(is_requirement_id(id), "{id} is well formed");
        }
    }

    #[test]
    fn requirements_are_named_once_and_at_least_once() {
        let text = json!({ "id": "a-case", "title": "T", "requirements": ["OPS-02", "OPS-02"],
            "axis": "graph", "kind": "accept", "steps": [create()] })
        .to_string();
        assert_eq!(refusal(&text).message, "OPS-02 is listed twice");
        let none = json!({ "id": "a-case", "title": "T", "requirements": [],
            "axis": "graph", "kind": "accept", "steps": [create()] })
        .to_string();
        assert!(refusal(&none).message.contains("at least one requirement"));
    }

    #[test]
    fn malformed_case_ids_are_refused() {
        for id in ["A-case", "a_case", "a--case", "-a", "a-", ""] {
            let text = json!({ "id": id, "title": "T", "requirements": ["OPS-02"],
                "axis": "graph", "kind": "accept", "steps": [create()] })
            .to_string();
            assert!(
                refusal(&text).message.starts_with("a case id is"),
                "{id:?} must be refused"
            );
        }
    }

    #[test]
    fn a_pending_case_cannot_have_steps_even_empty_ones() {
        for steps in [json!([create()]), json!([])] {
            let text = json!({ "id": "a-case", "title": "T", "requirements": ["CHEM-01"],
                "axis": "numeric", "kind": "accept", "steps": steps,
                "pending": { "followup": "P19", "reason": "not built" } })
            .to_string();
            assert_eq!(refusal(&text).message, "a pending case cannot have steps");
        }
    }

    #[test]
    fn a_pending_case_names_a_roadmap_item_and_a_reason() {
        let pending = |followup: &str, reason: &str| {
            json!({ "id": "a-case", "title": "T", "requirements": ["CHEM-01"],
                "axis": "numeric", "kind": "accept",
                "pending": { "followup": followup, "reason": reason } })
            .to_string()
        };
        let loaded = parse_case(&pending("P19", "not built")).unwrap();
        assert_eq!(
            loaded.body,
            Body::Pending(Pending {
                followup: "P19".to_owned(),
                reason: "not built".to_owned(),
            })
        );
        assert!(refusal(&pending("p19", "x")).message.contains("roadmap id"));
        assert!(refusal(&pending("P", "x")).message.contains("roadmap id"));
        assert!(
            refusal(&pending("P19", " "))
                .message
                .contains("gives a reason")
        );
    }

    #[test]
    fn a_runnable_case_needs_steps() {
        let absent = json!({ "id": "a-case", "title": "T", "requirements": ["OPS-02"],
            "axis": "graph", "kind": "accept" })
        .to_string();
        let empty = case("accept", json!([]));
        for text in [absent, empty] {
            assert!(
                refusal(&text)
                    .message
                    .starts_with("a runnable case needs steps"),
                "{text}"
            );
        }
    }

    #[test]
    fn a_reject_case_needs_a_step_expecting_an_error() {
        let refused = refusal(&case(
            "reject",
            json!([create(), { "call": "get", "doc": "d" }]),
        ));
        assert_eq!(
            refused.message,
            "a reject case expects an error (expect_error) in some step"
        );
    }

    #[test]
    fn an_accept_case_needs_a_step_expecting_a_successful_reply() {
        // Expected errors only: counting this as accepted would count refusals as results.
        let errors_only = case(
            "accept",
            json!([
                { "call": "create", "doc": "d" },
                { "call": "undo", "doc": "d", "expected_revision": 0,
                  "expect_error": { "code": "nothing_to_undo" } },
            ]),
        );
        let message = "an accept case expects a successful reply (expect) in some step";
        assert_eq!(refusal(&errors_only).message, message);
        let unchecked = case("accept", json!([{ "call": "create", "doc": "d" }]));
        assert_eq!(refusal(&unchecked).message, message);
    }

    #[test]
    fn unknown_case_fields_axes_and_kinds_are_refused() {
        let extra = json!({ "id": "a-case", "title": "T", "requirements": ["OPS-02"],
            "axis": "graph", "kind": "accept", "steps": [create()], "skip": true })
        .to_string();
        assert!(refusal(&extra).message.contains("unknown field"));
        assert!(
            refusal(&case("maybe", json!([create()])))
                .message
                .contains("unknown variant")
        );
        let axis = json!({ "id": "a-case", "title": "T", "requirements": ["OPS-02"],
            "axis": "vibes", "kind": "accept", "steps": [create()] })
        .to_string();
        assert!(refusal(&axis).message.contains("unknown variant"));
    }

    fn write_suite(directory: &Path, cases: &[(&str, String)]) -> PathBuf {
        let mut names = Vec::new();
        for (name, text) in cases {
            fs::write(directory.join(name), text).unwrap();
            names.push(*name);
        }
        let suite = directory.join("suite.json");
        fs::write(
            &suite,
            json!({ "suite_version": "1.0.0", "cases": names }).to_string(),
        )
        .unwrap();
        suite
    }

    #[test]
    fn a_suite_loads_its_cases_in_order() {
        let directory = tempfile::tempdir().unwrap();
        let second = case("accept", json!([create()])).replace("a-case", "b-case");
        let suite = write_suite(
            directory.path(),
            &[
                ("b.json", second),
                ("a.json", case("accept", json!([create()]))),
            ],
        );
        let loaded = load_suite(&suite).unwrap();
        assert_eq!(loaded.version, "1.0.0");
        let ids: Vec<&str> = loaded.cases.iter().map(|case| case.id.as_str()).collect();
        assert_eq!(ids, ["b-case", "a-case"]);
    }

    #[test]
    fn duplicate_case_ids_are_refused() {
        let directory = tempfile::tempdir().unwrap();
        let text = case("accept", json!([create()]));
        let suite = write_suite(
            directory.path(),
            &[("one.json", text.clone()), ("two.json", text)],
        );
        let refused = load_suite(&suite).expect_err("duplicate ids must be refused");
        assert_eq!(refused.message, "case id \"a-case\" is used twice");
        assert_eq!(refused.file, suite);
    }

    #[test]
    fn case_paths_stay_under_the_suite_file() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("a.json"),
            case("accept", json!([create()])),
        )
        .unwrap();
        for entry in [
            "../a.json",
            "/tmp/a.json",
            "./a.json",
            "cases/../a.json",
            "cases//a.json",
            "cases/./a.json",
            "a.json/",
            "C:a.json",
            "\\a.json",
            "",
        ] {
            let suite = directory.path().join("suite.json");
            fs::write(
                &suite,
                json!({ "suite_version": "1.0.0", "cases": [entry] }).to_string(),
            )
            .unwrap();
            let refused = load_suite(&suite).expect_err(entry);
            assert!(
                refused
                    .message
                    .contains("must be relative to the suite file"),
                "{entry:?}: {}",
                refused.message
            );
        }
    }

    #[test]
    fn a_case_error_names_its_file_case_and_step() {
        let directory = tempfile::tempdir().unwrap();
        let bad = case(
            "accept",
            json!([create(), { "call": "get", "doc": "d", "x": 1 }]),
        );
        let suite = write_suite(directory.path(), &[("bad.json", bad)]);
        let refused = load_suite(&suite).expect_err("the bad step must be refused");
        assert_eq!(refused.file, directory.path().join("bad.json"));
        assert_eq!(refused.case.as_deref(), Some("a-case"));
        assert_eq!(refused.step, Some(1));
        assert_eq!(refused.message, "unknown field \"x\" for get");
    }
}
