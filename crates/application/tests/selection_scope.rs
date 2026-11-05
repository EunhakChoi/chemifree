//! Effective selection scope and atomic union movement through the shared operation contract.

use std::sync::atomic::{AtomicU64, Ordering};

use chemifree_application::Application;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};

type App = Application<LocalCapabilities, FileDocumentRepository>;

fn call(app: &App, operation: &str, parameters: Value) -> Value {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let request = json!({"contract":1,"request_id":format!("scope-{}", NEXT.fetch_add(1, Ordering::Relaxed)),
        "profile":"preserve-semantics","operation":operation,"parameters":parameters});
    serde_json::to_value(app.execute(request.to_string().as_bytes())).unwrap()
}

fn snapshot(app: &App, id: &str) -> Value {
    call(app, "document.snapshot", json!({"document_id":id}))["result"].clone()
}

/// s6 contains s5(s1 placement, s2 line) and s3 line. s4 is another placement.
/// A reference from s2 to b2 is deliberately not containment.
fn setup() -> (App, tempfile::TempDir, String) {
    let directory = tempfile::tempdir().unwrap();
    let app = Application::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    );
    let created = call(&app, "document.create", json!({"title":"Union scope"}));
    let id = created["result"]["document"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut commands: Vec<Value> = [0, 10, 20, 30]
        .into_iter()
        .map(|x| json!({"op":"add_atom","element":"C","position":{"x":x,"y":0}}))
        .collect();
    commands.extend([
        json!({"op":"add_bond","begin":"$0","end":"$1","order":"single"}),
        json!({"op":"add_bond","begin":"$1","end":"$2","order":"single"}),
        json!({"op":"add_graphic","shape":"line","points":[{"x":40,"y":0},{"x":45,"y":0}]}),
        json!({"op":"add_graphic","shape":"line","points":[{"x":80,"y":0},{"x":85,"y":0}]}),
        json!({"op":"add_instance","source":"s1","offset":{"x":100,"y":0}}),
        json!({"op":"group","objects":["s1","s2"]}),
        json!({"op":"group","objects":["s5","s3"]}),
        json!({"op":"add_reference","source":"s2","target":{"kind":"bond","id":"b2"}}),
    ]);
    let reply = call(
        &app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":0,"commands":commands}),
    );
    assert_eq!(reply["outcome"], "ok", "{reply}");
    assert_eq!(reply["document_revision"], 1);
    (app, directory, id)
}

fn scope(app: &App, id: &str, selection: Value, instance: Option<&str>) -> Value {
    let mut parameters = json!({"document_id":id,"selection":selection});
    if let Some(instance) = instance {
        parameters["instance"] = instance.into();
    }
    call(app, "document.selection_scope", parameters)
}

fn positions(instance: &str, atoms: &[&str]) -> Vec<Value> {
    atoms
        .iter()
        .map(|atom| json!({"instance":instance,"atom":atom}))
        .collect()
}

fn content(document: &Value) -> Value {
    let mut copy = document.clone();
    let object = copy.as_object_mut().unwrap();
    object.remove("revision");
    object.remove("selection");
    copy
}

#[test]
fn scope_reports_the_exact_union_and_snapshot_without_following_references_or_editing() {
    let (app, _directory, id) = setup();
    let before = snapshot(&app, &id);
    let reply = scope(
        &app,
        &id,
        json!({"objects":["s6","s5","s2","s2"],"atoms":["a1"],"bonds":["b1"]}),
        Some("s1"),
    );
    assert_eq!(reply["outcome"], "ok", "{reply}");
    assert_eq!(reply["result"]["snapshot"], before);
    assert_eq!(reply["document_revision"], 1);
    assert_eq!(
        reply["result"]["effective"],
        json!({
            "objects":["s1","s2","s3","s5","s6"],"graphics":["s2","s3"],
            "positions":positions("s1",&["a1","a2","a3","a4"])
        })
    );
    let second = scope(
        &app,
        &id,
        json!({"objects":["s1"],"bonds":["b1"]}),
        Some("s4"),
    );
    let expected: Vec<Value> = positions("s1", &["a1", "a2", "a3", "a4"])
        .into_iter()
        .chain(positions("s4", &["a1", "a2"]))
        .collect();
    assert_eq!(second["result"]["effective"]["positions"], json!(expected));
    let referenced = scope(&app, &id, json!({"objects":["s2"]}), None);
    assert_eq!(
        referenced["result"]["effective"],
        json!({"objects":["s2"],"graphics":["s2"],"positions":[]})
    );
    assert_eq!(snapshot(&app, &id), before);
}

