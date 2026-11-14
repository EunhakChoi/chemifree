//! The `layout.structure` preview through the application: the shape and placements of a successful
//! reply, each refusal's code, path and reason, the instance rules, and a stop under a time limit or a
//! cancel. The preview changes nothing, so no reply and no document changes after it. The expected
//! positions come from the chain layout rules: a bond is 30 pt long, and a bond at 30 degrees moves a
//! point by `(15 * sqrt(3), -15)`.

use std::sync::atomic::{AtomicUsize, Ordering};

use chemifree_application::{Application, CancelFlag, Interrupt, Interrupted};
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};

type App = Application<LocalCapabilities, FileDocumentRepository>;

/// The model that the preview's replies name.
const MODEL_ID: &str = "chemifree-chain-layout-1";
/// The limit of a request that sets none, in milliseconds.
const DEFAULT_LIMIT_MS: u64 = 5_000;
/// The largest limit a request may set, in milliseconds.
const MAXIMUM_LIMIT_MS: u64 = 8_000;
/// The tolerance, in pt, of every compared coordinate.
const TOLERANCE: f64 = 1e-6;

/// `15 * sqrt(3)`, the horizontal step of a 30 pt bond at 30 degrees.
fn dx() -> f64 {
    15.0 * 3f64.sqrt()
}

fn new_app(directory: &tempfile::TempDir) -> App {
    Application::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    )
}

fn request(id: &str, operation: &str, parameters: Value) -> Vec<u8> {
    json!({"contract": 1, "request_id": id, "profile": "preserve-semantics", "operation": operation, "parameters": parameters})
        .to_string()
        .into_bytes()
}

fn call(app: &App, id: &str, operation: &str, parameters: Value) -> Value {
    serde_json::to_value(app.execute(&request(id, operation, parameters))).unwrap()
}

fn call_with(
    app: &App,
    id: &str,
    operation: &str,
    parameters: Value,
    cancel: &dyn Interrupt,
) -> Value {
    serde_json::to_value(app.execute_with(&request(id, operation, parameters), cancel)).unwrap()
}

