//! The P48 S2 layout of coordinate-free MOL and SDF imports through `document.import`: a 2D record
//! whose coordinates are all zero is laid out below the drawing, reports `generated_layout` before
//! the projection's losses, needs the caller's acceptance, lays out a V3000 record like a V2000 one,
//! places a record exactly where the SMILES import places the same graph, keeps the origin, bond
//! length and record selection rules, packs a fallback component on the grid, and replays and undoes
//! as one transaction.

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
/// Header line 2 of a file that declares 2D. Its dimensional code is bytes 21-22, at indices 20..22.
const HEADER_2D: &str = "     RDKit          2D";
/// The origin of the layout box in an empty drawing.
const EMPTY_ORIGIN: (f64, f64) = (40.0, 40.0);

/// The horizontal step of a 30 degree zigzag bond of `bond_length_pt`: `b * sqrt(3) / 2`.
fn step(bond_length_pt: f64) -> f64 {
    bond_length_pt * 3f64.sqrt() / 2.0
}

/// Whether two coordinates or lengths agree within `TOLERANCE`.
fn close(actual: f64, expected: f64) -> bool {
    (actual - expected).abs() <= TOLERANCE
}

/// The positions of CCO in the layout box whose top-left corner is `origin`: the middle carbon at
/// the top of a zigzag of bonds `bond_length_pt` long, and both ends one half bond lower.
fn cco_positions(origin: (f64, f64), bond_length_pt: f64) -> [(f64, f64); 3] {
    let h = step(bond_length_pt);
    let low = bond_length_pt / 2.0;
    [
        (origin.0, origin.1 + low),
        (origin.0 + h, origin.1),
        (origin.0 + 2.0 * h, origin.1 + low),
    ]
}

/// CCO as a 2D record whose coordinates are all zero.
fn cco() -> String {
    mol(
        HEADER_2D,
        &[
            ("C", 0.0, 0.0, 0.0),
            ("C", 0.0, 0.0, 0.0),
            ("O", 0.0, 0.0, 0.0),
        ],
        &[(1, 2, 1, 0), (2, 3, 1, 0)],
    )
}

/// A V2000 record of `atoms` (element and coordinates) and `bonds` (one-based atom numbers, bond
/// order and stereo). Its title is empty and its header line 2 is `header`.
fn mol(header: &str, atoms: &[(&str, f64, f64, f64)], bonds: &[(usize, usize, u8, u8)]) -> String {
    let mut text = format!(
        "\n{header}\n\n{:>3}{:>3}  0  0  0  0  0  0  0  0999 V2000\n",
        atoms.len(),
        bonds.len()
    );
    for (element, x, y, z) in atoms {
        text.push_str(&format!(
            "{x:>10.4}{y:>10.4}{z:>10.4} {element:<3} 0  0  0  0  0  0  0  0  0  0  0  0\n"
        ));
    }
    for (begin, end, order, stereo) in bonds {
        text.push_str(&format!(
            "{begin:>3}{end:>3}{order:>3}{stereo:>3}  0  0  0\n"
        ));
    }
    text.push_str("M  END\n");
    text
}

/// A V3000 record of `elements`, all at the origin, and `bonds` (one-based atom numbers and bond
/// order), in the layout of the V3000 fixtures. Its header line 2 is `header`.
fn v3000(header: &str, elements: &[&str], bonds: &[(usize, usize, u8)]) -> String {
    let mut text = format!(
        "\n{header}\n\n  0  0  0     0  0            999 V3000\nM  V30 BEGIN CTAB\nM  V30 COUNTS {} {} 0 0 0\nM  V30 BEGIN ATOM\n",
        elements.len(),
        bonds.len()
    );
    for (number, element) in elements.iter().enumerate() {
        text.push_str(&format!(
            "M  V30 {} {element} 0.0000 0.0000 0.0000 0\n",
            number + 1
        ));
    }
    text.push_str("M  V30 END ATOM\nM  V30 BEGIN BOND\n");
    for (number, (begin, end, order)) in bonds.iter().enumerate() {
        text.push_str(&format!("M  V30 {} {order} {begin} {end}\n", number + 1));
    }
    text.push_str("M  V30 END BOND\nM  V30 END CTAB\nM  END\n");
    text
}

