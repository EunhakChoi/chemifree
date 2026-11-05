//! P32c checked geometry preview and transaction wiring through the actual operation boundary.

use std::sync::atomic::{AtomicU64, Ordering};

use chemifree_application::Application;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};

type App = Application<LocalCapabilities, FileDocumentRepository>;

fn send(app: &App, request_id: &str, operation: &str, parameters: Value) -> Value {
    let request = json!({"contract":1,"request_id":request_id,"profile":"preserve-semantics","operation":operation,"parameters":parameters});
    serde_json::to_value(app.execute(request.to_string().as_bytes())).unwrap()
}
fn call(app: &App, operation: &str, parameters: Value) -> Value {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    send(
        app,
        &format!("transform-{}", NEXT.fetch_add(1, Ordering::Relaxed)),
        operation,
        parameters,
    )
}
fn snapshot(app: &App, id: &str) -> Value {
    call(app, "document.snapshot", json!({"document_id":id}))["result"].clone()
}

/// Group s5 owns primary placement s1, rectangle s2 and arrow s3; s4 is another
/// placement. A stored tetrahedral configuration, wedge and absolute group are preserved.
fn setup() -> (App, tempfile::TempDir, String) {
    let directory = tempfile::tempdir().unwrap();
    let app = Application::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    );
    let created = call(&app, "document.create", json!({"title":"Geometry only"}));
    let id = created["result"]["document"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let commands = json!([
        {"op":"add_atom","element":"C","position":{"x":0,"y":0}},
        {"op":"add_atom","element":"F","position":{"x":0,"y":-3}},
        {"op":"add_atom","element":"Cl","position":{"x":2,"y":1}},
        {"op":"add_atom","element":"Br","position":{"x":-2,"y":1}},
        {"op":"add_bond","begin":"$0","end":"$1","order":"single"},
        {"op":"add_bond","begin":"$0","end":"$2","order":"single"},
        {"op":"add_bond","begin":"$0","end":"$3","order":"single"},
        {"op":"set_bond_mark","bond":"$5","mark":{"kind":"wedge","narrow":"begin"}},
        {"op":"set_atom_configuration","atom":"$0","configuration":{"kind":"tetrahedral","neighbors":["$1","$2","$3","implicit_hydrogen"],"turn":"clockwise"}},
        {"op":"create_stereo_group","kind":"absolute","atoms":["$0"],"number":null},
        {"op":"add_graphic","shape":"rectangle","points":[{"x":1,"y":1},{"x":3,"y":2}]},
        {"op":"add_graphic","shape":"arrow","points":[{"x":4,"y":1},{"x":5,"y":2}]},
        {"op":"add_instance","source":"s1","offset":{"x":100,"y":0}},
        {"op":"group","objects":["s1","s2","s3"]},
        {"op":"add_reference","source":"s3","target":{"kind":"atom","id":"a1"}}
    ]);
    let edited = call(
        &app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":0,"commands":commands}),
    );
    assert_eq!(edited["outcome"], "ok", "{edited}");
    (app, directory, id)
}
fn selected() -> Value {
    json!({"objects":["s5","s2","s1","s2"],"atoms":["a1"],"bonds":["b1"]})
}
fn parameters(id: &str, selection: Value, transform: Value) -> Value {
    json!({"document_id":id,"selection":selection,"instance":"s1","pivot":{"x":0,"y":0},"transform":transform})
}
fn edit(selection: Value, transform: Value) -> Value {
    json!({"op":"transform_selection","selection":selection,"instance":"s1","pivot":{"x":0,"y":0},"transform":transform})
}
fn graphic<'a>(snapshot: &'a Value, id: &str) -> &'a Value {
    snapshot["document"]["scene"]["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|object| object["id"] == id)
        .unwrap()
}
fn instance<'a>(snapshot: &'a Value, id: &str) -> &'a Value {
    snapshot["document"]["scene"]["instances"]
        .as_array()
        .unwrap()
        .iter()
        .find(|instance| instance["id"] == id)
        .unwrap()
}
fn content(snapshot: &Value) -> Value {
    let mut document = snapshot["document"].clone();
    document.as_object_mut().unwrap().remove("revision");
    document.as_object_mut().unwrap().remove("selection");
    document
}

