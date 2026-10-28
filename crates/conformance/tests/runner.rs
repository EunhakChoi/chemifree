use chemifree_conformance::{
    Body, CaseResult, Failure, Kind, KindCounts, Outcome, Pending, Suite, parse_case, run_case,
    run_suite,
};
use serde_json::{Value, json};

fn case(id: &str, kind: &str, steps: Value) -> chemifree_conformance::Case {
    parse_case(
        &json!({
            "id": id, "title": "T", "requirements": ["OPS-02"], "axis": "persistence",
            "kind": kind, "steps": steps,
        })
        .to_string(),
    )
    .unwrap()
}

fn add_carbon(expected_revision: u64) -> Value {
    json!({ "call": "transact", "doc": "d", "expected_revision": expected_revision,
        "commands": [{ "op": "add_atom", "element": "C", "position": { "x": 0, "y": 0 } }] })
}

fn failure(result: &CaseResult) -> &Failure {
    assert_eq!(result.outcome, Outcome::Failed, "{result:?}");
    result.failure.as_ref().expect("a failed case says why")
}

#[test]
fn a_restart_keeps_storage_and_drops_the_working_copy() {
    let result = run_case(&case(
        "restart",
        "accept",
        json!([
            { "call": "create", "doc": "d" },
            add_carbon(0),
            { "call": "save", "doc": "d", "expected_revision": 1 },
            add_carbon(1),
            { "call": "restart" },
            { "call": "get", "doc": "d",
              "expect": { "document": { "revision": 1, "atoms": [{ "id": "a1" }] },
                          "session": { "unsaved_changes": false } } },
        ]),
    ));
    assert_eq!(result.outcome, Outcome::Passed, "{result:?}");
}

#[test]
fn without_a_restart_the_working_copy_is_still_there() {
    // The same steps without `restart` must fail, or the case above proves nothing.
    let result = run_case(&case(
        "no-restart",
        "accept",
        json!([
            { "call": "create", "doc": "d" },
            add_carbon(0),
            { "call": "save", "doc": "d", "expected_revision": 1 },
            add_carbon(1),
            { "call": "get", "doc": "d", "expect": { "document": { "revision": 1 } } },
        ]),
    ));
    assert_eq!(failure(&result).step, Some(4));
    assert_eq!(
        failure(&result).message,
        "get: $.document.revision: expected 1, got 2"
    );
}

#[test]
fn every_case_starts_on_an_empty_data_directory() {
    let suite = Suite {
        version: "test".to_owned(),
        cases: vec![
            case(
                "first",
                "accept",
                json!([{ "call": "create", "doc": "d", "expect": { "document": { "revision": 0 } } }]),
            ),
            case(
                "second",
                "accept",
                json!([{ "call": "capabilities", "expect": { "capabilities": { "$contains": [{ "id": "persistence" }] } } }]),
            ),
        ],
    };
    let report = run_suite(&suite);
    assert_eq!(report.counts.accept.passed, 2, "{report:?}");
}

#[test]
fn an_unexpected_error_fails_at_its_step() {
    let result = run_case(&case(
        "unexpected-error",
        "accept",
        json!([
            { "call": "create", "doc": "d", "expect": { "document": { "revision": 0 } } },
            { "call": "undo", "doc": "d", "expected_revision": 0 },
        ]),
    ));
    let failure = failure(&result);
    assert_eq!(failure.step, Some(1));
    assert!(
        failure
            .message
            .starts_with("undo: expected success, got error"),
        "{}",
        failure.message
    );
    assert!(
        failure.message.contains("nothing_to_undo"),
        "{}",
        failure.message
    );
}

#[test]
fn an_expected_error_that_does_not_happen_fails() {
    let result = run_case(&case(
        "missing-error",
        "reject",
        json!([
            { "call": "create", "doc": "d" },
            { "call": "get", "doc": "d", "expect_error": { "code": "document_not_found" } },
        ]),
    ));
    assert_eq!(
        failure(&result).message,
        "get: expected an error, but the call succeeded"
    );
}

#[test]
fn an_error_with_other_fields_than_expected_fails() {
    let result = run_case(&case(
        "wrong-error",
        "reject",
        json!([
            { "call": "create", "doc": "d" },
            add_carbon(0),
            { "call": "undo", "doc": "d", "expected_revision": 0,
              "expect_error": { "code": "revision_conflict", "current_revision": 2 } },
        ]),
    ));
    assert_eq!(
        failure(&result).message,
        "undo: error envelope $.current_revision: expected 2, got 1"
    );
}

#[test]
fn a_document_whose_create_failed_cannot_be_used() {
    let result = run_case(&case(
        "failed-create",
        "reject",
        json!([
            { "call": "create", "doc": "d", "title": "",
              "expect_error": { "code": "invalid_request" } },
            { "call": "get", "doc": "d" },
        ]),
    ));
    assert_eq!(failure(&result).step, Some(1));
    assert_eq!(
        failure(&result).message,
        "get: document \"d\" was not created"
    );
}

