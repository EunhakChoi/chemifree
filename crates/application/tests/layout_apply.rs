//! The `layout.structure` apply through the application: a successful apply commits the preview's
//! proposal as one undoable edit at the revision it names. The tests check the committed positions
//! against the worked examples and against the preview at the same revision, the shape of the
//! document reply and the diagnostics of a success, the move of the named instance alone, the
//! overlap warning, and the 500-atom limit of one edit.

use std::collections::BTreeMap;
use std::error::Error;

use chemifree_application::Application;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};

type App = Application<LocalCapabilities, FileDocumentRepository>;
/// The result of a test or helper, so that a failed read is an error rather than a panic.
type TestResult<T = ()> = Result<T, Box<dyn Error>>;
/// Atom IDs mapped to the bit patterns of their `[x, y]`, so that a change in the last bit shows.
type Positions = BTreeMap<String, [u64; 2]>;

/// The model that the apply's replies name.
const MODEL_ID: &str = "chemifree-chain-layout-1";
/// The largest limit a request may set, in milliseconds.
const MAXIMUM_LIMIT_MS: u64 = 8_000;
/// The tolerance, in pt, of an applied position against its proposal.
const APPLIED_TOLERANCE: f64 = 1e-9;
/// The most atoms that one layout may place, and so the most that one edit may move.
const LAYOUT_LIMIT: usize = 500;
/// The message of the preview's `layout_preview_only` diagnostic.
const PREVIEW_ONLY_MESSAGE: &str =
    "Nothing was changed; mode \"apply\" applies this layout as one undoable edit.";

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

fn call(app: &App, id: &str, operation: &str, parameters: Value) -> TestResult<Value> {
    Ok(serde_json::to_value(
        app.execute(&request(id, operation, parameters)),
    )?)
}

/// The whole document reply of document `id`, as the read operation returns it.
fn document_json(app: &App, id: &str) -> TestResult<Value> {
    Ok(serde_json::to_value(app.document(id)?)?)
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
    label: &str,
    id: &str,
    expected_revision: u64,
    commands: Value,
) -> TestResult {
    let reply = call(
        app,
        &format!("transact-{label}"),
        "document.transaction",
        json!({"document_id": id, "expected_revision": expected_revision, "commands": commands}),
    )?;
    assert_eq!(reply["outcome"], "ok", "{label}: {reply}");
    Ok(())
}

/// Creates a document titled `title`, applies `commands` to it at revision 0, and returns its id.
fn draw(app: &App, title: &str, commands: Value) -> TestResult<String> {
    let id = app.create_document(None, Some(title))?.document.id;
    transact(app, title, &id, 0, commands)?;
    Ok(id)
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

/// `count` carbons 30 pt apart along the x axis, each bonded to the one before it, so that they form
/// one chain whose atoms are `a1` to `a{count}`.
fn chain(count: usize) -> Value {
    let commands: Vec<Value> = (0..count)
        .map(|index| {
            let mut command =
                json!({"op": "add_atom", "element": "C", "position": {"x": 30 * index, "y": 0}});
            if index > 0 {
                command["bond_to"] = json!({"atom": format!("${}", index - 1), "order": "single"});
            }
            command
        })
        .collect();
    Value::Array(commands)
}

/// The parameters of a preview of `atoms` in document `id`, with the default scope and instance.
fn preview_parameters(id: &str, atoms: &[&str]) -> Value {
    json!({"document_id": id, "mode": "preview", "selection": {"atoms": atoms}})
}

/// The parameters of an apply of `atoms` in document `id` at `expected_revision`, with the default
/// scope and instance.
fn apply_parameters(id: &str, expected_revision: u64, atoms: &[&str]) -> Value {
    json!({"document_id": id, "mode": "apply", "expected_revision": expected_revision, "selection": {"atoms": atoms}})
}

/// `parameters` with `scope` set, when a scope is given.
fn with_scope(mut parameters: Value, scope: Option<&str>) -> Value {
    if let Some(scope) = scope {
        parameters["scope"] = json!(scope);
    }
    parameters
}

/// The zigzag proposal for the free butane of [`butane`] when its first carbon starts at the origin:
/// the layout keeps the centroid (45, 0) and puts the carbons at 120 degrees, alternately 7.5 pt
/// above and below the axis.
fn butane_after() -> [[f64; 2]; 4] {
    let centre = 45.0;
    [
        [centre - 1.5 * dx(), 7.5],
        [centre - 0.5 * dx(), -7.5],
        [centre + 0.5 * dx(), 7.5],
        [centre + 1.5 * dx(), -7.5],
    ]
}

/// The diagnostics of `reply`, in order; empty when the reply has none.
fn diagnostics(reply: &Value) -> &[Value] {
    match reply["diagnostics"].as_array() {
        Some(list) => list.as_slice(),
        None => &[],
    }
}

/// The diagnostic codes of `reply`, in order.
fn codes(reply: &Value) -> Vec<&str> {
    diagnostics(reply)
        .iter()
        .map(|diagnostic| diagnostic["code"].as_str().unwrap_or(""))
        .collect()
}

/// The first diagnostic of `reply` with `code`.
fn find_diagnostic<'a>(reply: &'a Value, code: &str) -> TestResult<&'a Value> {
    diagnostics(reply)
        .iter()
        .find(|diagnostic| diagnostic["code"] == code)
        .ok_or_else(|| format!("{code} is not among the diagnostics: {reply}").into())
}