/// An SDF file of `records`, each terminated by `$$$$`.
fn sdf(records: &[String]) -> String {
    records
        .iter()
        .map(|record| format!("{record}$$$$\n"))
        .collect()
}

/// Two SDF records. Record 1 is 2D with coordinates (0, 0) and (1.5, 0), bonded. Record 2 is 2D with
/// both atoms at the origin, bonded.
fn two_record_sdf() -> String {
    let source = mol(
        HEADER_2D,
        &[("C", 0.0, 0.0, 0.0), ("C", 1.5, 0.0, 0.0)],
        &[(1, 2, 1, 0)],
    );
    let zeros = mol(
        HEADER_2D,
        &[("C", 0.0, 0.0, 0.0), ("C", 0.0, 0.0, 0.0)],
        &[(1, 2, 1, 0)],
    );
    sdf(&[source, zeros])
}

/// The `document.import` parameters of record `record` of `source`, read as `format`, appended to
/// document `id` at `revision`.
fn ct_import(id: &str, revision: u64, format: &str, source: &str, record: usize) -> Value {
    json!({"document_id": id, "expected_revision": revision, "format": format, "source": source, "record": record})
}

fn app(path: &std::path::Path) -> App {
    Application::new(LocalCapabilities, FileDocumentRepository::new(path))
        .with_ctfile_codec(V2000Codec)
        .with_interchange_codec(LocalInterchangeCodec)
        .with_smiles_codec(OpenSmilesCodec)
}

fn call(app: &App, request: &str, operation: &str, parameters: Value) -> TestResult<Value> {
    let request = json!({"contract":1,"request_id":request,"profile":"preserve-semantics","operation":operation,"parameters":parameters});
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

/// The document of `id` as stored.
fn snapshot(app: &App, id: &str) -> TestResult<Value> {
    let reply = call(
        app,
        "snapshot",
        "document.snapshot",
        json!({"document_id": id}),
    )?;
    Ok(reply["result"]["document"].clone())
}

/// The preview of the import request `parameters`, which is a request without `preview`.
fn preview(app: &App, request: &str, parameters: &Value) -> TestResult<Value> {
    let mut parameters = parameters.clone();
    parameters["preview"] = json!(true);
    call(app, request, "document.import", parameters)
}

/// The commit of the import request `parameters` with the digest of its preview and the caller's
/// acceptance of the losses it reports.
fn commit(
    app: &App,
    request: &str,
    parameters: &Value,
    digest: &str,
    accept_loss: bool,
) -> TestResult<Value> {
    let mut parameters = parameters.clone();
    parameters["preview_digest"] = json!(digest);
    parameters["accept_loss"] = json!(accept_loss);
    call(app, request, "document.import", parameters)
}

/// The preview digest of a preview reply.
fn digest_of(previewed: &Value) -> TestResult<&str> {
    let digest = previewed["result"]["preview_digest"]
        .as_str()
        .ok_or_else(|| format!("no preview digest in {previewed}"))?;
    Ok(digest)
}

/// Previews the import request `parameters` and commits it, accepting its losses; returns the commit.
fn import(app: &App, request: &str, parameters: &Value) -> TestResult<Value> {
    let previewed = preview(app, &format!("{request}-preview"), parameters)?;
    let digest = digest_of(&previewed)?;
    commit(app, request, parameters, digest, true)
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
    for entry in items(reply, "loss_report")? {
        names.push(entry["feature"].as_str().ok_or("a loss has no feature")?);
    }
    Ok(names)
}

/// The first loss of a reply for `feature`.
fn loss<'a>(reply: &'a Value, feature: &str) -> TestResult<&'a Value> {
    let losses = items(reply, "loss_report")?;
    let Some(found) = losses.iter().find(|entry| entry["feature"] == feature) else {
        return Err(format!("no {feature} loss in {reply}").into());
    };
    Ok(found)
}

