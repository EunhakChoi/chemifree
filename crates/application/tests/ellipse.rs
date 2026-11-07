//! Ellipse edits and versioned clipboard profiles share the public operation boundary.

use std::sync::atomic::{AtomicU64, Ordering};

use chemifree_application::Application;
use chemifree_providers::interchange_codec::LocalInterchangeCodec;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities, OpenSmilesCodec, V2000Codec};
use serde_json::{Value, json};

type App = Application<LocalCapabilities, FileDocumentRepository>;
fn send(app: &App, id: &str, operation: &str, parameters: Value) -> Value {
    serde_json::to_value(app.execute(json!({"contract":1,"request_id":id,"profile":"preserve-semantics","operation":operation,"parameters":parameters}).to_string().as_bytes())).unwrap()
}
fn call(app: &App, operation: &str, parameters: Value) -> Value {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    send(
        app,
        &format!("ellipse-{}", NEXT.fetch_add(1, Ordering::Relaxed)),
        operation,
        parameters,
    )
}
fn context() -> (App, tempfile::TempDir) {
    let root = tempfile::tempdir().unwrap();
    (
        Application::new(LocalCapabilities, FileDocumentRepository::new(root.path()))
            .with_ctfile_codec(V2000Codec)
            .with_smiles_codec(OpenSmilesCodec)
            .with_interchange_codec(LocalInterchangeCodec),
        root,
    )
}
fn create(app: &App) -> String {
    call(app, "document.create", json!({"title":"Ellipse"}))["result"]["document"]["id"]
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
fn drawing(app: &App) -> String {
    let id = create(app);
    edit(
        app,
        &id,
        json!([{"op":"add_atom","element":"C","position":{"x":2,"y":3}},{"op":"add_graphic","shape":"ellipse","points":[{"x":-4,"y":-2},{"x":6,"y":8}]},{"op":"set_graphic_line_width","object":"s2","line_width_pt":2.75},{"op":"add_graphic","shape":"line","points":[{"x":0,"y":0},{"x":20,"y":10}]},{"op":"group","objects":["s2","s3"]},{"op":"add_reference","source":"s2","target":{"kind":"object","id":"s3"}}]),
    );
    id
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
fn copied(app: &App, id: &str, root: &str) -> Value {
    let result = call(
        app,
        "document.clipboard_scene_copy",
        json!({"document_id":id,"selection":{"objects":[root]}}),
    );
    assert_eq!(result["outcome"], "ok", "{result}");
    result["result"].clone()
}
fn wire(value: &Value) -> Value {
    serde_json::from_str(value["payload"].as_str().unwrap()).unwrap()
}

#[test]
fn ellipse_clipboard_v4_preserves_nullable_width_refs_offsets_replay_and_native_history() {
    let (app, _root) = context();
    let source = drawing(&app);
    let original = snapshot(&app, &source);
    let copy = copied(&app, &source, "s4");
    assert_eq!(snapshot(&app, &source), original);
    let raw = wire(&copy);
    assert_eq!(raw["version"], 4);
    for object in raw["objects"].as_array().unwrap() {
        if object["kind"] == "graphic" {
            assert!(object.get("line_width_pt").is_some());
        }
    }
    let target = create(&app);
    let before = edit(
        &app,
        &target,
        json!([{"op":"add_atom","element":"N","position":{"x":7,"y":8}},{"op":"add_graphic","shape":"rectangle","points":[{"x":100,"y":100},{"x":110,"y":110}]}]),
    );
    let parameters = json!({"document_id":target,"expected_revision":1,"payload":copy["payload"],"offset":{"x":10,"y":20}});
    let pasted = send(
        &app,
        "paste-ellipse-once",
        "document.clipboard_scene_paste",
        parameters.clone(),
    );
    assert_eq!(pasted["outcome"], "ok", "{pasted}");
    assert_eq!(
        send(
            &app,
            "paste-ellipse-once",
            "document.clipboard_scene_paste",
            parameters
        ),
        pasted
    );
    let after = &pasted["result"]["snapshot"];
    assert_eq!(graphic(after, "s3")["shape"], "ellipse");
    assert_eq!(graphic(after, "s3")["line_width_pt"], 2.75);
    assert_eq!(
        graphic(after, "s3")["points"],
        json!([{"x":6.0,"y":18.0},{"x":16.0,"y":28.0}])
    );
    assert_eq!(graphic(after, "s3")["parent"], "s5");
    assert_eq!(graphic(after, "s4")["line_width_pt"], Value::Null);
    assert_eq!(
        after["document"]["scene"]["references"],
        json!([{"source":"s3","target":{"kind":"object","id":"s4"}}])
    );
    for field in ["atoms", "bonds", "molecule", "stereo", "settings"] {
        assert_eq!(after["document"][field], before["document"][field]);
    }
    app.save(&target, None, 2).unwrap();
    app.close(&target, None, false, None).unwrap();
    assert_eq!(snapshot(&app, &target)["document"], after["document"]);
    let undo = call(
        &app,
        "history.undo",
        json!({"document_id":target,"expected_revision":2}),
    );
    assert_eq!(
        undo["result"]["document"]["scene"],
        before["document"]["scene"]
    );
    let redo = call(
        &app,
        "history.redo",
        json!({"document_id":target,"expected_revision":3}),
    );
    assert_eq!(graphic(&redo["result"], "s3")["shape"], "ellipse");
    edit(
        &app,
        &source,
        json!([{"op":"set_graphic_line_width","object":"s2","line_width_pt":null}]),
    );
    let inherited = wire(&copied(&app, &source, "s4"));
    assert_eq!(inherited["version"], 4);
    for object in inherited["objects"].as_array().unwrap() {
        if object["kind"] == "graphic" {
            assert_eq!(object.get("line_width_pt"), Some(&Value::Null));
        }
    }
}

#[test]
fn clipboard_ellipse_requires_v4_and_legacy_shape_forests_keep_v2_v3_selection() {
    let (app, _root) = context();
    let source = drawing(&app);
    let raw = wire(&copied(&app, &source, "s4"));
    assert_eq!(wire(&copied(&app, &source, "s3"))["version"], 2);
    edit(
        &app,
        &source,
        json!([{"op":"set_graphic_line_width","object":"s3","line_width_pt":1.2}]),
    );
    assert_eq!(wire(&copied(&app, &source, "s3"))["version"], 3);
    assert_eq!(wire(&copied(&app, &source, "s4"))["version"], 4);
    let target = create(&app);
    let before = snapshot(&app, &target);
    let mut invalid = Vec::new();
    for version in [2, 3] {
        let mut old = raw.clone();
        old["version"] = json!(version);
        if version == 2 {
            for object in old["objects"].as_array_mut().unwrap() {
                object.as_object_mut().unwrap().remove("line_width_pt");
            }
        }
        invalid.push((old, "clipboard_fragment_invalid"));
    }
    let mut missing = raw.clone();
    missing["objects"][1]
        .as_object_mut()
        .unwrap()
        .remove("line_width_pt");
    invalid.push((missing, "clipboard_fragment_invalid"));
    let mut range = raw.clone();
    range["objects"][1]["line_width_pt"] = json!(10.001);
    invalid.push((range, "clipboard_fragment_invalid"));
    invalid.push((
        json!({"format":"chemifree.clipboard","version":5,"future_body":[]}),
        "clipboard_format_unsupported",
    ));
    for (payload, reason) in invalid {
        let result = call(
            &app,
            "document.clipboard_scene_paste",
            json!({"document_id":target,"expected_revision":0,"payload":payload.to_string(),"offset":{"x":0,"y":0}}),
        );
        assert_eq!(
            result["diagnostics"][0]["details"]["reason"], reason,
            "{result}"
        );
        assert_eq!(snapshot(&app, &target), before);
    }
    for (inherited, version) in [(false, 3), (true, 2)] {
        let mut legacy = raw.clone();
        for object in legacy["objects"].as_array_mut().unwrap() {
            if object["shape"] == "ellipse" {
                object["shape"] = json!("rectangle");
            }
            if inherited && object["kind"] == "graphic" {
                object["line_width_pt"] = Value::Null;
            }
        }
        let id = create(&app);
        let result = call(
            &app,
            "document.clipboard_scene_paste",
            json!({"document_id":id,"expected_revision":0,"payload":legacy.to_string(),"offset":{"x":0,"y":0}}),
        );
        assert_eq!(result["outcome"], "ok", "{result}");
        assert_eq!(wire(&copied(&app, &id, "s4"))["version"], version);
    }
}

#[test]
fn ellipse_rotation_is_atomic_and_exports_keep_graph_with_explicit_rendering_loss() {
    let (app, _root) = context();
    let id = drawing(&app);
    let before = snapshot(&app, &id);
    let parameters = json!({"document_id":id,"selection":{"objects":["s4"],"atoms":["a1"]},"pivot":{"x":0,"y":0},"transform":{"type":"rotate","degrees":37}});
    let plan = call(&app, "document.transform_plan", parameters);
    assert_eq!(
        plan["diagnostics"][0]["details"]["reason"],
        "transform_ellipse_unsupported"
    );
    let failed = call(
        &app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":1,"commands":[{"op":"rename","title":"rollback"},{"op":"transform_selection","selection":{"objects":["s4"],"atoms":["a1"]},"pivot":{"x":0,"y":0},"transform":{"type":"rotate","degrees":37}}]}),
    );
    assert_eq!(
        failed["diagnostics"][0]["details"]["reason"],
        "transform_ellipse_unsupported"
    );
    assert_eq!(snapshot(&app, &id), before);
    let preview = call(
        &app,
        "document.transform_plan",
        json!({"document_id":id,"selection":{"objects":["s4"]},"pivot":{"x":0,"y":0},"transform":{"type":"rotate","degrees":90}}),
    );
    assert_eq!(preview["outcome"], "ok", "{preview}");
    assert_eq!(preview["result"]["graphics"][0]["shape"], "ellipse");
    let rotated = edit(
        &app,
        &id,
        json!([{"op":"transform_selection","selection":{"objects":["s4"]},"pivot":{"x":0,"y":0},"transform":{"type":"rotate","degrees":90}}]),
    );
    assert_eq!(
        graphic(&rotated, "s2")["points"],
        json!([{"x":2.0,"y":-4.0},{"x":-8.0,"y":6.0}])
    );
    assert_eq!(rotated["document"]["atoms"], before["document"]["atoms"]);
    for parameters in [
        json!({"format":"mol","records":[{"document_id":id}]}),
        json!({"format":"sdf","records":[{"document_id":id}]}),
        json!({"format":"smiles","document_id":id,"style":"recorded"}),
        json!({"format":"cdxml","document_id":id}),
        json!({"format":"cdx","document_id":id}),
    ] {
        let exported = call(&app, "document.export", parameters);
        assert_eq!(exported["outcome"], "partial", "{exported}");
        assert!(
            exported["loss_report"]
                .as_array()
                .unwrap()
                .iter()
                .any(|loss| loss["feature"] == "scene_object"
                    && loss["source_path"]
                        .as_str()
                        .is_some_and(|path| path.contains("s2"))
                    && loss["semantic_impact"] == "rendering"),
            "{exported}"
        );
    }
    let formula = call(
        &app,
        "chem.properties",
        json!({"document_id":id,"scope":"molecule","property":"formula"}),
    );
    assert_eq!(formula["result"]["components"][0]["formula"], "CH4");
}