fn number(value: &Value) -> TestResult<f64> {
    value
        .as_f64()
        .ok_or_else(|| format!("{value} is not a coordinate").into())
}

/// The `[x, y]` of a coordinate object.
fn point(value: &Value) -> TestResult<[f64; 2]> {
    Ok([number(&value["x"])?, number(&value["y"])?])
}

/// The position of atom `atom` in the document of an apply `reply`, as `[x, y]`.
fn reply_position(reply: &Value, atom: &str) -> TestResult<[f64; 2]> {
    let atoms = reply["result"]["document"]["atoms"]
        .as_array()
        .ok_or_else(|| format!("the reply has no atoms: {reply}"))?;
    let entry = atoms
        .iter()
        .find(|entry| entry["id"] == atom)
        .ok_or_else(|| format!("{atom} is not in the reply: {reply}"))?;
    point(&entry["position"])
}

/// Asserts that `actual` lies within `APPLIED_TOLERANCE` of `expected`.
fn assert_near(actual: f64, expected: f64, label: &str) {
    assert!(
        (actual - expected).abs() <= APPLIED_TOLERANCE,
        "{label}: expected {expected}, got {actual}"
    );
}

/// Asserts that `position` lies within `APPLIED_TOLERANCE` of `expected` in both coordinates.
fn assert_at(position: [f64; 2], expected: [f64; 2], label: &str) {
    let [x, y] = position;
    let [expected_x, expected_y] = expected;
    assert_near(x, expected_x, &format!("{label} x"));
    assert_near(y, expected_y, &format!("{label} y"));
}

/// Asserts that `reply` is an apply reply in the ordinary document shape: its `result` holds exactly
/// the document, session, recovery and notes of a document reply, so no preview field is among them.
fn assert_document_reply(reply: &Value) -> TestResult {
    let result = reply["result"]
        .as_object()
        .ok_or_else(|| format!("the reply has no result object: {reply}"))?;
    let mut keys: Vec<&str> = result.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        ["document", "notes", "recovery", "session"],
        "{reply}"
    );
    assert_eq!(reply["model_id"], MODEL_ID, "{reply}");
    Ok(())
}

/// The bit patterns of the coordinates of a point.
fn bits(x: f64, y: f64) -> [u64; 2] {
    [x.to_bits(), y.to_bits()]
}

/// The atom positions of document `id`, which mirror the first scene instance, read through the typed
/// document.
fn atom_positions(app: &App, id: &str) -> TestResult<Positions> {
    let document = app.document(id)?.document;
    Ok(document
        .atoms
        .into_iter()
        .map(|atom| (atom.id, bits(atom.position.x, atom.position.y)))
        .collect())
}

