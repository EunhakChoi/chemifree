//! SMILES import of components that the chain and simple ring layout does not cover (P48 S1). Such
//! a component stays on the grid and is reported as `layout_not_covered`; the other components keep
//! their laid-out positions; a replay reports the same losses as the commit it repeats.

use chemifree_application::Application;
use chemifree_providers::interchange_codec::LocalInterchangeCodec;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities, OpenSmilesCodec, V2000Codec};
use serde_json::{Value, json};
use tempfile::TempDir;

type App = Application<LocalCapabilities, FileDocumentRepository>;
type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

/// Tolerance for coordinates in points.
const TOLERANCE: f64 = 1e-9;

/// The grid cell of input atom `k` (from 0) on an empty drawing: ten columns, 30 pt apart, from
/// (40, 40).
fn grid(k: u32) -> (f64, f64) {
    (
        40.0 + 30.0 * f64::from(k % 10),
        40.0 + 30.0 * f64::from(k / 10),
    )
}

fn setup() -> TestResult<(App, TempDir, String)> {
    let dir = tempfile::tempdir()?;
    let app = Application::new(LocalCapabilities, FileDocumentRepository::new(dir.path()))
        .with_ctfile_codec(V2000Codec)
        .with_interchange_codec(LocalInterchangeCodec)
        .with_smiles_codec(OpenSmilesCodec);
    let created = call(&app, "create", "document.create", json!({}))?;
    let id = created["result"]["document"]["id"]
        .as_str()
        .ok_or("document.create returned no id")?
        .to_owned();
    Ok((app, dir, id))
}

fn call(app: &App, request_id: &str, operation: &str, parameters: Value) -> TestResult<Value> {
    let request = json!({"contract":1,"request_id":request_id,"profile":"preserve-semantics","operation":operation,"parameters":parameters});
    Ok(serde_json::to_value(
        app.execute(request.to_string().as_bytes()),
    )?)
}

/// The preview digest of importing `source` at `revision` into the document `id`.
fn preview_digest(app: &App, id: &str, revision: u64, source: &str) -> TestResult<String> {
    let preview = call(
        app,
        "preview",
        "document.import",
        json!({"document_id":id,"expected_revision":revision,"format":"smiles","source":source,"preview":true}),
    )?;
    preview["result"]["preview_digest"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("preview has no digest: {preview}").into())
}

fn commit_request(id: &str, revision: u64, source: &str, digest: &str) -> Value {
    json!({"document_id":id,"expected_revision":revision,"format":"smiles","source":source,"preview_digest":digest})
}

/// Previews and then commits `source` without accepting any loss; returns the commit's reply.
fn import(app: &App, id: &str, revision: u64, source: &str) -> TestResult<Value> {
    let digest = preview_digest(app, id, revision, source)?;
    call(
        app,
        "commit",
        "document.import",
        commit_request(id, revision, source, &digest),
    )
}

/// The features of the reply's loss report, in report order.
fn features(reply: &Value) -> Vec<&str> {
    reply["loss_report"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|loss| loss["feature"].as_str())
        .collect()
}

/// The only loss of `feature` in the reply's loss report.
fn single_loss<'a>(reply: &'a Value, feature: &str) -> TestResult<&'a Value> {
    let matching: Vec<&Value> = reply["loss_report"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|loss| loss["feature"] == feature)
        .collect();
    match matching.as_slice() {
        [loss] => Ok(*loss),
        found => Err(format!(
            "expected one {feature} loss, found {}: {reply}",
            found.len()
        )
        .into()),
    }
}

/// The atom positions of a document, in atom order.
fn positions(document: &Value) -> TestResult<Vec<(f64, f64)>> {
    let atoms = document["atoms"]
        .as_array()
        .ok_or("document has no atom list")?;
    atoms
        .iter()
        .map(|atom| -> TestResult<(f64, f64)> {
            let x = atom["position"]["x"]
                .as_f64()
                .ok_or("atom has no x position")?;
            let y = atom["position"]["y"]
                .as_f64()
                .ok_or("atom has no y position")?;
            Ok((x, y))
        })
        .collect()
}

fn assert_positions(actual: &[(f64, f64)], expected: &[(f64, f64)], context: &str) {
    assert_eq!(
        actual.len(),
        expected.len(),
        "{context}: atom count, got {actual:?}"
    );
    for (index, (got, want)) in actual.iter().zip(expected).enumerate() {
        let close = (got.0 - want.0).abs() < TOLERANCE && (got.1 - want.1).abs() < TOLERANCE;
        assert!(
            close,
            "{context}: atom {} at {got:?}, expected {want:?}",
            index + 1
        );
    }
}

