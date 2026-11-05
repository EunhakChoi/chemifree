//! Native graphics clipboard through the operation boundary: ordered fresh forests and atomic refusals.

use std::sync::atomic::{AtomicU64, Ordering};

use chemifree_application::Application;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};

type App = Application<LocalCapabilities, FileDocumentRepository>;

fn application() -> (App, tempfile::TempDir) {
    let directory = tempfile::tempdir().unwrap();
    let app = Application::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    );
    (app, directory)
}

fn request(app: &App, request_id: &str, operation: &str, parameters: Value) -> Value {
    let body = json!({"contract":1,"request_id":request_id,"profile":"preserve-semantics",
        "operation":operation,"parameters":parameters});
    serde_json::to_value(app.execute(body.to_string().as_bytes())).unwrap()
}

fn call(app: &App, operation: &str, parameters: Value) -> Value {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    request(
        app,
        &format!("scene-clipboard-{}", NEXT.fetch_add(1, Ordering::Relaxed)),
        operation,
        parameters,
    )
}

fn create(app: &App, title: &str) -> String {
    let result = call(app, "document.create", json!({"title":title}));
    assert_eq!(result["outcome"], "ok", "{result}");
    result["result"]["document"]["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn snapshot(app: &App, id: &str) -> Value {
    let result = call(app, "document.snapshot", json!({"document_id":id}));
    assert_eq!(result["outcome"], "ok", "{result}");
    result["result"].clone()
}

fn edit(app: &App, id: &str, commands: Value) -> Value {
    let revision = snapshot(app, id)["document"]["revision"].clone();
    let result = call(
        app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":revision,"commands":commands}),
    );
    assert_eq!(result["outcome"], "ok", "{result}");
    result["result"].clone()
}

fn graph_commands() -> Vec<Value> {
    vec![
        json!({"op":"add_atom","element":"C","position":{"x":0.0,"y":0.0}}),
        json!({"op":"add_atom","element":"O","position":{"x":30.0,"y":0.0},"bond_to":{"atom":"a1","order":"double"}}),
        json!({"op":"set_bond_mark","bond":"b1","mark":{"kind":"crossed"}}),
        json!({"op":"set_bond_configuration","bond":"b1","configuration":{"kind":"unknown"}}),
    ]
}

fn graphic(shape: &str, first: (f64, f64), second: (f64, f64)) -> Value {
    json!({"op":"add_graphic","shape":shape,"points":[{"x":first.0,"y":first.1},{"x":second.0,"y":second.1}]})
}

fn source(app: &App) -> String {
    let id = create(app, "Source graphic forest");
    let mut commands = graph_commands();
    commands.extend([
        graphic("line", (0.1, -2.0), (30.0, 4.0)),
        graphic("arrow", (5.0, 6.0), (7.0, 8.0)),
        graphic("rectangle", (20.0, 25.0), (10.0, 15.0)),
        json!({"op":"group","objects":["s2","s3"]}),
        json!({"op":"group","objects":["s5","s4"]}),
        graphic("line", (40.0, 50.0), (60.0, 70.0)),
        graphic("line", (-10.0, -20.0), (-30.0, -40.0)),
        json!({"op":"add_reference","source":"s2","target":{"kind":"object","id":"s2"}}),
        json!({"op":"add_reference","source":"s2","target":{"kind":"object","id":"s3"}}),
        json!({"op":"add_reference","source":"s3","target":{"kind":"object","id":"s2"}}),
        json!({"op":"add_reference","source":"s6","target":{"kind":"object","id":"s2"}}),
        json!({"op":"add_reference","source":"s7","target":{"kind":"object","id":"s5"}}),
        json!({"op":"add_reference","source":"s8","target":{"kind":"object","id":"s2"}}),
    ]);
    edit(app, &id, json!(commands));
    id
}

fn destination(app: &App) -> String {
    let id = create(app, "Destination graph retained");
    let mut commands = graph_commands();
    commands.extend([
        json!({"op":"add_instance","source":"s1","offset":{"x":10.0,"y":20.0}}),
        json!({"op":"add_reference","source":"s1","target":{"kind":"atom","id":"a1"}}),
    ]);
    edit(app, &id, json!(commands));
    id
}

fn copied(app: &App, id: &str) -> Value {
    let result = call(
        app,
        "document.clipboard_scene_copy",
        json!({"document_id":id,
        "selection":{"objects":["s7","s2","s6","s7"]}}),
    );
    assert_eq!(result["outcome"], "ok", "{result}");
    assert_eq!(result["loss_report"], json!([]));
    result["result"].clone()
}

fn paste_parameters(id: &str, revision: u64, payload: &str) -> Value {
    json!({"document_id":id,"expected_revision":revision,"payload":payload,"offset":{"x":100.0,"y":200.0}})
}

fn object<'a>(snapshot: &'a Value, id: &str) -> &'a Value {
    snapshot["document"]["scene"]["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|object| object["id"] == id)
        .unwrap()
}