fn document(app: &App, id: &str) -> Value {
    serde_json::to_value(app.document(id).unwrap()).unwrap()
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

/// Counts the questions a computation asks, and stops at the `cancel_at`-th with `reason`.
struct CancelAt {
    cancel_at: usize,
    reason: Interrupted,
    calls: AtomicUsize,
}

impl CancelAt {
    fn new(cancel_at: usize, reason: Interrupted) -> Self {
        Self {
            cancel_at,
            reason,
            calls: AtomicUsize::new(0),
        }
    }

    fn asked(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl Interrupt for CancelAt {
    fn check(&self) -> Result<(), Interrupted> {
        if self.calls.fetch_add(1, Ordering::SeqCst) + 1 == self.cancel_at {
            Err(self.reason)
        } else {
            Ok(())
        }
    }
}

/// Applies `commands` to document `id` at `expected_revision`, which must succeed.
fn transact(app: &App, label: &str, id: &str, expected_revision: u64, commands: Value) {
    let reply = call(
        app,
        &format!("transact-{label}"),
        "document.transaction",
        json!({"document_id": id, "expected_revision": expected_revision, "commands": commands}),
    );
    assert_eq!(reply["outcome"], "ok", "{label}: {reply}");
}

/// Creates a document titled `title`, applies `commands` to it at revision 0, and returns its id.
fn draw(app: &App, title: &str, commands: Value) -> String {
    let id = app.create_document(None, Some(title)).unwrap().document.id;
    transact(app, title, &id, 0, commands);
    id
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
fn loose_carbon(index: usize) -> Value {
    let x = index as f64 * 30.0;
    json!({"op": "add_atom", "element": "C", "position": {"x": x, "y": 0}})
}

/// The parameters of a preview of `atoms` in document `id`, with the default scope and instance.
fn preview_parameters(id: &str, atoms: &[&str]) -> Value {
    json!({"document_id": id, "mode": "preview", "selection": {"atoms": atoms}})
}

/// Where the butane of [`butane`] lands when its first carbon starts at (`x0`, 0). Its carbons start
/// at `x0`, `x0 + 30`, `x0 + 60` and `x0 + 90`, so the layout keeps the centroid (`x0 + 45`, 0) and
/// zigzags about it: the first carbon sits 1.5 steps left of it and the last 1.5 steps right.
fn butane_after(x0: f64) -> [(f64, f64); 4] {
    let centre = x0 + 45.0;
    [
        (centre - 1.5 * dx(), 7.5),
        (centre - 0.5 * dx(), -7.5),
        (centre + 0.5 * dx(), 7.5),
        (centre + 1.5 * dx(), -7.5),
    ]
}

/// The `before` and `after` coordinates of atom `atom` in the placements of a successful `reply`, as
/// `[before_x, before_y, after_x, after_y]`.
fn placement(reply: &Value, atom: &str) -> [f64; 4] {
    let entry = reply["result"]["positions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["atom"] == atom)
        .unwrap_or_else(|| panic!("{atom} has no placement: {reply}"));
    [
        coordinate(&entry["before"]["x"]),
        coordinate(&entry["before"]["y"]),
        coordinate(&entry["after"]["x"]),
        coordinate(&entry["after"]["y"]),
    ]
}

fn coordinate(value: &Value) -> f64 {
    value
        .as_f64()
        .unwrap_or_else(|| panic!("{value} is not a coordinate"))
}

/// Asserts that `actual` lies within `TOLERANCE` of `expected`.
fn assert_near(actual: f64, expected: f64, label: &str) {
    assert!(
        (actual - expected).abs() < TOLERANCE,
        "{label}: expected {expected}, got {actual}"
    );
}

/// Asserts that atom `atom` of a successful `reply` starts at `before` and is placed at `after`.
fn assert_placement(reply: &Value, atom: &str, before: (f64, f64), after: (f64, f64)) {
    let [before_x, before_y, after_x, after_y] = placement(reply, atom);
    assert_near(before_x, before.0, &format!("{atom} before x"));
    assert_near(before_y, before.1, &format!("{atom} before y"));
    assert_near(after_x, after.0, &format!("{atom} after x"));
    assert_near(after_y, after.1, &format!("{atom} after y"));
}

/// The diagnostic codes of `reply`, in order.
fn codes(reply: &Value) -> Vec<&str> {
    reply["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|diagnostic| diagnostic["code"].as_str().unwrap())
        .collect()
}

/// The reply fields that a preview which is not stopped must leave as they are.
fn observed(reply: &Value) -> Value {
    json!({
        "outcome": reply["outcome"],
        "document_revision": reply["document_revision"],
        "result": reply["result"],
        "diagnostics": reply["diagnostics"],
        "loss_report": reply["loss_report"],
        "model_id": reply["model_id"],
    })
}

/// Asserts that `reply` is an error with exactly one diagnostic, of `code` at `path`, and returns it.
fn sole_diagnostic<'a>(reply: &'a Value, code: &str, path: &str) -> &'a Value {
    assert_eq!(reply["outcome"], "error", "{reply}");
    let diagnostics = reply["diagnostics"].as_array().unwrap();
    assert_eq!(diagnostics.len(), 1, "{reply}");
    assert_eq!(diagnostics[0]["code"], code, "{reply}");
    assert_eq!(diagnostics[0]["path"], path, "{reply}");
    &diagnostics[0]
}

/// Asserts that `reply` is a stop of the preview with `code`: an error with no result, the analysed
/// `revision`, the preview's model and exactly one diagnostic.
fn assert_stopped(reply: &Value, code: &str, revision: u64) {
    assert_eq!(reply["outcome"], "error", "{reply}");
    assert!(reply["result"].is_null(), "{reply}");
    assert_eq!(reply["document_revision"], revision, "{reply}");
    assert_eq!(reply["model_id"], MODEL_ID, "{reply}");
    let diagnostics = reply["diagnostics"].as_array().unwrap();
    assert_eq!(diagnostics.len(), 1, "{reply}");
    assert_eq!(diagnostics[0]["code"], code, "{reply}");
}

#[test]
fn a_free_chain_is_laid_out_on_the_zigzag_in_the_components_scope_by_default() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let id = draw(&app, "butane", butane());
    let mut ids = RequestIds::default();
    let reply = call(
        &app,
        &ids.fresh(),
        "layout.structure",
        preview_parameters(&id, &["a2"]),
    );

    assert_eq!(reply["outcome"], "ok", "{reply}");
    assert_eq!(reply["document_revision"], 1, "{reply}");
    assert_eq!(reply["model_id"], MODEL_ID, "{reply}");
    let result = &reply["result"];
    assert_eq!(result["mode"], "preview", "{reply}");
    assert_eq!(result["instance"], "s1", "{reply}");
    assert_eq!(result["scope"], "components", "{reply}");
    assert_near(
        coordinate(&result["bond_length_pt"]),
        30.0,
        "bond_length_pt",
    );
    assert_eq!(
        result["layout"],
        json!({"atoms": ["a1", "a2", "a3", "a4"], "bonds": ["b1", "b2", "b3"]}),
        "{reply}"
    );
    assert_eq!(result["fixed_atom_count"], 0, "{reply}");
    assert_eq!(result["fixed_ring_systems"], 0, "{reply}");
    assert_eq!(result["boundary"], json!([]), "{reply}");
    assert_eq!(
        result["components"],
        json!([{
            "atoms": ["a1", "a2", "a3", "a4"],
            "bonds": ["b1", "b2", "b3"],
            "anchor": null,
            "root": "a1",
            "chains": [{"atoms": ["a1", "a2", "a3", "a4"], "branch_from": null}],
        }]),
        "{reply}"
    );
    let placed: Vec<&str> = result["positions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["atom"].as_str().unwrap())
        .collect();
    assert_eq!(placed, ["a1", "a2", "a3", "a4"], "{reply}");
    for (index, (before, after)) in [(0.0, 0.0), (30.0, 0.0), (60.0, 0.0), (90.0, 0.0)]
        .into_iter()
        .zip(butane_after(0.0))
        .enumerate()
    {
        assert_placement(&reply, &format!("a{}", index + 1), before, after);
    }
    assert_eq!(
        codes(&reply),
        ["layout_preview_only", "layout_not_implemented"],
        "{reply}"
    );
    assert_eq!(reply["diagnostics"][0]["severity"], "info", "{reply}");
}

#[test]
fn the_preview_changes_no_document_and_repeats_exactly() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let id = draw(&app, "butane", butane());
    let before = document(&app, &id);
    let mut ids = RequestIds::default();
    let parameters = preview_parameters(&id, &["a2"]);
    let first = call(&app, &ids.fresh(), "layout.structure", parameters.clone());
    let second = call(&app, &ids.fresh(), "layout.structure", parameters);

    assert_eq!(first["outcome"], "ok", "{first}");
    assert_eq!(first["document_revision"], 1, "{first}");
    assert_eq!(
        observed(&second),
        observed(&first),
        "a repeated preview differs from the first"
    );
    assert_eq!(
        document(&app, &id),
        before,
        "the preview changed the document"
    );
}

#[test]
fn a_selected_end_atom_is_anchored_to_its_fixed_neighbour() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let id = draw(&app, "butane", butane());
    let mut ids = RequestIds::default();
    let mut parameters = preview_parameters(&id, &["a4"]);
    parameters["scope"] = json!("selection");
    let reply = call(&app, &ids.fresh(), "layout.structure", parameters);

    assert_eq!(reply["outcome"], "ok", "{reply}");
    let result = &reply["result"];
    assert_eq!(result["scope"], "selection", "{reply}");
    assert_eq!(
        result["layout"],
        json!({"atoms": ["a4"], "bonds": []}),
        "{reply}"
    );
    assert_eq!(result["fixed_atom_count"], 3, "{reply}");
    assert_eq!(
        result["boundary"],
        json!([{"atom": "a4", "fixed_atom": "a3", "bond": "b3"}]),
        "{reply}"
    );
    assert_eq!(
        result["components"][0]["anchor"],
        json!({"fixed_atom": "a3", "bond": "b3"}),
        "{reply}"
    );
    // Carbon 3 has one other fixed neighbour, carbon 2, at 180 degrees, so the tie between the
    // candidates at 300 and 60 degrees takes 300 degrees.
    assert_placement(&reply, "a4", (90.0, 0.0), (75.0, -dx()));
    assert_eq!(
        codes(&reply),
        [
            "layout_preview_only",
            "layout_no_bonds",
            "layout_not_implemented"
        ],
        "{reply}"
    );
}

#[test]
fn a_request_without_a_mode_is_refused_as_unreadable_parameters() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let id = draw(&app, "butane", butane());
    let mut ids = RequestIds::default();
    let reply = call(
        &app,
        &ids.fresh(),
        "layout.structure",
        json!({"document_id": id, "selection": {"atoms": ["a2"]}}),
    );

    let diagnostic = sole_diagnostic(&reply, "invalid_request", "parameters");
    assert!(
        diagnostic["message"]
            .as_str()
            .unwrap()
            .starts_with("Invalid parameters"),
        "{reply}"
    );
    assert!(reply["model_id"].is_null(), "{reply}");
}

#[test]
fn a_mode_other_than_preview_is_refused_with_the_supported_modes() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let id = draw(&app, "butane", butane());
    let mut ids = RequestIds::default();
    let mut parameters = preview_parameters(&id, &["a2"]);
    parameters["mode"] = json!("apply");
    let reply = call(&app, &ids.fresh(), "layout.structure", parameters);

    let diagnostic = sole_diagnostic(&reply, "invalid_request", "parameters.mode");
    assert_eq!(
        diagnostic["details"]["supported"],
        json!(["preview"]),
        "{reply}"
    );
    assert_eq!(reply["model_id"], MODEL_ID, "{reply}");
    assert!(reply["document_revision"].is_null(), "{reply}");
}

