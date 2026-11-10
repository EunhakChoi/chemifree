//! The analyses and the MOL export read their document without its undo and redo history, and
//! their replies still state the session's history. `chem.properties`, `chem.stereo` and
//! `document.export` with `format: "mol"` each read a copy that `Document::without_history` makes
//! under the session table. `save_as` copies its working copy the same way, so the copy starts with
//! no history.
//!
//! Each test first draws the same drawing by requests, so that both stacks are non-empty when the
//! analysis runs. The expected values are written out here, from the chemistry (ethanol, C2H6O) and
//! from the sequence of requests, not from the code under test. Every request has its own request
//! ID, so that no reply is replayed from another request's record.

use chemifree_application::Application;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities, V2000Codec};
use serde_json::{Value, json};

type App = Application<LocalCapabilities, FileDocumentRepository>;

/// The title, program and comment lines that come before the counts line of a V2000 connection
/// table.
const MOL_HEADER_LINES: usize = 3;

/// The revision and the undo and redo depths that a document view or snapshot states.
#[derive(Debug, PartialEq)]
struct DocumentState {
    revision: u64,
    undo_depth: u64,
    redo_depth: u64,
}

fn new_app(directory: &tempfile::TempDir) -> App {
    Application::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    )
    .with_ctfile_codec(V2000Codec)
}

fn call(app: &App, request_id: &str, operation: &str, parameters: Value) -> Value {
    let request = json!({
        "contract": 1,
        "request_id": request_id,
        "profile": "preserve-semantics",
        "operation": operation,
        "parameters": parameters,
    });
    serde_json::to_value(app.execute(request.to_string().as_bytes())).unwrap()
}

/// The `document.snapshot` reply of `id`.
fn snapshot(app: &App, request_id: &str, id: &str) -> Value {
    let reply = call(
        app,
        request_id,
        "document.snapshot",
        json!({"document_id": id}),
    );
    assert_eq!(reply["outcome"], "ok", "{reply}");
    reply
}

/// The state that a view states. A snapshot in a reply and a copy reply both hold `document` and
/// `session` objects.
fn state_of(view: &Value) -> DocumentState {
    DocumentState {
        revision: view["document"]["revision"].as_u64().unwrap(),
        undo_depth: view["session"]["undo_depth"].as_u64().unwrap(),
        redo_depth: view["session"]["redo_depth"].as_u64().unwrap(),
    }
}

/// Asserts that `view` states `expected`. `label` names the view in a failure.
fn assert_state(view: &Value, expected: &DocumentState, label: &str) {
    assert_eq!(
        state_of(view),
        *expected,
        "{label} states a different revision or history from the drawing's"
    );
}

/// The ID of the only atom of `element` at horizontal position `x` in a document view. Atoms are
/// found by what they are, not by where they sit in the list.
fn atom_at(document: &Value, element: &str, x: f64) -> String {
    let found: Vec<&Value> = document["atoms"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|atom| atom["element"] == element && atom["position"]["x"].as_f64() == Some(x))
        .collect();
    let [atom] = found.as_slice() else {
        let count = found.len();
        panic!("expected one {element} atom at x = {x}, found {count}");
    };
    atom["id"].as_str().unwrap().to_owned()
}

/// The IDs of the atoms of a document view, in the order the view lists them.
fn atom_ids(document: &Value) -> Vec<String> {
    document["atoms"]
        .as_array()
        .unwrap()
        .iter()
        .map(|atom| atom["id"].as_str().unwrap().to_owned())
        .collect()
}

/// The element symbols of the atoms of a document view, sorted.
fn element_symbols(document: &Value) -> Vec<String> {
    let mut symbols: Vec<String> = document["atoms"]
        .as_array()
        .unwrap()
        .iter()
        .map(|atom| atom["element"].as_str().unwrap().to_owned())
        .collect();
    symbols.sort();
    symbols
}

/// The atom count and the element symbols of the atom block of a V2000 connection table. The
/// counts line follows the header, and its first three columns hold the atom count. Each atom
/// line holds its element symbol in columns 32 to 34.
fn mol_atom_block(content: &str) -> (usize, Vec<String>) {
    let mut lines = content.lines().skip(MOL_HEADER_LINES);
    let counts = lines.next().expect("the MOL export has a counts line");
    let count: usize = counts[..3]
        .trim()
        .parse()
        .unwrap_or_else(|error| panic!("the atom count of {counts:?}: {error}"));
    let symbols = lines
        .take(count)
        .map(|line| line[31..34].trim().to_owned())
        .collect();
    (count, symbols)
}