fn content(snapshot: &Value) -> Value {
    let mut document = snapshot["document"].clone();
    document.as_object_mut().unwrap().remove("revision");
    document
}

fn assert_refusal(result: &Value, reason: &str) {
    assert_eq!(result["outcome"], "error", "{result}");
    assert_eq!(result["result"], Value::Null, "{result}");
    assert_eq!(
        result["diagnostics"][0]["code"], "invalid_command",
        "{result}"
    );
    assert_eq!(
        result["diagnostics"][0]["details"]["reason"], reason,
        "{result}"
    );
}

#[test]
fn scene_copy_is_read_only_deduplicates_subtrees_and_keeps_paint_order_and_internal_links() {
    let (app, _directory) = application();
    let id = source(&app);
    let before = snapshot(&app, &id);
    let result = copied(&app, &id);
    assert_eq!(result["snapshot"], before);
    assert_eq!(result["roots"], json!(["s6", "s7"]));
    assert_eq!(
        result["copied"],
        json!({"objects":["s6","s5","s2","s3","s4","s7"],"atoms":[],"bonds":[]})
    );
    let payload: Value = serde_json::from_str(result["payload"].as_str().unwrap()).unwrap();
    assert_eq!(payload["format"], "chemifree.clipboard");
    assert_eq!(payload["version"], 2);
    assert_eq!(payload["kind"], "graphics");
    assert_eq!(payload["settings"], before["document"]["settings"]);
    assert_eq!(payload["roots"], json!(["s6", "s7"]));
    assert_eq!(
        payload["objects"][0],
        json!({"kind":"group","id":"s6","children":["s5","s4"]})
    );
    assert_eq!(
        payload["objects"][1],
        json!({"kind":"group","id":"s5","children":["s2","s3"]})
    );
    assert_eq!(
        payload["references"],
        json!([
            {"source":"s2","target":{"kind":"object","id":"s2"}},
            {"source":"s2","target":{"kind":"object","id":"s3"}},
            {"source":"s3","target":{"kind":"object","id":"s2"}},
            {"source":"s6","target":{"kind":"object","id":"s2"}},
            {"source":"s7","target":{"kind":"object","id":"s5"}}
        ])
    );
    assert_eq!(
        snapshot(&app, &id),
        before,
        "outside-owned incoming reference and all source content remain untouched"
    );
}

#[test]
fn paste_maps_nonmonotonic_root_ids_without_touching_graph_placements_stereo_or_style() {
    let (app, _directory) = application();
    let source_id = source(&app);
    let source_before = snapshot(&app, &source_id);
    let copy = copied(&app, &source_id);
    let id = destination(&app);
    let before = snapshot(&app, &id);
    let pasted = call(
        &app,
        "document.clipboard_scene_paste",
        paste_parameters(&id, 1, copy["payload"].as_str().unwrap()),
    );
    assert_eq!(pasted["outcome"], "ok", "{pasted}");
    assert_eq!(pasted["loss_report"], json!([]));
    assert_eq!(
        pasted["result"]["mapping"],
        json!({"objects":[
            {"source":"s6","target":"s8"},{"source":"s5","target":"s7"},
            {"source":"s2","target":"s3"},{"source":"s3","target":"s4"},
            {"source":"s4","target":"s5"},{"source":"s7","target":"s6"}
        ]})
    );
    let after = &pasted["result"]["snapshot"];
    assert_eq!(after["document"]["revision"], 2);
    assert_eq!(after["notes"], json!([]));
    assert_eq!(
        after["document"]["scene"]["roots"],
        json!(["s1", "s2", "s8", "s6"]),
        "painting order is not identifier order"
    );
    assert_eq!(
        after["document"]["selection"],
        json!({"objects":["s6","s8"],"atoms":[],"bonds":[]}),
        "stored selection uses canonical identifier order"
    );
    assert_eq!(
        object(after, "s8"),
        &json!({"kind":"group","id":"s8","parent":null,"children":["s7","s5"]})
    );
    assert_eq!(
        object(after, "s7"),
        &json!({"kind":"group","id":"s7","parent":"s8","children":["s3","s4"]})
    );
    assert_eq!(
        object(after, "s3"),
        &json!({"kind":"graphic","id":"s3","parent":"s7","shape":"line","points":[{"x":100.1,"y":198.0},{"x":130.0,"y":204.0}]})
    );
    assert_eq!(
        object(after, "s4")["points"],
        json!([{"x":105.0,"y":206.0},{"x":107.0,"y":208.0}])
    );
    assert_eq!(
        object(after, "s5")["points"],
        json!([{"x":120.0,"y":225.0},{"x":110.0,"y":215.0}]),
        "opposite-corner identities remain in their source order"
    );
    assert_eq!(object(after, "s6")["parent"], Value::Null);
    assert_eq!(
        after["document"]["scene"]["references"],
        json!([
            {"source":"s1","target":{"kind":"atom","id":"a1"}},
            {"source":"s3","target":{"kind":"object","id":"s3"}},
            {"source":"s3","target":{"kind":"object","id":"s4"}},
            {"source":"s4","target":{"kind":"object","id":"s3"}},
            {"source":"s6","target":{"kind":"object","id":"s7"}},
            {"source":"s8","target":{"kind":"object","id":"s3"}}
        ])
    );
    for field in ["atoms", "bonds", "stereo", "settings"] {
        assert_eq!(
            after["document"][field], before["document"][field],
            "{field}"
        );
    }
    assert_eq!(
        after["document"]["scene"]["instances"],
        before["document"]["scene"]["instances"]
    );
    assert_eq!(snapshot(&app, &source_id), source_before);
}

