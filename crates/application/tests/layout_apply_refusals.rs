//! The refusals of `layout.structure` in mode `"apply"`, and the checks that the two modes share.
//! Every refusal of the preview is the same refusal of the apply, with the same outcome, code, path,
//! details and revision, and a refused apply changes nothing. The fields of the other mode are refused
//! by name, the checks run in the contract's order, a stale revision is answered before any layout
//! refusal, and every reply names the layout model.

use std::error::Error;

use chemifree_application::Application;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};

type App = Application<LocalCapabilities, FileDocumentRepository>;
type TestResult<T = ()> = Result<T, Box<dyn Error>>;
/// Makes the document of a case, using `ids` for the requests that build it, and returns its id.
type Draw = fn(&App, &mut RequestIds) -> TestResult<String>;

/// The model that every reply of `layout.structure` names.
const MODEL_ID: &str = "chemifree-chain-layout-1";
/// The largest limit a request may set, in milliseconds.
const MAXIMUM_LIMIT_MS: u64 = 8_000;

fn new_app(directory: &tempfile::TempDir) -> App {
    Application::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    )
}

/// The request bytes of `operation` with `parameters`, under the request ID `id`.
fn request(id: &str, operation: &str, parameters: Value) -> Vec<u8> {
    json!({
        "contract": 1,
        "request_id": id,
        "profile": "preserve-semantics",
        "operation": operation,
        "parameters": parameters,
    })
    .to_string()
    .into_bytes()
}

fn call(app: &App, id: &str, operation: &str, parameters: Value) -> TestResult<Value> {
    Ok(serde_json::to_value(
        app.execute(&request(id, operation, parameters)),
    )?)
}

/// The document as the application returns it, as JSON.
fn document(app: &App, id: &str) -> TestResult<Value> {
    Ok(serde_json::to_value(app.document(id)?)?)
}

fn current_revision(app: &App, id: &str) -> TestResult<u64> {
    Ok(app.document(id)?.document.revision)
}

/// Request IDs that no two calls of a test share, so that no call answers from another's record.
#[derive(Default)]
struct RequestIds(usize);

impl RequestIds {
    fn fresh(&mut self) -> String {
        self.0 += 1;
        format!("request-{}", self.0)
    }
}

/// Applies `commands` to document `id` at `expected_revision`, which must succeed.
fn transact(
    app: &App,
    ids: &mut RequestIds,
    id: &str,
    expected_revision: u64,
    commands: Value,
) -> TestResult {
    let parameters = json!({
        "document_id": id,
        "expected_revision": expected_revision,
        "commands": commands,
    });
    let reply = call(app, &ids.fresh(), "document.transaction", parameters)?;
    assert_eq!(reply["outcome"], "ok", "{reply}");
    Ok(())
}

/// Creates a document titled `title` and applies `commands` to it at revision 0. Returns its id.
fn draw_document(
    app: &App,
    ids: &mut RequestIds,
    title: &str,
    commands: Value,
) -> TestResult<String> {
    let id = app.create_document(None, Some(title))?.document.id;
    transact(app, ids, &id, 0, commands)?;
    Ok(id)
}

/// The parameters `parameters` have in mode `mode`, bound to `expected_revision` when one is given.
fn in_mode(mut parameters: Value, mode: &str, expected_revision: Option<u64>) -> Value {
    parameters["mode"] = json!(mode);
    if let Some(revision) = expected_revision {
        parameters["expected_revision"] = json!(revision);
    }
    parameters
}

/// Carbons a1 to a4 in a row 30 pt apart, bonded in turn, so that bonds b1 to b3 join them.
fn butane() -> Value {
    json!([
        {"op": "add_atom", "element": "C", "position": {"x": 0, "y": 0}},
        {"op": "add_atom", "element": "C", "position": {"x": 30, "y": 0}, "bond_to": {"atom": "$0", "order": "single"}},
        {"op": "add_atom", "element": "C", "position": {"x": 60, "y": 0}, "bond_to": {"atom": "$1", "order": "single"}},
        {"op": "add_atom", "element": "C", "position": {"x": 90, "y": 0}, "bond_to": {"atom": "$2", "order": "single"}}
    ])
}

