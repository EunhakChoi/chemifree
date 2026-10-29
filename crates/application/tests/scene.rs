//! Scene objects and the selection through the shared application: the v1 commands, view, and
//! replay, the `reason` code of each scene refusal in the error envelope and the operation
//! diagnostic, and selection-only requests that record nothing.

use chemifree_application::{
    Application, ApplicationError, CommandDto, DocumentResponse, ErrorCode, SelectionDto,
};
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};
use tempfile::TempDir;

type App = Application<LocalCapabilities, FileDocumentRepository>;

fn application() -> (App, TempDir) {
    let data = tempfile::tempdir().unwrap();
    let app = Application::new(LocalCapabilities, FileDocumentRepository::new(data.path()));
    (app, data)
}

fn commands(value: Value) -> Vec<CommandDto> {
    serde_json::from_value(value).unwrap()
}

fn selection(value: Value) -> SelectionDto {
    serde_json::from_value(value).unwrap()
}

fn json_of(response: &DocumentResponse) -> Value {
    serde_json::to_value(response).unwrap()
}

/// A carbon drawn twice (`s1`, `s2` moved by 100) and a rectangle `s3`, at revision 2.
fn drawing(app: &App) -> String {
    let id = app
        .create_document(None, Some("Scene"))
        .unwrap()
        .document
        .id;
    app.apply_transaction(
        &id,
        None,
        0,
        commands(json!([{"op": "add_atom", "element": "C", "position": {"x": 0, "y": 0}}])),
    )
    .unwrap();
    app.apply_transaction(
        &id,
        None,
        1,
        commands(json!([
            {"op": "add_instance", "offset": {"x": 100, "y": 0}},
            {"op": "add_graphic", "shape": "rectangle", "points": [{"x": 0, "y": 50}, {"x": 40, "y": 70}]}
        ])),
    )
    .unwrap();
    id
}

fn refused(
    app: &App,
    id: &str,
    revision: u64,
    value: Value,
    chosen: Option<Value>,
) -> ApplicationError {
    let before = json_of(&app.document(id).unwrap());
    let error = app
        .apply_transaction_with_selection(
            id,
            None,
            revision,
            commands(value),
            chosen.map(selection),
        )
        .unwrap_err();
    assert_eq!(
        json_of(&app.document(id).unwrap()),
        before,
        "{error} changed nothing"
    );
    error
}

#[test]
fn the_view_lists_the_scene_tree_and_the_selection() {
    let (app, _data) = application();
    let id = drawing(&app);
    let response = app
        .apply_transaction_with_selection(
            &id,
            Some("group-1"),
            2,
            commands(json!([{"op": "group", "objects": ["s2", "s3"]}])),
            Some(selection(
                json!({"objects": ["s2", "s3"], "atoms": ["a1"], "bonds": []}),
            )),
        )
        .unwrap();
    let document = json_of(&response)["document"].clone();
    assert_eq!(
        document["scene"]["objects"],
        json!([
            {"kind": "molecule", "id": "s1", "parent": "p1", "molecule": "m1"},
            {"kind": "group", "id": "s4", "parent": "p1", "children": ["s2", "s3"]},
            {"kind": "molecule", "id": "s2", "parent": "s4", "molecule": "m1"},
            {"kind": "graphic", "id": "s3", "parent": "s4", "shape": "rectangle",
             "points": [{"x": 0.0, "y": 50.0}, {"x": 40.0, "y": 70.0}]}
        ])
    );
    let instances: Vec<&str> = document["scene"]["instances"]
        .as_array()
        .unwrap()
        .iter()
        .map(|instance| instance["id"].as_str().unwrap())
        .collect();
    assert_eq!(instances, ["s1", "s2"], "instances stay in creation order");
    assert_eq!(
        document["selection"],
        json!({"objects": ["s2", "s3"], "atoms": ["a1"], "bonds": []})
    );
}

#[test]
fn undo_and_redo_reply_with_the_selection_before_and_after() {
    let (app, _data) = application();
    let id = drawing(&app);
    app.apply_transaction_with_selection(
        &id,
        None,
        2,
        commands(json!([{"op": "group", "objects": ["s2", "s3"]}])),
        Some(selection(
            json!({"objects": ["s3"], "atoms": [], "bonds": []}),
        )),
    )
    .unwrap();
    // Both the group and its child are listed: the child's positions move once.
    let moved = app
        .apply_transaction_with_selection(
            &id,
            None,
            3,
            commands(json!([{"op": "move_objects", "objects": ["s4", "s2"], "offset": {"x": 5, "y": 0}}])),
            Some(selection(json!({"objects": ["s4", "s2"], "atoms": [], "bonds": []}))),
        )
        .unwrap();
    assert_eq!(moved.document.scene.instances[1].positions[0].x, 105.0);
    let undone = app.undo(&id, None, 4).unwrap();
    assert_eq!(undone.document.scene.instances[1].positions[0].x, 100.0);
    assert_eq!(
        json_of(&undone)["document"]["selection"],
        json!({"objects": ["s3"], "atoms": [], "bonds": []})
    );
    let redone = app.redo(&id, None, 5).unwrap();
    assert_eq!(
        json_of(&redone)["document"]["selection"],
        json!({"objects": ["s2", "s4"], "atoms": [], "bonds": []})
    );
}

