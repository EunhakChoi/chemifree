//! The P42 layout of coordinate-free SMILES imports through `document.import`: the geometry of a
//! laid-out component, the shelf packing and wrap of components, the bond length of the document,
//! identifier independence, the engine identity of a later layout preview, and that a preview holds
//! the positions its commit writes.

use std::collections::BTreeMap;
use std::error::Error;

use chemifree_application::Application;
use chemifree_providers::interchange_codec::LocalInterchangeCodec;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities, OpenSmilesCodec, V2000Codec};
use serde_json::{Value, json};
use tempfile::TempDir;

type App = Application<LocalCapabilities, FileDocumentRepository>;
/// The result of a test or helper, so that a failed read is an error rather than a panic.
type TestResult<T = ()> = Result<T, Box<dyn Error>>;

/// The tolerance, in pt, of a computed position or bond length against its derived value.
const TOLERANCE: f64 = 1e-9;
/// The origin of the layout box in an empty drawing: the first grid cell of the imports.
const EMPTY_ORIGIN: (f64, f64) = (40.0, 40.0);

/// The horizontal step of a 30 degree zigzag bond of `bond_length_pt`: `b * sqrt(3) / 2`, which is
/// `15 * sqrt(3)` for the default 30 pt bond.
fn step(bond_length_pt: f64) -> f64 {
    bond_length_pt * 3f64.sqrt() / 2.0
}

/// Whether two coordinates or lengths agree within `TOLERANCE`.
fn close(actual: f64, expected: f64) -> bool {
    (actual - expected).abs() <= TOLERANCE
}

fn app(path: &std::path::Path) -> App {
    Application::new(LocalCapabilities, FileDocumentRepository::new(path))
        .with_ctfile_codec(V2000Codec)
        .with_interchange_codec(LocalInterchangeCodec)
        .with_smiles_codec(OpenSmilesCodec)
}

fn call(app: &App, id: &str, operation: &str, parameters: Value) -> TestResult<Value> {
    let request = json!({"contract":1,"request_id":id,"profile":"preserve-semantics","operation":operation,"parameters":parameters});
    Ok(serde_json::to_value(
        app.execute(request.to_string().as_bytes()),
    )?)
}

/// Creates an empty document and returns its id; `request` names the call.
fn new_document(app: &App, request: &str) -> TestResult<String> {
    let created = call(app, request, "document.create", json!({}))?;
    let id = created["result"]["document"]["id"]
        .as_str()
        .ok_or_else(|| format!("no document id in {created}"))?;
    Ok(id.to_owned())
}

/// A fresh application over its own directory, with one empty document.
fn setup() -> TestResult<(App, TempDir, String)> {
    let directory = tempfile::tempdir()?;
    let application = app(directory.path());
    let id = new_document(&application, "create")?;
    Ok((application, directory, id))
}

/// Applies `commands` to document `id` at `revision`, which must succeed.
fn transact(
    app: &App,
    request: &str,
    id: &str,
    revision: u64,
    commands: Value,
) -> TestResult<Value> {
    let reply = call(
        app,
        request,
        "document.transaction",
        json!({"document_id": id, "expected_revision": revision, "commands": commands}),
    )?;
    assert_eq!(reply["outcome"], "ok", "{request}: {reply}");
    Ok(reply)
}

/// The preview of `source` as an import into document `id` at `revision`.
fn preview(app: &App, id: &str, revision: u64, source: &str) -> TestResult<Value> {
    call(
        app,
        &format!("preview-{id}-{revision}"),
        "document.import",
        json!({"document_id": id, "expected_revision": revision, "format": "smiles", "source": source, "preview": true}),
    )
}

/// The commit of `source` into document `id` at `revision` with `digest`, accepting no loss.
fn commit(app: &App, id: &str, revision: u64, source: &str, digest: &str) -> TestResult<Value> {
    call(
        app,
        &format!("commit-{id}-{revision}"),
        "document.import",
        json!({"document_id": id, "expected_revision": revision, "format": "smiles", "source": source, "preview_digest": digest, "accept_loss": false}),
    )
}

/// Previews `source` into document `id` at `revision` and commits that preview, returning the commit.
fn import(app: &App, id: &str, revision: u64, source: &str) -> TestResult<Value> {
    let previewed = preview(app, id, revision, source)?;
    let digest = previewed["result"]["preview_digest"]
        .as_str()
        .ok_or_else(|| format!("no preview digest in {previewed}"))?;
    commit(app, id, revision, source, digest)
}