#[test]
fn an_unknown_scope_is_refused_by_its_parameter() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let id = draw(&app, "butane", butane());
    let mut ids = RequestIds::default();
    let mut parameters = preview_parameters(&id, &["a2"]);
    parameters["scope"] = json!("bogus");
    let reply = call(&app, &ids.fresh(), "layout.structure", parameters);

    sole_diagnostic(&reply, "invalid_request", "parameters.scope");
    assert_eq!(reply["model_id"], MODEL_ID, "{reply}");
    assert!(reply["document_revision"].is_null(), "{reply}");
}

#[test]
fn an_unknown_instance_is_refused_by_name_and_names_no_revision() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let id = draw(&app, "butane", butane());
    let mut ids = RequestIds::default();
    let mut parameters = preview_parameters(&id, &["a2"]);
    parameters["instance"] = json!("s9");
    let reply = call(&app, &ids.fresh(), "layout.structure", parameters);

    let diagnostic = sole_diagnostic(&reply, "invalid_request", "parameters.instance");
    assert_eq!(
        diagnostic["message"], "s9 is not a scene instance of this document.",
        "{reply}"
    );
    assert_eq!(reply["model_id"], MODEL_ID, "{reply}");
    assert!(reply["document_revision"].is_null(), "{reply}");
}