/// An unbonded carbon at `index` times 30 pt along the x axis.
fn loose_carbon(index: u32) -> Value {
    json!({"op": "add_atom", "element": "C", "position": {"x": f64::from(index) * 30.0, "y": 0}})
}

/// The atom identifiers `a1` to `a{count}`.
fn atom_names(count: u32) -> Vec<String> {
    (1..=count).map(|serial| format!("a{serial}")).collect()
}

fn butane_document(app: &App, ids: &mut RequestIds) -> TestResult<String> {
    draw_document(app, ids, "butane", butane())
}

/// Butane with a second scene instance, offset 200 pt along x. The second instance is `s2`.
fn two_instance_document(app: &App, ids: &mut RequestIds) -> TestResult<String> {
    let id = butane_document(app, ids)?;
    let second = json!([{"op": "add_instance", "offset": {"x": 200, "y": 0}}]);
    transact(app, ids, &id, 1, second)?;
    Ok(id)
}

/// A ring that `insert_ring` draws at the origin of the first instance.
fn ring_document(app: &App, ids: &mut RequestIds) -> TestResult<String> {
    let ring = json!([{"op": "insert_ring", "instance": "s1", "center": {"x": 0, "y": 0}}]);
    draw_document(app, ids, "ring", ring)
}

/// Butane whose first bond, `b1`, is a wedge that begins at `a1`.
fn wedge_document(app: &App, ids: &mut RequestIds) -> TestResult<String> {
    let id = butane_document(app, ids)?;
    let wedge = json!([{"op": "set_bond_mark", "bond": "b1", "mark": {"kind": "wedge", "narrow": "begin"}}]);
    transact(app, ids, &id, 1, wedge)?;
    Ok(id)
}

/// An O-C-C chain whose ethyl end, `a2` and `a3`, is collapsed to an abbreviation.
fn abbreviation_document(app: &App, ids: &mut RequestIds) -> TestResult<String> {
    let chain = json!([
        {"op": "add_atom", "element": "O", "position": {"x": 0, "y": 0}},
        {"op": "add_atom", "element": "C", "position": {"x": 30, "y": 0}, "bond_to": {"atom": "$0", "order": "single"}},
        {"op": "add_atom", "element": "C", "position": {"x": 45, "y": 25}, "bond_to": {"atom": "$1", "order": "single"}}
    ]);
    let id = draw_document(app, ids, "abbreviation", chain)?;
    let collapse =
        json!([{"op": "collapse_ethyl", "instance": "s1", "anchor": "a2", "terminal": "a3"}]);
    transact(app, ids, &id, 1, collapse)?;
    Ok(id)
}

/// Three carbons: `a1` and `a2` at one point and bonded, and `a3` bonded to `a2` at 60 pt, so that
/// the fixed `a2` has a fixed neighbour at its own place.
fn coincident_document(app: &App, ids: &mut RequestIds) -> TestResult<String> {
    let carbons = json!([
        {"op": "add_atom", "element": "C", "position": {"x": 0, "y": 0}},
        {"op": "add_atom", "element": "C", "position": {"x": 0, "y": 0}, "bond_to": {"atom": "$0", "order": "single"}},
        {"op": "add_atom", "element": "C", "position": {"x": 60, "y": 0}, "bond_to": {"atom": "$1", "order": "single"}}
    ]);
    draw_document(app, ids, "coincident", carbons)
}

/// Two carbons: `a1` at 99990 pt, near the right edge of the coordinate range, and `a2` bonded to it at
/// 99960 pt. A layout of `a2` is placed 30 pt from `a1` at -30 degrees, which leaves the range.
fn far_document(app: &App, ids: &mut RequestIds) -> TestResult<String> {
    let carbons = json!([
        {"op": "add_atom", "element": "C", "position": {"x": 99990, "y": 0}},
        {"op": "add_atom", "element": "C", "position": {"x": 99960, "y": 0}, "bond_to": {"atom": "$0", "order": "single"}}
    ]);
    draw_document(app, ids, "far", carbons)
}

