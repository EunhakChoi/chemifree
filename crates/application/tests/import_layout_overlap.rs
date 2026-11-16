//! The overlap report of generated import layouts (P48 S4). A non-bonded atom pair of a laid-out
//! component closer than 0.3 bond lengths is one `generated_layout_overlap` loss for that
//! component, for a SMILES import and for a coordinate-free MOL record. The loss names the atoms in
//! the input's numbering, needs no consent, and leaves the generated positions as they are. Grid
//! components, source-point components and the grid spacing are not checked.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;

use chemifree_application::Application;
use chemifree_providers::interchange_codec::LocalInterchangeCodec;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities, OpenSmilesCodec, V2000Codec};
use serde_json::{Value, json};
use tempfile::TempDir;

type App = Application<LocalCapabilities, FileDocumentRepository>;
/// The result of a test or helper, so that a failed read is an error rather than a panic.
type TestResult<T = ()> = Result<T, Box<dyn Error>>;

/// The bond length of every fixture: the default of a new document, in pt.
const BOND_LENGTH: f64 = 30.0;
/// The threshold of this report, in bond lengths (CONTRACT, P48 S4).
const OVERLAP_FACTOR: f64 = 0.3;
/// The threshold of P42's `layout_overlap` warning, which this report does not use.
const P42_OVERLAP_FACTOR: f64 = 0.5;
/// Tetra-tert-butylmethane, 17 atoms. Its pairs 3-13, 4-17, 8-12 and 9-16 coincide exactly.
const TTBM: &str = "CC(C)(C)C(C(C)(C)C)(C(C)(C)C)C(C)(C)C";
/// A 21-atom isomer. Its pairs 8-16 and 11-13 lie 2 - sqrt(3) bond lengths apart, and one further
/// pair lies between 0.3 and 0.5 bond lengths apart.
const TNPM: &str = "C(CC(C)(C)C)(CC(C)(C)C)(CC(C)(C)C)CC(C)(C)C";
/// The bonds of TTBM in the order its SMILES text reads them, 1-based.
const TTBM_BONDS: [(usize, usize); 16] = [
    (1, 2),
    (2, 3),
    (2, 4),
    (2, 5),
    (5, 6),
    (6, 7),
    (6, 8),
    (6, 9),
    (5, 10),
    (10, 11),
    (10, 12),
    (10, 13),
    (5, 14),
    (14, 15),
    (14, 16),
    (14, 17),
];
/// The reason of the report for TTBM's four pairs.
const TTBM_REASON: &str = "Non-bonded atom pairs closer than 0.3 bond lengths in the generated layout (chemifree-chain-layout-2): 4 (3-13, 4-17, 8-12, 9-16). Overlaps are reported, not resolved; the generated positions are kept.";
/// Header line 2 of a record that declares 2D: its dimensional code is bytes 20-21.
const HEADER_2D: &str = "     RDKit          2D";

fn app(path: &std::path::Path) -> App {
    Application::new(LocalCapabilities, FileDocumentRepository::new(path))
        .with_ctfile_codec(V2000Codec)
        .with_interchange_codec(LocalInterchangeCodec)
        .with_smiles_codec(OpenSmilesCodec)
}

/// Runs one request. `request` is its request id, so a repeated id replays the earlier reply.
fn call(app: &App, request: &str, operation: &str, parameters: Value) -> TestResult<Value> {
    let request = json!({"contract": 1, "request_id": request, "profile": "preserve-semantics", "operation": operation, "parameters": parameters});
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

/// The `document.import` parameters of `source` read as SMILES, into document `id` at `revision`.
fn smiles_parameters(id: &str, revision: u64, source: &str) -> Value {
    json!({"document_id": id, "expected_revision": revision, "format": "smiles", "source": source})
}

/// The `document.import` parameters of record 1 of `source` read as MOL, into document `id` at
/// `revision`.
fn mol_parameters(id: &str, revision: u64, source: &str) -> Value {
    json!({"document_id": id, "expected_revision": revision, "format": "mol", "source": source, "record": 1})
}

/// The preview of an import request, which `request` names.
fn preview(app: &App, request: &str, parameters: &Value) -> TestResult<Value> {
    let mut parameters = parameters.clone();
    parameters["preview"] = json!(true);
    call(app, request, "document.import", parameters)
}

/// The commit of an import request with the digest of its preview and the caller's acceptance of
/// the losses that need it.
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

/// The losses of a reply named `feature`, in report order.
fn losses_named<'a>(reply: &'a Value, feature: &str) -> TestResult<Vec<&'a Value>> {
    let mut found = Vec::new();
    for loss in items(reply, "loss_report")? {
        if loss["feature"] == feature {
            found.push(loss);
        }
    }
    Ok(found)
}