#[test]
fn union_move_translates_overlaps_once_and_history_selection_is_independent() {
    let (app, _directory, id) = setup();
    let before = snapshot(&app, &id);
    let selected_before = json!({"objects":["s2"],"atoms":["a4"],"bonds":[]});
    let selected_after = json!({"objects":[],"atoms":[],"bonds":["b2"]});
    let moved = call(
        &app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":1,
        "selection_before":selected_before,"selection_after":selected_after,
        "commands":[{"op":"move_selection","selection":{"objects":["s6","s5","s2"],"atoms":["a1"],"bonds":["b1"]},"instance":"s1","delta":{"x":7,"y":-3}}]}),
    );
    assert_eq!(moved["outcome"], "ok", "{moved}");
    let document = &moved["result"]["document"];
    assert_eq!(moved["document_revision"], 2);
    assert_eq!(document["selection"], selected_after);
    let scene = &document["scene"];
    assert_eq!(
        scene["instances"][0]["positions"],
        json!([
            {"atom":"a1","x":7.0,"y":-3.0},{"atom":"a2","x":17.0,"y":-3.0},
            {"atom":"a3","x":27.0,"y":-3.0},{"atom":"a4","x":37.0,"y":-3.0}
        ])
    );
    assert_eq!(
        scene["instances"][1],
        before["document"]["scene"]["instances"][1]
    );
    let graphics: Vec<Value> = scene["objects"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|object| object["kind"] == "graphic")
        .map(|object| object["points"].clone())
        .collect();
    assert_eq!(
        graphics,
        json!([[{"x":47.0,"y":-3.0},{"x":52.0,"y":-3.0}],[{"x":87.0,"y":-3.0},{"x":92.0,"y":-3.0}]])
            .as_array()
            .unwrap()
            .clone()
    );
    assert_eq!(document["bonds"], before["document"]["bonds"]);
    assert_eq!(document["stereo"], before["document"]["stereo"]);
    let undone = call(
        &app,
        "history.undo",
        json!({"document_id":id,"expected_revision":2}),
    );
    assert_eq!(undone["result"]["document"]["selection"], selected_before);
    assert_eq!(
        content(&undone["result"]["document"]),
        content(&before["document"])
    );
    let redone = call(
        &app,
        "history.redo",
        json!({"document_id":id,"expected_revision":3}),
    );
    assert_eq!(redone["result"]["document"]["selection"], selected_after);
    assert_eq!(content(&redone["result"]["document"]), content(document));
}

#[test]
fn stale_revision_unknown_selection_and_coordinate_overflow_roll_back_the_whole_transaction() {
    let (app, _directory, id) = setup();
    let before = snapshot(&app, &id);
    let stale = call(
        &app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":0,
        "commands":[{"op":"move_selection","selection":{"atoms":["a1"]},"instance":"s1","delta":{"x":1,"y":0}}]}),
    );
    assert_eq!(
        stale["diagnostics"][0]["code"], "revision_conflict",
        "{stale}"
    );
    for (selection, delta) in [
        (json!({"atoms":["a999"]}), json!({"x":1,"y":0})),
        (json!({"objects":["s6"]}), json!({"x":100000,"y":0})),
    ] {
        let failed = call(
            &app,
            "document.transaction",
            json!({"document_id":id,"expected_revision":1,"commands":[
                {"op":"rename","title":"Must roll back"},
                {"op":"move_selection","selection":selection,"instance":"s1","delta":delta}
            ]}),
        );
        assert_eq!(failed["outcome"], "error", "{failed}");
        assert_eq!(failed["diagnostics"][0]["code"], "invalid_command");
        assert_eq!(failed["diagnostics"][0]["details"]["command_index"], 1);
        assert_eq!(snapshot(&app, &id), before);
    }
}

#[test]
fn scope_and_movement_share_instance_and_unknown_selection_refusals() {
    let (app, _directory, id) = setup();
    let before = snapshot(&app, &id);
    for (selection, instance, reason) in [
        (json!({"atoms":["a1"]}), None, None),
        (json!({}), Some("s2"), Some("object_kind_mismatch")),
        (
            json!({"atoms":["a999"]}),
            Some("s1"),
            Some("selection_unknown"),
        ),
    ] {
        let read = scope(&app, &id, selection.clone(), instance);
        assert_eq!(read["outcome"], "error", "{read}");
        assert_eq!(read["diagnostics"][0]["code"], "invalid_command");
        if let Some(reason) = reason {
            assert_eq!(read["diagnostics"][0]["details"]["reason"], reason);
        }
        let mut command =
            json!({"op":"move_selection","selection":selection,"delta":{"x":1,"y":1}});
        if let Some(instance) = instance {
            command["instance"] = instance.into();
        }
        let movement = call(
            &app,
            "document.transaction",
            json!({"document_id":id,"expected_revision":1,"commands":[command]}),
        );
        assert_eq!(movement["outcome"], "error", "{movement}");
        assert_eq!(
            read["diagnostics"][0]["message"],
            movement["diagnostics"][0]["message"]
                .as_str()
                .unwrap()
                .strip_prefix("command 0: ")
                .unwrap()
        );
        assert_eq!(snapshot(&app, &id), before);
    }
    let no_op = call(
        &app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":1,
        "commands":[{"op":"move_selection","selection":{},"delta":{"x":2,"y":3}}]}),
    );
    assert_eq!(no_op["document_revision"], 1, "{no_op}");
    assert_eq!(snapshot(&app, &id), before);
}