/// 501 unbonded carbons, `a1` to `a501`, in two transactions: the first 250, then the other 251.
fn many_carbons_document(app: &App, ids: &mut RequestIds) -> TestResult<String> {
    let first = Value::Array((0..250).map(loose_carbon).collect());
    let id = draw_document(app, ids, "many", first)?;
    let second = Value::Array((250..501).map(loose_carbon).collect());
    transact(app, ids, &id, 1, second)?;
    Ok(id)
}

/// The refusal a case must give: the outcome, and the code, path and `details.reason` of its one
/// diagnostic. `names_revision` says whether the reply names the revision the request was checked at.
#[derive(Clone, Copy)]
struct Expected {
    outcome: &'static str,
    code: &'static str,
    path: Option<&'static str>,
    reason: Option<&'static str>,
    names_revision: bool,
}

/// One row of a table: the document that `draw` makes, the parameters that `request` builds from its
/// id, and the refusal that they must give.
struct Case {
    name: &'static str,
    draw: Draw,
    request: fn(&str) -> Value,
    expected: Expected,
}

/// The refusals of the preview that the apply shares, one case each (A38). The apply of each case
/// names the refusal that the preview names, and a refused apply changes nothing. No case covers
/// `missing_position`: an atom has a position in every instance, so commands cannot leave one without.
fn refusals() -> Vec<Case> {
    vec![
        Case {
            name: "a time limit above the maximum",
            draw: butane_document,
            request: |id: &str| {
                json!({
                    "document_id": id,
                    "mode": "preview",
                    "selection": {"atoms": ["a2"]},
                    "time_limit_ms": MAXIMUM_LIMIT_MS + 1,
                })
            },
            expected: Expected {
                outcome: "error",
                code: "invalid_request",
                path: Some("parameters.time_limit_ms"),
                reason: None,
                names_revision: false,
            },
        },
        Case {
            name: "an unknown scope",
            draw: butane_document,
            request: |id: &str| {
                json!({
                    "document_id": id,
                    "mode": "preview",
                    "selection": {"atoms": ["a2"]},
                    "scope": "rings",
                })
            },
            expected: Expected {
                outcome: "error",
                code: "invalid_request",
                path: Some("parameters.scope"),
                reason: None,
                names_revision: false,
            },
        },
        Case {
            name: "a malformed instance",
            draw: butane_document,
            request: |id: &str| {
                json!({
                    "document_id": id,
                    "mode": "preview",
                    "selection": {"atoms": ["a2"]},
                    "instance": "x!",
                })
            },
            expected: Expected {
                outcome: "error",
                code: "invalid_request",
                path: Some("parameters.instance"),
                reason: None,
                names_revision: false,
            },
        },
        Case {
            name: "a malformed atom",
            draw: butane_document,
            request: |id: &str| {
                json!({
                    "document_id": id,
                    "mode": "preview",
                    "selection": {"atoms": ["x!"]},
                })
            },
            expected: Expected {
                outcome: "error",
                code: "invalid_request",
                path: Some("parameters.selection"),
                reason: Some("selection_unknown"),
                names_revision: false,
            },
        },
        Case {
            name: "an instance the document does not have",
            draw: butane_document,
            request: |id: &str| {
                json!({
                    "document_id": id,
                    "mode": "preview",
                    "selection": {"atoms": ["a2"]},
                    "instance": "s9",
                })
            },
            expected: Expected {
                outcome: "error",
                code: "invalid_request",
                path: Some("parameters.instance"),
                reason: None,
                names_revision: false,
            },
        },
        Case {
            name: "an omitted instance in a document with two",
            draw: two_instance_document,
            request: |id: &str| {
                json!({
                    "document_id": id,
                    "mode": "preview",
                    "selection": {"atoms": ["a2"]},
                })
            },
            expected: Expected {
                outcome: "error",
                code: "invalid_request",
                path: Some("parameters.instance"),
                reason: None,
                names_revision: false,
            },
        },
        Case {
            name: "a selection of scene objects",
            draw: butane_document,
            request: |id: &str| {
                json!({
                    "document_id": id,
                    "mode": "preview",
                    "selection": {"objects": ["s1"]},
                })
            },
            expected: Expected {
                outcome: "unsupported",
                code: "layout_unsupported",
                path: None,
                reason: Some("object_selection"),
                names_revision: true,
            },
        },
        Case {
            name: "an unknown atom",
            draw: butane_document,
            request: |id: &str| {
                json!({
                    "document_id": id,
                    "mode": "preview",
                    "selection": {"atoms": ["a9"]},
                })
            },
            expected: Expected {
                outcome: "error",
                code: "invalid_request",
                path: Some("parameters.selection"),
                reason: Some("selection_unknown"),
                names_revision: true,
            },
        },
        Case {
            name: "an empty selection",
            draw: butane_document,
            request: |id: &str| {
                json!({
                    "document_id": id,
                    "mode": "preview",
                    "selection": {},
                })
            },
            expected: Expected {
                outcome: "error",
                code: "invalid_request",
                path: Some("parameters.selection"),
                reason: Some("layout_selection_empty"),
                names_revision: true,
            },
        },
        Case {
            name: "more than 500 layout atoms",
            draw: many_carbons_document,
            request: |id: &str| {
                json!({
                    "document_id": id,
                    "mode": "preview",
                    "selection": {"atoms": atom_names(501)},
                    "scope": "selection",
                })
            },
            expected: Expected {
                outcome: "error",
                code: "resource_limit",
                path: Some("parameters.selection"),
                reason: None,
                names_revision: true,
            },
        },
        Case {
            name: "a ring atom in the layout set",
            draw: ring_document,
            request: |id: &str| {
                json!({
                    "document_id": id,
                    "mode": "preview",
                    "selection": {"atoms": ["a1"]},
                    "scope": "selection",
                })
            },
            expected: Expected {
                outcome: "unsupported",
                code: "layout_unsupported",
                path: None,
                reason: Some("ring_in_layout_set"),
                names_revision: true,
            },
        },
        Case {
            name: "a wedge bond in the layout set",
            draw: wedge_document,
            request: |id: &str| {
                json!({
                    "document_id": id,
                    "mode": "preview",
                    "selection": {"atoms": ["a1", "a2"]},
                    "scope": "selection",
                })
            },
            expected: Expected {
                outcome: "unsupported",
                code: "layout_unsupported",
                path: None,
                reason: Some("stereo_in_layout"),
                names_revision: true,
            },
        },
        Case {
            name: "an abbreviation in the layout set",
            draw: abbreviation_document,
            request: |id: &str| {
                json!({
                    "document_id": id,
                    "mode": "preview",
                    "selection": {"atoms": ["a2", "a3"]},
                    "scope": "selection",
                })
            },
            expected: Expected {
                outcome: "unsupported",
                code: "layout_unsupported",
                path: None,
                reason: Some("display_in_layout_set"),
                names_revision: true,
            },
        },
        Case {
            name: "two attachments of one component",
            draw: butane_document,
            request: |id: &str| {
                json!({
                    "document_id": id,
                    "mode": "preview",
                    "selection": {"atoms": ["a2", "a3"]},
                    "scope": "selection",
                })
            },
            expected: Expected {
                outcome: "unsupported",
                code: "layout_unsupported",
                path: None,
                reason: Some("multiple_attachments"),
                names_revision: true,
            },
        },
        Case {
            name: "a fixed neighbour at the anchor's place",
            draw: coincident_document,
            request: |id: &str| {
                json!({
                    "document_id": id,
                    "mode": "preview",
                    "selection": {"atoms": ["a3"]},
                    "scope": "selection",
                })
            },
            expected: Expected {
                outcome: "unsupported",
                code: "layout_unsupported",
                path: None,
                reason: Some("coincident_fixed_atoms"),
                names_revision: true,
            },
        },
        Case {
            name: "a placed position out of range",
            draw: far_document,
            request: |id: &str| {
                json!({
                    "document_id": id,
                    "mode": "preview",
                    "selection": {"atoms": ["a2"]},
                    "scope": "selection",
                })
            },
            expected: Expected {
                outcome: "unsupported",
                code: "layout_unsupported",
                path: None,
                reason: Some("coordinate_out_of_range"),
                names_revision: true,
            },
        },
    ]
}