/// The positions of every scene instance of document `id`, by instance ID and then atom ID, read
/// through the typed document.
fn placements(app: &App, id: &str) -> TestResult<BTreeMap<String, Positions>> {
    let document = app.document(id)?.document;
    Ok(document
        .scene
        .instances
        .into_iter()
        .map(|instance| {
            let positions: Positions = instance
                .positions
                .into_iter()
                .map(|position| (position.atom, bits(position.x, position.y)))
                .collect();
            (instance.id, positions)
        })
        .collect())
}

/// The position of `atom` in an atom map from [`atom_positions`] or [`placements`], as `[x, y]`.
fn stored_position(positions: &Positions, atom: &str) -> TestResult<[f64; 2]> {
    let [x, y] = positions
        .get(atom)
        .ok_or_else(|| format!("{atom} is not in the document"))?;
    Ok([f64::from_bits(*x), f64::from_bits(*y)])
}

/// `positions` without the entry of `atom`.
fn without(positions: &Positions, atom: &str) -> Positions {
    positions
        .iter()
        .filter(|(id, _)| id.as_str() != atom)
        .map(|(id, position)| (id.clone(), *position))
        .collect()
}

/// The document fields that an apply of atoms in one instance must leave as they are.
fn untouched_fields(document: &Value) -> Value {
    json!({
        "bonds": document["document"]["bonds"],
        "title": document["document"]["title"],
        "settings": document["document"]["settings"],
        "stereo": document["document"]["stereo"],
        "objects": document["document"]["scene"]["objects"],
        "references": document["document"]["scene"]["references"],
    })
}

#[test]
fn applying_butane_commits_the_worked_example() -> TestResult {
    let directory = tempfile::tempdir()?;
    let app = new_app(&directory);
    let id = draw(&app, "butane", butane())?;
    let mut ids = RequestIds::default();
    let reply = call(
        &app,
        &ids.fresh(),
        "layout.structure",
        apply_parameters(&id, 1, &["a2"]),
    )?;

    assert_eq!(reply["outcome"], "ok", "{reply}");
    assert_eq!(reply["document_revision"], 2, "{reply}");
    assert_eq!(reply["loss_report"], json!([]), "{reply}");
    assert_document_reply(&reply)?;
    assert_eq!(reply["result"]["session"]["undo_depth"], 2, "{reply}");
    assert_eq!(codes(&reply), ["layout_not_implemented"], "{reply}");

    let stored = atom_positions(&app, &id)?;
    for (index, after) in butane_after().into_iter().enumerate() {
        let atom = format!("a{}", index + 1);
        assert_at(
            reply_position(&reply, &atom)?,
            after,
            &format!("{atom} in the reply"),
        );
        assert_at(
            stored_position(&stored, &atom)?,
            after,
            &format!("{atom} stored"),
        );
    }
    Ok(())
}

#[test]
fn the_anchored_example_moves_only_a4() -> TestResult {
    let directory = tempfile::tempdir()?;
    let app = new_app(&directory);
    let id = draw(&app, "butane", butane())?;
    let before = atom_positions(&app, &id)?;
    let mut ids = RequestIds::default();
    let reply = call(
        &app,
        &ids.fresh(),
        "layout.structure",
        with_scope(apply_parameters(&id, 1, &["a4"]), Some("selection")),
    )?;

    assert_eq!(reply["outcome"], "ok", "{reply}");
    assert_eq!(reply["document_revision"], 2, "{reply}");
    assert_eq!(
        codes(&reply),
        ["layout_no_bonds", "layout_not_implemented"],
        "{reply}"
    );
    let after = atom_positions(&app, &id)?;
    assert_eq!(
        without(&after, "a4"),
        without(&before, "a4"),
        "an atom other than a4 changed its position"
    );
    assert_at(stored_position(&after, "a4")?, [75.0, -dx()], "a4");
    Ok(())
}

