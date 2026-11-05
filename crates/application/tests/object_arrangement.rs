//! Checked arrangement previews and atomic commands through the shared operation boundary.

use std::sync::atomic::{AtomicU64, Ordering};

use chemifree_application::Application;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};

type App = Application<LocalCapabilities, FileDocumentRepository>;

fn call(app: &App, operation: &str, parameters: Value) -> Value {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let request = json!({"contract":1,"request_id":format!("arrangement-{}", NEXT.fetch_add(1,Ordering::Relaxed)),
        "profile":"preserve-semantics","operation":operation,"parameters":parameters});
    serde_json::to_value(app.execute(request.to_string().as_bytes())).unwrap()
}

fn snapshot(app: &App, id: &str) -> Value {
    call(app, "document.snapshot", json!({"document_id":id}))["result"].clone()
}

fn setup(rectangles: &[(f64, f64, f64, f64)]) -> (App, tempfile::TempDir, String) {
    let directory = tempfile::tempdir().unwrap();
    let app = Application::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    );
    let created = call(
        &app,
        "document.create",
        json!({"title":"Rigid arrangement"}),
    );
    let id = created["result"]["document"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let commands: Vec<Value> = rectangles
        .iter()
        .map(|(left, top, right, bottom)| {
            json!({"op":"add_graphic","shape":"rectangle",
        "points":[{"x":left,"y":top},{"x":right,"y":bottom}]})
        })
        .collect();
    let added = call(
        &app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":0,"commands":commands}),
    );
    assert_eq!(added["outcome"], "ok", "{added}");
    (app, directory, id)
}

fn preview(app: &App, id: &str, objects: &[&str], mode: &str) -> Value {
    call(
        app,
        "document.arrangement_plan",
        json!({"document_id":id,"objects":objects,"mode":mode}),
    )
}

fn graphic<'a>(snapshot: &'a Value, id: &str) -> &'a Value {
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

#[test]
fn all_six_align_modes_preview_exact_deltas_and_commit_the_same_rigid_motion() {
    for (mode, deltas) in [
        ("align_left", [(0.0, 0.0), (-30.0, 0.0)]),
        ("align_horizontal_center", [(20.0, 0.0), (-15.0, 0.0)]),
        ("align_right", [(40.0, 0.0), (0.0, 0.0)]),
        ("align_top", [(0.0, 0.0), (0.0, -40.0)]),
        ("align_vertical_center", [(0.0, 20.0), (0.0, -20.0)]),
        ("align_bottom", [(0.0, 40.0), (0.0, 0.0)]),
    ] {
        let (app, _directory, id) = setup(&[
            (0.0, 0.0, 10.0, 20.0),
            (30.0, 40.0, 50.0, 60.0),
            (80.0, 80.0, 90.0, 90.0),
        ]);
        let before = snapshot(&app, &id);
        let planned = preview(&app, &id, &["s3", "s2"], mode);
        assert_eq!(planned["outcome"], "ok", "{mode}: {planned}");
        assert_eq!(planned["result"]["snapshot"], before);
        assert_eq!(planned["result"]["mode"], mode);
        assert_eq!(
            planned["result"]["units"],
            json!([
                {"object":"s2","bounds":{"left":0.0,"top":0.0,"right":10.0,"bottom":20.0},"delta":{"x":deltas[0].0,"y":deltas[0].1},"scope":{"objects":["s2"],"positions":[],"graphics":["s2"]}},
                {"object":"s3","bounds":{"left":30.0,"top":40.0,"right":50.0,"bottom":60.0},"delta":{"x":deltas[1].0,"y":deltas[1].1},"scope":{"objects":["s3"],"positions":[],"graphics":["s3"]}}
            ]),
            "{mode}"
        );
        assert_eq!(
            planned["result"]["scope"],
            json!({"objects":["s2","s3"],"positions":[],"graphics":["s2","s3"]})
        );
        assert_eq!(snapshot(&app, &id), before, "{mode}: preview edits nothing");
        let committed = call(
            &app,
            "document.transaction",
            json!({"document_id":id,"expected_revision":1,
            "commands":[{"op":"arrange_objects","objects":["s3","s2"],"mode":mode}]}),
        );
        assert_eq!(committed["outcome"], "ok", "{mode}: {committed}");
        for ((object, (dx, dy)), (x, y, right, bottom)) in ["s2", "s3"]
            .into_iter()
            .zip(deltas)
            .zip([(0.0, 0.0, 10.0, 20.0), (30.0, 40.0, 50.0, 60.0)])
        {
            assert_eq!(
                graphic(&committed["result"], object)["points"],
                json!([{"x":x+dx,"y":y+dy},{"x":right+dx,"y":bottom+dy}]),
                "{mode}: {object}"
            );
        }
        assert_eq!(graphic(&committed["result"], "s4"), graphic(&before, "s4"));
        let undone = call(
            &app,
            "history.undo",
            json!({"document_id":id,"expected_revision":2}),
        );
        assert_eq!(
            content(&undone["result"]),
            content(&before),
            "{mode}: one undo restores all units"
        );
    }
}