#[test]
fn one_selection_aware_history_step_replays_immutably_and_persists_forest_and_id_counter() {
    let (app, directory) = application();
    let source_id = source(&app);
    let copy = copied(&app, &source_id);
    let id = destination(&app);
    let before = snapshot(&app, &id);
    let history_selection = json!({"objects":["s1"],"atoms":["a1"],"bonds":["b1"]});
    let mut parameters = paste_parameters(&id, 1, copy["payload"].as_str().unwrap());
    parameters["selection_before"] = history_selection.clone();
    let pasted = request(
        &app,
        "scene-paste-once",
        "document.clipboard_scene_paste",
        parameters.clone(),
    );
    assert_eq!(pasted["outcome"], "ok", "{pasted}");
    let after = &pasted["result"]["snapshot"];
    assert_eq!(after["session"]["undo_depth"], 2);
    let undone = call(
        &app,
        "history.undo",
        json!({"document_id":id,"expected_revision":2}),
    );
    assert_eq!(undone["outcome"], "ok", "{undone}");
    let mut expected_before = content(&before);
    expected_before["selection"] = history_selection;
    assert_eq!(
        content(&undone["result"]),
        expected_before,
        "one undo removes every pasted object, group and reference"
    );
    let redone = call(
        &app,
        "history.redo",
        json!({"document_id":id,"expected_revision":3}),
    );
    assert_eq!(redone["outcome"], "ok", "{redone}");
    assert_eq!(content(&redone["result"]), content(after));
    edit(
        &app,
        &id,
        json!([
            graphic("arrow", (500.0, 600.0), (510.0, 610.0)),
            {"op":"rename","title":"Later scene edit"}
        ]),
    );
    let later = snapshot(&app, &id);
    assert_eq!(object(&later, "s9")["shape"], "arrow");
    assert_eq!(
        request(
            &app,
            "scene-paste-once",
            "document.clipboard_scene_paste",
            parameters.clone()
        ),
        pasted
    );
    assert_eq!(
        snapshot(&app, &id),
        later,
        "mapping must be rebuilt from immutable replay response, not live IDs or selection"
    );
    parameters["offset"]["x"] = json!(101.0);
    let reused = request(
        &app,
        "scene-paste-once",
        "document.clipboard_scene_paste",
        parameters,
    );
    assert_eq!(reused["outcome"], "error");
    assert_eq!(reused["diagnostics"][0]["code"], "request_id_reused");
    assert_eq!(snapshot(&app, &id), later);
    let saved = call(
        &app,
        "document.save",
        json!({"document_id":id,"expected_revision":5}),
    );
    assert_eq!(saved["outcome"], "ok", "{saved}");
    let closed = call(
        &app,
        "document.close",
        json!({"document_id":id,"expected_revision":5}),
    );
    assert_eq!(closed["outcome"], "ok", "{closed}");
    let reopened = Application::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    );
    let restored = snapshot(&reopened, &id);
    assert_eq!(restored["document"], saved["result"]["document"]);
    assert_eq!(
        restored["session"]["undo_depth"],
        saved["result"]["session"]["undo_depth"]
    );
    let advanced = edit(
        &reopened,
        &id,
        json!([graphic("line", (700.0, 800.0), (710.0, 810.0))]),
    );
    assert_eq!(
        object(&advanced, "s10")["shape"],
        "line",
        "reopen must retain the fresh object counter"
    );
}