/// The only loss of a reply named `feature`; the reply must report exactly one.
fn single_loss<'a>(reply: &'a Value, feature: &str) -> TestResult<&'a Value> {
    let found = losses_named(reply, feature)?;
    let [loss] = found.as_slice() else {
        return Err(format!("expected one {feature} loss in {reply}").into());
    };
    Ok(*loss)
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
    let mut by_id = BTreeMap::new();
    for atom in items(&reply["result"]["document"], "atoms")? {
        let id = atom["id"].as_str().ok_or("an atom has no id")?;
        by_id.insert(id.to_owned(), point(&atom["position"])?);
    }
    let mut positions = Vec::new();
    for id in import_ids(reply)? {
        let position = by_id
            .get(&id)
            .ok_or_else(|| format!("atom {id} is not in the document"))?;
        positions.push(*position);
    }
    Ok(positions)
}

/// The 0-based number of the atom with the id `id`, which a bond end names.
fn number_of_id(numbers: &BTreeMap<String, usize>, id: &Value) -> TestResult<usize> {
    let id = id.as_str().ok_or("a bond end is not an atom id")?;
    let number = numbers
        .get(id)
        .ok_or_else(|| format!("no atom {id} in the document"))?;
    Ok(*number)
}

/// Every non-bonded atom pair of `document` with its distance in pt, in ascending order. The pairs
/// are numbered from 1, in the order the document lists its atoms, which is input order for an
/// import into an empty drawing. This is computed from the positions and bonds alone, so it is an
/// oracle for the report.
fn non_bonded_distances(document: &Value) -> TestResult<Vec<((usize, usize), f64)>> {
    let mut numbers = BTreeMap::new();
    let mut positions = Vec::new();
    for (number, atom) in items(document, "atoms")?.iter().enumerate() {
        let id = atom["id"].as_str().ok_or("an atom has no id")?;
        numbers.insert(id.to_owned(), number);
        positions.push(point(&atom["position"])?);
    }
    let mut bonded = BTreeSet::new();
    for bond in items(document, "bonds")? {
        let begin = number_of_id(&numbers, &bond["begin"])?;
        let end = number_of_id(&numbers, &bond["end"])?;
        bonded.insert((begin.min(end), begin.max(end)));
    }
    let mut distances = Vec::new();
    for (i, &(xi, yi)) in positions.iter().enumerate() {
        for (j, &(xj, yj)) in positions.iter().enumerate().skip(i + 1) {
            if !bonded.contains(&(i, j)) {
                distances.push(((i + 1, j + 1), (xj - xi).hypot(yj - yi)));
            }
        }
    }
    Ok(distances)
}

/// The pairs of `distances` closer than `limit`, in their order.
fn pairs_under(distances: &[((usize, usize), f64)], limit: f64) -> Vec<(usize, usize)> {
    distances
        .iter()
        .filter(|(_, distance)| *distance < limit)
        .map(|(pair, _)| *pair)
        .collect()
}

/// The non-bonded pairs of `document` closer than `OVERLAP_FACTOR` bond lengths, from its own
/// positions: the oracle that the report must match.
fn close_pairs(document: &Value) -> TestResult<Vec<(usize, usize)>> {
    let distances = non_bonded_distances(document)?;
    Ok(pairs_under(&distances, OVERLAP_FACTOR * BOND_LENGTH))
}

/// The reason of the report for `count` overlapping pairs, which `listed` names, in the form that
/// CONTRACT (P48 S4) gives.
fn overlap_reason(count: usize, listed: &str) -> String {
    format!(
        "Non-bonded atom pairs closer than 0.3 bond lengths in the generated layout (chemifree-chain-layout-2): {count} ({listed}). Overlaps are reported, not resolved; the generated positions are kept."
    )
}

