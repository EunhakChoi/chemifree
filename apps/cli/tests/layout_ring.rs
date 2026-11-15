//! The CLI reaches the simple-ring layout in one `operations` process: a hexagon preview names its
//! ring as one `rings` entry with no chain, applying that preview changes nothing, and every reply
//! names the layout model.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

/// The layout model that every reply of `layout.structure` names.
const MODEL_ID: &str = "chemifree-chain-layout-2";

/// Atom IDs mapped to their `[x, y]` in pt.
type Positions = BTreeMap<String, [f64; 2]>;

/// A `chemifree operations` process. Each request line is answered before the next one is sent,
/// so a later request can name the document that an earlier reply created.
struct Cli {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl Cli {
    fn start(data_dir: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_chemifree"))
            .env_remove("CHEMIFREE_MODEL_DATA_ROOT")
            .env_remove("CHEMIFREE_MODEL_TRUSTED_REGISTRY")
            .arg("--data-dir")
            .arg(data_dir)
            .arg("operations")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        Self {
            child,
            input,
            output,
        }
    }

    /// Sends `value` as one line and returns the reply that answers it.
    fn call(&mut self, value: Value) -> Value {
        writeln!(self.input, "{value}").unwrap();
        let mut line = String::new();
        self.output.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap_or_else(|error| panic!("no reply to {value}: {error}"))
    }

    /// Closes the input, which ends the process, and asserts that it exits successfully.
    fn finish(mut self) {
        drop(self.input);
        let status = self.child.wait().unwrap();
        assert!(status.success(), "the CLI exited with {status}");
    }
}

fn request(id: &str, operation: &str, parameters: Value) -> Value {
    json!({"contract":1,"request_id":id,"profile":"preserve-semantics","operation":operation,"parameters":parameters})
}

/// The `[x, y]` of a coordinate object.
fn point(value: &Value) -> [f64; 2] {
    [value["x"].as_f64().unwrap(), value["y"].as_f64().unwrap()]
}

/// The atom positions of a document object, by atom ID.
fn positions(document: &Value) -> Positions {
    let mut positions = Positions::new();
    for atom in document["atoms"].as_array().unwrap() {
        let id = atom["id"].as_str().unwrap();
        positions.insert(id.to_owned(), point(&atom["position"]));
    }
    positions
}

/// The proposed `after` position of each entry of a preview reply's `positions`, by atom ID.
fn proposed(preview: &Value) -> Positions {
    let mut proposed = Positions::new();
    for entry in preview["result"]["positions"].as_array().unwrap() {
        let atom = entry["atom"].as_str().unwrap();
        proposed.insert(atom.to_owned(), point(&entry["after"]));
    }
    proposed
}

/// Asserts that each atom of `expected` lies within `tolerance` pt of its position in `actual`.
fn assert_near(actual: &Positions, expected: &[(&str, [f64; 2])], tolerance: f64, label: &str) {
    for &(atom, [x, y]) in expected {
        let [actual_x, actual_y] = actual
            .get(atom)
            .unwrap_or_else(|| panic!("{label}: {atom} has no position"));
        assert!(
            (actual_x - x).abs() <= tolerance && (actual_y - y).abs() <= tolerance,
            "{label}: {atom} is at ({actual_x}, {actual_y}), expected ({x}, {y})"
        );
    }
}

/// The diagnostic codes of a reply, in the order they are reported.
fn codes(reply: &Value) -> Vec<&str> {
    reply["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|diagnostic| diagnostic["code"].as_str().unwrap())
        .collect()
}

#[test]
fn cli_hexagon_preview_names_its_ring_and_apply_changes_nothing() {
    let root = tempfile::tempdir().unwrap();
    let mut cli = Cli::start(root.path());
    let created = cli.call(request("create", "document.create", json!({})));
    let id = created["result"]["document"]["id"].as_str().unwrap();

    // insert_ring draws the plain cyclohexane at the origin, with a1 to a6 in ring order.
    let ring = json!([{"op": "insert_ring", "instance": "s1", "center": {"x": 0, "y": 0}}]);
    let build = json!({"document_id": id, "expected_revision": 0, "commands": ring});
    let built = cli.call(request("ring", "document.transaction", build));
    assert_eq!(built["outcome"], "ok", "{built}");
    assert_eq!(built["document_revision"], 1, "{built}");
    let before = positions(&built["result"]["document"]);
    let atoms: Vec<&str> = before.keys().map(String::as_str).collect();
    assert_eq!(atoms, ["a1", "a2", "a3", "a4", "a5", "a6"], "{built}");

    // The whole ring is one component, free of any anchor, and it is one ring with no chain.
    let preview = json!({"document_id": id, "mode": "preview", "selection": {"atoms": ["a1"]}});
    let previewed = cli.call(request("preview", "layout.structure", preview));
    assert_eq!(previewed["outcome"], "ok", "{previewed}");
    assert_eq!(previewed["model_id"], MODEL_ID, "{previewed}");
    assert_eq!(previewed["document_revision"], 1, "{previewed}");
    let components = previewed["result"]["components"].as_array().unwrap();
    assert_eq!(components.len(), 1, "{previewed}");
    let component = &previewed["result"]["components"][0];
    assert_eq!(component["anchor"], Value::Null, "{previewed}");
    assert_eq!(component["root"], "a1", "{previewed}");
    assert_eq!(component["chains"], json!([]), "{previewed}");
    let ring_entry = json!([{"atoms": ["a1", "a2", "a3", "a4", "a5", "a6"], "branch_from": null}]);
    assert_eq!(component["rings"], ring_entry, "{previewed}");

    // The ring is already laid out, so the proposal lies on the drawn positions.
    let proposal = proposed(&previewed);
    assert_eq!(proposal.len(), 6, "{previewed}");
    let expected: Vec<(&str, [f64; 2])> = before
        .iter()
        .map(|(atom, position)| (atom.as_str(), *position))
        .collect();
    assert_near(&proposal, &expected, 1e-9, "the preview");
    assert_eq!(
        codes(&previewed),
        ["layout_preview_only", "layout_not_implemented"],
        "{previewed}"
    );

    // Applying the same selection commits nothing: no revision, no undo entry, no moved atom.
    let apply = json!({
        "document_id": id,
        "mode": "apply",
        "expected_revision": 1,
        "selection": {"atoms": ["a1"]}
    });
    let applied = cli.call(request("apply", "layout.structure", apply));
    assert_eq!(applied["outcome"], "ok", "{applied}");
    assert_eq!(applied["model_id"], MODEL_ID, "{applied}");
    assert_eq!(applied["document_revision"], 1, "{applied}");
    assert_eq!(applied["result"]["session"]["undo_depth"], 1, "{applied}");
    assert_eq!(
        codes(&applied),
        ["layout_unchanged", "layout_not_implemented"],
        "{applied}"
    );
    assert_eq!(
        positions(&applied["result"]["document"]),
        before,
        "{applied}"
    );

    let snapshot = cli.call(request(
        "read",
        "document.snapshot",
        json!({"document_id": id}),
    ));
    assert_eq!(
        positions(&snapshot["result"]["document"]),
        before,
        "{snapshot}"
    );
    cli.finish();
}
