//! Every refusal of the case and suite loader, one scenario each.

use std::fs;
use std::path::Path;

use chemifree_conformance::case::{Problem, StepProblem, parse_case, parse_case_bytes};
use chemifree_conformance::expect::ExpectationError;
use chemifree_conformance::{check_case_path, load_suite};
use serde_json::{Value, json};

/// A valid runnable accept case with the given steps after a create binding `d`.
fn with_steps(steps: Value) -> Value {
    let mut all = vec![json!({"call": "create", "bind": "d"})];
    all.extend(steps.as_array().expect("steps are a list").iter().cloned());
    json!({
        "id": "sample",
        "title": "Sample",
        "requirements": ["OPS-02"],
        "axis": "graph",
        "kind": "accept",
        "steps": all,
    })
}

fn get_expect() -> Value {
    json!({"call": "get", "document": "d", "expect": {"document": {"revision": 0}}})
}

fn valid() -> Value {
    with_steps(json!([get_expect()]))
}

fn step_problem(case: &Value) -> StepProblem {
    match parse_case(case) {
        Err(Problem::Step { problem, .. }) => problem,
        other => panic!("expected a step problem, got {other:?}"),
    }
}

fn set(case: &mut Value, field: &str, value: Value) {
    case.as_object_mut()
        .expect("a case is an object")
        .insert(field.to_owned(), value);
}

fn remove(case: &mut Value, field: &str) {
    case.as_object_mut()
        .expect("a case is an object")
        .remove(field);
}

#[test]
fn the_sample_case_loads() {
    let case = parse_case(&valid()).unwrap();
    assert_eq!(case.id, "sample");
}

#[test]
fn refuses_unknown_case_fields() {
    let mut case = valid();
    set(&mut case, "note", json!("x"));
    assert_eq!(
        parse_case(&case).unwrap_err(),
        Problem::UnknownField("note".to_owned())
    );
}

#[test]
fn refuses_unknown_step_fields_per_call() {
    // `title` is a create field, not a transact one; `expected_revision` is not a get field.
    let transact = with_steps(json!([{
        "call": "transact", "document": "d", "expected_revision": 0,
        "commands": [], "title": "x"
    }]));
    assert_eq!(
        step_problem(&transact),
        StepProblem::UnknownField("title".to_owned())
    );
    let get = with_steps(json!([{
        "call": "get", "document": "d", "expected_revision": 0,
        "expect": {"document": {"revision": 0}}
    }]));
    assert_eq!(
        step_problem(&get),
        StepProblem::UnknownField("expected_revision".to_owned())
    );
    let restart = with_steps(json!([{"call": "restart", "document": "d"}]));
    assert_eq!(step_problem(&restart), StepProblem::DocumentNotTaken);
}

#[test]
fn refuses_unknown_command_fields_in_the_body() {
    let case = with_steps(json!([{
        "call": "transact", "document": "d", "expected_revision": 0,
        "commands": [{"op": "add_atom", "element": "C", "position": {"x": 0, "y": 0}, "hydrogens": 3}]
    }, get_expect()]));
    assert!(
        matches!(step_problem(&case), StepProblem::Body(message) if message.contains("hydrogens")),
        "an unknown command field must be refused as the HTTP adapter would"
    );
}

#[test]
fn refuses_a_body_missing_a_required_field() {
    let case = with_steps(json!([{"call": "undo", "document": "d"}, get_expect()]));
    assert!(
        matches!(step_problem(&case), StepProblem::Body(message) if message.contains("expected_revision"))
    );
}

#[test]
fn refuses_request_id_on_calls_that_take_none() {
    for call in [
        json!({"call": "get", "document": "d", "request_id": "r1"}),
        json!({"call": "capabilities", "request_id": "r1"}),
        json!({"call": "restart", "request_id": "r1"}),
    ] {
        let case = with_steps(json!([call.clone(), get_expect()]));
        assert_eq!(
            step_problem(&case),
            StepProblem::RequestIdNotTaken,
            "{call}"
        );
    }
}