/// A V2000 record with an empty title and comment: header line 2 `header`, the atoms as
/// (element, x, y, z) and the bonds as (begin, end, order, stereo), all 1-based.
fn mol(header: &str, atoms: &[(&str, f64, f64, f64)], bonds: &[(usize, usize, u8, u8)]) -> String {
    let mut text = format!(
        "\n{header}\n\n{:>3}{:>3}  0  0  0  0  0  0  0  0999 V2000\n",
        atoms.len(),
        bonds.len()
    );
    for &(element, x, y, z) in atoms {
        text.push_str(&format!(
            "{x:>10.4}{y:>10.4}{z:>10.4} {element:<3} 0  0  0  0  0  0  0  0  0  0  0  0\n"
        ));
    }
    for &(begin, end, order, stereo) in bonds {
        text.push_str(&format!(
            "{begin:>3}{end:>3}{order:>3}{stereo:>3}  0  0  0\n"
        ));
    }
    text.push_str("M  END\n");
    text
}

/// TTBM as a 2D record whose 17 atoms are all at the origin, with the bonds of `TTBM_BONDS`. Its
/// first bond has the stereo value `first_stereo` (1 is a wedge) and the other bonds have none.
fn ttbm_record(first_stereo: u8) -> String {
    let atoms = [("C", 0.0, 0.0, 0.0); 17];
    let mut bonds = Vec::new();
    for (number, &(begin, end)) in TTBM_BONDS.iter().enumerate() {
        let stereo = if number == 0 { first_stereo } else { 0 };
        bonds.push((begin, end, 1, stereo));
    }
    mol(HEADER_2D, &atoms, &bonds)
}

/// TTBM's four coincident pairs are one `generated_layout_overlap` loss of its one component, with
/// 1-based atoms, the exact reason, and no consent (P48 S4, V13 and V14).
#[test]
fn coincident_atoms_are_reported_as_one_generated_layout_overlap() -> TestResult {
    let (app, _directory, id) = setup()?;
    let previewed = preview(&app, "ttbm", &smiles_parameters(&id, 0, TTBM))?;
    assert_eq!(previewed["outcome"], "partial", "{previewed}");
    assert_eq!(
        features(&previewed)?,
        ["generated_layout", "generated_layout_overlap"],
        "{previewed}"
    );
    let overlap = single_loss(&previewed, "generated_layout_overlap")?;
    assert_eq!(
        overlap["source_path"], "atoms[3, 4, 8, 9, 12, 13, 16, 17]",
        "{previewed}"
    );
    assert_eq!(overlap["reason"], TTBM_REASON, "{previewed}");
    assert_eq!(overlap["disposition"], "normalized", "{previewed}");
    assert_eq!(overlap["semantic_impact"], "geometry", "{previewed}");
    assert_eq!(
        close_pairs(&previewed["result"]["document"])?,
        [(3, 13), (4, 17), (8, 12), (9, 16)],
        "{previewed}"
    );
    Ok(())
}

/// The threshold is 0.3 bond lengths, not P42's 0.5: TNPM's pair 0.27 bond lengths apart is
/// reported, and the pair between 0.3 and 0.5 bond lengths apart is not.
#[test]
fn the_threshold_is_0_3_bond_lengths_not_p42s_0_5() -> TestResult {
    let (app, _directory, id) = setup()?;
    let previewed = preview(&app, "tnpm", &smiles_parameters(&id, 0, TNPM))?;
    assert_eq!(previewed["outcome"], "partial", "{previewed}");
    let document = &previewed["result"]["document"];
    assert_eq!(
        document["settings"]["bond_length_pt"], BOND_LENGTH,
        "{previewed}"
    );
    let distances = non_bonded_distances(document)?;
    let limit = OVERLAP_FACTOR * BOND_LENGTH;
    assert_eq!(
        pairs_under(&distances, limit),
        [(8, 16), (11, 13)],
        "{previewed}"
    );
    let p42_limit = P42_OVERLAP_FACTOR * BOND_LENGTH;
    assert!(
        distances
            .iter()
            .any(|(_, distance)| (limit..p42_limit).contains(distance)),
        "no pair lies between 0.3 and 0.5 bond lengths apart: {distances:?}"
    );
    let overlap = single_loss(&previewed, "generated_layout_overlap")?;
    assert_eq!(
        overlap["source_path"], "atoms[8, 11, 13, 16]",
        "{previewed}"
    );
    assert_eq!(
        overlap["reason"],
        overlap_reason(2, "8-16, 11-13"),
        "{previewed}"
    );
    Ok(())
}

