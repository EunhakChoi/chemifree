//! The command corpus, and the command schema's golden file when the `schema` feature is on.
//! Each corpus case records serde's verdict on one command. The verdict is checked through the
//! typed read and through the operation envelope's read, so the two cannot drift apart unnoticed.

use std::collections::BTreeSet;

use serde::Deserialize;
use serde_json::Value;

use super::command::CommandDto;
use crate::json;

/// One corpus case. Exactly one of `command` (a JSON value, written back as text) and `raw`
/// (text sent as it is, which can repeat a key) is set. `expected` is serde's verdict. `schema`
/// and `web` name a consumer that reaches a different verdict, and why.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    name: String,
    command: Option<Value>,
    raw: Option<String>,
    #[serde(rename = "serde")]
    expected: Verdict,
    schema: Option<Override>,
    web: Option<Override>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Verdict {
    Accept,
    Reject,
}

/// A verdict another consumer reaches, with the reason it differs from serde's.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Override {
    verdict: Verdict,
    why: String,
}

const CORPUS: &str = include_str!("../../tests/data/command-schema/corpus.json");

/// The start of the reason for a schema reject where serde accepts. The schema describes only the
/// documented object and string forms, so such a case is a form serde reads but the contract does
/// not document.
const UNDOCUMENTED_FORM: &str = "undocumented serde form:";

/// The typed read: `CommandDto` deserialized straight from the text.
fn read_typed(text: &str) -> Result<(), String> {
    serde_json::from_str::<CommandDto>(text)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// The operation envelope's read: the strict parser refuses a repeated key at any depth, then
/// serde reads the value, as `Application::execute_with` does with a request's parameters.
fn read_envelope(text: &str) -> Result<(), String> {
    let value = json::parse_strict(text.as_bytes()).map_err(|error| error.to_string())?;
    serde_json::from_value::<CommandDto>(value)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn assert_verdict(case: &Case, text: &str, path: &str, read: Result<(), String>) {
    let (actual, detail) = match read {
        Ok(()) => (Verdict::Accept, "accepted".to_owned()),
        Err(error) => (Verdict::Reject, format!("refused: {error}")),
    };
    assert_eq!(
        actual, case.expected,
        "case {:?} via {path}, input {text}: expected {:?}, got {detail}",
        case.name, case.expected
    );
}

/// Checks an override against its case. It needs a reason, and it has to differ from serde's
/// verdict, since an override that repeats serde's verdict is stale.
fn check_override(case: &Case, other: &Override) -> Result<(), String> {
    if other.why.trim().is_empty() {
        return Err(format!("case {:?}: an override needs a reason", case.name));
    }
    if other.verdict == case.expected {
        return Err(format!(
            "case {:?}: an override that repeats the serde verdict is stale",
            case.name
        ));
    }
    Ok(())
}

/// Checks a schema override. A schema reject where serde accepts refuses a form serde reads, so
/// its reason must name that form with [`UNDOCUMENTED_FORM`].
fn check_schema_override(case: &Case, schema: &Override) -> Result<(), String> {
    check_override(case, schema)?;
    let rejects_an_accepted_form =
        schema.verdict == Verdict::Reject && case.expected == Verdict::Accept;
    if rejects_an_accepted_form && !schema.why.starts_with(UNDOCUMENTED_FORM) {
        return Err(format!(
            "case {:?}: a schema reject where serde accepts needs a reason that starts \
             with {UNDOCUMENTED_FORM:?}",
            case.name
        ));
    }
    Ok(())
}

#[test]
fn command_corpus_verdicts_match_serde_on_both_read_paths() {
    let cases: Vec<Case> = serde_json::from_str(CORPUS)
        .unwrap_or_else(|error| panic!("the command corpus does not match its shape: {error}"));
    assert!(!cases.is_empty(), "the command corpus has no cases");
    let mut names = BTreeSet::new();
    for case in &cases {
        assert!(
            names.insert(case.name.as_str()),
            "the case name {:?} is used twice",
            case.name
        );
        let text = match (&case.command, &case.raw) {
            (Some(command), None) => command.to_string(),
            (None, Some(raw)) => raw.clone(),
            _ => panic!("case {:?} needs exactly one of command and raw", case.name),
        };
        // A refusal has to be about the command, not about text that is not JSON at all.
        assert!(
            serde_json::from_str::<Value>(&text).is_ok(),
            "case {:?}: its text is not JSON: {text}",
            case.name
        );
        if let Some(schema) = &case.schema {
            check_schema_override(case, schema).unwrap_or_else(|message| panic!("{message}"));
        }
        if let Some(web) = &case.web {
            check_override(case, web).unwrap_or_else(|message| panic!("{message}"));
        }
        assert_verdict(
            case,
            &text,
            "serde_json::from_str::<CommandDto>",
            read_typed(&text),
        );
        assert_verdict(case, &text, "operation envelope", read_envelope(&text));
    }
}

#[test]
fn schema_reject_of_an_accepted_form_needs_the_undocumented_form_prefix() {
    let accepted = Case {
        name: "synthetic: serde accepts, the schema refuses".to_owned(),
        command: None,
        raw: Some(r#"["rename","x"]"#.to_owned()),
        expected: Verdict::Accept,
        schema: None,
        web: None,
    };
    let unprefixed = Override {
        verdict: Verdict::Reject,
        why: "the schema describes only the object form".to_owned(),
    };
    let message = check_schema_override(&accepted, &unprefixed)
        .expect_err("a schema reject where serde accepts must name its undocumented form");
    assert!(
        message.contains(UNDOCUMENTED_FORM),
        "the refusal must name the prefix, got {message}"
    );

    let prefixed = Override {
        verdict: Verdict::Reject,
        why: format!("{UNDOCUMENTED_FORM} the schema describes only the object form"),
    };
    assert_eq!(check_schema_override(&accepted, &prefixed), Ok(()));
}

#[test]
fn schema_accept_of_a_refused_form_needs_only_a_reason() {
    let refused = Case {
        name: "synthetic: serde refuses, the schema accepts".to_owned(),
        command: None,
        raw: Some("5e0".to_owned()),
        expected: Verdict::Reject,
        schema: None,
        web: None,
    };
    let accepting = Override {
        verdict: Verdict::Accept,
        why: "JSON Schema integer admits 5e0".to_owned(),
    };
    assert_eq!(check_schema_override(&refused, &accepting), Ok(()));
}

#[cfg(feature = "schema")]
#[test]
fn command_schema_matches_the_golden_file() {
    const BLESS: &str = "CHEMIFREE_BLESS_COMMAND_SCHEMA";
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/command-schema/commands.schema.json");
    let actual = super::schema::command_schema_text();
    if std::env::var_os(BLESS).is_some() {
        std::fs::write(&path, &actual)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    assert_eq!(
        actual,
        expected,
        "the command schema differs from {}; review the change, then bless with {BLESS}=1",
        path.display()
    );
}