#[test]
fn an_instance_must_be_named_when_the_document_has_two() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let id = draw(&app, "butane", butane());
    transact(
        &app,
        "second-instance",
        &id,
        1,
        json!([{"op": "add_instance", "offset": {"x": 200, "y": 0}}]),
    );
    let mut ids = RequestIds::default();
    let reply = call(
        &app,
        &ids.fresh(),
        "layout.structure",
        preview_parameters(&id, &["a2"]),
    );

    assert_eq!(reply["outcome"], "error", "{reply}");
    assert_eq!(
        reply["diagnostics"][0]["code"], "invalid_request",
        "{reply}"
    );
    assert_eq!(
        reply["diagnostics"][0]["message"],
        "instance is required: the document has 2 scene instances.",
        "{reply}"
    );
}

#[test]
fn a_named_second_instance_is_laid_out_at_its_own_positions() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let id = draw(&app, "butane", butane());
    transact(
        &app,
        "second-instance",
        &id,
        1,
        json!([{"op": "add_instance", "offset": {"x": 200, "y": 0}}]),
    );
    let mut ids = RequestIds::default();
    let mut parameters = preview_parameters(&id, &["a2"]);
    parameters["instance"] = json!("s2");
    let reply = call(&app, &ids.fresh(), "layout.structure", parameters);

    assert_eq!(reply["outcome"], "ok", "{reply}");
    assert_eq!(reply["document_revision"], 2, "{reply}");
    assert_eq!(reply["result"]["instance"], "s2", "{reply}");
    for (index, (before, after)) in [(200.0, 0.0), (230.0, 0.0), (260.0, 0.0), (290.0, 0.0)]
        .into_iter()
        .zip(butane_after(200.0))
        .enumerate()
    {
        assert_placement(&reply, &format!("a{}", index + 1), before, after);
    }
}

#[test]
fn an_unknown_atom_is_refused_as_an_unknown_selection() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let id = draw(&app, "butane", butane());
    let mut ids = RequestIds::default();
    let reply = call(
        &app,
        &ids.fresh(),
        "layout.structure",
        preview_parameters(&id, &["a9"]),
    );

    let diagnostic = sole_diagnostic(&reply, "invalid_request", "parameters.selection");
    assert_eq!(
        diagnostic["details"]["reason"], "selection_unknown",
        "{reply}"
    );
    assert_eq!(reply["document_revision"], 1, "{reply}");
}