/// The stale-revision cases (A39): a layout that the document would refuse on its own terms, and a
/// ring layout, which the layout would refuse as unsupported if it were checked first.
fn stale_cases() -> Vec<Case> {
    const STALE: Expected = Expected {
        outcome: "error",
        code: "revision_conflict",
        path: None,
        reason: None,
        names_revision: true,
    };
    vec![
        Case {
            name: "a butane layout",
            draw: butane_document,
            request: |id: &str| {
                json!({
                    "document_id": id,
                    "mode": "preview",
                    "selection": {"atoms": ["a2"]},
                })
            },
            expected: STALE,
        },
        Case {
            name: "a ring atom in the layout set",
            draw: ring_document,
            request: |id: &str| {
                json!({
                    "document_id": id,
                    "mode": "preview",
                    "selection": {"atoms": ["a1"]},
                    "scope": "selection",
                })
            },
            expected: STALE,
        },
    ]
}

/// The one diagnostic of `reply`.
fn sole_diagnostic(reply: &Value) -> TestResult<&Value> {
    match reply["diagnostics"].as_array().map(Vec::as_slice) {
        Some([diagnostic]) => Ok(diagnostic),
        _ => Err(format!("a reply with one diagnostic was expected: {reply}").into()),
    }
}

/// Asserts that `reply` is the refusal that `expected` describes, and that it names the model. `label`
/// names the case in a failure message. `revision` is the revision the reply names when `expected`
/// says it names one.
fn assert_refusal(label: &str, reply: &Value, expected: Expected, revision: u64) -> TestResult {
    assert_eq!(reply["outcome"], expected.outcome, "{label}: {reply}");
    assert_eq!(reply["model_id"], MODEL_ID, "{label}: {reply}");
    let named = if expected.names_revision {
        json!(revision)
    } else {
        Value::Null
    };
    assert_eq!(reply["document_revision"], named, "{label}: {reply}");
    let diagnostic = sole_diagnostic(reply)?;
    assert_eq!(diagnostic["code"], expected.code, "{label}: {reply}");
    assert_eq!(diagnostic["path"], json!(expected.path), "{label}: {reply}");
    assert_eq!(
        diagnostic["details"]["reason"],
        json!(expected.reason),
        "{label}: {reply}"
    );
    Ok(())
}

