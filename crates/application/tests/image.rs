//! Public image commands use the real bounded PNG provider and retain exact source data.
use chemifree_application::Application;
use chemifree_domain::image::{PngDecoder, PngMetadata};
use chemifree_providers::png_image::EmbeddedPngDecoder;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities, OpenSmilesCodec, V2000Codec};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DQAAAEgQGALFXOsAAAAABJRU5ErkJggg==";
type App = Application<LocalCapabilities, FileDocumentRepository>;
struct Counting(Arc<AtomicUsize>);
impl PngDecoder for Counting {
    fn validate(&self, text: &str) -> Result<PngMetadata, String> {
        self.0.fetch_add(1, Ordering::Relaxed);
        EmbeddedPngDecoder.validate(text)
    }
}
fn context() -> (App, tempfile::TempDir, Arc<AtomicUsize>) {
    let root = tempfile::tempdir().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let app = App::new(LocalCapabilities, FileDocumentRepository::new(root.path()))
        .with_image_decoder(Counting(calls.clone()))
        .with_ctfile_codec(V2000Codec)
        .with_smiles_codec(OpenSmilesCodec);
    (app, root, calls)
}
fn send(app: &App, request: &str, operation: &str, parameters: Value) -> Value {
    serde_json::to_value(app.execute(json!({"contract":1,"request_id":request,"profile":"preserve-semantics","operation":operation,"parameters":parameters}).to_string().as_bytes())).unwrap()
}
fn call(app: &App, operation: &str, parameters: Value) -> Value {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    send(
        app,
        &format!("image-{}", NEXT.fetch_add(1, Ordering::Relaxed)),
        operation,
        parameters,
    )
}
fn create(app: &App) -> String {
    call(app, "document.create", json!({"title":"Image"}))["result"]["document"]["id"]
        .as_str()
        .unwrap()
        .into()
}
fn snapshot(app: &App, id: &str) -> Value {
    call(app, "document.snapshot", json!({"document_id":id}))["result"].clone()
}
fn add() -> Value {
    json!({"op":"add_image","mime_type":"image/png","data_base64":PNG,"origin":{"x":1,"y":2},"size_pt":{"x":10,"y":20}})
}
fn edit(app: &App, id: &str, commands: Value) -> Value {
    let rev = snapshot(app, id)["document"]["revision"].clone();
    call(
        app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":rev,"commands":commands}),
    )
}
fn image(snapshot: &Value, id: &str) -> Value {
    snapshot["document"]["scene"]["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["id"] == id)
        .unwrap()
        .clone()
}
#[test]
fn png_is_decoded_once_across_replay_geometry_group_move_undo_and_exact_native_roundtrip() {
    let (app, _root, calls) = context();
    let id = create(&app);
    let parameters = json!({"document_id":id,"expected_revision":0,"commands":[{"op":"add_atom","element":"C","position":{"x":0,"y":0}},add(),add(),{"op":"group","objects":["s2","s3"]},{"op":"add_reference","source":"s2","target":{"kind":"object","id":"s3"}}]});
    let drawn = send(
        &app,
        "draw-once",
        "document.transaction",
        parameters.clone(),
    );
    assert_eq!(drawn["outcome"], "ok", "{drawn}");
    assert_eq!(
        send(&app, "draw-once", "document.transaction", parameters),
        drawn
    );
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    let changed = edit(
        &app,
        &id,
        json!([{"op":"update_image_geometry","object":"s2","origin":{"x":5,"y":6},"size_pt":{"x":7,"y":8}},{"op":"move_selection","selection":{"objects":["s4","s2"]},"delta":{"x":2,"y":3}}]),
    );
    assert_eq!(changed["outcome"], "ok", "{changed}");
    let after = changed["result"].clone();
    assert_eq!(image(&after, "s2")["origin"], json!({"x":7.,"y":9.}));
    assert_eq!(image(&after, "s2")["data_base64"], PNG);
    assert_eq!(image(&after, "s2")["pixel_width"], 1);
    assert_eq!(image(&after, "s2")["size_pt"], json!({"x":7.,"y":8.}));
    assert_eq!(
        after["document"]["atoms"],
        drawn["result"]["document"]["atoms"]
    );
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    let undo = call(
        &app,
        "history.undo",
        json!({"document_id":id,"expected_revision":2}),
    );
    assert_eq!(
        image(&undo["result"], "s2")["origin"],
        json!({"x":1.,"y":2.})
    );
    let redo = call(
        &app,
        "history.redo",
        json!({"document_id":id,"expected_revision":3}),
    );
    assert_eq!(image(&redo["result"], "s2"), image(&after, "s2"));
    app.save(&id, None, 4).unwrap();
    app.close(&id, None, false, None).unwrap();
    assert_eq!(image(&snapshot(&app, &id), "s2"), image(&after, "s2"));
    assert_eq!(calls.load(Ordering::Relaxed), 1);
}
#[test]
fn image_transforms_clipboard_and_bad_geometry_fail_atomically_without_source_replacement() {
    let (app, _root, _) = context();
    let id = create(&app);
    assert_eq!(edit(&app, &id, json!([add()]))["outcome"], "ok");
    let before = snapshot(&app, &id);
    for transform in [
        json!({"type":"rotate","degrees":0}),
        json!({"type":"rotate","degrees":90}),
        json!({"type":"reflect_horizontal"}),
    ] {
        let failed = edit(
            &app,
            &id,
            json!([{"op":"rename","title":"rollback"},{"op":"transform_selection","selection":{"objects":["s2"]},"pivot":{"x":0,"y":0},"transform":transform}]),
        );
        assert_eq!(
            failed["diagnostics"][0]["details"]["reason"], "image_unsupported",
            "{failed}"
        );
        assert_eq!(snapshot(&app, &id), before);
    }
    let copy = call(
        &app,
        "document.clipboard_scene_copy",
        json!({"document_id":id,"selection":{"objects":["s2"]}}),
    );
    assert_eq!(
        copy["diagnostics"][0]["details"]["reason"],
        "image_clipboard_unsupported"
    );
    assert_eq!(snapshot(&app, &id), before);
    for cmd in [
        json!({"op":"update_image_geometry","object":"s2","origin":{"x":100000,"y":0},"size_pt":{"x":1,"y":1}}),
        json!({"op":"update_image_geometry","object":"s2","origin":{"x":0,"y":0},"size_pt":{"x":0,"y":1}}),
    ] {
        let failed = edit(&app, &id, json!([cmd]));
        assert_eq!(
            failed["diagnostics"][0]["details"]["reason"],
            "image_invalid"
        );
        assert_eq!(snapshot(&app, &id), before);
    }
    let mut replacement = json!({"op":"update_image_geometry","object":"s2","origin":{"x":0,"y":0},"size_pt":{"x":1,"y":1}});
    replacement["data_base64"] = json!(PNG);
    assert_eq!(edit(&app, &id, json!([replacement]))["outcome"], "error");
    assert_eq!(snapshot(&app, &id), before);
}
#[test]
fn source_rejections_are_explicit_and_scientific_exports_keep_the_graph_with_rendering_loss() {
    let (app, _root, _) = context();
    let id = create(&app);
    let before = snapshot(&app, &id);
    for (mime, data) in [
        ("image/jpeg", PNG.to_owned()),
        ("image/png", "https://example.test/image.png".into()),
        ("image/png", format!("{PNG}\n")),
    ] {
        let mut cmd = add();
        cmd["mime_type"] = json!(mime);
        cmd["data_base64"] = json!(data);
        let failed = edit(&app, &id, json!([cmd]));
        assert_eq!(
            failed["diagnostics"][0]["details"]["reason"], "image_invalid",
            "{failed}"
        );
        assert_eq!(snapshot(&app, &id), before);
    }
    assert_eq!(
        edit(
            &app,
            &id,
            json!([{"op":"add_atom","element":"C","position":{"x":0,"y":0}},add()])
        )["outcome"],
        "ok"
    );
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

#[test]
fn real_max_pixel_images_count_every_instance_before_mutating_and_geometry_never_redecodes() {
    let (app, _root, calls) = context();
    let id = create(&app);
    let mut command = add();
    command["data_base64"] = json!(include_str!("fixtures/image-1024-rgba.base64").trim());
    let result = edit(&app, &id, json!(vec![command.clone(); 4]));
    assert_eq!(result["outcome"], "ok", "{result}");
    assert_eq!(image(&result["result"], "s2")["pixel_width"], 1024);
    let before = snapshot(&app, &id);
    let failed = edit(&app, &id, json!([command]));
    assert_eq!(
        failed["diagnostics"][0]["details"]["reason"], "image_pixel_limit",
        "{failed}"
    );
    assert_eq!(snapshot(&app, &id), before);
    assert_eq!(calls.load(Ordering::Relaxed), 1);
}