#[test]
fn preview_reports_exact_scope_and_coordinates_and_commit_matches_without_chemistry_changes() {
    let (app, _directory, id) = setup();
    let before = snapshot(&app, &id);
    let transform = json!({"type":"rotate","degrees":90});
    let preview = call(
        &app,
        "document.transform_plan",
        parameters(&id, selected(), transform.clone()),
    );
    assert_eq!(preview["outcome"], "ok", "{preview}");
    assert_eq!(preview["document_revision"], 1);
    assert_eq!(preview["result"]["snapshot"], before);
    assert_eq!(preview["result"]["transform"], transform);
    assert_eq!(preview["result"]["pivot"], json!({"x":0.0,"y":0.0}));
    assert_eq!(
        preview["result"]["scope"],
        json!({"objects":["s1","s2","s3","s5"],"positions":[{"instance":"s1","atom":"a1"},{"instance":"s1","atom":"a2"},{"instance":"s1","atom":"a3"},{"instance":"s1","atom":"a4"}],"graphics":["s2","s3"]})
    );
    assert_eq!(
        preview["result"]["positions"],
        json!([
            {"instance":"s1","atom":"a1","before":{"x":0.0,"y":0.0},"after":{"x":0.0,"y":0.0}},
            {"instance":"s1","atom":"a2","before":{"x":0.0,"y":-3.0},"after":{"x":3.0,"y":0.0}},
            {"instance":"s1","atom":"a3","before":{"x":2.0,"y":1.0},"after":{"x":-1.0,"y":2.0}},
            {"instance":"s1","atom":"a4","before":{"x":-2.0,"y":1.0},"after":{"x":-1.0,"y":-2.0}}
        ])
    );
    assert_eq!(
        preview["result"]["graphics"],
        json!([
            {"object":"s2","shape":"rectangle","before":[{"x":1.0,"y":1.0},{"x":3.0,"y":2.0}],"after":[{"x":-1.0,"y":1.0},{"x":-2.0,"y":3.0}]},
            {"object":"s3","shape":"arrow","before":[{"x":4.0,"y":1.0},{"x":5.0,"y":2.0}],"after":[{"x":-1.0,"y":4.0},{"x":-2.0,"y":5.0}]}
        ])
    );
    assert_eq!(preview["result"]["drawing_stereo_may_change"], true);
    assert_eq!(snapshot(&app, &id), before);
    let committed = call(
        &app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":1,"commands":[edit(selected(),transform)]}),
    );
    assert_eq!(committed["outcome"], "ok", "{committed}");
    assert_eq!(committed["document_revision"], 2);
    assert_eq!(committed["result"]["notes"], json!([]));
    assert_eq!(committed["loss_report"], json!([]));
    assert_eq!(
        instance(&committed["result"], "s1")["positions"],
        json!([{"atom":"a1","x":0.0,"y":0.0},{"atom":"a2","x":3.0,"y":0.0},{"atom":"a3","x":-1.0,"y":2.0},{"atom":"a4","x":-1.0,"y":-2.0}])
    );
    assert_eq!(
        graphic(&committed["result"], "s2")["points"],
        preview["result"]["graphics"][0]["after"]
    );
    assert_eq!(
        graphic(&committed["result"], "s3")["points"],
        preview["result"]["graphics"][1]["after"]
    );
    assert_eq!(
        instance(&committed["result"], "s4"),
        instance(&before, "s4")
    );
    for key in ["bonds", "stereo"] {
        assert_eq!(
            committed["result"]["document"][key],
            before["document"][key]
        );
    }
    assert_eq!(
        committed["result"]["document"]["scene"]["references"],
        before["document"]["scene"]["references"]
    );
}