#[test]
fn every_s1_refusal_row_is_the_same_in_apply_mode() -> TestResult {
    let directory = tempfile::tempdir()?;
    let app = new_app(&directory);
    let mut ids = RequestIds::default();
    for case in refusals() {
        let id = (case.draw)(&app, &mut ids)?;
        let revision = current_revision(&app, &id)?;
        let before = document(&app, &id)?;
        let parameters = (case.request)(&id);

        let preview = call(&app, &ids.fresh(), "layout.structure", parameters.clone())?;
        assert_refusal(case.name, &preview, case.expected, revision)?;

        let apply_parameters = in_mode(parameters, "apply", Some(revision));
        let apply = call(&app, &ids.fresh(), "layout.structure", apply_parameters)?;
        assert_refusal(case.name, &apply, case.expected, revision)?;
        assert_eq!(
            apply["outcome"], preview["outcome"],
            "{}: {apply}",
            case.name
        );
        assert_eq!(
            apply["document_revision"], preview["document_revision"],
            "{}: {apply}",
            case.name
        );
        assert_eq!(
            apply["diagnostics"], preview["diagnostics"],
            "{}: the apply's diagnostics differ from the preview's",
            case.name
        );
        assert!(apply["result"].is_null(), "{}: {apply}", case.name);
        assert_eq!(
            document(&app, &id)?,
            before,
            "{}: the refused apply changed the document",
            case.name
        );
    }
    Ok(())
}

#[test]
fn an_unknown_mode_is_refused_with_both_modes() -> TestResult {
    let directory = tempfile::tempdir()?;
    let app = new_app(&directory);
    let mut ids = RequestIds::default();
    let id = butane_document(&app, &mut ids)?;
    let parameters = json!({"document_id": id, "mode": "rotate", "selection": {"atoms": ["a2"]}});
    let reply = call(&app, &ids.fresh(), "layout.structure", parameters)?;

    let expected = Expected {
        outcome: "error",
        code: "invalid_request",
        path: Some("parameters.mode"),
        reason: None,
        names_revision: false,
    };
    assert_refusal("mode rotate", &reply, expected, 1)?;
    assert_eq!(
        sole_diagnostic(&reply)?["details"],
        json!({"supported": ["preview", "apply"]}),
        "{reply}"
    );
    Ok(())
}

