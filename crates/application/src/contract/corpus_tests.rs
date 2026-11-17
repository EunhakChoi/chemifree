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
        for other in [&case.schema, &case.web].into_iter().flatten() {
            assert!(
                !other.why.trim().is_empty(),
                "case {:?}: an override needs a reason",
                case.name
            );
            assert_ne!(
                other.verdict, case.expected,
                "case {:?}: an override that repeats the serde verdict is stale",
                case.name
            );
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

#[cfg(all(test, feature = "schema"))]
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
