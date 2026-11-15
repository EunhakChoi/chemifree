//! The guards of the MOL/SDF layout of P48 S2. A record is laid out only when its header declares 2D
//! and every coordinate is zero; every other record keeps its file coordinates and reports no layout
//! loss. A single atom at the origin is laid out. A component with a bond mark stays one coincident
//! point, packed as a zero box, and a V3000 collection over that point is refused as it is without a
//! layout.

use std::collections::BTreeMap;
use std::error::Error;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use chemifree_application::Application;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities, V2000Codec};
use serde_json::{Value, json};
use tempfile::TempDir;

type App = Application<LocalCapabilities, FileDocumentRepository>;
/// The result of a test or helper, so that a failed read is an error rather than a panic.
type TestResult<T = ()> = Result<T, Box<dyn Error>>;

/// The tolerance, in pt, of a computed position against its derived value.
const TOLERANCE: f64 = 1e-9;
/// The origin of the layout box in an empty drawing.
const EMPTY_ORIGIN: (f64, f64) = (40.0, 40.0);
/// The program name at the start of header line 2.
const PROGRAM: &str = "     RDKit";
/// The bonds of ethanol, 1-based: C1-C2 and C2-O3, plain single bonds.
const ETHANOL_BONDS: [(usize, usize, u8, u8); 2] = [(1, 2, 1, 0), (2, 3, 1, 0)];
/// Ethanol with every coordinate zero.
const ZERO_ETHANOL: [(&str, f64, f64, f64); 3] = [
    ("C", 0.0, 0.0, 0.0),
    ("C", 0.0, 0.0, 0.0),
    ("O", 0.0, 0.0, 0.0),
];

fn app(path: &Path) -> App {
    Application::new(LocalCapabilities, FileDocumentRepository::new(path))
        .with_ctfile_codec(V2000Codec)
}

/// A fresh application over its own directory, with one empty document.
fn setup() -> TestResult<(App, TempDir, String)> {
    let directory = tempfile::tempdir()?;
    let application = app(directory.path());
    let id = new_document(&application, "setup")?;
    Ok((application, directory, id))
}

/// Runs one request through the application. Every request gets its own request id, since a reused
/// id names a different request.
fn call(app: &App, label: &str, operation: &str, parameters: Value) -> TestResult<Value> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let request_id = format!("{label}-{}", NEXT.fetch_add(1, Ordering::Relaxed));
    let body = json!({"contract": 1, "request_id": request_id, "profile": "preserve-semantics",
                      "operation": operation, "parameters": parameters});
    Ok(serde_json::to_value(
        app.execute(body.to_string().as_bytes()),
    )?)
}

fn new_document(app: &App, label: &str) -> TestResult<String> {
    let created = call(app, label, "document.create", json!({}))?;
    let id = created["result"]["document"]["id"]
        .as_str()
        .ok_or_else(|| format!("no document id in {created}"))?;
    Ok(id.to_owned())
}

fn snapshot(app: &App, id: &str) -> TestResult<Value> {
    let reply = call(
        app,
        "snapshot",
        "document.snapshot",
        json!({"document_id": id}),
    )?;
    Ok(reply["result"]["document"].clone())
}

/// Previews record 1 of `source`, read as MOL, into document `id` at revision 0.
fn preview(app: &App, id: &str, source: &str) -> TestResult<Value> {
    call(
        app,
        "preview",
        "document.import",
        json!({"document_id": id, "expected_revision": 0, "format": "mol",
               "source": source, "record": 1, "preview": true}),
    )
}

/// Previews and then commits record 1 of `source` into document `id`, accepting its losses. The
/// commit must be partial: its losses are reported and its atoms are written.
fn import(app: &App, id: &str, source: &str) -> TestResult<Value> {
    let previewed = preview(app, id, source)?;
    let digest = previewed["result"]["preview_digest"]
        .as_str()
        .ok_or_else(|| format!("no preview digest in {previewed}"))?;
    let committed = call(
        app,
        "commit",
        "document.import",
        json!({"document_id": id, "expected_revision": 0, "format": "mol",
               "source": source, "record": 1, "preview_digest": digest, "accept_loss": true}),
    )?;
    assert_eq!(committed["outcome"], "partial", "{committed}");
    Ok(committed)
}

/// The array under `key` of `value`.
fn items<'a>(value: &'a Value, key: &str) -> TestResult<&'a [Value]> {
    let array = value[key]
        .as_array()
        .ok_or_else(|| format!("{key} is not an array"))?;
    Ok(array.as_slice())
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