/// Two components give one loss each, in component order and numbered over the whole input. The
/// SMILES commit needs no consent, and a repeated request replays the same report.
#[test]
fn each_component_reports_its_own_overlap_in_input_order() -> TestResult {
    let (app, _directory, id) = setup()?;
    let source = format!("{TTBM}.{TNPM}");
    let parameters = smiles_parameters(&id, 0, &source);
    let previewed = preview(&app, "both-preview", &parameters)?;
    assert_eq!(previewed["outcome"], "partial", "{previewed}");
    let overlaps = losses_named(&previewed, "generated_layout_overlap")?;
    let [first, second] = overlaps.as_slice() else {
        return Err(format!("two overlap losses expected: {previewed}").into());
    };
    assert_eq!(
        first["source_path"], "atoms[3, 4, 8, 9, 12, 13, 16, 17]",
        "{previewed}"
    );
    assert_eq!(first["reason"], TTBM_REASON, "{previewed}");
    assert_eq!(
        second["source_path"], "atoms[25, 28, 30, 33]",
        "{previewed}"
    );
    assert_eq!(
        second["reason"],
        overlap_reason(2, "25-33, 28-30"),
        "{previewed}"
    );
    assert_eq!(
        close_pairs(&previewed["result"]["document"])?,
        [(3, 13), (4, 17), (8, 12), (9, 16), (25, 33), (28, 30)],
        "{previewed}"
    );

    let digest = digest_of(&previewed)?;
    let committed = commit(&app, "both-commit", &parameters, digest, false)?;
    assert_eq!(committed["outcome"], "partial", "{committed}");
    assert_eq!(committed["document_revision"], 1, "{committed}");
    let replayed = commit(&app, "both-commit", &parameters, digest, false)?;
    assert_eq!(
        replayed["loss_report"], committed["loss_report"],
        "{replayed}"
    );
    Ok(())
}

/// A chain whose atoms are all farther apart than 0.3 bond lengths reports no overlap.
#[test]
fn a_chain_without_a_close_pair_reports_no_overlap() -> TestResult {
    let (app, _directory, id) = setup()?;
    let previewed = preview(&app, "cco", &smiles_parameters(&id, 0, "CCO"))?;
    assert_eq!(features(&previewed)?, ["generated_layout"], "{previewed}");
    assert_eq!(
        close_pairs(&previewed["result"]["document"])?,
        Vec::<(usize, usize)>::new(),
        "{previewed}"
    );
    Ok(())
}

/// The grid component keeps its 30 pt spacing, which is under 0.3 of a 200 pt bond, and the
/// laid-out component beside it has no close pair. Neither is reported.
#[test]
fn grid_components_are_not_checked_for_overlap() -> TestResult {
    let (app, _directory, id) = setup()?;
    transact(
        &app,
        "wide-bonds",
        &id,
        0,
        json!([{"op": "update_settings", "bond_length_pt": 200}]),
    )?;
    let previewed = preview(&app, "grid", &smiles_parameters(&id, 1, "F[C@H](Cl)Br.CC"))?;
    assert_eq!(previewed["outcome"], "partial", "{previewed}");
    let names = features(&previewed)?;
    assert!(
        names.contains(&"layout_not_covered"),
        "the stereocentre must stay on the grid: {previewed}"
    );
    assert!(
        names.contains(&"generated_layout"),
        "the CC component must be laid out: {previewed}"
    );
    assert!(!names.contains(&"generated_layout_overlap"), "{previewed}");
    Ok(())
}

/// A coordinate-free record reports its overlap under its own `records[1].` path, with the same
/// reason text as a SMILES import of the same atoms.
#[test]
fn a_coordinate_free_record_reports_its_overlap_under_its_record_path() -> TestResult {
    // A fixture check: the dimensional code is the two bytes at indices 20 and 21 of header line 2.
    assert_eq!(HEADER_2D.as_bytes().get(20..22), Some(b"2D".as_slice()));
    let (app, _directory, id) = setup()?;
    let previewed = preview(
        &app,
        "mol-preview",
        &mol_parameters(&id, 0, &ttbm_record(0)),
    )?;
    assert_eq!(previewed["outcome"], "partial", "{previewed}");
    let overlap = single_loss(&previewed, "generated_layout_overlap")?;
    assert_eq!(
        overlap["source_path"], "records[1].atoms[3, 4, 8, 9, 12, 13, 16, 17]",
        "{previewed}"
    );
    assert_eq!(overlap["reason"], TTBM_REASON, "{previewed}");
    assert_eq!(
        close_pairs(&previewed["result"]["document"])?,
        [(3, 13), (4, 17), (8, 12), (9, 16)],
        "{previewed}"
    );
    Ok(())
}