/// The array under `key` of `value`.
fn items<'a>(value: &'a Value, key: &str) -> TestResult<&'a [Value]> {
    let array = value[key]
        .as_array()
        .ok_or_else(|| format!("{key} is not an array"))?;
    Ok(array.as_slice())
}

/// The feature of each loss of a reply, in report order.
fn features(reply: &Value) -> TestResult<Vec<&str>> {
    let mut names = Vec::new();
    for loss in items(reply, "loss_report")? {
        names.push(loss["feature"].as_str().ok_or("a loss has no feature")?);
    }
    Ok(names)
}

/// The `(x, y)` of a position object, such as the `position` of a document atom.
fn point(position: &Value) -> TestResult<(f64, f64)> {
    let x = position["x"]
        .as_f64()
        .ok_or("a position has no numeric x")?;
    let y = position["y"]
        .as_f64()
        .ok_or("a position has no numeric y")?;
    Ok((x, y))
}

/// The positions of the atoms of a document, in the document's order.
fn ordered_positions(document: &Value) -> TestResult<Vec<(f64, f64)>> {
    let mut positions = Vec::new();
    for atom in items(document, "atoms")? {
        positions.push(point(&atom["position"])?);
    }
    Ok(positions)
}

/// The position of every atom of a document, by atom id.
fn positions_by_id(document: &Value) -> TestResult<BTreeMap<String, (f64, f64)>> {
    let mut positions = BTreeMap::new();
    for atom in items(document, "atoms")? {
        let id = atom["id"].as_str().ok_or("an atom has no id")?;
        positions.insert(id.to_owned(), point(&atom["position"])?);
    }
    Ok(positions)
}

/// The atom ids that an import reply assigned to the input atoms, in input order.
fn import_ids(reply: &Value) -> TestResult<Vec<String>> {
    let mut ids = Vec::new();
    for id in items(&reply["result"]["import_mapping"], "atoms")? {
        let id = id.as_str().ok_or("an import mapping entry is not an id")?;
        ids.push(id.to_owned());
    }
    Ok(ids)
}

/// The positions that an import reply gave its input atoms, in input order.
fn imported_positions(reply: &Value) -> TestResult<Vec<(f64, f64)>> {
    let by_id = positions_by_id(&reply["result"]["document"])?;
    let mut positions = Vec::new();
    for id in import_ids(reply)? {
        let position = by_id
            .get(&id)
            .ok_or_else(|| format!("atom {id} is not in the document"))?;
        positions.push(*position);
    }
    Ok(positions)
}

/// Asserts that `actual` has one position per expected one, each within `TOLERANCE`. A failure
/// names the input atom, numbered from 1, and both positions.
fn assert_positions(actual: &[(f64, f64)], expected: &[(f64, f64)], context: &str) {
    assert_eq!(actual.len(), expected.len(), "{context}: {actual:?}");
    for (index, (got, want)) in actual.iter().zip(expected).enumerate() {
        assert!(
            close(got.0, want.0) && close(got.1, want.1),
            "{context}: atom {} is at {got:?}, expected {want:?}",
            index + 1
        );
    }
}

/// Asserts that `document` has `count` bonds and that each joins two atoms `length` pt apart,
/// within `TOLERANCE`.
fn assert_bond_lengths(document: &Value, count: usize, length: f64) -> TestResult {
    let positions = positions_by_id(document)?;
    let bonds = items(document, "bonds")?;
    assert_eq!(bonds.len(), count, "{document}");
    for bond in bonds {
        let begin = atom_position(&positions, &bond["begin"])?;
        let end = atom_position(&positions, &bond["end"])?;
        let measured = (begin.0 - end.0).hypot(begin.1 - end.1);
        assert!(
            close(measured, length),
            "bond {bond} is {measured} pt long, expected {length} pt"
        );
    }
    Ok(())
}

/// The position of the atom with the id `id`, which a bond end names.
fn atom_position(positions: &BTreeMap<String, (f64, f64)>, id: &Value) -> TestResult<(f64, f64)> {
    let id = id.as_str().ok_or("a bond end is not an atom id")?;
    let position = positions
        .get(id)
        .ok_or_else(|| format!("no atom {id} in the document"))?;
    Ok(*position)
}