/// Asserts that an apply of `atoms` at revision 1 commits every position that the preview at revision
/// 1 proposes, to within `APPLIED_TOLERANCE`.
fn assert_apply_matches_preview(atoms: &[&str], scope: Option<&str>) -> TestResult {
    let directory = tempfile::tempdir()?;
    let app = new_app(&directory);
    let id = draw(&app, "butane", butane())?;
    let mut ids = RequestIds::default();
    let preview = call(
        &app,
        &ids.fresh(),
        "layout.structure",
        with_scope(preview_parameters(&id, atoms), scope),
    )?;
    assert_eq!(preview["outcome"], "ok", "{preview}");
    assert_eq!(preview["document_revision"], 1, "{preview}");
    assert_eq!(
        preview["diagnostics"][0]["code"], "layout_preview_only",
        "{preview}"
    );
    assert_eq!(
        preview["diagnostics"][0]["message"], PREVIEW_ONLY_MESSAGE,
        "{preview}"
    );

    let reply = call(
        &app,
        &ids.fresh(),
        "layout.structure",
        with_scope(apply_parameters(&id, 1, atoms), scope),
    )?;
    assert_eq!(reply["outcome"], "ok", "{reply}");
    assert_eq!(reply["document_revision"], 2, "{reply}");

    let proposed = preview["result"]["positions"]
        .as_array()
        .ok_or_else(|| format!("the preview has no positions: {preview}"))?;
    assert!(
        !proposed.is_empty(),
        "the preview proposes no position: {preview}"
    );
    for entry in proposed {
        let atom = entry["atom"]
            .as_str()
            .ok_or_else(|| format!("a proposed position names no atom: {entry}"))?;
        assert_at(
            reply_position(&reply, atom)?,
            point(&entry["after"])?,
            &format!("{atom} against the preview"),
        );
    }
    Ok(())
}

#[test]
fn the_applied_positions_match_the_preview_at_the_same_revision_for_a_free_atom() -> TestResult {
    assert_apply_matches_preview(&["a2"], None)
}

#[test]
fn the_applied_positions_match_the_preview_at_the_same_revision_for_an_anchored_atom() -> TestResult
{
    assert_apply_matches_preview(&["a4"], Some("selection"))
}

/// Asserts that an apply of `a2` to instance `named` at revision 2 changes the positions of `named`
/// alone, and that `atoms[].position` keeps mirroring the first instance `s1`.
fn assert_only_instance_moves(named: &str, other: &str) -> TestResult {
    let directory = tempfile::tempdir()?;
    let app = new_app(&directory);
    let id = draw(&app, "butane", butane())?;
    transact(
        &app,
        "second-instance",
        &id,
        1,
        json!([{"op": "add_instance", "offset": {"x": 200, "y": 0}}]),
    )?;
    let placements_before = placements(&app, &id)?;
    let atoms_before = atom_positions(&app, &id)?;
    assert_eq!(placements_before.len(), 2, "the document has two instances");
    assert_eq!(
        placements_before.get("s1"),
        Some(&atoms_before),
        "atoms[].position mirrors s1 before the apply"
    );
    let document_before = untouched_fields(&document_json(&app, &id)?);

    let mut ids = RequestIds::default();
    let mut parameters = apply_parameters(&id, 2, &["a2"]);
    parameters["instance"] = json!(named);
    let reply = call(&app, &ids.fresh(), "layout.structure", parameters)?;
    assert_eq!(reply["outcome"], "ok", "{reply}");
    assert_eq!(reply["document_revision"], 3, "{reply}");

    let placements_after = placements(&app, &id)?;
    assert_eq!(
        placements_after.get(other),
        placements_before.get(other),
        "the apply to {named} changed the positions of {other}"
    );
    assert_ne!(
        placements_after.get(named),
        placements_before.get(named),
        "the apply to {named} changed no position of {named}"
    );
    // `atoms[].position` mirrors `s1`. When `s1` is the other instance, that makes it bit-equal to
    // before; when `s1` is named, it takes the new `s1` positions.
    let atoms_after = atom_positions(&app, &id)?;
    assert_eq!(
        placements_after.get("s1"),
        Some(&atoms_after),
        "atoms[].position does not mirror s1 after the apply to {named}"
    );
    assert_eq!(
        untouched_fields(&document_json(&app, &id)?),
        document_before,
        "the apply to {named} changed a field other than the positions"
    );
    Ok(())
}

#[test]
fn only_the_named_instance_moves_for_s2() -> TestResult {
    assert_only_instance_moves("s2", "s1")
}