#[test]
fn a_selection_only_change_records_nothing() {
    let (app, _data) = application();
    let id = drawing(&app);
    let before = json_of(&app.document(&id).unwrap());
    let response = app
        .apply_transaction_with_selection(
            &id,
            Some("select-only"),
            2,
            commands(json!([{"op": "move_objects", "objects": [], "offset": {"x": 3, "y": 3}}])),
            Some(selection(
                json!({"objects": ["s3"], "atoms": [], "bonds": []}),
            )),
        )
        .unwrap();
    assert_eq!(
        json_of(&response),
        before,
        "no revision, history entry, or selection"
    );
}

#[test]
fn a_replayed_transaction_returns_its_reply_and_a_changed_selection_reuses_the_id() {
    let (app, _data) = application();
    let id = drawing(&app);
    let body =
        || commands(json!([{"op": "move_objects", "objects": ["s3"], "offset": {"x": 1, "y": 0}}]));
    let chosen = || {
        Some(selection(
            json!({"objects": ["s3"], "atoms": [], "bonds": []}),
        ))
    };
    let first = app
        .apply_transaction_with_selection(&id, Some("move-1"), 2, body(), chosen())
        .unwrap();
    let again = app
        .apply_transaction_with_selection(&id, Some("move-1"), 2, body(), chosen())
        .unwrap();
    assert_eq!(first, again);
    assert_eq!(again.document.revision, 3, "applied once");
    let reused = app
        .apply_transaction_with_selection(&id, Some("move-1"), 2, body(), None)
        .unwrap_err();
    assert_eq!(reused.code, ErrorCode::RequestIdReused);
}

#[test]
fn each_scene_refusal_carries_its_reason_and_command_index() {
    let (app, _data) = application();
    let id = drawing(&app);
    let cases = [
        (
            json!({"op": "delete_objects", "objects": ["s3"]}),
            "delete_mode_missing",
        ),
        (json!({"op": "group", "objects": ["s9"]}), "object_unknown"),
        (
            json!({"op": "ungroup", "group": "s3"}),
            "object_kind_mismatch",
        ),
        (
            json!({"op": "reorder", "object": "s3", "index": 3}),
            "index_out_of_range",
        ),
        (
            json!({"op": "delete_objects", "objects": ["s1", "s2"], "mode": "object_only"}),
            "last_instance",
        ),
        (
            json!({"op": "delete_instance", "instance": "s3"}),
            "object_kind_mismatch",
        ),
    ];
    for (command, reason) in cases {
        let error = refused(
            &app,
            &id,
            2,
            json!([{"op": "rename", "title": "Renamed"}, command]),
            None,
        );
        assert_eq!(
            (error.code, error.command_index, error.reason_code()),
            (ErrorCode::InvalidCommand, Some(1), Some(reason)),
            "{command}: {error}"
        );
        let envelope = serde_json::to_value(error.to_response()).unwrap();
        assert_eq!(envelope["reason"], reason, "{command}");
    }
}

#[test]
fn a_group_of_objects_with_different_parents_is_refused() {
    let (app, _data) = application();
    let id = drawing(&app);
    app.apply_transaction(
        &id,
        None,
        2,
        commands(json!([{"op": "group", "objects": ["s3"]}])),
    )
    .unwrap();
    let error = refused(
        &app,
        &id,
        3,
        json!([{"op": "group", "objects": ["s1", "s3"]}]),
        None,
    );
    assert_eq!(error.reason_code(), Some("group_mixed_parents"));
}

#[test]
fn a_selection_naming_what_the_edit_removes_is_refused_without_a_command_index() {
    let (app, _data) = application();
    let id = drawing(&app);
    let error = refused(
        &app,
        &id,
        2,
        json!([{"op": "delete_objects", "objects": ["s3"], "mode": "subtree"}]),
        Some(json!({"objects": ["s3"], "atoms": [], "bonds": []})),
    );
    assert_eq!(
        (error.code, error.command_index, error.reason_code()),
        (ErrorCode::InvalidCommand, None, Some("selection_unknown"))
    );
    let error = refused(
        &app,
        &id,
        2,
        json!([{"op": "rename", "title": "Renamed"}]),
        Some(json!({"objects": ["x3"], "atoms": [], "bonds": []})),
    );
    assert_eq!(
        (error.code, error.reason_code()),
        (ErrorCode::InvalidRequest, None),
        "a malformed identifier makes the body malformed"
    );
}

#[test]
fn the_operation_diagnostic_carries_the_scene_reason_and_selection_runs_through_it() {
    let (app, _data) = application();
    let id = drawing(&app);
    let request = |request_id: &str, commands: Value, selection: Value| {
        json!({
            "contract": 1, "request_id": request_id, "profile": "preserve-semantics",
            "operation": "document.transaction",
            "parameters": {"document_id": id, "expected_revision": 2, "commands": commands, "selection": selection}
        })
    };
    let send = |request: Value| -> Value {
        serde_json::to_value(app.execute(request.to_string().as_bytes())).unwrap()
    };
    let refused = send(request(
        "op-1",
        json!([{"op": "delete_objects", "objects": ["s3"]}]),
        json!({"objects": [], "atoms": [], "bonds": []}),
    ));
    assert_eq!(refused["outcome"], "error");
    assert_eq!(
        refused["diagnostics"][0]["details"],
        json!({"command_index": 0, "reason": "delete_mode_missing"})
    );
    let done = send(request(
        "op-2",
        json!([{"op": "add_graphic", "shape": "arrow", "points": [{"x": 0, "y": 0}, {"x": 9, "y": 0}], "parent": null}]),
        json!({"objects": ["s4"], "atoms": [], "bonds": []}),
    ));
    assert_eq!(done["outcome"], "ok");
    assert_eq!(
        done["result"]["document"]["selection"],
        json!({"objects": ["s4"], "atoms": [], "bonds": []})
    );
}
