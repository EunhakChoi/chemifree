//! Runner behavior: how steps reach the application and how outcomes are judged.

use chemifree_conformance::case::parse_case;
use chemifree_conformance::run_case;
use serde_json::{Value, json};

fn run(kind: &str, steps: Value) -> Result<(), String> {
    let case = parse_case(&json!({
        "id": "sample", "title": "Sample", "requirements": ["OPS-02"],
        "axis": "history", "kind": kind, "steps": steps,
    }))
    .expect("the test case loads");
    run_case(&case)
}

fn add_carbon(revision: u64) -> Value {
    json!({
        "call": "transact", "document": "d", "expected_revision": revision,
        "commands": [{"op": "add_atom", "element": "C", "position": {"x": 0, "y": 0}}]
    })
}

#[test]
fn generated_request_ids_differ_per_step() {
    // Two identical writes without request_id both apply: they are not replays of each other.
    let outcome = run(
        "accept",
        json!([
            {"call": "create", "bind": "d"},
            add_carbon(0),
            add_carbon(1),
            {"call": "get", "document": "d", "expect": {"document": {"revision": 2}}}
        ]),
    );
    assert_eq!(outcome, Ok(()));
}

#[test]
fn a_given_request_id_is_sent_as_is() {
    let mut first = add_carbon(0);
    first["request_id"] = json!("same");
    let outcome = run(
        "accept",
        json!([
            {"call": "create", "bind": "d"},
            first.clone(),
            first,
            {"call": "get", "document": "d", "expect": {"document": {"revision": 1}}}
        ]),
    );
    assert_eq!(outcome, Ok(()));
}

#[test]
fn a_wrong_expectation_fails_the_case() {
    let outcome = run(
        "accept",
        json!([
            {"call": "create", "bind": "d"},
            add_carbon(0),
            {"call": "get", "document": "d", "expect": {"document": {"revision": 2}}}
        ]),
    );
    assert_eq!(
        outcome,
        Err("step 2 (get): response at \"/document/revision\": expected 2, found 1".to_owned())
    );
}

#[test]
fn an_unexpected_error_fails_the_case() {
    let outcome = run(
        "accept",
        json!([
            {"call": "create", "bind": "d"},
            add_carbon(5),
            {"call": "get", "document": "d", "expect": {"document": {"revision": 0}}}
        ]),
    );
    let failure = outcome.unwrap_err();
    assert!(
        failure.starts_with("step 1 (transact): expected success, got error")
            && failure.contains("revision_conflict"),
        "{failure}"
    );
}

#[test]
fn an_expected_error_that_does_not_happen_fails_the_case() {
    let mut step = add_carbon(0);
    step["expect_error"] = json!({"code": "revision_conflict"});
    let outcome = run("reject", json!([{"call": "create", "bind": "d"}, step]));
    assert_eq!(
        outcome,
        Err("step 1 (transact): expected an error, got success".to_owned())
    );
}

#[test]
fn the_error_envelope_is_matched() {
    let mut step = add_carbon(3);
    step["expect_error"] = json!({"code": "revision_conflict", "current_revision": 1});
    let outcome = run("reject", json!([{"call": "create", "bind": "d"}, step]));
    assert_eq!(
        outcome,
        Err("step 1 (transact): error at \"/current_revision\": expected 1, found 0".to_owned())
    );
}

#[test]
fn restart_loses_undrafted_edits_but_keeps_saves() {
    let outcome = run(
        "accept",
        json!([
            {"call": "create", "bind": "d"},
            add_carbon(0),
            {"call": "save", "document": "d", "expected_revision": 1},
            add_carbon(1),
            {"call": "restart"},
            {"call": "get", "document": "d", "expect": {
                "document": {"revision": 1, "atoms": [{"id": "a1"}]},
                "recovery": null
            }}
        ]),
    );
    assert_eq!(outcome, Ok(()));
}

#[test]
fn without_restart_the_edit_is_still_there() {
    // The counterpart of the restart test: the same steps without restart see revision 2,
    // so the restart step is what removed the edit.
    let outcome = run(
        "accept",
        json!([
            {"call": "create", "bind": "d"},
            add_carbon(0),
            {"call": "save", "document": "d", "expected_revision": 1},
            add_carbon(1),
            {"call": "get", "document": "d", "expect": {"document": {"revision": 1}}}
        ]),
    );
    assert!(outcome.is_err_and(|failure| failure.contains("found 2")));
}

#[test]
fn restart_releases_draft_ownership() {
    // A draft written before the restart is abandoned afterwards, not in use by a live owner.
    let outcome = run(
        "accept",
        json!([
            {"call": "create", "bind": "d"},
            add_carbon(0),
            {"call": "autosave", "document": "d"},
            {"call": "restart"},
            {"call": "get", "document": "d", "expect": {"recovery": {"state": "recoverable"}}}
        ]),
    );
    assert_eq!(outcome, Ok(()));
}

#[test]
fn each_run_starts_with_an_empty_data_directory() {
    // Run twice: a document saved by the first run must not exist in the second, which
    // creates its first document again at revision 0 with no recovery left over.
    for _ in 0..2 {
        let outcome = run(
            "accept",
            json!([
                {"call": "create", "bind": "d", "expect": {"document": {"revision": 0}, "recovery": null}},
                add_carbon(0),
                {"call": "autosave", "document": "d"}
            ]),
        );
        assert_eq!(outcome, Ok(()));
    }
}

fn operation_request(id: &str, operation: &str, parameters: Value) -> Value {
    json!({"contract": 1, "request_id": id, "profile": "preserve-semantics",
        "operation": operation, "parameters": parameters})
}

#[test]
fn an_operation_runs_on_its_bound_document_and_binds_the_one_it_creates() {
    let outcome = run(
        "accept",
        json!([
            {"call": "operation", "bind": "d",
             "request": operation_request("c", "document.create", json!({"title": "Op"})),
             "expect": {"outcome": "ok", "document_revision": 0}},
            {"call": "operation", "document": "d",
             "request": operation_request("e", "document.transaction", json!({"expected_revision": 0,
                 "commands": [{"op": "add_atom", "element": "C", "position": {"x": 0, "y": 0}}]})),
             "expect": {"outcome": "ok", "document_revision": 1}},
            {"call": "get", "document": "d", "expect": {"document": {"title": "Op", "revision": 1}}}
        ]),
    );
    assert_eq!(outcome, Ok(()));
}

#[test]
fn an_operation_with_another_outcome_fails_and_raw_text_is_sent_as_is() {
    let wrong = run(
        "accept",
        json!([{"call": "operation",
            "request": operation_request("p", "chem.properties", json!({})),
            "expect": {"outcome": "ok"}}]),
    );
    assert!(
        wrong
            .as_ref()
            .is_err_and(|message| message.contains("outcome")),
        "{wrong:?}"
    );
    let raw = run(
        "reject",
        json!([{"call": "operation", "request": "{\"contract\": 1,",
            "expect": {"outcome": "error", "envelope": "protocol_error", "request_id": null}}]),
    );
    assert_eq!(raw, Ok(()));
}