#[test]
fn the_mode_gated_fields_are_refused_by_name() -> TestResult {
    let directory = tempfile::tempdir()?;
    let app = new_app(&directory);
    let mut ids = RequestIds::default();
    let id = butane_document(&app, &mut ids)?;
    let before = document(&app, &id)?;
    let cases = [
        (
            "a preview with expected_revision",
            json!({"document_id": id, "mode": "preview", "selection": {"atoms": ["a2"]}, "expected_revision": 1}),
            "parameters.expected_revision",
            json!({"mode": "preview"}),
        ),
        (
            "a preview with selection_before",
            json!({"document_id": id, "mode": "preview", "selection": {"atoms": ["a2"]}, "selection_before": {}}),
            "parameters.selection_before",
            json!({"mode": "preview"}),
        ),
        (
            "a preview with selection_after",
            json!({"document_id": id, "mode": "preview", "selection": {"atoms": ["a2"]}, "selection_after": {}}),
            "parameters.selection_after",
            json!({"mode": "preview"}),
        ),
        (
            "an apply without expected_revision",
            json!({"document_id": id, "mode": "apply", "selection": {"atoms": ["a2"]}}),
            "parameters.expected_revision",
            json!({}),
        ),
    ];
    for (label, parameters, path, details) in cases {
        let reply = call(&app, &ids.fresh(), "layout.structure", parameters)?;
        let expected = Expected {
            outcome: "error",
            code: "invalid_request",
            path: Some(path),
            reason: None,
            names_revision: false,
        };
        assert_refusal(label, &reply, expected, 1)?;
        assert_eq!(
            sole_diagnostic(&reply)?["details"],
            details,
            "{label}: {reply}"
        );
        assert_eq!(
            document(&app, &id)?,
            before,
            "{label}: the refused request changed the document"
        );
    }
    Ok(())
}

#[test]
fn the_checks_run_in_the_contract_order() -> TestResult {
    let directory = tempfile::tempdir()?;
    let app = new_app(&directory);
    let mut ids = RequestIds::default();
    let id = butane_document(&app, &mut ids)?;
    let before = document(&app, &id)?;
    let cases = [
        (
            "the time limit before the mode",
            json!({"document_id": id, "mode": "rotate", "selection": {"atoms": ["a2"]}, "time_limit_ms": MAXIMUM_LIMIT_MS + 1}),
            Expected {
                outcome: "error",
                code: "invalid_request",
                path: Some("parameters.time_limit_ms"),
                reason: None,
                names_revision: false,
            },
        ),
        (
            "the mode before the fields of the other mode",
            json!({"document_id": id, "mode": "rotate", "selection": {"atoms": ["a2"]}, "expected_revision": 1}),
            Expected {
                outcome: "error",
                code: "invalid_request",
                path: Some("parameters.mode"),
                reason: None,
                names_revision: false,
            },
        ),
        (
            "expected_revision before selection_before and selection_after",
            json!({"document_id": id, "mode": "preview", "selection": {"atoms": ["a2"]}, "expected_revision": 1, "selection_before": {}, "selection_after": {}}),
            Expected {
                outcome: "error",
                code: "invalid_request",
                path: Some("parameters.expected_revision"),
                reason: None,
                names_revision: false,
            },
        ),
        (
            "selection_before before selection_after",
            json!({"document_id": id, "mode": "preview", "selection": {"atoms": ["a2"]}, "selection_before": {}, "selection_after": {}}),
            Expected {
                outcome: "error",
                code: "invalid_request",
                path: Some("parameters.selection_before"),
                reason: None,
                names_revision: false,
            },
        ),
        (
            "expected_revision before the scope",
            json!({"document_id": id, "mode": "apply", "selection": {"atoms": ["a2"]}, "scope": "rings"}),
            Expected {
                outcome: "error",
                code: "invalid_request",
                path: Some("parameters.expected_revision"),
                reason: None,
                names_revision: false,
            },
        ),
        (
            "the scope before the instance",
            json!({"document_id": id, "mode": "apply", "expected_revision": 1, "selection": {"atoms": ["a2"]}, "scope": "rings", "instance": "x!"}),
            Expected {
                outcome: "error",
                code: "invalid_request",
                path: Some("parameters.scope"),
                reason: None,
                names_revision: false,
            },
        ),
        (
            "the selection before selection_after",
            json!({"document_id": id, "mode": "apply", "expected_revision": 1, "selection": {"atoms": ["x!"]}, "selection_after": {"atoms": ["x!"]}}),
            Expected {
                outcome: "error",
                code: "invalid_request",
                path: Some("parameters.selection"),
                reason: Some("selection_unknown"),
                names_revision: false,
            },
        ),
        (
            "the revision before the instance",
            json!({"document_id": id, "mode": "apply", "expected_revision": 99, "selection": {"atoms": ["a2"]}, "instance": "s9"}),
            Expected {
                outcome: "error",
                code: "revision_conflict",
                path: None,
                reason: None,
                names_revision: true,
            },
        ),
    ];
    for (label, parameters, expected) in cases {
        let reply = call(&app, &ids.fresh(), "layout.structure", parameters)?;
        assert_refusal(label, &reply, expected, 1)?;
        assert_eq!(
            document(&app, &id)?,
            before,
            "{label}: the refused request changed the document"
        );
    }
    Ok(())
}