/// Imports `source` into an empty drawing. It commits with the generated-layout loss alone, and its
/// atoms sit at `expected`, in input order. Returns the commit, so that a test can check more.
fn assert_free_layout(source: &str, expected: &[(f64, f64)]) -> TestResult<Value> {
    let (app, _directory, id) = setup()?;
    let done = import(&app, &id, 0, source)?;
    assert_eq!(done["outcome"], "partial", "{source}: {done}");
    assert_eq!(features(&done)?, ["generated_layout"], "{source}: {done}");
    assert_eq!(done["document_revision"], 1, "{source}: {done}");
    assert_positions(&imported_positions(&done)?, expected, source);
    Ok(done)
}

#[test]
fn cco_is_a_zigzag_at_the_top_left_of_an_empty_drawing() -> TestResult {
    let h = step(30.0);
    let done = assert_free_layout(
        "CCO",
        &[(40.0, 55.0), (40.0 + h, 40.0), (40.0 + 2.0 * h, 55.0)],
    )?;
    assert_bond_lengths(&done["result"]["document"], 2, 30.0)?;
    Ok(())
}

#[test]
fn benzene_is_the_free_hexagon_shifted_to_the_box_origin() -> TestResult {
    let h = step(30.0);
    // A hexagon of radius 30 pt centred at (70, 40 + h), entered at its rightmost vertex.
    assert_free_layout(
        "c1ccccc1",
        &[
            (100.0, 40.0 + h),
            (85.0, 40.0 + 2.0 * h),
            (55.0, 40.0 + 2.0 * h),
            (40.0, 40.0 + h),
            (55.0, 40.0),
            (85.0, 40.0),
        ],
    )?;
    Ok(())
}

#[test]
fn methylcyclohexane_follows_the_p42_worked_example() -> TestResult {
    let h = step(30.0);
    assert_free_layout(
        "CC1CCCCC1",
        &[
            (40.0, 100.0),
            (40.0 + h, 85.0),
            (40.0 + 2.0 * h, 100.0),
            (40.0 + 3.0 * h, 85.0),
            (40.0 + 3.0 * h, 55.0),
            (40.0 + 2.0 * h, 40.0),
            (40.0 + h, 55.0),
        ],
    )?;
    Ok(())
}

#[test]
fn components_pack_left_to_right_with_a_gap_of_two_bond_lengths() -> TestResult {
    let (app, _directory, id) = setup()?;
    let done = import(&app, &id, 0, "CCO.c1ccccc1")?;
    assert_eq!(done["outcome"], "partial", "{done}");
    assert_eq!(features(&done)?, ["generated_layout"], "{done}");
    let h = step(30.0);
    // Ethanol's box is 2h wide; the next box starts a gap of 2b = 60 pt after it.
    let cursor = 40.0 + 2.0 * h + 60.0;
    let mut expected = vec![(40.0, 55.0), (40.0 + h, 40.0), (40.0 + 2.0 * h, 55.0)];
    // Benzene's box has its minimum x at 40 in its own layout; its box moves to the cursor.
    let benzene = [
        (100.0, 40.0 + h),
        (85.0, 40.0 + 2.0 * h),
        (55.0, 40.0 + 2.0 * h),
        (40.0, 40.0 + h),
        (55.0, 40.0),
        (85.0, 40.0),
    ];
    for (x, y) in benzene {
        expected.push((x - 40.0 + cursor, y));
    }
    assert_positions(&imported_positions(&done)?, &expected, "CCO.c1ccccc1");
    Ok(())
}

#[test]
fn a_row_wraps_after_twenty_bond_lengths() -> TestResult {
    let (app, _directory, id) = setup()?;
    let source = ["[Na+]"; 12].join(".");
    let done = import(&app, &id, 0, &source)?;
    assert_eq!(done["outcome"], "partial", "{done}");
    assert_eq!(features(&done)?, ["generated_layout"], "{done}");
    // Single atoms are 0 pt wide, so each box is followed by the gap of 2b = 60 pt. A row holds
    // boxes that end within 20b = 600 pt of the origin, so the eleventh box (at 40 + 600) still
    // fits and the twelfth starts a new row one gap below the first row.
    let mut expected = Vec::new();
    for k in 0u32..11 {
        expected.push((40.0 + 60.0 * f64::from(k), 40.0));
    }
    expected.push((40.0, 40.0 + 60.0));
    assert_positions(&imported_positions(&done)?, &expected, &source);
    Ok(())
}