/// Without acceptance the record's commit is refused with `loss_consent_required`, and nothing is
/// written.
#[test]
fn a_record_overlap_is_refused_without_acceptance_and_writes_nothing() -> TestResult {
    let (app, _directory, id) = setup()?;
    let parameters = mol_parameters(&id, 0, &ttbm_record(0));
    let previewed = preview(&app, "mol-preview-refused", &parameters)?;
    let refused = commit(
        &app,
        "mol-refused",
        &parameters,
        digest_of(&previewed)?,
        false,
    )?;
    assert_eq!(refused["outcome"], "error", "{refused}");
    assert_eq!(
        refused["diagnostics"][0]["code"], "loss_consent_required",
        "{refused}"
    );
    let snapshot = call(
        &app,
        "mol-snapshot",
        "document.snapshot",
        json!({"document_id": id}),
    )?;
    assert_eq!(snapshot["result"]["document"]["revision"], 0, "{snapshot}");
    Ok(())
}

#[test]
fn an_accepted_record_writes_the_smiles_positions_bit_for_bit() -> TestResult {
    let (app, _directory, smiles_id) = setup()?;
    let smiles = smiles_parameters(&smiles_id, 0, TTBM);
    let smiles_previewed = preview(&app, "smiles-preview", &smiles)?;
    let from_smiles = commit(
        &app,
        "smiles-commit",
        &smiles,
        digest_of(&smiles_previewed)?,
        false,
    )?;
    assert_eq!(from_smiles["outcome"], "partial", "{from_smiles}");

    let mol_id = new_document(&app, "mol-create")?;
    let parameters = mol_parameters(&mol_id, 0, &ttbm_record(0));
    let previewed = preview(&app, "mol-preview-accepted", &parameters)?;
    let from_mol = commit(
        &app,
        "mol-accepted",
        &parameters,
        digest_of(&previewed)?,
        true,
    )?;
    assert_eq!(from_mol["outcome"], "partial", "{from_mol}");
    assert_eq!(from_mol["document_revision"], 1, "{from_mol}");
    let overlap = single_loss(&from_mol, "generated_layout_overlap")?;
    assert_eq!(
        overlap["source_path"], "records[1].atoms[3, 4, 8, 9, 12, 13, 16, 17]",
        "{from_mol}"
    );
    assert_eq!(
        imported_positions(&from_mol)?,
        imported_positions(&from_smiles)?,
        "the record's positions differ from the SMILES positions"
    );
    Ok(())
}

/// A repeated accepted request replays the same report, overlap included.
#[test]
fn a_replayed_accepted_record_reports_the_same_overlap() -> TestResult {
    let (app, _directory, id) = setup()?;
    let parameters = mol_parameters(&id, 0, &ttbm_record(0));
    let previewed = preview(&app, "mol-preview-replay", &parameters)?;
    let digest = digest_of(&previewed)?;
    let first = commit(&app, "mol-replay", &parameters, digest, true)?;
    assert_eq!(first["outcome"], "partial", "{first}");
    let replayed = commit(&app, "mol-replay", &parameters, digest, true)?;
    single_loss(&replayed, "generated_layout_overlap")?;
    assert_eq!(replayed["loss_report"], first["loss_report"], "{replayed}");
    Ok(())
}

/// A record whose first bond is a wedge is one source point: its atoms coincide as they do in the
/// file, and the component is reported as not laid out, not as overlapping.
#[test]
fn a_source_point_component_reports_no_overlap() -> TestResult {
    let (app, _directory, id) = setup()?;
    let previewed = preview(&app, "mol-marked", &mol_parameters(&id, 0, &ttbm_record(1)))?;
    assert_ne!(previewed["outcome"], "error", "{previewed}");
    // A fixture check: the marked component is one point in the preview.
    let positions = ordered_positions(&previewed["result"]["document"])?;
    let [first, rest @ ..] = positions.as_slice() else {
        return Err(format!("the preview holds no atoms: {previewed}").into());
    };
    assert!(
        rest.iter().all(|position| position == first),
        "the marked atoms do not coincide: {previewed}"
    );
    let names = features(&previewed)?;
    assert!(!names.contains(&"generated_layout_overlap"), "{previewed}");
    let not_covered = single_loss(&previewed, "layout_not_covered")?;
    assert!(
        not_covered["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("one shared point")),
        "{previewed}"
    );
    Ok(())
}
