//! Spline commands keep exact points and use a lossless versioned clipboard profile.
use chemifree_application::Application;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicU64, Ordering};
type App = Application<LocalCapabilities, FileDocumentRepository>;
fn send(app: &App, id: &str, operation: &str, parameters: Value) -> Value {
    serde_json::to_value(app.execute(json!({"contract":1,"request_id":id,"profile":"preserve-semantics","operation":operation,"parameters":parameters}).to_string().as_bytes())).unwrap()
}
fn call(app: &App, op: &str, p: Value) -> Value {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    send(
        app,
        &format!("poly-{}", NEXT.fetch_add(1, Ordering::Relaxed)),
        op,
        p,
    )
}
fn create(app: &App) -> String {
    call(app, "document.create", json!({"title":"Spline"}))["result"]["document"]["id"]
        .as_str()
        .unwrap()
        .into()
}
fn snapshot(app: &App, id: &str) -> Value {
    call(app, "document.snapshot", json!({"document_id":id}))["result"].clone()
}
fn edit(app: &App, id: &str, commands: Value) -> Value {
    let rev = snapshot(app, id)["document"]["revision"].clone();
    call(
        app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":rev,"commands":commands}),
    )
}
fn points() -> Value {
    json!([{"x":0.,"y":0.},{"x":8.,"y":2.},{"x":-4.,"y":6.},{"x":0.,"y":0.}])
}
#[test]
fn spline_clipboard_v6_preserves_group_refs_width_offset_and_replay_without_changing_graph() {
    let root = tempfile::tempdir().unwrap();
    let app = App::new(LocalCapabilities, FileDocumentRepository::new(root.path()));
    let id = create(&app);
    let drawn = edit(
        &app,
        &id,
        json!([{"op":"add_atom","element":"C","position":{"x":1,"y":2}},{"op":"add_spline","points":points()},{"op":"add_graphic","shape":"ellipse","points":[{"x":1,"y":1},{"x":4,"y":5}]},{"op":"set_graphic_line_width","object":"s2","line_width_pt":10},{"op":"group","objects":["s2","s3"]},{"op":"add_reference","source":"s2","target":{"kind":"object","id":"s3"}}]),
    );
    assert_eq!(drawn["outcome"], "ok", "{drawn}");
    let copy = call(
        &app,
        "document.clipboard_scene_copy",
        json!({"document_id":id,"selection":{"objects":["s4"]}}),
    );
    assert_eq!(copy["outcome"], "ok", "{copy}");
    let wire: Value = serde_json::from_str(copy["result"]["payload"].as_str().unwrap()).unwrap();
    assert_eq!(wire["version"], 6);
    assert_eq!(
        wire["objects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["shape"] == "spline")
            .unwrap()["points"],
        points()
    );
    for object in wire["objects"].as_array().unwrap() {
        if object["kind"] == "graphic" {
            assert!(object.get("line_width_pt").is_some());
        }
    }
    let target = create(&app);
    let parameters = json!({"document_id":target,"expected_revision":0,"payload":wire.to_string(),"offset":{"x":5,"y":-3}});
    let paste = send(
        &app,
        "paste-poly",
        "document.clipboard_scene_paste",
        parameters.clone(),
    );
    assert_eq!(paste["outcome"], "ok", "{paste}");
    assert_eq!(
        send(
            &app,
            "paste-poly",
            "document.clipboard_scene_paste",
            parameters
        ),
        paste
    );
    let document = &paste["result"]["snapshot"]["document"];
    assert_eq!(document["atoms"], json!([]));
    let poly = document["scene"]["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["shape"] == "spline")
        .unwrap();
    assert_eq!(poly["line_width_pt"], 10.);
    assert_eq!(
        poly["points"],
        json!([{"x":5.,"y":-3.},{"x":13.,"y":-1.},{"x":1.,"y":3.},{"x":5.,"y":-3.}])
    );
    assert_eq!(document["scene"]["references"].as_array().unwrap().len(), 1);
    let unchanged = snapshot(&app, &id);
    assert_eq!(
        unchanged["document"]["atoms"],
        drawn["result"]["document"]["atoms"]
    );
}
#[test]
fn malformed_profiles_cardinalities_and_old_pair_commands_refuse_atomically() {
    let root = tempfile::tempdir().unwrap();
    let app = App::new(LocalCapabilities, FileDocumentRepository::new(root.path()));
    let id = create(&app);
    assert_eq!(
        edit(&app, &id, json!([{"op":"add_spline","points":points()}]))["outcome"],
        "ok"
    );
    let before = snapshot(&app, &id);
    for cmd in [
        json!({"op":"add_spline","points":vec![json!({"x":0,"y":0});3]}),
        json!({"op":"update_spline_points","object":"s2","points":vec![json!({"x":0,"y":0});5]}),
        json!({"op":"update_spline_points","object":"s2","points":[{"x":0,"y":0},{"x":0,"y":0},{"x":100001,"y":0},{"x":0,"y":0}]}),
        json!({"op":"add_graphic","shape":"spline","points":[{"x":0,"y":0},{"x":1,"y":1}]}),
        json!({"op":"update_graphic_points","object":"s2","points":[{"x":0,"y":0},{"x":1,"y":1}]}),
    ] {
        let failed = edit(&app, &id, json!([cmd]));
        assert_eq!(failed["outcome"], "error", "{failed}");
        assert_eq!(snapshot(&app, &id), before);
    }
    let copy = call(
        &app,
        "document.clipboard_scene_copy",
        json!({"document_id":id,"selection":{"objects":["s2"]}}),
    );
    let raw: Value = serde_json::from_str(copy["result"]["payload"].as_str().unwrap()).unwrap();
    for version in [2, 3, 4, 7] {
        let mut payload = raw.clone();
        payload["version"] = json!(version);
        let result = call(
            &app,
            "document.clipboard_scene_paste",
            json!({"document_id":id,"expected_revision":1,"payload":payload.to_string(),"offset":{"x":0,"y":0}}),
        );
        assert_eq!(result["outcome"], "error", "{result}");
        assert_eq!(snapshot(&app, &id), before);
    }
    for version in 2..=5 {
        let mut payload = raw.clone();
        payload["version"] = json!(version);
        let object = &mut payload["objects"][0];
        object["shape"] = json!("spline");
        object["points"] = json!([
            {"x":0,"y":0},{"x":10,"y":20},{"x":20,"y":20},{"x":30,"y":0}
        ]);
        if version == 2 {
            object.as_object_mut().unwrap().remove("line_width_pt");
        }
        let result = call(
            &app,
            "document.clipboard_scene_paste",
            json!({"document_id":id,"expected_revision":1,"payload":payload.to_string(),"offset":{"x":0,"y":0}}),
        );
        assert_eq!(
            result["diagnostics"][0]["details"]["reason"], "clipboard_fragment_invalid",
            "{result}"
        );
        assert_eq!(snapshot(&app, &id), before);
    }
    for modification in ["missing", "range", "count"] {
        let mut payload = raw.clone();
        let object = &mut payload["objects"][0];
        match modification {
            "missing" => {
                object.as_object_mut().unwrap().remove("line_width_pt");
            }
            "range" => object["line_width_pt"] = json!(10.1),
            _ => object["points"] = json!([]),
        }
        let result = call(
            &app,
            "document.clipboard_scene_paste",
            json!({"document_id":id,"expected_revision":1,"payload":payload.to_string(),"offset":{"x":0,"y":0}}),
        );
        assert_eq!(result["outcome"], "error", "{result}");
        assert_eq!(snapshot(&app, &id), before);
    }
}

#[test]
fn spline_copy_byte_limit_does_not_mutate_content_selection_or_history() {
    let root = tempfile::tempdir().unwrap();
    let app = App::new(LocalCapabilities, FileDocumentRepository::new(root.path()));
    let id = create(&app);
    let points = (0..4)
        .map(|n| json!({"x":0.12345678901234567+n as f64,"y":-0.12345678901234567-n as f64}))
        .collect::<Vec<_>>();
    let commands = (0..500)
        .map(|_| json!({"op":"add_spline","points":points}))
        .collect::<Vec<_>>();
    let added = edit(&app, &id, json!(commands));
    assert_eq!(added["outcome"], "ok", "{added}");
    let before = snapshot(&app, &id);
    let objects = (2..502).map(|n| format!("s{n}")).collect::<Vec<_>>();
    let result = call(
        &app,
        "document.clipboard_scene_copy",
        json!({"document_id":id,"selection":{"objects":objects}}),
    );
    assert_eq!(
        result["diagnostics"][0]["details"]["reason"], "clipboard_byte_limit",
        "{result}"
    );
    assert_eq!(snapshot(&app, &id), before);
}

#[test]
fn spline_vertex_edits_and_free_rotation_preserve_graph_formula_and_explicit_export_loss() {
    use chemifree_providers::{OpenSmilesCodec, V2000Codec};
    let root = tempfile::tempdir().unwrap();
    let app = App::new(LocalCapabilities, FileDocumentRepository::new(root.path()))
        .with_ctfile_codec(V2000Codec)
        .with_smiles_codec(OpenSmilesCodec);
    let id = create(&app);
    let drawn = edit(
        &app,
        &id,
        json!([{"op":"add_atom","element":"C","position":{"x":1,"y":2}},{"op":"add_spline","points":points()}]),
    );
    assert_eq!(drawn["outcome"], "ok");
    let before = snapshot(&app, &id);
    let edited = edit(
        &app,
        &id,
        json!([{"op":"update_spline_points","object":"s2","points":[{"x":1,"y":2},{"x":3,"y":4},{"x":5,"y":6},{"x":7,"y":8}]},{"op":"transform_selection","selection":{"objects":["s2"]},"pivot":{"x":0,"y":0},"transform":{"type":"rotate","degrees":37}}]),
    );
    assert_eq!(edited["outcome"], "ok", "{edited}");
    for field in ["atoms", "bonds", "molecule", "stereo", "settings"] {
        assert_eq!(
            edited["result"]["document"][field],
            before["document"][field]
        );
    }
    let formula = call(
        &app,
        "chem.properties",
        json!({"document_id":id,"scope":"molecule","property":"formula"}),
    );
    assert_eq!(formula["result"]["components"][0]["formula"], "CH4");
    for parameters in [
        json!({"format":"mol","records":[{"document_id":id}]}),
        json!({"format":"smiles","document_id":id,"style":"recorded"}),
    ] {
        let exported = call(&app, "document.export", parameters);
        assert_eq!(exported["outcome"], "partial", "{exported}");
        assert!(
            exported["loss_report"]
                .as_array()
                .unwrap()
                .iter()
                .any(|loss| loss["feature"] == "scene_object"
                    && loss["semantic_impact"] == "rendering"),
            "{exported}"
        );
    }
}