/// How many losses of a reply are for `feature`.
fn count_loss(reply: &Value, feature: &str) -> TestResult<usize> {
    let count = items(reply, "loss_report")?
        .iter()
        .filter(|entry| entry["feature"] == feature)
        .count();
    Ok(count)
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

/// The element of each atom of a document, in the document's order.
fn elements_of(document: &Value) -> TestResult<Vec<&str>> {
    let mut elements = Vec::new();
    for atom in items(document, "atoms")? {
        elements.push(
            atom["element"]
                .as_str()
                .ok_or("a document atom has no element")?,
        );
    }
    Ok(elements)
}

/// The position of every atom of a document, by atom id.
fn positions_by_id(document: &Value) -> TestResult<std::collections::BTreeMap<String, (f64, f64)>> {
    let mut positions = std::collections::BTreeMap::new();
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

#[test]
fn a_coordinate_free_v2000_record_is_laid_out_below_an_empty_drawing() -> TestResult {
    // A fixture check: the dimensional code is the two bytes at indices 20 and 21 of header line 2.
    assert_eq!(HEADER_2D.as_bytes().get(20..22), Some(b"2D".as_slice()));
    let (app, _directory, id) = setup()?;
    let parameters = ct_import(&id, 0, "mol", &cco(), 1);
    let previewed = preview(&app, "preview", &parameters)?;
    assert_eq!(previewed["outcome"], "partial", "{previewed}");
    assert_eq!(previewed["document_revision"], 0, "{previewed}");
    assert_eq!(
        features(&previewed)?,
        [
            "generated_layout",
            "source_header",
            "default_hydrogen_interpretation"
        ],
        "{previewed}"
    );
    let generated = loss(&previewed, "generated_layout")?;
    assert_eq!(generated["source_path"], "records[1].atoms", "{previewed}");
    assert_eq!(generated["disposition"], "normalized", "{previewed}");
    assert_eq!(generated["semantic_impact"], "geometry", "{previewed}");
    // The preview's record keeps the source zeros, while its document holds the placed atoms.
    let records = items(&previewed["result"], "records")?;
    let [record] = records else {
        return Err(format!("the preview holds one record, got {}", records.len()).into());
    };
    for atom in items(record, "atoms")? {
        for key in ["x", "y", "z"] {
            assert_eq!(atom[key].as_f64(), Some(0.0), "{atom}");
        }
    }
    let expected = cco_positions(EMPTY_ORIGIN, 30.0);
    assert_positions(
        &ordered_positions(&previewed["result"]["document"])?,
        &expected,
        "the preview",
    );
    let digest = digest_of(&previewed)?;
    let refused = commit(&app, "commit-refused", &parameters, digest, false)?;
    assert_eq!(
        refused["diagnostics"][0]["code"], "loss_consent_required",
        "{refused}"
    );
    assert_eq!(snapshot(&app, &id)?["revision"], 0);
    let committed = commit(&app, "commit-accepted", &parameters, digest, true)?;
    assert_eq!(committed["outcome"], "partial", "{committed}");
    assert_eq!(committed["document_revision"], 1, "{committed}");
    assert_eq!(features(&committed)?, features(&previewed)?, "{committed}");
    assert_eq!(
        ordered_positions(&committed["result"]["document"])?,
        ordered_positions(&previewed["result"]["document"])?,
        "the commit writes the positions of its preview"
    );
    Ok(())
}

#[test]
fn the_v3000_dialect_is_laid_out_like_v2000() -> TestResult {
    for format in ["mol", "mol-v3000"] {
        let (app, _directory, id) = setup()?;
        let source = v3000(HEADER_2D, &["C", "C", "O"], &[(1, 2, 1), (2, 3, 1)]);
        let done = import(&app, "import", &ct_import(&id, 0, format, &source, 1))?;
        assert_eq!(done["outcome"], "partial", "{format}: {done}");
        assert_eq!(
            features(&done)?.first().copied(),
            Some("generated_layout"),
            "{format}: {done}"
        );
        assert_positions(
            &imported_positions(&done)?,
            &cco_positions(EMPTY_ORIGIN, 30.0),
            format,
        );
    }
    Ok(())
}

#[test]
fn mol_positions_equal_smiles_positions_bit_for_bit() -> TestResult {
    for smiles in [
        "CCO",
        "C1=CC=CC=C1",
        "CC1CCCCC1",
        "CC(C)CO",
        "C=CC",
        "CCO.CC",
    ] {
        let (app, _directory, source_id) = setup()?;
        let read = preview(
            &app,
            "smiles",
            &json!({"document_id": source_id, "expected_revision": 0, "format": "smiles", "source": smiles}),
        )?;
        // The SMILES atoms and bonds, in the order the text reads them, as the MOL record states them.
        let mut atoms: Vec<(&str, f64, f64, f64)> = Vec::new();
        for atom in items(&read["result"], "atoms")? {
            let element = atom["element"]
                .as_str()
                .ok_or("a SMILES atom has no element")?;
            atoms.push((element, 0.0, 0.0, 0.0));
        }
        let mut bonds: Vec<(usize, usize, u8, u8)> = Vec::new();
        for bond in items(&read["result"], "bonds")? {
            let begin = usize::try_from(bond["begin"].as_u64().ok_or("a bond has no begin")?)?;
            let end = usize::try_from(bond["end"].as_u64().ok_or("a bond has no end")?)?;
            let order: u8 = match bond["order"].as_str() {
                Some("single") => 1,
                Some("double") => 2,
                Some("triple") => 3,
                other => return Err(format!("{smiles}: unexpected bond order {other:?}").into()),
            };
            bonds.push((begin, end, order, 0));
        }
        let source = mol(HEADER_2D, &atoms, &bonds);
        let target = new_document(&app, "mol-target")?;
        let from_mol = preview(&app, "mol", &ct_import(&target, 0, "mol", &source, 1))?;
        assert_eq!(from_mol["outcome"], "partial", "{smiles}: {from_mol}");
        assert_eq!(
            elements_of(&from_mol["result"]["document"])?,
            elements_of(&read["result"]["document"])?,
            "{smiles}: the atoms are in the same order"
        );
        let from_smiles = ordered_positions(&read["result"]["document"])?;
        assert_eq!(from_smiles.len(), atoms.len(), "{smiles}");
        assert_eq!(
            ordered_positions(&from_mol["result"]["document"])?,
            from_smiles,
            "{smiles}: the MOL positions differ from the SMILES positions"
        );
    }
    Ok(())
}

#[test]
fn an_append_starts_60_pt_below_the_drawing() -> TestResult {
    let (app, _directory, id) = setup()?;
    transact(
        &app,
        "carbon",
        &id,
        0,
        json!([{"op": "add_atom", "element": "C", "position": {"x": 40, "y": 40}}]),
    )?;
    let done = import(&app, "append", &ct_import(&id, 1, "mol", &cco(), 1))?;
    assert_eq!(done["outcome"], "partial", "{done}");
    assert_eq!(done["document_revision"], 2, "{done}");
    // The drawing's lowest atom is at y 40, so the origin is at y 100 and x stays at the leftmost 40.
    assert_positions(
        &imported_positions(&done)?,
        &cco_positions((40.0, 100.0), 30.0),
        "CCO appended below one atom",
    );
    Ok(())
}

#[test]
fn the_bond_length_of_the_document_scales_the_layout() -> TestResult {
    let (app, _directory, id) = setup()?;
    transact(
        &app,
        "bond-length",
        &id,
        0,
        json!([{"op": "update_settings", "bond_length_pt": 20}]),
    )?;
    let done = import(&app, "import", &ct_import(&id, 1, "mol", &cco(), 1))?;
    assert_eq!(done["outcome"], "partial", "{done}");
    assert_eq!(
        done["result"]["document"]["settings"]["bond_length_pt"], 20.0,
        "{done}"
    );
    assert_positions(
        &imported_positions(&done)?,
        &cco_positions(EMPTY_ORIGIN, 20.0),
        "CCO at 20 pt",
    );
    Ok(())
}

#[test]
fn the_selected_zero_record_is_laid_out_after_its_unselected_neighbour() -> TestResult {
    let (app, _directory, id) = setup()?;
    let done = import(
        &app,
        "import",
        &ct_import(&id, 0, "sdf", &two_record_sdf(), 2),
    )?;
    assert_eq!(done["outcome"], "partial", "{done}");
    let names = features(&done)?;
    let unselected = names.iter().position(|name| *name == "unselected_record");
    let generated = names.iter().position(|name| *name == "generated_layout");
    assert!(
        matches!((unselected, generated), (Some(before), Some(after)) if before < after),
        "unselected_record must come before generated_layout: {names:?}"
    );
    assert_eq!(
        loss(&done, "unselected_record")?["source_path"],
        "records[1]"
    );
    assert_eq!(
        loss(&done, "generated_layout")?["source_path"],
        "records[2].atoms"
    );
    let h = step(30.0);
    assert_positions(
        &imported_positions(&done)?,
        &[(40.0, 55.0), (40.0 + h, 40.0)],
        "the zero record",
    );
    Ok(())
}

#[test]
fn the_selected_nonzero_record_keeps_its_source_coordinates() -> TestResult {
    let (app, _directory, id) = setup()?;
    let done = import(
        &app,
        "import",
        &ct_import(&id, 0, "sdf", &two_record_sdf(), 1),
    )?;
    assert_eq!(done["outcome"], "partial", "{done}");
    assert_eq!(
        imported_positions(&done)?,
        [(0.0, 0.0), (30.0, 0.0)],
        "{done}"
    );
    assert_eq!(count_loss(&done, "generated_layout")?, 0, "{done}");
    Ok(())
}

#[test]
fn a_fused_ring_stays_on_the_grid_and_the_next_component_packs_after_it() -> TestResult {
    // Kekule naphthalene as atoms 1-10 (two fused six-membered rings), then ethane as atoms 11 and 12.
    let (app, _directory, id) = setup()?;
    let atoms = [("C", 0.0, 0.0, 0.0); 12];
    let bonds: [(usize, usize, u8, u8); 12] = [
        (1, 2, 2, 0),
        (2, 3, 1, 0),
        (3, 4, 2, 0),
        (4, 5, 1, 0),
        (5, 10, 2, 0),
        (10, 1, 1, 0),
        (5, 6, 1, 0),
        (6, 7, 2, 0),
        (7, 8, 1, 0),
        (8, 9, 2, 0),
        (9, 10, 1, 0),
        (11, 12, 1, 0),
    ];
    let source = mol(HEADER_2D, &atoms, &bonds);
    let done = import(&app, "import", &ct_import(&id, 0, "mol", &source, 1))?;
    assert_eq!(done["outcome"], "partial", "{done}");
    // The ten ring atoms sit on the grid, 30 pt apart in one row. The box is 270 pt wide, so the
    // ethane box starts 60 pt after it, at 40 + 270 + 60 = 370, and is laid out as a chain there.
    let h = step(30.0);
    let mut expected: Vec<(f64, f64)> = (0u32..10)
        .map(|k| (40.0 + 30.0 * f64::from(k), 40.0))
        .collect();
    expected.extend([(370.0, 55.0), (370.0 + h, 40.0)]);
    assert_positions(
        &imported_positions(&done)?,
        &expected,
        "naphthalene and ethane",
    );
    let uncovered = items(&done, "loss_report")?
        .iter()
        .filter(|entry| entry["feature"] == "layout_not_covered")
        .collect::<Vec<_>>();
    let [only] = uncovered.as_slice() else {
        return Err(format!(
            "{} layout_not_covered losses, expected one: {done}",
            uncovered.len()
        )
        .into());
    };
    assert_eq!(
        only["source_path"], "records[1].atoms[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]",
        "{done}"
    );
    let reason = only["reason"]
        .as_str()
        .ok_or("a layout_not_covered loss has no reason")?;
    assert!(
        reason.contains("fused_ring_in_layout_set") && reason.contains("placed on a grid"),
        "{reason}"
    );
    Ok(())
}

#[test]
fn a_replayed_commit_adds_nothing_and_one_undo_removes_the_import() -> TestResult {
    let (app, _directory, id) = setup()?;
    let parameters = ct_import(&id, 0, "mol", &cco(), 1);
    let previewed = preview(&app, "replay-preview", &parameters)?;
    let digest = digest_of(&previewed)?;
    let first = commit(&app, "R", &parameters, digest, true)?;
    assert_eq!(first["document_revision"], 1, "{first}");
    let replay = commit(&app, "R", &parameters, digest, true)?;
    assert_eq!(replay, first, "a replay returns the first reply");
    assert_eq!(
        replay["result"]["document"]["revision"],
        first["result"]["document"]["revision"]
    );
    assert_eq!(imported_positions(&replay)?, imported_positions(&first)?);
    assert_eq!(replay["loss_report"], first["loss_report"]);
    assert_eq!(items(&snapshot(&app, &id)?, "atoms")?.len(), 3);
    let undone = call(
        &app,
        "undo",
        "history.undo",
        json!({"document_id": id, "expected_revision": 1}),
    )?;
    assert_eq!(undone["result"]["document"]["atoms"], json!([]), "{undone}");
    Ok(())
}