/// The position of every atom of a document, by atom id.
fn positions_by_id(document: &Value) -> TestResult<BTreeMap<String, (f64, f64)>> {
    let mut positions = BTreeMap::new();
    for atom in items(document, "atoms")? {
        let id = atom["id"].as_str().ok_or("an atom has no id")?;
        positions.insert(id.to_owned(), point(&atom["position"])?);
    }
    Ok(positions)
}

/// The positions that a committed import gave its input atoms, in input order.
fn imported_positions(reply: &Value) -> TestResult<Vec<(f64, f64)>> {
    let by_id = positions_by_id(&reply["result"]["document"])?;
    let mut positions = Vec::new();
    for id in items(&reply["result"]["import_mapping"], "atoms")? {
        let id = id.as_str().ok_or("an import mapping entry is not an id")?;
        let position = by_id
            .get(id)
            .ok_or_else(|| format!("atom {id} is not in the document"))?;
        positions.push(*position);
    }
    Ok(positions)
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

/// The losses of a reply in report order, without those whose feature is in `excluded`.
fn losses_except(reply: &Value, excluded: &[&str]) -> TestResult<Vec<Value>> {
    let mut kept = Vec::new();
    for loss in items(reply, "loss_report")? {
        let feature = loss["feature"].as_str().ok_or("a loss has no feature")?;
        if !excluded.contains(&feature) {
            kept.push(loss.clone());
        }
    }
    Ok(kept)
}

/// The mark of each bond of a committed reply, in document order; `null` for an unmarked bond.
fn bond_marks(reply: &Value) -> TestResult<Vec<Value>> {
    let mut marks = Vec::new();
    for bond in items(&reply["result"]["document"], "bonds")? {
        marks.push(bond["mark"].clone());
    }
    Ok(marks)
}

/// The refusal that a reply reports: its first diagnostic, which the reply must carry as an error.
fn refusal(reply: &Value) -> TestResult<Value> {
    assert_eq!(reply["outcome"], "error", "{reply}");
    Ok(reply["diagnostics"][0].clone())
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

/// Whether two coordinates agree within `TOLERANCE`.
fn close(actual: f64, expected: f64) -> bool {
    (actual - expected).abs() <= TOLERANCE
}

/// Asserts that `reply` reports neither a generated layout nor an uncovered component.
fn assert_no_layout(reply: &Value, context: &str) -> TestResult {
    let names = features(reply)?;
    assert!(
        !names.contains(&"generated_layout") && !names.contains(&"layout_not_covered"),
        "{context}: {reply}"
    );
    Ok(())
}

/// The horizontal step of a 30 degree zigzag bond of `bond_length_pt`: `b * sqrt(3) / 2`, which is
/// `15 * sqrt(3)` for the default 30 pt bond.
fn step(bond_length_pt: f64) -> f64 {
    bond_length_pt * 3f64.sqrt() / 2.0
}

/// Header line 2 with `code` at byte `column`, after the program name. The dimensional code of a
/// MOL record is bytes 20-21 of this line, so `header(20, "2D")` declares 2D.
fn header(column: usize, code: &str) -> String {
    format!("{PROGRAM:<column$}{code}")
}

/// A V2000 record with an empty title and comment, header line 2 `program`, the atoms as
/// (element, x, y, z) and the bonds as (begin, end, order, stereo), all 1-based.
fn mol(program: &str, atoms: &[(&str, f64, f64, f64)], bonds: &[(usize, usize, u8, u8)]) -> String {
    let mut text = format!(
        "\n{program}\n\n{:>3}{:>3}  0  0  0  0  0  0  0  0999 V2000\n",
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

/// The G-T7 record. Atom 2 is the centre of atoms 1-4: its bond to atom 3 (F) is a wedge, narrow at
/// atom 2, and its bonds to atoms 1 (C) and 4 (Cl) are plain. Atoms 5-7 are ethanol. Every atom is
/// at the origin.
fn marked_record(program: &str) -> String {
    mol(
        program,
        &[
            ("C", 0.0, 0.0, 0.0),
            ("C", 0.0, 0.0, 0.0),
            ("F", 0.0, 0.0, 0.0),
            ("Cl", 0.0, 0.0, 0.0),
            ("C", 0.0, 0.0, 0.0),
            ("C", 0.0, 0.0, 0.0),
            ("O", 0.0, 0.0, 0.0),
        ],
        &[
            (1, 2, 1, 0),
            (2, 3, 1, 1),
            (2, 4, 1, 0),
            (5, 6, 1, 0),
            (6, 7, 1, 0),
        ],
    )
}

/// A V3000 record of a carbon with F, Cl and Br neighbours, all at the origin. The bond from the
/// carbon to Cl is a wedge, and a STEABS collection names the carbon as an absolute stereocentre.
fn v3000_centre(program: &str) -> String {
    format!(
        "\n{program}\n\n  0  0  0     0  0            999 V3000\nM  V30 BEGIN CTAB\nM  V30 COUNTS 4 3 0 0 0\nM  V30 BEGIN ATOM\nM  V30 1 C 0.0000 0.0000 0.0000 0\nM  V30 2 F 0.0000 0.0000 0.0000 0\nM  V30 3 Cl 0.0000 0.0000 0.0000 0\nM  V30 4 Br 0.0000 0.0000 0.0000 0\nM  V30 END ATOM\nM  V30 BEGIN BOND\nM  V30 1 1 1 2\nM  V30 2 1 1 3 CFG=1\nM  V30 3 1 1 4\nM  V30 END BOND\nM  V30 BEGIN COLLECTION\nM  V30 MDLV30/STEABS ATOMS=(1 1)\nM  V30 END COLLECTION\nM  V30 END CTAB\nM  END\n"
    )
}

/// G-T1. A 3D record whose coordinates are all zero is not moved: its header does not declare 2D.
#[test]
fn a_3d_record_of_zeros_is_not_moved() -> TestResult {
    assert_eq!(
        header(20, "2D").as_bytes().get(20..22),
        Some(b"2D".as_slice()),
        "fixture: the 2D code is at bytes 20-21"
    );
    let (app, _directory, id) = setup()?;
    let reply = import(
        &app,
        &id,
        &mol(&header(20, "3D"), &ZERO_ETHANOL, &ETHANOL_BONDS),
    )?;
    assert_eq!(imported_positions(&reply)?, vec![(0.0, 0.0); 3], "{reply}");
    assert_no_layout(&reply, "a 3D record")
}

/// G-T2. A 2D record with one nonzero x is not moved, and every atom keeps its file position.
#[test]
fn one_nonzero_x_is_not_moved() -> TestResult {
    let (app, _directory, id) = setup()?;
    let source = mol(
        &header(20, "2D"),
        &[
            ("C", 0.0, 0.0, 0.0),
            ("C", 0.0, 0.0, 0.0),
            ("O", 1.0, 0.0, 0.0),
        ],
        &ETHANOL_BONDS,
    );
    let reply = import(&app, &id, &source)?;
    // Each file coordinate maps to (20 x, -20 y): a zero y is the negative zero of -0.0 * 20.0.
    assert_eq!(
        imported_positions(&reply)?,
        vec![(0.0, -0.0 * 20.0), (0.0, -0.0 * 20.0), (20.0, -0.0 * 20.0)],
        "{reply}"
    );
    assert_no_layout(&reply, "a record with one nonzero x")
}

/// G-T3. A 2D record with one nonzero z is not moved, and reports the z coordinate it keeps.
#[test]
fn one_nonzero_z_is_not_moved() -> TestResult {
    let (app, _directory, id) = setup()?;
    let source = mol(
        &header(20, "2D"),
        &[
            ("C", 0.0, 0.0, 0.0),
            ("C", 0.0, 0.0, 1.0),
            ("O", 0.0, 0.0, 0.0),
        ],
        &ETHANOL_BONDS,
    );
    let reply = import(&app, &id, &source)?;
    assert_eq!(imported_positions(&reply)?, vec![(0.0, 0.0); 3], "{reply}");
    let z_losses = losses_named(&reply, "z_coordinate")?;
    assert_eq!(z_losses.len(), 1, "{reply}");
    assert!(
        z_losses
            .iter()
            .all(|loss| loss["source_path"] == "records[1].atoms[2].z"),
        "{reply}"
    );
    assert_no_layout(&reply, "a record with one nonzero z")
}

/// G-T4. A header that does not declare 2D, in any of the ways a reader can see, is not laid out.
#[test]
fn header_variants_are_not_laid_out() -> TestResult {
    let variants = [
        ("a lower-case code", header(20, "2d")),
        ("a 2D code at bytes 18-19", header(18, "2D")),
        ("a 21-byte line", header(20, "2")),
        ("an empty line", String::new()),
    ];
    for (label, program) in variants {
        let (app, _directory, id) = setup()?;
        let reply = import(&app, &id, &mol(&program, &ZERO_ETHANOL, &ETHANOL_BONDS))?;
        assert_eq!(
            imported_positions(&reply)?,
            vec![(0.0, 0.0); 3],
            "{label}: {reply}"
        );
        assert_no_layout(&reply, label)?;
    }
    Ok(())
}

/// G-T5. A 2D record without atoms keeps today's empty import: the document does not change.
#[test]
fn a_2d_record_without_atoms_is_todays_no_op() -> TestResult {
    let (app, _directory, id) = setup()?;
    let before = snapshot(&app, &id)?;
    let reply = import(&app, &id, &mol(&header(20, "2D"), &[], &[]))?;
    assert_eq!(reply["document_revision"], before["revision"], "{reply}");
    assert_eq!(snapshot(&app, &id)?, before, "{reply}");
    assert_no_layout(&reply, "a record without atoms")
}

/// G-T6. A 2D record with one atom at the origin is laid out at the origin of an empty drawing.
#[test]
fn a_single_atom_at_the_origin_is_laid_out() -> TestResult {
    let (app, _directory, id) = setup()?;
    let reply = import(
        &app,
        &id,
        &mol(&header(20, "2D"), &[("C", 0.0, 0.0, 0.0)], &[]),
    )?;
    let context = reply.to_string();
    assert_positions(&imported_positions(&reply)?, &[EMPTY_ORIGIN], &context);
    assert!(features(&reply)?.contains(&"generated_layout"), "{context}");
    Ok(())
}

/// G-T7. A component with a bond mark keeps one coincident point, packed as a zero box, and derives
/// the same stereo as the same record with an empty header line 2.
#[test]
fn a_marked_component_keeps_one_point_and_packs_as_a_zero_box() -> TestResult {
    let (app, _directory, id) = setup()?;
    let reply = import(&app, &id, &marked_record(&header(20, "2D")))?;
    let context = reply.to_string();
    let positions = imported_positions(&reply)?;
    assert_eq!(positions.len(), 7, "{context}");
    let (zero_box, ethanol) = positions.split_at(4);
    assert_eq!(zero_box, [(40.0, 40.0); 4].as_slice(), "{context}");
    let h = step(30.0);
    assert_positions(
        ethanol,
        &[(100.0, 55.0), (100.0 + h, 40.0), (100.0 + 2.0 * h, 55.0)],
        &context,
    );

    let uncovered = losses_named(&reply, "layout_not_covered")?;
    let [only] = uncovered.as_slice() else {
        return Err(format!("expected one layout_not_covered: {context}").into());
    };
    assert_eq!(
        only["source_path"], "records[1].atoms[1, 2, 3, 4]",
        "{context}"
    );
    let reason = only["reason"].as_str().ok_or("a loss has no reason")?;
    assert!(
        reason.contains("stereo_in_layout") && reason.contains("source coordinates"),
        "{reason}"
    );
    assert_eq!(
        losses_named(&reply, "generated_layout")?.len(),
        1,
        "{context}"
    );

    let marks = bond_marks(&reply)?;
    assert!(
        marks.iter().any(|mark| !mark.is_null()),
        "the wedge is kept: {context}"
    );

    let plain_id = new_document(&app, "plain")?;
    let plain = import(&app, &plain_id, &marked_record(""))?;
    assert_eq!(
        reply["result"]["document"]["stereo"], plain["result"]["document"]["stereo"],
        "{context}"
    );
    assert!(
        items(&reply["result"]["document"]["stereo"], "atoms")?.is_empty(),
        "no configuration is stored from the coincident centre: {context}"
    );
    assert_eq!(bond_marks(&reply)?, bond_marks(&plain)?, "{context}");
    let excluded = ["generated_layout", "layout_not_covered", "source_header"];
    assert_eq!(
        losses_except(&reply, &excluded)?,
        losses_except(&plain, &excluded)?,
        "{context}"
    );
    Ok(())
}

/// G-T8. A V3000 collection over a coincident centre is refused exactly as it is with an empty
/// header line 2, and the refused import changes nothing.
#[test]
fn a_v3000_collection_over_a_coincident_centre_is_refused_as_without_layout() -> TestResult {
    let (app, _directory, id) = setup()?;
    let before = snapshot(&app, &id)?;
    let laid_out = refusal(&preview(&app, &id, &v3000_centre(&header(20, "2D")))?)?;
    let plain = refusal(&preview(&app, &id, &v3000_centre(""))?)?;
    assert_eq!(
        laid_out["code"], "v3000_collection_member_not_stereocenter",
        "{laid_out}"
    );
    assert_eq!(laid_out, plain);
    assert_eq!(snapshot(&app, &id)?, before);
    Ok(())
}