#[test]
fn malformed_wrong_profile_style_and_coordinate_refusals_preserve_destination_and_redo() {
    let (app, _directory) = application();
    let source_id = source(&app);
    let copied = copied(&app, &source_id);
    let payload = copied["payload"].as_str().unwrap();
    let id = destination(&app);
    edit(
        &app,
        &id,
        json!([{"op":"rename","title":"Undo this rename"}]),
    );
    let undone = call(
        &app,
        "history.undo",
        json!({"document_id":id,"expected_revision":2}),
    );
    assert_eq!(undone["outcome"], "ok", "{undone}");
    let before = snapshot(&app, &id);
    assert_eq!(before["session"]["redo_depth"], 1);
    let mut injected: Value = serde_json::from_str(payload).unwrap();
    injected["commands"] = json!([{"op":"delete_atoms","atoms":["a1"]}]);
    let mut styled: Value = serde_json::from_str(payload).unwrap();
    styled["settings"]["line_width_pt"] = json!(2.0);
    let duplicate_key = format!("{{\"kind\":\"graphics\",{}", &payload[1..]);
    let graph_copy = call(
        &app,
        "document.clipboard_copy",
        json!({"document_id":source_id,"instance":"s1","selection":{"atoms":["a1","a2"]}}),
    );
    assert_eq!(graph_copy["outcome"], "ok", "{graph_copy}");
    for (text, reason, offset) in [
        (
            injected.to_string(),
            "clipboard_fragment_invalid",
            json!({"x":100.0,"y":200.0}),
        ),
        (
            duplicate_key,
            "clipboard_fragment_invalid",
            json!({"x":100.0,"y":200.0}),
        ),
        (
            graph_copy["result"]["payload"].as_str().unwrap().to_owned(),
            "clipboard_format_unsupported",
            json!({"x":100.0,"y":200.0}),
        ),
        (
            styled.to_string(),
            "clipboard_scene_style_mismatch",
            json!({"x":100.0,"y":200.0}),
        ),
        (
            payload.to_owned(),
            "clipboard_scene_fragment_invalid",
            json!({"x":100000.0,"y":0.0}),
        ),
    ] {
        let mut parameters = paste_parameters(&id, 3, &text);
        parameters["offset"] = offset;
        let result = call(&app, "document.clipboard_scene_paste", parameters);
        assert_refusal(&result, reason);
        assert_eq!(
            snapshot(&app, &id),
            before,
            "{reason}: coordinates, counters, selection and redo must remain unchanged"
        );
    }
    let graph_paste = call(
        &app,
        "document.clipboard_paste",
        paste_parameters(&id, 3, payload),
    );
    assert_refusal(&graph_paste, "clipboard_format_unsupported");
    assert_eq!(snapshot(&app, &id), before);
}

#[test]
fn scene_copy_refuses_graph_mixed_instance_outgoing_unknown_and_empty_selections_atomically() {
    let (app, _directory) = application();
    let id = source(&app);
    let before = snapshot(&app, &id);
    for (selection, reason) in [
        (
            json!({"atoms":["a1"]}),
            "clipboard_scene_graph_selection_unsupported",
        ),
        (
            json!({"objects":["s6"],"bonds":["b1"]}),
            "clipboard_scene_graph_selection_unsupported",
        ),
        (
            json!({"objects":["s1"]}),
            "clipboard_scene_instance_unsupported",
        ),
        (
            json!({"objects":["s2"]}),
            "clipboard_scene_reference_unsupported",
        ),
        (
            json!({"objects":["s99"]}),
            "clipboard_scene_selection_unknown",
        ),
        (json!({}), "clipboard_scene_empty_selection"),
    ] {
        let result = call(
            &app,
            "document.clipboard_scene_copy",
            json!({"document_id":id,"selection":selection}),
        );
        assert_refusal(&result, reason);
        assert_eq!(snapshot(&app, &id), before, "{reason}");
    }
    let unknown_parameter = call(
        &app,
        "document.clipboard_scene_copy",
        json!({"document_id":id,"selection":{"objects":["s6","s7"]},"instance":"s1"}),
    );
    assert_eq!(unknown_parameter["outcome"], "error");
    assert_eq!(
        unknown_parameter["diagnostics"][0]["code"],
        "invalid_request"
    );
    assert_eq!(snapshot(&app, &id), before);
}

#[test]
fn scene_copy_refuses_existing_groups_reduced_to_one_or_zero_children() {
    let (app, _directory) = application();
    for removed in [json!(["s2"]), json!(["s2", "s3"])] {
        let id = create(&app, "Reduced source group");
        edit(
            &app,
            &id,
            json!([
                graphic("line", (0.0, 0.0), (10.0, 0.0)),
                graphic("arrow", (20.0, 0.0), (30.0, 0.0)),
                {"op":"group","objects":["s2","s3"]},
                {"op":"delete_objects","objects":removed,"mode":"subtree"}
            ]),
        );
        let before = snapshot(&app, &id);
        let result = call(
            &app,
            "document.clipboard_scene_copy",
            json!({"document_id":id,"selection":{"objects":["s4"]}}),
        );
        assert_refusal(&result, "clipboard_scene_group_too_small");
        assert_eq!(snapshot(&app, &id), before);
    }
}