#[test]
fn refuses_a_document_used_before_a_create_binds_it() {
    let case = json!({
        "id": "sample", "title": "Sample", "requirements": ["OPS-02"],
        "axis": "graph", "kind": "accept",
        "steps": [
            {"call": "get", "document": "d", "expect": {"document": {"revision": 0}}},
            {"call": "create", "bind": "d"}
        ]
    });
    assert_eq!(
        step_problem(&case),
        StepProblem::UnboundDocument("d".to_owned())
    );
    let other = with_steps(json!([{"call": "get", "document": "e", "expect": {"recovery": null}}]));
    assert_eq!(
        step_problem(&other),
        StepProblem::UnboundDocument("e".to_owned())
    );
}

#[test]
fn refuses_a_document_bound_twice() {
    let case = with_steps(json!([{"call": "create", "bind": "d"}, get_expect()]));
    assert_eq!(step_problem(&case), StepProblem::BoundTwice("d".to_owned()));
}

#[test]
fn refuses_bind_outside_create_and_on_a_failing_create() {
    let case = with_steps(json!([{"call": "get", "document": "d", "bind": "e"}, get_expect()]));
    assert_eq!(step_problem(&case), StepProblem::BindNotTaken);
    let failing = with_steps(json!([
        {"call": "create", "bind": "e", "title": "", "expect_error": {"code": "invalid_request"}},
        get_expect()
    ]));
    assert_eq!(step_problem(&failing), StepProblem::BindExpectsError);
}

#[test]
fn refuses_a_missing_or_untaken_document() {
    let missing = with_steps(json!([{"call": "get", "expect": {"recovery": null}}]));
    assert_eq!(step_problem(&missing), StepProblem::MissingDocument);
    let untaken = with_steps(json!([{"call": "capabilities", "document": "d"}, get_expect()]));
    assert_eq!(step_problem(&untaken), StepProblem::DocumentNotTaken);
}

#[test]
fn refuses_unknown_calls() {
    let case = with_steps(json!([{"call": "import", "document": "d"}, get_expect()]));
    assert_eq!(
        step_problem(&case),
        StepProblem::UnknownCall("import".to_owned())
    );
}

#[test]
fn refuses_malformed_ids() {
    for id in ["Sample", "sample-", "-sample", "sam--ple", "sam ple", ""] {
        let mut case = valid();
        set(&mut case, "id", json!(id));
        assert_eq!(
            parse_case(&case).unwrap_err(),
            Problem::InvalidCaseId(id.to_owned()),
            "case id {id:?}"
        );
    }
    for id in ["OPS-2", "ops-02", "OPS02", "OPS-002", "-02"] {
        let mut case = valid();
        set(&mut case, "requirements", json!([id]));
        assert_eq!(
            parse_case(&case).unwrap_err(),
            Problem::InvalidRequirementId(id.to_owned()),
            "requirement id {id:?}"
        );
    }
    for id in ["p13", "P", "P1234", "PP1", "13"] {
        let mut case = valid();
        remove(&mut case, "steps");
        set(
            &mut case,
            "pending",
            json!({"followup": id, "reason": "later"}),
        );
        assert_eq!(
            parse_case(&case).unwrap_err(),
            Problem::InvalidRoadmapId(id.to_owned()),
            "roadmap id {id:?}"
        );
    }
}

#[test]
fn refuses_empty_or_repeated_requirements() {
    let mut case = valid();
    set(&mut case, "requirements", json!([]));
    assert_eq!(parse_case(&case).unwrap_err(), Problem::NoRequirements);
    set(&mut case, "requirements", json!(["OPS-02", "OPS-02"]));
    assert_eq!(
        parse_case(&case).unwrap_err(),
        Problem::DuplicateRequirement("OPS-02".to_owned())
    );
}

