//! The CLI reaches the same `layout.structure` apply in one `operations` process: the preview's
//! positions are committed as one undoable edit, undo restores the positions exactly, and a stale
//! revision is refused unchanged.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

/// Atom IDs mapped to their `[x, y]` in pt.
type Positions = BTreeMap<String, [f64; 2]>;

/// The worked example of the applied butane, rounded to 2 decimals, so checked to 0.01 pt.
const WORKED: [(&str, [f64; 2]); 4] = [
    ("a1", [6.03, 7.5]),
    ("a2", [32.01, -7.5]),
    ("a3", [57.99, 7.5]),
    ("a4", [83.97, -7.5]),
];

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

/// Carbons a1 to a4 in a row, 30 pt apart, each bonded to the one before it.
fn butane() -> Value {
    json!([
        {"op":"add_atom","element":"C","position":{"x":0,"y":0}},
        {"op":"add_atom","element":"C","position":{"x":30,"y":0},"bond_to":{"atom":"$0","order":"single"}},
        {"op":"add_atom","element":"C","position":{"x":60,"y":0},"bond_to":{"atom":"$1","order":"single"}},
        {"op":"add_atom","element":"C","position":{"x":90,"y":0},"bond_to":{"atom":"$2","order":"single"}}
    ])
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

#[test]
fn cli_layout_apply_commits_undoes_exactly_and_refuses_a_stale_revision() {
    let root = tempfile::tempdir().unwrap();
    let mut cli = Cli::start(root.path());
    let created = cli.call(request("create", "document.create", json!({})));
    let id = created["result"]["document"]["id"].as_str().unwrap();
    let build = json!({"document_id": id, "expected_revision": 0, "commands": butane()});
    let built = cli.call(request("butane", "document.transaction", build));
    assert_eq!(built["outcome"], "ok", "{built}");
    let before = positions(&built["result"]["document"]);

    let preview = json!({"document_id": id, "mode": "preview", "selection": {"atoms": ["a2"]}});
    let previewed = cli.call(request("preview", "layout.structure", preview));
    assert_eq!(previewed["outcome"], "ok", "{previewed}");
    let proposed: Vec<(&str, [f64; 2])> = previewed["result"]["positions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| (entry["atom"].as_str().unwrap(), point(&entry["after"])))
        .collect();
    assert_eq!(proposed.len(), 4, "{previewed}");

    let apply = json!({"document_id": id, "mode": "apply", "expected_revision": 1, "selection": {"atoms": ["a2"]}});
    let applied = cli.call(request("apply", "layout.structure", apply));
    assert_eq!(applied["outcome"], "ok", "{applied}");
    assert_eq!(applied["document_revision"], 2, "{applied}");
    let after = positions(&applied["result"]["document"]);
    assert_near(&after, &proposed, 1e-9, "the preview");
    assert_near(&after, &WORKED, 0.01, "the worked example");

    let undo = json!({"document_id": id, "expected_revision": 2});
    let undone = cli.call(request("undo", "history.undo", undo));
    assert_eq!(undone["outcome"], "ok", "{undone}");
    assert_eq!(undone["document_revision"], 3, "{undone}");
    let restored = positions(&undone["result"]["document"]);
    assert_eq!(restored, before, "{undone}");

    // A client that still holds revision 2 is refused, and the positions stay as undo left them.
    let stale = json!({"document_id": id, "mode": "apply", "expected_revision": 2, "selection": {"atoms": ["a2"]}});
    let refused = cli.call(request("stale", "layout.structure", stale));
    assert_eq!(refused["outcome"], "error", "{refused}");
    assert_eq!(
        refused["diagnostics"][0]["code"], "revision_conflict",
        "{refused}"
    );
    assert_eq!(refused["document_revision"], 3, "{refused}");
    let snapshot = cli.call(request(
        "read",
        "document.snapshot",
        json!({"document_id": id}),
    ));
    let now = positions(&snapshot["result"]["document"]);
    assert_eq!(now, before, "{snapshot}");
    cli.finish();
}