#[test]
fn only_the_named_instance_moves_for_s1() -> TestResult {
    assert_only_instance_moves("s1", "s2")
}

#[test]
fn an_overlapping_apply_commits_with_the_overlap_warning() -> TestResult {
    let directory = tempfile::tempdir()?;
    let app = new_app(&directory);
    let mut commands = butane();
    commands
        .as_array_mut()
        .ok_or("butane is a list of commands")?
        .push(json!({"op": "add_atom", "element": "C", "position": {"x": 6, "y": 8}}));
    let id = draw(&app, "butane-and-carbon", commands)?;
    let mut ids = RequestIds::default();
    let preview = call(
        &app,
        &ids.fresh(),
        "layout.structure",
        preview_parameters(&id, &["a2"]),
    )?;
    let before = atom_positions(&app, &id)?;
    let reply = call(
        &app,
        &ids.fresh(),
        "layout.structure",
        apply_parameters(&id, 1, &["a2"]),
    )?;

    assert_eq!(reply["outcome"], "ok", "{reply}");
    assert_eq!(reply["document_revision"], 2, "{reply}");
    assert_eq!(reply["loss_report"], json!([]), "{reply}");
    assert_eq!(
        codes(&reply),
        ["layout_overlap", "layout_not_implemented"],
        "{reply}"
    );
    let warning = diagnostics(&reply)
        .first()
        .ok_or_else(|| format!("the apply has no diagnostic: {reply}"))?;
    assert_eq!(warning["severity"], "warning", "{reply}");
    let proposed = find_diagnostic(&preview, "layout_overlap")?;
    assert_eq!(
        proposed["details"]["pairs"],
        json!([["a1", "a5"]]),
        "{preview}"
    );
    assert_eq!(warning["details"], proposed["details"], "{reply}");
    assert_eq!(
        atom_positions(&app, &id)?.get("a5"),
        before.get("a5"),
        "the fixed atom a5 moved, although only the layout set moves"
    );
    Ok(())
}

#[test]
fn a_500_atom_chain_applies_as_one_edit_and_501_is_a_resource_limit() -> TestResult {
    let directory = tempfile::tempdir()?;
    let app = new_app(&directory);
    let mut ids = RequestIds::default();

    let id = draw(&app, "chain", chain(LAYOUT_LIMIT))?;
    let mut parameters = apply_parameters(&id, 1, &["a1"]);
    parameters["time_limit_ms"] = json!(MAXIMUM_LIMIT_MS);
    let reply = call(&app, &ids.fresh(), "layout.structure", parameters)?;
    assert_eq!(reply["outcome"], "ok", "{reply}");
    assert_eq!(reply["document_revision"], 2, "{reply}");
    assert_eq!(reply["result"]["session"]["undo_depth"], 2, "{reply}");

    let longer = draw(&app, "chain-plus-one", chain(LAYOUT_LIMIT))?;
    transact(
        &app,
        "chain-plus-one-tail",
        &longer,
        1,
        json!([{"op": "add_atom", "element": "C", "position": {"x": 15000, "y": 0}, "bond_to": {"atom": "a500", "order": "single"}}]),
    )?;
    let before = document_json(&app, &longer)?;
    let reply = call(
        &app,
        &ids.fresh(),
        "layout.structure",
        apply_parameters(&longer, 2, &["a1"]),
    )?;
    assert_eq!(reply["outcome"], "error", "{reply}");
    assert_eq!(codes(&reply), ["resource_limit"], "{reply}");
    let refusal = diagnostics(&reply)
        .first()
        .ok_or_else(|| format!("the refusal has no diagnostic: {reply}"))?;
    assert_eq!(refusal["path"], "parameters.selection", "{reply}");
    assert_eq!(
        refusal["details"],
        json!({"atoms": LAYOUT_LIMIT + 1, "limit": LAYOUT_LIMIT}),
        "{reply}"
    );
    assert_eq!(reply["document_revision"], 2, "{reply}");
    assert_eq!(
        document_json(&app, &longer)?,
        before,
        "the refused apply changed the document"
    );
    Ok(())
}
