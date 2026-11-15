//! The ring layout through the application: a plain hexagon or pentagon previews onto its own
//! positions and applies as `layout_unchanged`, a ring with a substituent previews with its chain
//! and its ring, every refused ring names its reason and atoms, and a stop inside a ring is counted
//! like a stop inside a chain. The expected positions come from the ring rules: a regular polygon
//! of 30 pt sides, and a free component recentred on the centroid of its before positions.

use std::collections::BTreeMap;
use std::error::Error;
use std::ops::RangeInclusive;
use std::sync::atomic::{AtomicUsize, Ordering};

use chemifree_application::{Application, Interrupt, Interrupted};
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};

type App = Application<LocalCapabilities, FileDocumentRepository>;
/// The result of a test or helper, so that a failed read is an error rather than a panic.
type TestResult<T = ()> = Result<T, Box<dyn Error>>;

/// The model that the preview and apply replies name.
const MODEL_ID: &str = "chemifree-chain-layout-2";
/// The limit of a request that sets none, in milliseconds.
const DEFAULT_LIMIT_MS: u64 = 5_000;
/// The tolerance, in pt, of a placement that must not move.
const SAME_TOLERANCE: f64 = 1e-9;
/// The tolerance, in pt, of a placement against its exact value.
const TOLERANCE: f64 = 1e-6;
/// The message of the `layout_not_implemented` diagnostic, which names what stays unsupported.
const NOT_IMPLEMENTED_MESSAGE: &str = "Fused, bridged and spiro ring layout, rings of more than 12 \
    atoms, templates, scoring and search, and collision resolution are not implemented.";

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