#[test]
fn an_empty_selection_is_refused_with_its_reason() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let id = draw(&app, "butane", butane());
    let mut ids = RequestIds::default();
    let reply = call(
        &app,
        &ids.fresh(),
        "layout.structure",
        preview_parameters(&id, &[]),
    );

    let diagnostic = sole_diagnostic(&reply, "invalid_request", "parameters.selection");
    assert_eq!(
        diagnostic["details"]["reason"], "layout_selection_empty",
        "{reply}"
    );
    assert_eq!(reply["document_revision"], 1, "{reply}");
}

#[test]
fn a_selection_of_scene_objects_is_unsupported() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let id = draw(&app, "butane", butane());
    let mut ids = RequestIds::default();
    let mut parameters = preview_parameters(&id, &["a2"]);
    parameters["selection"]["objects"] = json!(["s1"]);
    let reply = call(&app, &ids.fresh(), "layout.structure", parameters);

    assert_eq!(reply["outcome"], "unsupported", "{reply}");
    assert_eq!(
        reply["diagnostics"][0]["code"], "layout_unsupported",
        "{reply}"
    );
    assert_eq!(
        reply["diagnostics"][0]["details"]["reason"], "object_selection",
        "{reply}"
    );
    assert_eq!(reply["document_revision"], 1, "{reply}");
    assert!(reply["result"].is_null(), "{reply}");
}

#[test]
fn a_ring_atom_in_the_layout_set_is_unsupported() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let id = draw(
        &app,
        "ring",
        json!([{"op": "insert_ring", "instance": "s1", "center": {"x": 0, "y": 0}}]),
    );
    let mut ids = RequestIds::default();
    let mut parameters = preview_parameters(&id, &["a1"]);
    parameters["scope"] = json!("selection");
    let reply = call(&app, &ids.fresh(), "layout.structure", parameters);

    assert_eq!(reply["outcome"], "unsupported", "{reply}");
    assert_eq!(
        reply["diagnostics"][0]["code"], "layout_unsupported",
        "{reply}"
    );
    assert_eq!(
        reply["diagnostics"][0]["details"]["reason"], "ring_in_layout_set",
        "{reply}"
    );
    assert_eq!(
        reply["diagnostics"][0]["details"]["atoms"],
        json!(["a1"]),
        "{reply}"
    );
    assert_eq!(reply["document_revision"], 1, "{reply}");
}

#[test]
fn a_layout_of_more_than_500_atoms_is_a_resource_limit() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let id = app.create_document(None, Some("many")).unwrap().document.id;
    transact(
        &app,
        "many-first",
        &id,
        0,
        Value::Array((0..250).map(loose_carbon).collect()),
    );
    transact(
        &app,
        "many-second",
        &id,
        1,
        Value::Array((250..501).map(loose_carbon).collect()),
    );
    let atoms: Vec<String> = (1..=501).map(|serial| format!("a{serial}")).collect();
    let mut ids = RequestIds::default();
    let reply = call(
        &app,
        &ids.fresh(),
        "layout.structure",
        json!({"document_id": id, "mode": "preview", "selection": {"atoms": atoms}, "scope": "selection"}),
    );

    let diagnostic = sole_diagnostic(&reply, "resource_limit", "parameters.selection");
    assert_eq!(diagnostic["details"]["atoms"], 501, "{reply}");
    assert_eq!(diagnostic["details"]["limit"], 500, "{reply}");
    assert_eq!(reply["model_id"], MODEL_ID, "{reply}");
    assert_eq!(reply["document_revision"], 2, "{reply}");
}

#[test]
fn a_pair_closer_than_half_a_bond_is_an_overlap_warning() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let id = draw(
        &app,
        "pair",
        json!([
            {"op": "add_atom", "element": "C", "position": {"x": 0, "y": 0}},
            {"op": "add_atom", "element": "C", "position": {"x": 10, "y": 0}}
        ]),
    );
    let mut ids = RequestIds::default();
    let reply = call(
        &app,
        &ids.fresh(),
        "layout.structure",
        preview_parameters(&id, &["a2"]),
    );

    assert_eq!(reply["outcome"], "ok", "{reply}");
    assert_eq!(
        codes(&reply),
        [
            "layout_preview_only",
            "layout_overlap",
            "layout_no_bonds",
            "layout_not_implemented"
        ],
        "{reply}"
    );
    let overlap = &reply["diagnostics"][1];
    assert_eq!(overlap["severity"], "warning", "{reply}");
    assert_eq!(
        overlap["details"]["pairs"],
        json!([["a1", "a2"]]),
        "{reply}"
    );
    assert_eq!(overlap["details"]["count"], 1, "{reply}");
}