/// Draws ethanol-like C-C-O in two transactions, undoes and redoes the second, then makes one more
/// edit and undoes it. Returns the document's ID and the state that `document.snapshot` reports
/// after the sequence.
fn ethanol_with_history(app: &App) -> (String, DocumentState) {
    let created = call(
        app,
        "create",
        "document.create",
        json!({"title": "ethanol"}),
    );
    assert_eq!(created["outcome"], "ok", "{created}");
    let id = created["result"]["document"]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    // Two carbons and the bond between them. `$0` and `$1` name the atoms that this transaction
    // adds first and second.
    let carbons = call(
        app,
        "draw-carbons",
        "document.transaction",
        json!({
            "document_id": id,
            "expected_revision": 0,
            "commands": [
                {"op": "add_atom", "element": "C", "position": {"x": 0, "y": 0}},
                {"op": "add_atom", "element": "C", "position": {"x": 20, "y": 0}},
                {"op": "add_bond", "begin": "$0", "end": "$1", "order": "single"},
            ],
        }),
    );
    assert_eq!(carbons["outcome"], "ok", "{carbons}");
    let second_carbon = atom_at(&carbons["result"]["document"], "C", 20.0);

    // The oxygen, bonded to that carbon.
    let oxygen = call(
        app,
        "draw-oxygen",
        "document.transaction",
        json!({
            "document_id": id,
            "expected_revision": carbons["document_revision"],
            "commands": [
                {"op": "add_atom", "element": "O", "position": {"x": 40, "y": 0}},
                {"op": "add_bond", "begin": second_carbon, "end": "$0", "order": "single"},
            ],
        }),
    );
    assert_eq!(oxygen["outcome"], "ok", "{oxygen}");

    let undone = call(
        app,
        "undo-oxygen",
        "history.undo",
        json!({"document_id": id, "expected_revision": oxygen["document_revision"]}),
    );
    assert_eq!(undone["outcome"], "ok", "{undone}");
    let redone = call(
        app,
        "redo-oxygen",
        "history.redo",
        json!({"document_id": id, "expected_revision": undone["document_revision"]}),
    );
    assert_eq!(redone["outcome"], "ok", "{redone}");

    // One more edit, which is then undone, so the redo stack holds it.
    let nitrogen = call(
        app,
        "draw-nitrogen",
        "document.transaction",
        json!({
            "document_id": id,
            "expected_revision": redone["document_revision"],
            "commands": [
                {"op": "add_atom", "element": "N", "position": {"x": 60, "y": -20}},
            ],
        }),
    );
    assert_eq!(nitrogen["outcome"], "ok", "{nitrogen}");
    let undone_again = call(
        app,
        "undo-nitrogen",
        "history.undo",
        json!({"document_id": id, "expected_revision": nitrogen["document_revision"]}),
    );
    assert_eq!(undone_again["outcome"], "ok", "{undone_again}");

    let state = state_of(&snapshot(app, "state", &id)["result"]);
    (id, state)
}

#[test]
fn the_drawing_has_both_stacks_non_empty_before_each_analysis() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let (id, state) = ethanol_with_history(&app);
    assert_eq!(
        state,
        DocumentState {
            revision: 6,
            undo_depth: 2,
            redo_depth: 1,
        },
        "the sequence of requests leaves revision 6, two undo entries and one redo entry"
    );
    let view = snapshot(&app, "check", &id);
    assert_eq!(
        element_symbols(&view["result"]["document"]),
        ["C", "C", "O"],
        "the drawing is C-C-O"
    );
    assert_eq!(
        view["result"]["document"]["bonds"]
            .as_array()
            .unwrap()
            .len(),
        2,
        "the drawing has two bonds"
    );
}