#[test]
fn outcomes_are_counted_per_kind_and_pending_cases_are_not_run() {
    let pending = parse_case(
        &json!({ "id": "later", "title": "T", "requirements": ["CHEM-01"], "axis": "numeric",
            "kind": "accept", "pending": { "followup": "P19", "reason": "not built" } })
        .to_string(),
    )
    .unwrap();
    assert!(matches!(pending.body, Body::Pending(_)));
    let suite = Suite {
        version: "test".to_owned(),
        cases: vec![
            case(
                "accepted",
                "accept",
                json!([{ "call": "create", "doc": "d", "expect": { "document": { "revision": 0 } } }]),
            ),
            case(
                "refused",
                "reject",
                json!([
                    { "call": "create", "doc": "d" },
                    { "call": "undo", "doc": "d", "expected_revision": 0,
                      "expect_error": { "code": "nothing_to_undo" } },
                ]),
            ),
            case(
                "broken",
                "reject",
                json!([
                    { "call": "create", "doc": "d" },
                    { "call": "undo", "doc": "d", "expected_revision": 0,
                      "expect_error": { "code": "revision_conflict" } },
                ]),
            ),
            pending,
        ],
    };
    let report = run_suite(&suite);
    assert_eq!(
        report.counts.accept,
        KindCounts {
            passed: 1,
            failed: 0,
            not_run: 1
        }
    );
    assert_eq!(
        report.counts.reject,
        KindCounts {
            passed: 1,
            failed: 1,
            not_run: 0
        }
    );
    assert_eq!(
        report.counts.total,
        KindCounts {
            passed: 2,
            failed: 1,
            not_run: 1
        }
    );
    assert!(!report.succeeded());
    let later = &report.results[3];
    assert_eq!(later.outcome, Outcome::NotRun);
    assert_eq!(later.kind, Kind::Accept);
    assert_eq!(
        later.pending,
        Some(Pending {
            followup: "P19".to_owned(),
            reason: "not built".to_owned()
        })
    );
    let ids: Vec<&str> = report
        .results
        .iter()
        .map(|result| result.id.as_str())
        .collect();
    assert_eq!(ids, ["accepted", "refused", "broken", "later"]);
}

#[test]
fn a_report_is_the_same_on_every_run() {
    let suite = Suite {
        version: "test".to_owned(),
        cases: vec![case(
            "stable",
            "accept",
            json!([{ "call": "create", "doc": "d", "expect": { "document": { "revision": 0 } } }]),
        )],
    };
    let first = serde_json::to_string(&run_suite(&suite)).unwrap();
    let second = serde_json::to_string(&run_suite(&suite)).unwrap();
    assert_eq!(first, second);
    let mut report: Value = serde_json::from_str(&first).unwrap();
    let fingerprint = report["results"][0]
        .as_object_mut()
        .unwrap()
        .remove("sha256")
        .expect("each result records its case file's SHA-256");
    assert_eq!(fingerprint, json!(suite.cases[0].sha256));
    assert_eq!(
        report,
        json!({
            "report_version": 2,
            "suite_version": "test",
            "counts": {
                "accept": { "passed": 1, "failed": 0, "not_run": 0 },
                "reject": { "passed": 0, "failed": 0, "not_run": 0 },
                "total": { "passed": 1, "failed": 0, "not_run": 0 },
            },
            "results": [{
                "id": "stable", "title": "T", "requirements": ["OPS-02"], "axis": "persistence",
                "kind": "accept", "outcome": "passed", "pending": null,
            }],
        })
    );
}

#[test]
fn writes_without_a_request_id_send_one_generated_from_the_step() {
    // HTTP sends a request_id on every write. Step 1 sends `conformance-step-1`, so reusing that
    // ID for a different edit is refused; without a generated ID it would be applied.
    let result = run_case(&case(
        "generated-request-id",
        "reject",
        json!([
            { "call": "create", "doc": "d" },
            add_carbon(0),
            { "call": "transact", "doc": "d", "expected_revision": 1, "request_id": "conformance-step-1",
              "commands": [{ "op": "add_atom", "element": "O", "position": { "x": 9, "y": 9 } }],
              "expect_error": { "code": "request_id_reused" } },
        ]),
    ));
    assert_eq!(result.outcome, Outcome::Passed, "{result:?}");
}

#[test]
fn a_step_request_id_overrides_the_generated_one() {
    let result = run_case(&case(
        "explicit-request-id",
        "accept",
        json!([
            { "call": "create", "doc": "d" },
            { "call": "transact", "doc": "d", "expected_revision": 0, "request_id": "mine",
              "commands": [{ "op": "add_atom", "element": "C", "position": { "x": 0, "y": 0 } }] },
            { "call": "transact", "doc": "d", "expected_revision": 0, "request_id": "mine",
              "commands": [{ "op": "add_atom", "element": "C", "position": { "x": 0, "y": 0 } }],
              "expect": { "document": { "revision": 1 } } },
        ]),
    ));
    assert_eq!(result.outcome, Outcome::Passed, "{result:?}");
}