#[test]
fn both_distributions_report_stationary_endpoints_and_order_units_by_geometry() {
    for (mode, delta) in [
        ("distribute_horizontal", json!({"x":10.0,"y":0.0})),
        ("distribute_vertical", json!({"x":0.0,"y":10.0})),
    ] {
        let (app, _directory, id) = setup(&[
            (80.0, 80.0, 90.0, 90.0),
            (0.0, 0.0, 10.0, 10.0),
            (30.0, 30.0, 40.0, 40.0),
        ]);
        let planned = preview(&app, &id, &["s4", "s2", "s3"], mode);
        assert_eq!(planned["outcome"], "ok", "{planned}");
        let units = planned["result"]["units"].as_array().unwrap();
        assert_eq!(
            units
                .iter()
                .map(|unit| unit["object"].clone())
                .collect::<Vec<_>>(),
            vec![json!("s3"), json!("s4"), json!("s2")]
        );
        assert_eq!(units[0]["delta"], json!({"x":0.0,"y":0.0}));
        assert_eq!(units[1]["delta"], delta);
        assert_eq!(units[2]["delta"], json!({"x":0.0,"y":0.0}));
        assert_eq!(
            planned["result"]["scope"]["graphics"],
            json!(["s2", "s3", "s4"])
        );
        let committed = call(
            &app,
            "document.transaction",
            json!({"document_id":id,"expected_revision":1,
            "commands":[{"op":"arrange_objects","objects":["s4","s2","s3"],"mode":mode}]}),
        );
        assert_eq!(committed["outcome"], "ok", "{committed}");
        let dx = delta["x"].as_f64().unwrap();
        let dy = delta["y"].as_f64().unwrap();
        assert_eq!(
            graphic(&committed["result"], "s4")["points"],
            json!([{"x":30.0+dx,"y":30.0+dy},{"x":40.0+dx,"y":40.0+dy}])
        );
        assert_eq!(
            graphic(&committed["result"], "s3")["points"],
            json!([{"x":0.0,"y":0.0},{"x":10.0,"y":10.0}])
        );
        assert_eq!(
            graphic(&committed["result"], "s2")["points"],
            json!([{"x":80.0,"y":80.0},{"x":90.0,"y":90.0}])
        );
    }
}

#[test]
fn ancestor_units_collapse_descendants_and_selection_history_remains_independent() {
    let (app, _directory, id) = setup(&[
        (0.0, 0.0, 10.0, 20.0),
        (30.0, 40.0, 50.0, 60.0),
        (80.0, 80.0, 90.0, 90.0),
    ]);
    let grouped = call(
        &app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":1,
        "commands":[{"op":"group","objects":["s2","s3"]}]}),
    );
    assert_eq!(grouped["outcome"], "ok", "{grouped}");
    let before = snapshot(&app, &id);
    let planned = preview(&app, &id, &["s5", "s2", "s4", "s2"], "align_top");
    assert_eq!(planned["result"]["units"].as_array().unwrap().len(), 2);
    assert_eq!(planned["result"]["units"][0]["object"], "s4");
    assert_eq!(planned["result"]["units"][1]["object"], "s5");
    assert_eq!(
        planned["result"]["units"][1]["scope"],
        json!({"objects":["s2","s3","s5"],"positions":[],"graphics":["s2","s3"]})
    );
    let before_selection = json!({"objects":["s5"],"atoms":[],"bonds":[]});
    let after_selection = json!({"objects":["s4"],"atoms":[],"bonds":[]});
    let moved = call(
        &app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":2,
        "selection_before":before_selection,"selection_after":after_selection,
        "commands":[{"op":"arrange_objects","objects":["s5","s2","s4"],"mode":"align_top"}]}),
    );
    assert_eq!(moved["outcome"], "ok", "{moved}");
    assert_eq!(moved["result"]["document"]["selection"], after_selection);
    assert_eq!(
        graphic(&moved["result"], "s2")["points"],
        graphic(&before, "s2")["points"]
    );
    let undone = call(
        &app,
        "history.undo",
        json!({"document_id":id,"expected_revision":3}),
    );
    assert_eq!(undone["result"]["document"]["selection"], before_selection);
    let redone = call(
        &app,
        "history.redo",
        json!({"document_id":id,"expected_revision":4}),
    );
    assert_eq!(redone["result"]["document"]["selection"], after_selection);
    assert_eq!(content(&redone["result"]), content(&moved["result"]));
}