#[test]
fn properties_states_the_session_history_and_the_formula_of_the_drawing() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let (id, recorded) = ethanol_with_history(&app);
    let reply = call(
        &app,
        "properties",
        "chem.properties",
        json!({"document_id": id, "scope": "molecule", "property": "formula"}),
    );
    assert_eq!(reply["outcome"], "ok", "{reply}");
    assert_eq!(reply["document_revision"], recorded.revision, "{reply}");
    assert_state(
        &reply["result"]["snapshot"],
        &recorded,
        "chem.properties snapshot",
    );
    let components = reply["result"]["components"].as_array().unwrap();
    let [component] = components.as_slice() else {
        panic!("one molecule is one component, got {reply}");
    };
    assert_eq!(component["formula"], "C2H6O", "{reply}");
    assert_eq!(component["atoms"].as_array().unwrap().len(), 3, "{reply}");
}

#[test]
fn stereo_states_the_session_history_for_a_drawing_without_stereo() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let (id, recorded) = ethanol_with_history(&app);
    let reply = call(&app, "stereo", "chem.stereo", json!({"document_id": id}));
    assert_eq!(reply["outcome"], "ok", "{reply}");
    assert_eq!(reply["document_revision"], recorded.revision, "{reply}");
    let view = &reply["result"]["snapshot"];
    assert_state(view, &recorded, "chem.stereo snapshot");
    // The instances that the analysis read are the scene instances that the snapshot states.
    let scene_instances: Vec<Value> = view["document"]["scene"]["instances"]
        .as_array()
        .unwrap()
        .iter()
        .map(|instance| instance["id"].clone())
        .collect();
    assert_eq!(
        reply["result"]["instances"],
        Value::Array(scene_instances),
        "{reply}"
    );
}

#[test]
fn mol_export_states_the_session_history_and_writes_every_atom() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let (id, recorded) = ethanol_with_history(&app);
    let reply = call(
        &app,
        "export",
        "document.export",
        json!({"format": "mol", "records": [{"document_id": id}]}),
    );
    // A V2000 export is `partial` for a drawing like this one: the file keeps no native IDs,
    // placement ID or drawing settings, and its default hydrogen fields do not fix the implicit
    // hydrogen count. These four losses are the same whatever the history is.
    assert_eq!(reply["outcome"], "partial", "{reply}");
    let mut features: Vec<&str> = reply["loss_report"]
        .as_array()
        .unwrap()
        .iter()
        .map(|loss| loss["feature"].as_str().unwrap())
        .collect();
    features.sort_unstable();
    assert_eq!(
        features,
        [
            "default_hydrogen_interpretation",
            "drawing_settings",
            "native_ids",
            "placement_id",
        ],
        "{reply}"
    );
    let result = &reply["result"];
    let records = result["records"].as_array().unwrap();
    let [record] = records.as_slice() else {
        panic!("one MOL export has one analyzed copy, got {reply}");
    };
    assert_state(
        &record["snapshot"],
        &recorded,
        "document.export record snapshot",
    );
    let (atom_count, mut symbols) = mol_atom_block(result["content"].as_str().unwrap());
    assert_eq!(
        atom_count, 3,
        "the counts line names the atom count: {reply}"
    );
    symbols.sort();
    assert_eq!(
        symbols,
        ["C", "C", "O"],
        "the atom block holds the drawn atoms"
    );
    assert_eq!(record["atoms"].as_array().unwrap().len(), 3, "{reply}");
}

#[test]
fn save_as_copy_starts_without_history_and_keeps_every_atom() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let (id, recorded) = ethanol_with_history(&app);
    let source = snapshot(&app, "source", &id);
    let source_atoms = atom_ids(&source["result"]["document"]);
    let copy = serde_json::to_value(
        app.save_as(&id, Some("save-as"), recorded.revision, None)
            .unwrap(),
    )
    .unwrap();
    assert_state(
        &copy,
        &DocumentState {
            revision: 0,
            undo_depth: 0,
            redo_depth: 0,
        },
        "the save_as copy",
    );
    assert_eq!(copy["document"]["title"], "ethanol (copy)", "{copy}");
    assert_ne!(
        copy["document"]["id"].as_str(),
        Some(id.as_str()),
        "the copy is a new document: {copy}"
    );
    let copy_atoms = atom_ids(&copy["document"]);
    assert_eq!(copy_atoms.len(), 3, "the copy's atom count: {copy}");
    assert_eq!(
        copy_atoms, source_atoms,
        "the copy holds the source's atoms: {copy}"
    );
    assert_eq!(copy["source"]["id"], id.as_str(), "{copy}");
    assert_eq!(copy["source"]["closed"], true, "{copy}");
    assert_eq!(copy["source"]["discarded_changes"], true, "{copy}");
}