#[test]
fn a_stereo_component_keeps_todays_grid() -> TestResult {
    let (app, _dir, id) = setup()?;
    let done = import(&app, &id, 0, "F[C@H](Cl)Br")?;
    assert_eq!(done["outcome"], "partial", "{done}");
    assert_eq!(done["document_revision"], 1, "{done}");
    assert_eq!(
        features(&done),
        ["generated_layout", "layout_not_covered"],
        "{done}"
    );
    let layout = single_loss(&done, "layout_not_covered")?;
    assert_eq!(layout["source_path"], "atoms[1, 2, 3, 4]", "{done}");
    assert_eq!(layout["disposition"], "normalized", "{layout}");
    assert_eq!(layout["semantic_impact"], "geometry", "{layout}");
    let reason = layout["reason"].as_str().unwrap_or_default();
    assert!(reason.contains("stereo_in_layout"), "{reason}");
    assert!(reason.contains("chemifree-chain-layout-2"), "{reason}");
    let expected: Vec<(f64, f64)> = (0..4).map(grid).collect();
    assert_positions(
        &positions(&done["result"]["document"])?,
        &expected,
        "F[C@H](Cl)Br",
    );
    Ok(())
}

#[test]
fn only_the_uncovered_component_falls_back() -> TestResult {
    let (app, _dir, id) = setup()?;
    let done = import(&app, &id, 0, "c1ccc2ccccc2c1.CC")?;
    assert_eq!(done["document_revision"], 1, "{done}");
    assert_eq!(
        features(&done),
        ["generated_layout", "layout_not_covered"],
        "{done}"
    );
    let layout = single_loss(&done, "layout_not_covered")?;
    assert_eq!(
        layout["source_path"], "atoms[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]",
        "{done}"
    );
    let reason = layout["reason"].as_str().unwrap_or_default();
    assert!(reason.contains("fused_ring_in_layout_set"), "{reason}");
    // Naphthalene keeps the first grid row. CC starts 60 pt past its 270 pt box, at x = 370; its
    // two atoms are the zigzag of a free chain of one bond, so the first is 15 pt lower.
    let height = 15.0 * 3f64.sqrt();
    let mut expected: Vec<(f64, f64)> = (0..10).map(grid).collect();
    expected.extend([(370.0, 55.0), (370.0 + height, 40.0)]);
    assert_positions(
        &positions(&done["result"]["document"])?,
        &expected,
        "naphthalene and CC",
    );
    Ok(())
}

#[test]
fn each_uncovered_ring_shape_names_its_code() -> TestResult {
    // (SMILES, atom count, the P42 code that names why the layout does not cover the ring)
    let rows: [(&str, u32, &str); 3] = [
        ("C1CCCCCCCCCCCC1", 13, "large_ring_in_layout_set"),
        ("C1CCC2(CC1)CC2", 8, "spiro_ring_in_layout_set"),
        ("C1CC2CCC1C2", 7, "fused_ring_in_layout_set"),
    ];
    for (source, count, code) in rows {
        let (app, _dir, id) = setup()?;
        let done = import(&app, &id, 0, source)?;
        assert_eq!(done["document_revision"], 1, "{source}: {done}");
        assert_eq!(
            features(&done),
            ["generated_layout", "layout_not_covered"],
            "{source}: {done}"
        );
        let layout = single_loss(&done, "layout_not_covered")?;
        let indices: Vec<String> = (1..=count).map(|n| n.to_string()).collect();
        let expected_path = format!("atoms[{}]", indices.join(", "));
        assert_eq!(
            layout["source_path"].as_str(),
            Some(expected_path.as_str()),
            "{source}: {layout}"
        );
        let reason = layout["reason"].as_str().unwrap_or_default();
        assert!(reason.contains(code), "{source}: {reason}");
        // Every atom of the ring is on the grid in input order: the 13th atom is k = 12, at
        // (100, 70).
        let expected: Vec<(f64, f64)> = (0..count).map(grid).collect();
        assert_positions(&positions(&done["result"]["document"])?, &expected, source);
    }
    Ok(())
}

#[test]
fn a_replay_reports_the_layout_losses_of_the_commit() -> TestResult {
    let (app, _dir, id) = setup()?;
    let source = "c1ccc2ccccc2c1.CC";
    let digest = preview_digest(&app, &id, 0, source)?;
    let request = commit_request(&id, 0, source, &digest);
    let commit = call(&app, "same-request", "document.import", request.clone())?;
    assert_eq!(commit["document_revision"], 1, "{commit}");
    assert_eq!(
        features(&commit),
        ["generated_layout", "layout_not_covered"],
        "{commit}"
    );
    // The same request ID and parameters: the committed reply is replayed, not applied again.
    let replay = call(&app, "same-request", "document.import", request)?;
    assert_eq!(replay["document_revision"], 1, "{replay}");
    assert_eq!(replay["loss_report"], commit["loss_report"], "{replay}");
    assert_eq!(
        replay["result"]["document"], commit["result"]["document"],
        "{replay}"
    );
    Ok(())
}