#[test]
fn invalid_plans_and_commands_share_domain_reasons_and_roll_back_prior_commands() {
    let (app, _directory, id) = setup(&[
        (0.0, 0.0, 50.0, 50.0),
        (10.0, 10.0, 60.0, 60.0),
        (20.0, 20.0, 70.0, 70.0),
    ]);
    let before = snapshot(&app, &id);
    for (objects, mode, reason) in [
        (vec!["s2"], "align_left", "arrangement_too_few_units"),
        (
            vec!["s2", "s3"],
            "distribute_horizontal",
            "arrangement_too_few_units",
        ),
        (vec!["s1", "s2"], "align_top", "arrangement_empty_geometry"),
        (vec!["s2", "s999"], "align_left", "object_unknown"),
        (
            vec!["s2", "s3", "s4"],
            "distribute_horizontal",
            "arrangement_negative_gap",
        ),
    ] {
        let planned = preview(&app, &id, &objects, mode);
        assert_eq!(planned["outcome"], "error", "{planned}");
        assert_eq!(planned["result"], Value::Null);
        assert_eq!(planned["diagnostics"][0]["details"]["reason"], reason);
        let failed = call(
            &app,
            "document.transaction",
            json!({"document_id":id,"expected_revision":1,"commands":[
                {"op":"rename","title":"Must roll back"},{"op":"arrange_objects","objects":objects,"mode":mode}
            ]}),
        );
        assert_eq!(
            failed["diagnostics"][0]["code"], "invalid_command",
            "{failed}"
        );
        assert_eq!(failed["diagnostics"][0]["details"]["reason"], reason);
        assert_eq!(failed["diagnostics"][0]["details"]["command_index"], 1);
        assert_eq!(snapshot(&app, &id), before);
    }
    let malformed = call(
        &app,
        "document.arrangement_plan",
        json!({"document_id":id,"objects":["s2","s3"],"mode":"align_left","atoms":["a1"]}),
    );
    assert_eq!(malformed["diagnostics"][0]["code"], "invalid_request");
    let mode = preview(&app, &id, &["s2", "s3"], "invented_mode");
    assert_eq!(mode["diagnostics"][0]["code"], "invalid_request");
    let stale = call(
        &app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":0,
        "commands":[{"op":"arrange_objects","objects":["s2","s3"],"mode":"align_left"}]}),
    );
    assert_eq!(stale["diagnostics"][0]["code"], "revision_conflict");
    assert_eq!(snapshot(&app, &id), before);
}

#[test]
fn a_noop_arrangement_preserves_revision_history_selection_and_redo() {
    let (app, _directory, id) = setup(&[(0.0, 0.0, 10.0, 10.0), (0.0, 20.0, 10.0, 30.0)]);
    let renamed = call(
        &app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":1,
        "commands":[{"op":"rename","title":"Temporary"}]}),
    );
    assert_eq!(renamed["outcome"], "ok", "{renamed}");
    let undone = call(
        &app,
        "history.undo",
        json!({"document_id":id,"expected_revision":2}),
    );
    assert_eq!(undone["result"]["session"]["redo_depth"], 1);
    let before = snapshot(&app, &id);
    let planned = preview(&app, &id, &["s2", "s3"], "align_left");
    assert_eq!(
        planned["result"]["units"][0]["delta"],
        json!({"x":0.0,"y":0.0})
    );
    assert_eq!(
        planned["result"]["units"][1]["delta"],
        json!({"x":0.0,"y":0.0})
    );
    let no_op = call(
        &app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":3,
        "commands":[{"op":"arrange_objects","objects":["s2","s3"],"mode":"align_left"}]}),
    );
    assert_eq!(no_op["document_revision"], 3, "{no_op}");
    assert_eq!(no_op["result"], before);
    assert_eq!(snapshot(&app, &id), before);
}