#[test]
fn the_bond_length_of_the_document_scales_bonds_and_gaps() -> TestResult {
    let (app, _directory, id) = setup()?;
    transact(
        &app,
        "bond-length",
        &id,
        0,
        json!([{"op": "update_settings", "bond_length_pt": 50}]),
    )?;
    let done = import(&app, &id, 1, "CCO")?;
    assert_eq!(done["outcome"], "partial", "{done}");
    assert_eq!(
        done["result"]["document"]["settings"]["bond_length_pt"],
        50.0
    );
    let bond = 50.0;
    let h = step(bond);
    assert_positions(
        &imported_positions(&done)?,
        &[
            (40.0, 40.0 + bond / 2.0),
            (40.0 + h, 40.0),
            (40.0 + 2.0 * h, 40.0 + bond / 2.0),
        ],
        "CCO at 50 pt",
    );
    assert_bond_lengths(&done["result"]["document"], 2, bond)?;
    Ok(())
}

#[test]
fn positions_do_not_depend_on_the_identifiers_the_target_assigns() -> TestResult {
    let (app, _directory, empty) = setup()?;
    let plain = import(&app, &empty, 0, "CC(C)CO")?;
    // A far atom at (500, 500) is the drawing's only atom, so the origin is (500, 500 + 60).
    let far = new_document(&app, "far-create")?;
    transact(
        &app,
        "far-atom",
        &far,
        0,
        json!([{"op": "add_atom", "element": "N", "position": {"x": 500, "y": 500}}]),
    )?;
    let offset = import(&app, &far, 1, "CC(C)CO")?;
    // Precondition: the far atom takes a1, so the import's identifiers start at a2 and not a1.
    assert_eq!(import_ids(&plain)?.first().map(String::as_str), Some("a1"));
    assert_eq!(import_ids(&offset)?.first().map(String::as_str), Some("a2"));
    let far_origin = (500.0, 500.0 + 60.0);
    let mut from_plain = Vec::new();
    for (x, y) in imported_positions(&plain)? {
        from_plain.push((x - EMPTY_ORIGIN.0, y - EMPTY_ORIGIN.1));
    }
    let mut from_far = Vec::new();
    for (x, y) in imported_positions(&offset)? {
        from_far.push((x - far_origin.0, y - far_origin.1));
    }
    assert_positions(&from_far, &from_plain, "CC(C)CO beside a far atom");
    Ok(())
}

#[test]
fn a_layout_preview_of_an_imported_component_proposes_no_move() -> TestResult {
    let (app, _directory, id) = setup()?;
    let done = import(&app, &id, 0, "CC1CCCCC1")?;
    assert_eq!(done["document_revision"], 1, "{done}");
    let reply = call(
        &app,
        "layout",
        "layout.structure",
        json!({"document_id": id, "mode": "preview", "selection": {"atoms": ["a1"]}}),
    )?;
    assert_eq!(reply["outcome"], "ok", "{reply}");
    let proposed = items(&reply["result"], "positions")?;
    // The preview covers the whole component: its seven atoms.
    assert_eq!(proposed.len(), 7, "{reply}");
    for entry in proposed {
        let before = point(&entry["before"])?;
        let after = point(&entry["after"])?;
        assert!(
            close(before.0, after.0) && close(before.1, after.1),
            "the layout moves an imported atom: {entry}"
        );
    }
    Ok(())
}

#[test]
fn the_preview_document_holds_the_committed_positions() -> TestResult {
    let (app, _directory, id) = setup()?;
    let previewed = preview(&app, &id, 0, "CC1CCCCC1")?;
    let digest = previewed["result"]["preview_digest"]
        .as_str()
        .ok_or_else(|| format!("no preview digest in {previewed}"))?;
    let committed = commit(&app, &id, 0, "CC1CCCCC1", digest)?;
    assert_eq!(committed["document_revision"], 1, "{committed}");
    let before = ordered_positions(&previewed["result"]["document"])?;
    let after = ordered_positions(&committed["result"]["document"])?;
    assert_eq!(before.len(), 7, "{previewed}");
    assert_eq!(
        before, after,
        "the preview holds the positions of the commit"
    );
    Ok(())
}