#[test]
fn refuses_an_empty_title() {
    for title in ["", "   "] {
        let mut case = valid();
        set(&mut case, "title", json!(title));
        assert_eq!(parse_case(&case).unwrap_err(), Problem::EmptyTitle);
    }
}

#[test]
fn refuses_unknown_axes_and_kinds() {
    let mut case = valid();
    set(&mut case, "axis", json!("everything"));
    assert_eq!(
        parse_case(&case).unwrap_err(),
        Problem::UnknownAxis("everything".to_owned())
    );
    let mut case = valid();
    set(&mut case, "kind", json!("skip"));
    assert_eq!(
        parse_case(&case).unwrap_err(),
        Problem::UnknownKind("skip".to_owned())
    );
}

#[test]
fn refuses_a_pending_case_with_steps() {
    let mut case = valid();
    set(
        &mut case,
        "pending",
        json!({"followup": "P13", "reason": "later"}),
    );
    assert_eq!(parse_case(&case).unwrap_err(), Problem::PendingWithSteps);
}

#[test]
fn refuses_a_pending_case_without_a_reason() {
    let mut case = valid();
    remove(&mut case, "steps");
    set(
        &mut case,
        "pending",
        json!({"followup": "P13", "reason": " "}),
    );
    assert_eq!(parse_case(&case).unwrap_err(), Problem::EmptyReason);
}

#[test]
fn refuses_a_runnable_case_without_steps() {
    let mut case = valid();
    set(&mut case, "steps", json!([]));
    assert_eq!(
        parse_case(&case).unwrap_err(),
        Problem::RunnableWithoutSteps
    );
    remove(&mut case, "steps");
    assert_eq!(
        parse_case(&case).unwrap_err(),
        Problem::RunnableWithoutSteps
    );
}

#[test]
fn refuses_both_expectations_on_one_step() {
    let case = with_steps(json!([{
        "call": "get", "document": "d",
        "expect": {"recovery": null}, "expect_error": {"code": "x"}
    }]));
    assert_eq!(step_problem(&case), StepProblem::BothExpectations);
}

#[test]
fn refuses_any_expectation_on_restart() {
    for expectation in ["expect", "expect_error"] {
        let mut restart = json!({"call": "restart"});
        restart
            .as_object_mut()
            .unwrap()
            .insert(expectation.to_owned(), json!({"code": "x"}));
        let case = with_steps(json!([restart, get_expect()]));
        assert_eq!(
            step_problem(&case),
            StepProblem::ExpectationOnRestart,
            "{expectation}"
        );
    }
}

#[test]
fn refuses_invalid_expectations_through_the_loader() {
    let nested_empty = with_steps(json!([{
        "call": "get", "document": "d", "expect": {"document": {"atoms": [{}]}}
    }]));
    assert_eq!(
        step_problem(&nested_empty),
        StepProblem::Expect(ExpectationError::EmptyObject {
            path: "/document/atoms/0".to_owned()
        })
    );
    let only_version = with_steps(json!([{
        "call": "get", "document": "d", "expect": {"api_version": "v1"}
    }]));
    assert_eq!(
        step_problem(&only_version),
        StepProblem::Expect(ExpectationError::NothingCompared)
    );
    let not_object = with_steps(json!([{"call": "get", "document": "d", "expect": true}]));
    assert_eq!(
        step_problem(&not_object),
        StepProblem::Expect(ExpectationError::NotAnObject)
    );
    let no_code = with_steps(json!([
        {"call": "get", "document": "d", "expect_error": {"message": "x"}},
        get_expect()
    ]));
    assert_eq!(
        step_problem(&no_code),
        StepProblem::ExpectError(ExpectationError::MissingCode)
    );
    let deep_contains = with_steps(json!([{
        "call": "capabilities",
        "expect": {"capabilities": {"$contains": [{"id": {"$contains": []}}]}}
    }]));
    assert_eq!(
        step_problem(&deep_contains),
        StepProblem::Expect(ExpectationError::ContainsEmpty {
            path: "/capabilities/$contains/0/id".to_owned()
        })
    );
}