fn call_with(
    app: &App,
    id: &str,
    operation: &str,
    parameters: Value,
    cancel: &dyn Interrupt,
) -> TestResult<Value> {
    Ok(serde_json::to_value(app.execute_with(
        &request(id, operation, parameters),
        cancel,
    ))?)
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
fn transact(app: &App, id: &str, expected_revision: u64, commands: Value) -> TestResult {
    let reply = call(
        app,
        &format!("transact-{expected_revision}"),
        "document.transaction",
        json!({"document_id": id, "expected_revision": expected_revision, "commands": commands}),
    )?;
    assert_eq!(reply["outcome"], "ok", "{reply}");
    Ok(())
}

/// Creates a document titled `title`, applies `commands` to it at revision 0, and returns its id.
fn draw(app: &App, title: &str, commands: Value) -> TestResult<String> {
    let id = app.create_document(None, Some(title))?.document.id;
    transact(app, &id, 0, commands)?;
    Ok(id)
}

/// The commands that draw a regular ring of `size` carbons about the origin in instance `s1`.
fn ring_commands(size: usize) -> Value {
    json!([{"op": "insert_ring", "instance": "s1", "center": {"x": 0, "y": 0}, "size": size}])
}

/// Carbons a1 to a{count} in a row 30 pt apart, then one single bond for each pair of 1-based
/// serials in `bonds`, so that a bonded set of carbons is drawn in one transaction.
fn carbons(count: usize, bonds: &[(usize, usize)]) -> Value {
    let mut commands: Vec<Value> = (0..count)
        .map(|index| json!({"op": "add_atom", "element": "C", "position": {"x": 30 * index, "y": 0}}))
        .collect();
    commands.extend(bonds.iter().map(|&(begin, end)| {
        json!({"op": "add_bond", "begin": format!("${}", begin - 1), "end": format!("${}", end - 1), "order": "single"})
    }));
    Value::Array(commands)
}

/// The bonds of the cycle through the serials 1 to `count`, which closes the last to the first.
fn ring_bonds(count: usize) -> Vec<(usize, usize)> {
    (1..=count)
        .map(|serial| (serial, serial % count + 1))
        .collect()
}

/// The atom IDs a1 to a{serial} for the serials in `serials`, in ascending order.
fn atom_ids(serials: RangeInclusive<usize>) -> Vec<String> {
    serials.map(|serial| format!("a{serial}")).collect()
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

/// The diagnostic of `reply` with `code`.
fn diagnostic<'a>(reply: &'a Value, code: &str) -> TestResult<&'a Value> {
    reply["diagnostics"]
        .as_array()
        .and_then(|list| list.iter().find(|diagnostic| diagnostic["code"] == code))
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

/// The `before` or `after` point, named by `side`, of atom `atom` in the placements of a
/// successful preview `reply`.
fn placed(reply: &Value, atom: &str, side: &str) -> TestResult<[f64; 2]> {
    let placements = reply["result"]["positions"]
        .as_array()
        .ok_or_else(|| format!("the reply has no positions: {reply}"))?;
    let entry = placements
        .iter()
        .find(|entry| entry["atom"] == atom)
        .ok_or_else(|| format!("{atom} has no placement: {reply}"))?;
    point(&entry[side])
}

/// Asserts that `actual` lies within `tolerance` of `expected` in both coordinates.
fn assert_within(actual: [f64; 2], expected: [f64; 2], tolerance: f64, label: &str) {
    let [x, y] = actual;
    let [expected_x, expected_y] = expected;
    assert!(
        (x - expected_x).abs() <= tolerance,
        "{label} x: expected {expected_x}, got {x}"
    );
    assert!(
        (y - expected_y).abs() <= tolerance,
        "{label} y: expected {expected_y}, got {y}"
    );
}

/// Asserts that every placement of a successful preview `reply` ends where it started.
fn assert_unmoved(reply: &Value) -> TestResult {
    let placements = reply["result"]["positions"]
        .as_array()
        .ok_or_else(|| format!("the reply has no positions: {reply}"))?;
    assert!(!placements.is_empty(), "{reply}");
    for entry in placements {
        let atom = entry["atom"]
            .as_str()
            .ok_or_else(|| format!("a placement has no atom: {reply}"))?;
        let before = point(&entry["before"])?;
        let after = point(&entry["after"])?;
        assert_within(
            after,
            before,
            SAME_TOLERANCE,
            &format!("{atom} against its before position"),
        );
    }
    Ok(())
}

/// Asserts that `reply` is a stop of the preview with `code`: an error with no result, the analysed
/// `revision`, the preview's model and exactly one diagnostic of `code`.
fn assert_stopped(reply: &Value, code: &str, revision: u64) {
    assert_eq!(reply["outcome"], "error", "{reply}");
    assert!(reply["result"].is_null(), "{reply}");
    assert_eq!(reply["document_revision"], revision, "{reply}");
    assert_eq!(reply["model_id"], MODEL_ID, "{reply}");
    assert_eq!(
        reply["diagnostics"].as_array().map(Vec::len),
        Some(1),
        "{reply}"
    );
    assert_eq!(reply["diagnostics"][0]["code"], code, "{reply}");
}

/// The atom positions of document `id` as bit patterns, so that a change in the last bit shows.
fn atom_positions(app: &App, id: &str) -> TestResult<BTreeMap<String, [u64; 2]>> {
    let document = app.document(id)?.document;
    Ok(document
        .atoms
        .into_iter()
        .map(|atom| {
            (
                atom.id,
                [atom.position.x.to_bits(), atom.position.y.to_bits()],
            )
        })
        .collect())
}

#[test]
fn a_hexagon_preview_names_its_ring_and_model() -> TestResult {
    let directory = tempfile::tempdir()?;
    let app = new_app(&directory);
    let id = draw(&app, "hexagon", ring_commands(6))?;
    let mut ids = RequestIds::default();
    let reply = call(
        &app,
        &ids.fresh(),
        "layout.structure",
        preview_parameters(&id, &["a1"]),
    )?;

    assert_eq!(reply["outcome"], "ok", "{reply}");
    assert_eq!(reply["model_id"], MODEL_ID, "{reply}");
    assert_eq!(reply["document_revision"], 1, "{reply}");
    assert_eq!(
        reply["result"]["components"],
        json!([{
            "atoms": atom_ids(1..=6),
            "bonds": ["b1", "b2", "b3", "b4", "b5", "b6"],
            "anchor": null,
            "root": "a1",
            "chains": [],
            "rings": [{"atoms": atom_ids(1..=6), "branch_from": null}],
        }]),
        "{reply}"
    );
    assert_eq!(
        reply["result"]["positions"].as_array().map(Vec::len),
        Some(6),
        "{reply}"
    );
    assert_unmoved(&reply)?;
    assert_eq!(
        diagnostic(&reply, "layout_not_implemented")?["message"],
        NOT_IMPLEMENTED_MESSAGE,
        "{reply}"
    );
    Ok(())
}

#[test]
fn applying_a_plain_hexagon_or_pentagon_changes_nothing() -> TestResult {
    for (kind, size) in [("hexagon", 6), ("pentagon", 5)] {
        let directory = tempfile::tempdir()?;
        let app = new_app(&directory);
        let id = draw(&app, kind, ring_commands(size))?;
        let before = atom_positions(&app, &id)?;
        let mut ids = RequestIds::default();
        let reply = call(
            &app,
            &ids.fresh(),
            "layout.structure",
            apply_parameters(&id, 1, &["a1"]),
        )?;

        assert_eq!(reply["outcome"], "ok", "{kind}: {reply}");
        assert_eq!(reply["document_revision"], 1, "{kind}: {reply}");
        assert_eq!(
            reply["result"]["session"]["undo_depth"], 1,
            "{kind}: {reply}"
        );
        diagnostic(&reply, "layout_unchanged")?;
        assert_eq!(
            atom_positions(&app, &id)?,
            before,
            "{kind}: an apply that moves nothing changed a position"
        );
    }
    Ok(())
}

#[test]
fn a_methylcyclohexane_preview_lists_its_chain_and_ring() -> TestResult {
    let directory = tempfile::tempdir()?;
    let app = new_app(&directory);
    let id = draw(&app, "methylcyclohexane", ring_commands(6))?;
    transact(
        &app,
        &id,
        1,
        json!([{"op": "add_atom", "element": "C", "position": {"x": 60, "y": 0}, "bond_to": {"atom": "a1", "order": "single"}}]),
    )?;
    let mut ids = RequestIds::default();
    let reply = call(
        &app,
        &ids.fresh(),
        "layout.structure",
        preview_parameters(&id, &["a1"]),
    )?;

    assert_eq!(reply["outcome"], "ok", "{reply}");
    assert_eq!(reply["model_id"], MODEL_ID, "{reply}");
    assert_eq!(reply["document_revision"], 2, "{reply}");
    let component = &reply["result"]["components"][0];
    assert_eq!(component["root"], "a7", "{reply}");
    assert_eq!(
        component["chains"],
        json!([{"atoms": ["a7"], "branch_from": null}]),
        "{reply}"
    );
    assert_eq!(
        component["rings"],
        json!([{"atoms": atom_ids(1..=6), "branch_from": "a7"}]),
        "{reply}"
    );

    // In the free layout a7 sits at the origin, a1 at (15 sqrt 3, -15), and the ring is about
    // (30 sqrt 3, -30). The free centroid is (180 sqrt 3 / 7, -180 / 7) and the before centroid is
    // (60 / 7, 0), so the recentring shifts every placement by their difference.
    let root3 = 3f64.sqrt();
    let shift = [(60.0 - 180.0 * root3) / 7.0, 180.0 / 7.0];
    let free = [
        ("a7", [0.0, 0.0]),
        ("a1", [15.0 * root3, -15.0]),
        ("a2", [30.0 * root3, 0.0]),
        ("a3", [45.0 * root3, -15.0]),
        ("a4", [45.0 * root3, -45.0]),
        ("a5", [30.0 * root3, -60.0]),
        ("a6", [15.0 * root3, -45.0]),
    ];
    for (atom, [x, y]) in free {
        assert_within(
            placed(&reply, atom, "after")?,
            [x + shift[0], y + shift[1]],
            TOLERANCE,
            &format!("{atom} after"),
        );
    }
    Ok(())
}

/// A document whose layout is refused: the commands that draw it, the selection and scope of its
/// preview, and the refusal that it must name with its atoms and, for a large ring, its limit.
struct Refusal {
    title: &'static str,
    commands: Value,
    selection: Vec<String>,
    scope: &'static str,
    reason: &'static str,
    atoms: Vec<String>,
    limit: Option<usize>,
}

/// One document for each ring refusal: a hexagon with one atom selected, a fused pair, a spiro pair
/// and a ring of 13 atoms.
fn refusals() -> Vec<Refusal> {
    vec![
        Refusal {
            title: "partly selected hexagon",
            commands: ring_commands(6),
            selection: atom_ids(1..=1),
            scope: "selection",
            reason: "ring_partly_in_layout_set",
            atoms: atom_ids(1..=1),
            limit: None,
        },
        Refusal {
            title: "fused rings",
            // Two hexagons that share the bond a1 to a2, which is a naphthalene skeleton.
            commands: carbons(
                10,
                &[
                    (1, 2),
                    (2, 3),
                    (3, 4),
                    (4, 5),
                    (5, 6),
                    (6, 1),
                    (1, 7),
                    (7, 8),
                    (8, 9),
                    (9, 10),
                    (10, 2),
                ],
            ),
            selection: atom_ids(1..=10),
            scope: "selection",
            reason: "fused_ring_in_layout_set",
            atoms: atom_ids(1..=10),
            limit: None,
        },
        Refusal {
            title: "spiro rings",
            // A pentagon and a hexagon that share only the atom a1, which is a spiro[4.5] skeleton.
            commands: carbons(
                10,
                &[
                    (1, 2),
                    (2, 3),
                    (3, 4),
                    (4, 5),
                    (5, 1),
                    (1, 6),
                    (6, 7),
                    (7, 8),
                    (8, 9),
                    (9, 10),
                    (10, 1),
                ],
            ),
            selection: atom_ids(1..=10),
            scope: "selection",
            reason: "spiro_ring_in_layout_set",
            atoms: atom_ids(1..=10),
            limit: None,
        },
        Refusal {
            title: "large ring",
            commands: carbons(13, &ring_bonds(13)),
            selection: atom_ids(1..=13),
            scope: "selection",
            reason: "large_ring_in_layout_set",
            atoms: atom_ids(1..=13),
            limit: Some(12),
        },
    ]
}

#[test]
fn ring_refusals_name_their_reason_and_atoms() -> TestResult {
    for case in refusals() {
        let directory = tempfile::tempdir()?;
        let app = new_app(&directory);
        let id = draw(&app, case.title, case.commands)?;
        let selection: Vec<&str> = case.selection.iter().map(String::as_str).collect();
        let mut parameters = preview_parameters(&id, &selection);
        parameters["scope"] = json!(case.scope);
        let mut ids = RequestIds::default();
        let reply = call(&app, &ids.fresh(), "layout.structure", parameters)?;

        let label = case.title;
        assert_eq!(reply["outcome"], "unsupported", "{label}: {reply}");
        assert_eq!(reply["model_id"], MODEL_ID, "{label}: {reply}");
        assert_eq!(reply["document_revision"], 1, "{label}: {reply}");
        assert_eq!(
            reply["diagnostics"].as_array().map(Vec::len),
            Some(1),
            "{label}: {reply}"
        );
        let mut details = json!({"reason": case.reason, "atoms": case.atoms});
        if let Some(limit) = case.limit {
            details["limit"] = json!(limit);
        }
        let refusal = diagnostic(&reply, "layout_unsupported")?;
        assert_eq!(refusal["details"], details, "{label}: {reply}");
    }
    Ok(())
}

#[test]
fn a_counted_stop_inside_a_ring_stops_the_preview() -> TestResult {
    let directory = tempfile::tempdir()?;
    let app = new_app(&directory);
    let id = draw(&app, "hexagon", ring_commands(6))?;
    let mut ids = RequestIds::default();

    // Question 1 is the check before the placement, question 2 the component, and questions 3 to 8
    // the six members in walk order, so question 5 is the third member.
    let stopper = CancelAt::new(5, Interrupted::TimeLimit);
    let stopped = call_with(
        &app,
        &ids.fresh(),
        "layout.structure",
        preview_parameters(&id, &["a1"]),
        &stopper,
    )?;
    assert_stopped(&stopped, "time_limit", 1);
    assert_eq!(stopper.asked(), 5, "{stopped}");
    assert_eq!(
        stopped["diagnostics"][0]["details"],
        json!({"limit_ms": DEFAULT_LIMIT_MS}),
        "{stopped}"
    );

    // With no stop, the preview asks one question before the placement, one for the component, one
    // for each of the six members, and one for each of the six atoms of the overlap scan: 14 in all.
    let counter = CancelAt::new(1_000, Interrupted::TimeLimit);
    let reply = call_with(
        &app,
        &ids.fresh(),
        "layout.structure",
        preview_parameters(&id, &["a1"]),
        &counter,
    )?;
    assert_eq!(reply["outcome"], "ok", "{reply}");
    assert_eq!(counter.asked(), 14, "{reply}");
    Ok(())
}