#[test]
fn a_zero_limit_stops_with_the_limit_and_changes_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let id = draw(&app, "butane", butane());
    let before = document(&app, &id);
    let mut ids = RequestIds::default();
    let mut parameters = preview_parameters(&id, &["a2"]);
    parameters["time_limit_ms"] = json!(0);
    let reply = call(&app, &ids.fresh(), "layout.structure", parameters);

    assert_stopped(&reply, "time_limit", 1);
    assert_eq!(
        reply["diagnostics"][0]["details"],
        json!({"limit_ms": 0}),
        "{reply}"
    );
    assert_eq!(
        document(&app, &id),
        before,
        "the stopped preview changed the document"
    );
}

#[test]
fn a_limit_above_the_maximum_is_refused_before_the_document_is_read() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let mut ids = RequestIds::default();
    let reply = call(
        &app,
        &ids.fresh(),
        "layout.structure",
        json!({
            "document_id": "no-such-document",
            "mode": "preview",
            "selection": {"atoms": ["a2"]},
            "time_limit_ms": MAXIMUM_LIMIT_MS + 1
        }),
    );

    let diagnostic = sole_diagnostic(&reply, "invalid_request", "parameters.time_limit_ms");
    assert_eq!(
        diagnostic["details"]["maximum_ms"], MAXIMUM_LIMIT_MS,
        "{reply}"
    );
    assert_eq!(reply["model_id"], MODEL_ID, "{reply}");
    assert!(reply["document_revision"].is_null(), "{reply}");
}

#[test]
fn a_cancel_before_the_placement_stops_with_cancelled() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let id = draw(&app, "butane", butane());
    let mut ids = RequestIds::default();
    let cancel = CancelFlag::new();
    cancel.cancel();
    let reply = call_with(
        &app,
        &ids.fresh(),
        "layout.structure",
        preview_parameters(&id, &["a2"]),
        &cancel,
    );

    assert_stopped(&reply, "cancelled", 1);
    assert_eq!(reply["diagnostics"][0]["details"], json!({}), "{reply}");
}

#[test]
fn a_stop_during_the_placement_names_the_limit_and_a_late_stop_changes_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let id = draw(&app, "butane", butane());
    let mut ids = RequestIds::default();

    // Question 1 is the check before the placement, and question 3 is the first placed carbon.
    let stopper = CancelAt::new(3, Interrupted::TimeLimit);
    let stopped = call_with(
        &app,
        &ids.fresh(),
        "layout.structure",
        preview_parameters(&id, &["a2"]),
        &stopper,
    );
    assert_stopped(&stopped, "time_limit", 1);
    assert_eq!(stopper.asked(), 3, "{stopped}");
    assert_eq!(
        stopped["diagnostics"][0]["details"],
        json!({"limit_ms": DEFAULT_LIMIT_MS}),
        "{stopped}"
    );

    let plain = call(
        &app,
        &ids.fresh(),
        "layout.structure",
        preview_parameters(&id, &["a2"]),
    );
    assert_eq!(plain["outcome"], "ok", "{plain}");
    let late = CancelAt::new(1_000, Interrupted::TimeLimit);
    let unstopped = call_with(
        &app,
        &ids.fresh(),
        "layout.structure",
        preview_parameters(&id, &["a2"]),
        &late,
    );
    assert_eq!(
        observed(&unstopped),
        observed(&plain),
        "a stop that never comes changed the reply"
    );
}

#[test]
fn a_cancel_during_the_placement_stops_with_cancelled_and_no_limit() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let id = draw(&app, "butane", butane());
    let mut ids = RequestIds::default();
    let stopper = CancelAt::new(3, Interrupted::Cancelled);
    let reply = call_with(
        &app,
        &ids.fresh(),
        "layout.structure",
        preview_parameters(&id, &["a2"]),
        &stopper,
    );

    assert_stopped(&reply, "cancelled", 1);
    assert_eq!(stopper.asked(), 3, "{reply}");
    assert_eq!(reply["diagnostics"][0]["details"], json!({}), "{reply}");
}