#[test]
fn refuses_a_reject_case_without_an_error_step() {
    let mut case = valid();
    set(&mut case, "kind", json!("reject"));
    assert_eq!(
        parse_case(&case).unwrap_err(),
        Problem::RejectWithoutErrorStep
    );
}

#[test]
fn refuses_an_accept_case_without_an_expect_step() {
    // Only an expected error and an unchecked success: nothing shows an accepted result.
    let case = with_steps(json!([
        {"call": "undo", "document": "d", "expected_revision": 0,
         "expect_error": {"code": "nothing_to_undo"}},
        {"call": "get", "document": "d"}
    ]));
    assert_eq!(
        parse_case(&case).unwrap_err(),
        Problem::AcceptWithoutExpectStep
    );
}

#[test]
fn refuses_duplicate_keys() {
    let text = br#"{"id": "sample", "id": "other", "title": "T", "requirements": ["OPS-02"],
        "axis": "graph", "kind": "accept", "steps": []}"#;
    assert!(
        matches!(parse_case_bytes(text), Err(Problem::Json(message)) if message.contains("duplicate key")),
        "duplicate keys must be refused"
    );
}

#[test]
fn case_paths_are_relative_without_dot_segments() {
    for (path, reason) in [
        ("/abs/case.json", "is absolute"),
        ("\\abs\\case.json", "is absolute"),
        ("C:case.json", "has a drive prefix"),
        ("c:/cases/case.json", "has a drive prefix"),
        ("cases//case.json", "has an empty, '.', or '..' segment"),
        ("cases/./case.json", "has an empty, '.', or '..' segment"),
        ("cases/../case.json", "has an empty, '.', or '..' segment"),
        ("cases\\..\\case.json", "has an empty, '.', or '..' segment"),
        ("cases/", "has an empty, '.', or '..' segment"),
        ("", "has an empty, '.', or '..' segment"),
    ] {
        assert_eq!(check_case_path(path), Err(reason), "{path:?}");
    }
    assert_eq!(check_case_path("cases/a.json"), Ok(()));
    assert_eq!(check_case_path("cases/..a.json"), Ok(()));
}

fn write(dir: &Path, name: &str, value: &Value) {
    let path = dir.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}

#[test]
fn the_suite_refuses_a_bad_case_path_before_reading() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "cases/a.json", &valid());
    write(
        dir.path(),
        "suite.json",
        &json!({"suite_version": 1, "cases": ["cases/../cases/a.json"]}),
    );
    assert!(matches!(
        load_suite(&dir.path().join("suite.json")),
        Err(Problem::InvalidCasePath { .. })
    ));
}

#[test]
fn the_suite_refuses_duplicate_case_ids() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "cases/a.json", &valid());
    write(dir.path(), "cases/b.json", &valid());
    write(
        dir.path(),
        "suite.json",
        &json!({"suite_version": 1, "cases": ["cases/a.json", "cases/b.json"]}),
    );
    assert_eq!(
        load_suite(&dir.path().join("suite.json")).unwrap_err(),
        Problem::DuplicateCaseId("sample".to_owned())
    );
}

#[test]
fn the_suite_loads_cases_relative_to_itself_with_their_hashes() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "nested/cases/a.json", &valid());
    write(
        dir.path(),
        "nested/suite.json",
        &json!({"suite_version": 3, "cases": ["cases/a.json"]}),
    );
    let suite = load_suite(&dir.path().join("nested/suite.json")).unwrap();
    assert_eq!(suite.version, 3);
    let bytes = fs::read(dir.path().join("nested/cases/a.json")).unwrap();
    // `sha256_hex` itself is pinned to a known vector in `sha256_matches_a_known_vector`.
    assert_eq!(
        suite.cases[0].sha256,
        chemifree_conformance::case::sha256_hex(&bytes)
    );
}