#[test]
fn a_malformed_selection_before_or_after_is_refused_before_the_document_is_read() -> TestResult {
    let directory = tempfile::tempdir()?;
    let app = new_app(&directory);
    let mut ids = RequestIds::default();
    let id = butane_document(&app, &mut ids)?;
    let before = document(&app, &id)?;
    for field in ["selection_before", "selection_after"] {
        let base = json!({"document_id": id, "mode": "preview", "selection": {"atoms": ["a2"]}});
        let mut parameters = in_mode(base, "apply", Some(1));
        parameters[field] = json!({"atoms": ["x!"]});
        let reply = call(&app, &ids.fresh(), "layout.structure", parameters)?;

        let expected = Expected {
            outcome: "error",
            code: "invalid_command",
            path: None,
            reason: Some("selection_unknown"),
            names_revision: false,
        };
        assert_refusal(field, &reply, expected, 1)?;
        assert_eq!(
            document(&app, &id)?,
            before,
            "{field}: the refused apply changed the document"
        );
    }
    Ok(())
}

#[test]
fn a_stale_revision_is_a_revision_conflict_before_any_layout_refusal() -> TestResult {
    let directory = tempfile::tempdir()?;
    let app = new_app(&directory);
    let mut ids = RequestIds::default();
    for case in stale_cases() {
        let id = (case.draw)(&app, &mut ids)?;
        let extra_atom =
            json!([{"op": "add_atom", "element": "C", "position": {"x": 500, "y": 500}}]);
        transact(&app, &mut ids, &id, 1, extra_atom)?;
        let current = current_revision(&app, &id)?;
        assert_eq!(
            current, 2,
            "{}: the extra atom must make revision 2",
            case.name
        );
        let before = document(&app, &id)?;

        let parameters = in_mode((case.request)(&id), "apply", Some(1));
        let reply = call(&app, &ids.fresh(), "layout.structure", parameters)?;
        assert_refusal(case.name, &reply, case.expected, current)?;
        assert_eq!(
            sole_diagnostic(&reply)?["details"]["current_revision"],
            current,
            "{}: {reply}",
            case.name
        );
        assert_eq!(
            document(&app, &id)?,
            before,
            "{}: the stale apply changed the document",
            case.name
        );
    }
    Ok(())
}

#[test]
fn an_unknown_document_names_the_model() -> TestResult {
    let directory = tempfile::tempdir()?;
    let app = new_app(&directory);
    let mut ids = RequestIds::default();
    let parameters = json!({
        "document_id": "no-such-document",
        "mode": "apply",
        "expected_revision": 0,
        "selection": {"atoms": ["a2"]},
    });
    let reply = call(&app, &ids.fresh(), "layout.structure", parameters)?;

    let expected = Expected {
        outcome: "error",
        code: "document_not_found",
        path: None,
        reason: None,
        names_revision: false,
    };
    assert_refusal("an unknown document", &reply, expected, 0)
}