#[test]
fn horizontal_and_vertical_reflections_have_explicit_axes_and_restore_history_selections() {
    for (kind, expected) in [
        (
            "reflect_horizontal",
            json!([{"x":1.0,"y":-1.0},{"x":3.0,"y":-2.0}]),
        ),
        (
            "reflect_vertical",
            json!([{"x":-1.0,"y":1.0},{"x":-3.0,"y":2.0}]),
        ),
    ] {
        let (app, _directory, id) = setup();
        let before = snapshot(&app, &id);
        let transform = json!({"type":kind});
        let preview = call(
            &app,
            "document.transform_plan",
            parameters(&id, selected(), transform.clone()),
        );
        assert_eq!(preview["outcome"], "ok", "{preview}");
        let committed = call(
            &app,
            "document.transaction",
            json!({"document_id":id,"expected_revision":1,"commands":[edit(selected(),transform)],"selection_before":{"objects":["s5"]},"selection_after":{"objects":["s2"]}}),
        );
        assert_eq!(committed["outcome"], "ok", "{committed}");
        assert_eq!(graphic(&committed["result"], "s2")["points"], expected);
        assert_eq!(
            committed["result"]["document"]["stereo"],
            before["document"]["stereo"]
        );
        assert_eq!(
            committed["result"]["document"]["bonds"],
            before["document"]["bonds"]
        );
        assert_eq!(committed["result"]["notes"], json!([]));
        let undone = call(
            &app,
            "history.undo",
            json!({"document_id":id,"expected_revision":2}),
        );
        assert_eq!(content(&undone["result"]), content(&before));
        assert_eq!(
            undone["result"]["document"]["selection"]["objects"],
            json!(["s5"])
        );
        let redone = call(
            &app,
            "history.redo",
            json!({"document_id":id,"expected_revision":3}),
        );
        assert_eq!(content(&redone["result"]), content(&committed["result"]));
        assert_eq!(
            redone["result"]["document"]["selection"]["objects"],
            json!(["s2"])
        );
    }
}

#[test]
fn rectangle_unknown_and_coordinate_failures_are_atomic_and_match_preview_reasons() {
    let (app, _directory, id) = setup();
    let before = snapshot(&app, &id);
    for (selection, transform, pivot, reason) in [
        (
            selected(),
            json!({"type":"rotate","degrees":45}),
            json!({"x":0,"y":0}),
            Some("transform_rectangle_unsupported"),
        ),
        (
            json!({"objects":["s999"]}),
            json!({"type":"rotate","degrees":0}),
            json!({"x":0,"y":0}),
            Some("selection_unknown"),
        ),
        (
            json!({"objects":["s5"]}),
            json!({"type":"reflect_vertical"}),
            json!({"x":100000,"y":0}),
            None,
        ),
    ] {
        let mut parameters = parameters(&id, selection.clone(), transform.clone());
        parameters["pivot"] = pivot.clone();
        let preview = call(&app, "document.transform_plan", parameters);
        assert_eq!(preview["outcome"], "error", "{preview}");
        assert_eq!(preview["diagnostics"][0]["code"], "invalid_command");
        if let Some(reason) = reason {
            assert_eq!(preview["diagnostics"][0]["details"]["reason"], reason);
        }
        let mut transform_command = edit(selection, transform);
        transform_command["pivot"] = pivot;
        let failed = call(
            &app,
            "document.transaction",
            json!({"document_id":id,"expected_revision":1,"commands":[{"op":"rename","title":"Must roll back"},transform_command]}),
        );
        assert_eq!(failed["outcome"], "error", "{failed}");
        assert_eq!(failed["diagnostics"][0]["details"]["command_index"], 1);
        if let Some(reason) = reason {
            assert_eq!(failed["diagnostics"][0]["details"]["reason"], reason);
        }
        assert_eq!(snapshot(&app, &id), before);
    }
}

