//! Versioned graphic clipboard data preserves overrides without weakening legacy parsing.

use std::sync::atomic::{AtomicU64, Ordering};

use chemifree_application::Application;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};

type App = Application<LocalCapabilities, FileDocumentRepository>;
fn send(app: &App, id: &str, operation: &str, parameters: Value) -> Value {
    serde_json::to_value(app.execute(json!({"contract":1,"request_id":id,"profile":"preserve-semantics","operation":operation,"parameters":parameters}).to_string().as_bytes())).unwrap()
}
fn call(app: &App, operation: &str, parameters: Value) -> Value {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    send(
        app,
        &format!("v3-{}", NEXT.fetch_add(1, Ordering::Relaxed)),
        operation,
        parameters,
    )
}
fn create(app: &App) -> String {
    call(app, "document.create", json!({"title":"Clipboard v3"}))["result"]["document"]["id"]
        .as_str()
        .unwrap()
        .into()
}
fn snapshot(app: &App, id: &str) -> Value {
    call(app, "document.snapshot", json!({"document_id":id}))["result"].clone()
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
fn source(app: &App, styled: bool) -> String {
    let id = create(app);
    edit(
        app,
        &id,
        json!([
         {"op":"add_atom","element":"C","position":{"x":1,"y":2}},
         {"op":"add_graphic","shape":"line","points":[{"x":0,"y":0},{"x":10,"y":5}]},
         {"op":"add_graphic","shape":"arrow","points":[{"x":20,"y":0},{"x":30,"y":5}]},
         {"op":"group","objects":["s2","s3"]},
         {"op":"add_reference","source":"s2","target":{"kind":"object","id":"s3"}}
        ]),
    );
    if styled {
        edit(
            app,
            &id,
            json!([{"op":"set_graphic_line_width","object":"s2","line_width_pt":2.75}]),
        );
    }
    id
}
fn copy(app: &App, id: &str, root: &str) -> Value {
    let result = call(
        app,
        "document.clipboard_scene_copy",
        json!({"document_id":id,"selection":{"objects":[root]}}),
    );
    assert_eq!(result["outcome"], "ok", "{result}");
    result["result"].clone()
}
fn graphic(snapshot: &Value, id: &str) -> Value {
    snapshot["document"]["scene"]["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|object| object["id"] == id)
        .unwrap()
        .clone()
}
fn payload(copy: &Value) -> Value {
    serde_json::from_str(copy["payload"].as_str().unwrap()).unwrap()
}
fn app() -> (App, tempfile::TempDir) {
    let root = tempfile::tempdir().unwrap();
    (
        Application::new(LocalCapabilities, FileDocumentRepository::new(root.path())),
        root,
    )
}

#[test]
fn mixed_width_forest_uses_v3_and_paste_replay_history_retain_graph_refs_offsets_and_native26() {
    let (app, _root) = app();
    let source = source(&app, true);
    let exact_source = snapshot(&app, &source);
    let copied = copy(&app, &source, "s4");
    assert_eq!(snapshot(&app, &source), exact_source);
    let wire = payload(&copied);
    assert_eq!(wire["version"], 3);
    for object in wire["objects"].as_array().unwrap() {
        if object["kind"] == "graphic" {
            assert!(object.get("line_width_pt").is_some());
        } else {
            assert!(object.get("line_width_pt").is_none());
        }
    }
    let id = create(&app);
    let before = edit(
        &app,
        &id,
        json!([{ "op":"add_atom","element":"N","position":{"x":7,"y":8}},{"op":"add_graphic","shape":"rectangle","points":[{"x":100,"y":100},{"x":110,"y":110}]},{"op":"set_graphic_line_width","object":"s2","line_width_pt":10},{"op":"add_reference","source":"s2","target":{"kind":"atom","id":"a1"}}]),
    );
    let parameters = json!({"document_id":id,"expected_revision":1,"payload":copied["payload"],"offset":{"x":50,"y":-20}});
    let pasted = send(
        &app,
        "paste-styled-once",
        "document.clipboard_scene_paste",
        parameters.clone(),
    );
    assert_eq!(pasted["outcome"], "ok", "{pasted}");
    assert_eq!(
        send(
            &app,
            "paste-styled-once",
            "document.clipboard_scene_paste",
            parameters
        ),
        pasted
    );
    assert_eq!(pasted["loss_report"], json!([]));
    let after = &pasted["result"]["snapshot"];
    assert_eq!(
        pasted["result"]["mapping"]["objects"],
        json!([{"source":"s4","target":"s5"},{"source":"s2","target":"s3"},{"source":"s3","target":"s4"}])
    );
    assert_eq!(graphic(after, "s3")["line_width_pt"], 2.75);
    assert_eq!(graphic(after, "s4")["line_width_pt"], Value::Null);
    assert_eq!(graphic(after, "s3")["parent"], "s5");
    assert_eq!(
        graphic(after, "s3")["points"],
        json!([{"x":50.0,"y":-20.0},{"x":60.0,"y":-15.0}])
    );
    assert_eq!(graphic(after, "s2"), graphic(&before, "s2"));
    for field in ["atoms", "bonds", "molecule", "stereo", "settings"] {
        assert_eq!(after["document"][field], before["document"][field]);
    }
    assert_eq!(
        after["document"]["scene"]["references"],
        json!([{"source":"s2","target":{"kind":"atom","id":"a1"}},{"source":"s3","target":{"kind":"object","id":"s4"}}])
    );
    let undone = call(
        &app,
        "history.undo",
        json!({"document_id":id,"expected_revision":2}),
    );
    assert_eq!(
        undone["result"]["document"]["scene"],
        before["document"]["scene"]
    );
    let redone = call(
        &app,
        "history.redo",
        json!({"document_id":id,"expected_revision":3}),
    );
    assert_eq!(graphic(&redone["result"], "s3")["line_width_pt"], 2.75);
    app.save(&id, None, 4).unwrap();
    app.close(&id, None, false, None).unwrap();
    assert_eq!(
        snapshot(&app, &id)["document"],
        redone["result"]["document"]
    );
}

#[test]
fn inherited_output_stays_exact_v2_and_v3_all_null_normalizes_only_when_copied_again() {
    let (app, _root) = app();
    let source = source(&app, false);
    let legacy = copy(&app, &source, "s4");
    assert_eq!(payload(&legacy)["version"], 2);
    edit(
        &app,
        &source,
        json!([{"op":"set_graphic_line_width","object":"s2","line_width_pt":1.2}]),
    );
    let styled = copy(&app, &source, "s4");
    assert_eq!(payload(&styled)["version"], 3);
    // Version selection is scoped to the copied forest, not unrelated document overrides.
    assert_eq!(payload(&copy(&app, &source, "s3"))["version"], 2);
    edit(
        &app,
        &source,
        json!([{"op":"set_graphic_line_width","object":"s2","line_width_pt":null}]),
    );
    assert_eq!(copy(&app, &source, "s4")["payload"], legacy["payload"]);
    let mut nulls = payload(&styled);
    for object in nulls["objects"].as_array_mut().unwrap() {
        if object["kind"] == "graphic" {
            object["line_width_pt"] = Value::Null;
        }
    }
    let target = create(&app);
    let result = call(
        &app,
        "document.clipboard_scene_paste",
        json!({"document_id":target,"expected_revision":0,"payload":nulls.to_string(),"offset":{"x":0,"y":0}}),
    );
    assert_eq!(result["outcome"], "ok", "{result}");
    let copied = payload(&copy(&app, &target, "s4"));
    assert_eq!(copied["version"], 2);
    for object in copied["objects"].as_array().unwrap() {
        assert!(object.get("line_width_pt").is_none());
    }
}

#[test]
fn v2_v3_field_presence_bounds_unknown_fields_and_future_headers_refuse_without_partial_paste() {
    let (app, _root) = app();
    let source = source(&app, true);
    let template = payload(&copy(&app, &source, "s4"));
    let target = create(&app);
    let original = snapshot(&app, &target);
    let graphics: Vec<usize> = template["objects"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .filter_map(|(i, object)| (object["kind"] == "graphic").then_some(i))
        .collect();
    let mut invalid = Vec::new();
    for index in graphics {
        let mut missing = template.clone();
        missing["objects"][index]
            .as_object_mut()
            .unwrap()
            .remove("line_width_pt");
        invalid.push((missing, "clipboard_fragment_invalid"));
        for width in [
            json!(false),
            json!("inherit"),
            json!([]),
            json!(0.099),
            json!(10.001),
        ] {
            let mut bad = template.clone();
            bad["objects"][index]["line_width_pt"] = width;
            invalid.push((bad, "clipboard_fragment_invalid"));
        }
    }
    for width in [Value::Null, json!(2.75)] {
        let mut old = template.clone();
        old["version"] = json!(2);
        for object in old["objects"].as_array_mut().unwrap() {
            object.as_object_mut().unwrap().remove("line_width_pt");
        }
        // Leave only the tested forbidden field; another graphic cannot mask acceptance.
        old["objects"][1]["line_width_pt"] = width;
        invalid.push((old, "clipboard_fragment_invalid"));
    }
    for version in [2, 3] {
        let mut ellipse = template.clone();
        ellipse["version"] = json!(version);
        if version == 2 {
            for object in ellipse["objects"].as_array_mut().unwrap() {
                object.as_object_mut().unwrap().remove("line_width_pt");
            }
        }
        ellipse["objects"][1]["shape"] = json!("ellipse");
        invalid.push((ellipse, "clipboard_fragment_invalid"));
    }
    let mut group_field = template.clone();
    group_field["objects"][0]["line_width_pt"] = Value::Null;
    invalid.push((group_field, "clipboard_fragment_invalid"));
    let mut unknown = template.clone();
    unknown["commands"] = json!([]);
    invalid.push((unknown, "clipboard_fragment_invalid"));
    let mut settings = template.clone();
    settings["settings"]["line_width_pt"] = json!(2.0);
    invalid.push((settings, "clipboard_scene_style_mismatch"));
    invalid.push((
        json!({"format":"chemifree.clipboard","version":5,"future_body":[]}),
        "clipboard_format_unsupported",
    ));
    for (raw, reason) in invalid {
        let result = call(
            &app,
            "document.clipboard_scene_paste",
            json!({"document_id":target,"expected_revision":0,"payload":raw.to_string(),"offset":{"x":0,"y":0}}),
        );
        assert_eq!(
            result["diagnostics"][0]["details"]["reason"], reason,
            "{result}"
        );
        assert_eq!(snapshot(&app, &target), original);
    }
    let duplicate = template.to_string().replace(
        "\"line_width_pt\":2.75",
        "\"line_width_pt\":2.75,\"line_width_pt\":null",
    );
    let result = call(
        &app,
        "document.clipboard_scene_paste",
        json!({"document_id":target,"expected_revision":0,"payload":duplicate,"offset":{"x":0,"y":0}}),
    );
    assert_eq!(
        result["diagnostics"][0]["details"]["reason"],
        "clipboard_fragment_invalid"
    );
    assert_eq!(snapshot(&app, &target), original);
}

#[test]
fn v3_keeps_the_existing_byte_boundary_and_copy_does_not_delete_before_cut_transaction() {
    let (app, _root) = app();
    let source = source(&app, true);
    let before = snapshot(&app, &source);
    let copied = copy(&app, &source, "s4");
    assert_eq!(snapshot(&app, &source), before);
    let source_text = copied["payload"].as_str().unwrap();
    let exact = format!("{}{}", source_text, " ".repeat(65536 - source_text.len()));
    let target = create(&app);
    let refused = call(
        &app,
        "document.clipboard_scene_paste",
        json!({"document_id":target,"expected_revision":0,"payload":format!("{exact} "),"offset":{"x":0,"y":0}}),
    );
    assert_eq!(
        refused["diagnostics"][0]["details"]["reason"],
        "clipboard_byte_limit"
    );
    assert_eq!(snapshot(&app, &target)["document"]["revision"], 0);
    let pasted = call(
        &app,
        "document.clipboard_scene_paste",
        json!({"document_id":target,"expected_revision":0,"payload":exact,"offset":{"x":0,"y":0}}),
    );
    assert_eq!(pasted["outcome"], "ok", "{pasted}");
    let failed = call(
        &app,
        "document.transaction",
        json!({"document_id":source,"expected_revision":2,"commands":[{"op":"delete_objects","objects":["s4"],"mode":"subtree"},{"op":"update_graphic_points","object":"s99","points":[{"x":0,"y":0},{"x":1,"y":1}]}]}),
    );
    assert_eq!(failed["outcome"], "error");
    assert_eq!(snapshot(&app, &source), before);
    let cut = edit(
        &app,
        &source,
        json!([{"op":"delete_objects","objects":["s4"],"mode":"subtree"}]),
    );
    assert_eq!(cut["document"]["atoms"], before["document"]["atoms"]);
    let undone = call(
        &app,
        "history.undo",
        json!({"document_id":source,"expected_revision":3}),
    );
    assert_eq!(
        undone["result"]["document"]["scene"],
        before["document"]["scene"]
    );
}