#[test]
fn sha256_matches_a_known_vector() {
    assert_eq!(
        chemifree_conformance::case::sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn the_suite_refuses_bad_fields() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "cases/a.json", &valid());
    let suite = dir.path().join("suite.json");
    for (value, problem) in [
        (
            json!({"suite_version": 1, "cases": ["cases/a.json"], "x": 1}),
            Problem::UnknownField("x".to_owned()),
        ),
        (
            json!({"suite_version": 0, "cases": ["cases/a.json"]}),
            Problem::InvalidSuiteVersion,
        ),
        (
            json!({"suite_version": 1.5, "cases": ["cases/a.json"]}),
            Problem::InvalidSuiteVersion,
        ),
        (json!({"suite_version": 1, "cases": []}), Problem::NoCases),
    ] {
        fs::write(&suite, serde_json::to_vec(&value).unwrap()).unwrap();
        assert_eq!(load_suite(&suite).unwrap_err(), problem, "{value}");
    }
}

fn operation(expect: Value) -> Value {
    json!({"call": "operation", "request": {"contract": 1, "parameters": {}}, "expect": expect})
}

#[test]
fn an_operation_step_expects_the_reply_outcome_and_no_error_envelope() {
    let mut with_error = operation(json!({"outcome": "error"}));
    with_error["expect_error"] = json!({"code": "x"});
    assert_eq!(
        step_problem(&with_steps(json!([with_error]))),
        StepProblem::OperationExpectsNoError
    );
    for step in [
        json!({"call": "operation", "request": {"parameters": {}}}),
        operation(json!({"envelope": "operation_result"})),
        operation(json!({"outcome": 1})),
    ] {
        assert_eq!(
            step_problem(&with_steps(json!([step.clone()]))),
            StepProblem::OperationExpectsOutcome,
            "{step}"
        );
    }
    let mut numeric = operation(json!({"outcome": "ok"}));
    numeric["request"] = json!(1);
    assert_eq!(
        step_problem(&with_steps(json!([numeric]))),
        StepProblem::OperationRequest("must be an object or a string")
    );
    let mut with_id = operation(json!({"outcome": "ok"}));
    with_id["request_id"] = json!("r");
    assert_eq!(
        step_problem(&with_steps(json!([with_id]))),
        StepProblem::RequestIdNotTaken
    );
}

#[test]
fn an_operation_document_needs_a_binding_and_a_parameters_object() {
    let mut unbound = operation(json!({"outcome": "ok"}));
    unbound["document"] = json!("other");
    assert_eq!(
        step_problem(&with_steps(json!([unbound]))),
        StepProblem::UnboundDocument("other".to_owned())
    );
    let mut raw = operation(json!({"outcome": "ok"}));
    raw["document"] = json!("d");
    raw["request"] = json!("{}");
    assert_eq!(
        step_problem(&with_steps(json!([raw]))),
        StepProblem::OperationRequest("needs a parameters object to receive the document")
    );
    let mut rebinding = operation(json!({"outcome": "ok"}));
    rebinding["bind"] = json!("d");
    assert_eq!(
        step_problem(&with_steps(json!([rebinding]))),
        StepProblem::BoundTwice("d".to_owned())
    );
}

#[test]
fn an_operation_counts_for_its_kind_by_the_outcome_it_expects() {
    let case = |kind: &str, outcome: &str| {
        json!({
            "id": "sample", "title": "Sample", "requirements": ["API-01"],
            "axis": "graph", "kind": kind, "steps": [operation(json!({"outcome": outcome}))],
        })
    };
    assert!(matches!(
        parse_case(&case("accept", "unsupported")),
        Err(Problem::AcceptWithoutExpectStep)
    ));
    assert!(matches!(
        parse_case(&case("reject", "ok")),
        Err(Problem::RejectWithoutErrorStep)
    ));
    assert!(parse_case(&case("accept", "partial")).is_ok());
    assert!(parse_case(&case("reject", "error")).is_ok());
}