#[test]
fn strict_transform_input_and_graphics_only_arbitrary_rotation_preserve_bounded_semantics() {
    let (app, _directory, id) = setup();
    let before = snapshot(&app, &id);
    for transform in [
        json!({"type":"rotate"}),
        json!({"type":"reflect_diagonal"}),
        json!({"type":"rotate","degrees":30,"other":1}),
        json!({"type":"reflect_vertical","degrees":90}),
    ] {
        let preview = call(
            &app,
            "document.transform_plan",
            parameters(&id, json!({}), transform),
        );
        assert_eq!(preview["outcome"], "error", "{preview}");
        assert_eq!(preview["diagnostics"][0]["code"], "invalid_request");
    }
    let mut bad_pivot = parameters(&id, json!({}), json!({"type":"rotate","degrees":0}));
    bad_pivot["pivot"] = json!({"x":100001,"y":0});
    assert_eq!(
        call(&app, "document.transform_plan", bad_pivot)["diagnostics"][0]["code"],
        "invalid_request"
    );
    let mut bad_instance = parameters(&id, json!({}), json!({"type":"rotate","degrees":0}));
    bad_instance["instance"] = json!("s999");
    assert_eq!(
        call(&app, "document.transform_plan", bad_instance)["diagnostics"][0]["details"]["reason"],
        "object_unknown"
    );
    let preview = call(
        &app,
        "document.transform_plan",
        parameters(
            &id,
            json!({"objects":["s3"]}),
            json!({"type":"rotate","degrees":30}),
        ),
    );
    assert_eq!(preview["outcome"], "ok", "{preview}");
    assert_eq!(preview["result"]["drawing_stereo_may_change"], false);
    assert_eq!(preview["result"]["positions"], json!([]));
    assert_eq!(preview["result"]["graphics"][0]["shape"], "arrow");
    assert_eq!(snapshot(&app, &id), before);
}

#[test]
fn identity_preserves_redo_mutations_replay_once_and_preview_is_fresh_with_revision_guard() {
    let (app, _directory, id) = setup();
    call(
        &app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":1,"commands":[{"op":"rename","title":"Undo me"}]}),
    );
    call(
        &app,
        "history.undo",
        json!({"document_id":id,"expected_revision":2}),
    );
    let before = snapshot(&app, &id);
    assert_eq!(before["session"]["redo_depth"], 1);
    let unchanged = call(
        &app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":3,"commands":[edit(selected(),json!({"type":"rotate","degrees":360}))]}),
    );
    assert_eq!(unchanged["document_revision"], 3, "{unchanged}");
    assert_eq!(unchanged["result"], before);
    assert_eq!(snapshot(&app, &id), before);
    let preview_parameters = parameters(&id, selected(), json!({"type":"rotate","degrees":90}));
    let planned = send(
        &app,
        "fresh-transform-preview",
        "document.transform_plan",
        preview_parameters.clone(),
    );
    assert_eq!(planned["document_revision"], 3);
    let mutation = json!({"document_id":id,"expected_revision":3,"commands":[edit(selected(),json!({"type":"rotate","degrees":90}))]});
    let first = send(
        &app,
        "one-transform",
        "document.transaction",
        mutation.clone(),
    );
    assert_eq!(first["outcome"], "ok", "{first}");
    let replay = send(&app, "one-transform", "document.transaction", mutation);
    assert_eq!(replay, first);
    let after = snapshot(&app, &id);
    assert_eq!(after["document"]["revision"], 4);
    let fresh = send(
        &app,
        "fresh-transform-preview",
        "document.transform_plan",
        preview_parameters,
    );
    assert_eq!(fresh["document_revision"], 4);
    assert_eq!(fresh["result"]["snapshot"], after);
    let changed_id = send(
        &app,
        "one-transform",
        "document.transaction",
        json!({"document_id":id,"expected_revision":4,"commands":[edit(selected(),json!({"type":"rotate","degrees":180}))]}),
    );
    assert_eq!(changed_id["diagnostics"][0]["code"], "request_id_reused");
    let stale = call(
        &app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":3,"commands":[edit(selected(),json!({"type":"rotate","degrees":90}))]}),
    );
    assert_eq!(stale["diagnostics"][0]["code"], "revision_conflict");
    assert_eq!(snapshot(&app, &id), after);
}
