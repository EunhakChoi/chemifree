//! The operation contract, run against the real file store.

use chemifree_application::{Application, ListQuery};
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};
use tempfile::TempDir;

type App = Application<LocalCapabilities, FileDocumentRepository>;

fn application() -> (App, TempDir) {
    let data = tempfile::tempdir().unwrap();
    let app = Application::new(LocalCapabilities, FileDocumentRepository::new(data.path()));
    (app, data)
}

/// Sends `request` (a JSON value) and returns the reply as JSON.
fn send(app: &App, request: &Value) -> Value {
    serde_json::to_value(app.execute(request.to_string().as_bytes())).unwrap()
}

fn request(id: &str, operation: &str, parameters: Value) -> Value {
    json!({
        "contract": 1,
        "request_id": id,
        "profile": "preserve-semantics",
        "operation": operation,
        "parameters": parameters,
    })
}

fn document_count(app: &App) -> usize {
    app.list_documents(&ListQuery::default())
        .unwrap()
        .documents
        .len()
}

fn created(app: &App) -> String {
    let reply = send(
        app,
        &request("create-1", "document.create", json!({"title": "Contract"})),
    );
    reply["result"]["document"]["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn a_document_is_created_edited_undone_redone_saved_read_and_closed_through_the_contract() {
    let (app, _data) = application();
    let id = created(&app);

    let edited = send(
        &app,
        &request(
            "edit-1",
            "document.transaction",
            json!({"document_id": id, "expected_revision": 0, "commands": [
                {"op": "add_atom", "element": "C", "position": {"x": 0.0, "y": 0.0}},
                {"op": "add_atom", "element": "O", "position": {"x": 20.0, "y": 0.0}, "bond_to": {"atom": "$0", "order": "single"}}
            ]}),
        ),
    );
    assert_eq!(edited["envelope"], "operation_result");
    assert_eq!(edited["outcome"], "ok");
    assert_eq!(edited["profile"], "preserve-semantics");
    assert_eq!(edited["document_revision"], 1);
    assert_eq!(
        edited["result"]["document"]["revision"], 1,
        "the revision field restates the result's"
    );
    assert_eq!(
        edited["result"]["document"]["atoms"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(edited["result"]["session"]["unsaved_changes"], true);
    assert_eq!(
        edited["model_id"],
        Value::Null,
        "no model produced a drawing edit"
    );
    assert_eq!(edited["diagnostics"], json!([]));
    assert_eq!(edited["loss_report"], json!([]));

    let undone = send(
        &app,
        &request(
            "undo-1",
            "history.undo",
            json!({"document_id": id, "expected_revision": 1}),
        ),
    );
    assert_eq!(undone["document_revision"], 2);
    assert_eq!(undone["result"]["document"]["atoms"], json!([]));
    let redone = send(
        &app,
        &request(
            "redo-1",
            "history.redo",
            json!({"document_id": id, "expected_revision": 2}),
        ),
    );
    assert_eq!(
        redone["result"]["document"]["atoms"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let saved = send(
        &app,
        &request(
            "save-1",
            "document.save",
            json!({"document_id": id, "expected_revision": 3}),
        ),
    );
    assert_eq!(saved["result"]["session"]["unsaved_changes"], false);

    let closed = send(
        &app,
        &request("close-1", "document.close", json!({"document_id": id})),
    );
    assert_eq!(closed["outcome"], "ok");
    assert_eq!(
        closed["result"],
        json!({"document_id": id, "discarded_changes": false})
    );
    assert_eq!(closed["document_revision"], Value::Null);

    let read = send(
        &app,
        &request("read-1", "document.snapshot", json!({"document_id": id})),
    );
    assert_eq!(
        read["result"]["document"]["atoms"]
            .as_array()
            .unwrap()
            .len(),
        2,
        "the saved drawing reopens"
    );
}

#[test]
fn a_failure_carries_the_same_code_and_facts_as_the_v1_error() {
    let (app, _data) = application();
    let id = created(&app);

    let stale = send(
        &app,
        &request(
            "stale-1",
            "document.transaction",
            json!({"document_id": id, "expected_revision": 5, "commands": []}),
        ),
    );
    let v1 = app
        .apply_transaction(&id, Some("stale-2"), 5, Vec::new())
        .unwrap_err()
        .to_response();
    assert_eq!(stale["outcome"], "error");
    assert_eq!(stale["result"], Value::Null);
    assert_eq!(stale["diagnostics"][0]["code"], v1.code);
    assert_eq!(stale["diagnostics"][0]["severity"], "error");
    assert_eq!(
        stale["diagnostics"][0]["details"],
        json!({"current_revision": v1.current_revision.unwrap()})
    );
    assert_eq!(
        stale["document_revision"], 0,
        "the conflict names the current revision"
    );

    let invalid = send(
        &app,
        &request(
            "bad-1",
            "document.transaction",
            json!({"document_id": id, "expected_revision": 0, "commands": [
                {"op": "add_atom", "element": "C", "position": {"x": 0.0, "y": 0.0}},
                {"op": "update_atom", "atom": "a9", "charge": 1}
            ]}),
        ),
    );
    assert_eq!(invalid["diagnostics"][0]["code"], "invalid_command");
    assert_eq!(invalid["diagnostics"][0]["path"], "parameters.commands[1]");
    assert_eq!(
        invalid["diagnostics"][0]["details"],
        json!({"command_index": 1})
    );
    let after = send(
        &app,
        &request("read-1", "document.snapshot", json!({"document_id": id})),
    );
    assert_eq!(
        after["result"]["document"]["revision"], 0,
        "a failed transaction changes nothing"
    );

    let missing = send(
        &app,
        &request(
            "read-2",
            "document.snapshot",
            json!({"document_id": "doc-ffffffffffffffff"}),
        ),
    );
    assert_eq!(missing["diagnostics"][0]["code"], "document_not_found");
}

#[test]
fn a_repeated_request_id_replays_its_reply_and_a_reused_one_is_refused() {
    let (app, _data) = application();
    let id = created(&app);
    let edit = request(
        "edit-1",
        "document.transaction",
        json!({"document_id": id, "expected_revision": 0, "commands": [{"op": "rename", "title": "Once"}]}),
    );
    let first = send(&app, &edit);
    let again = send(&app, &edit);
    assert_eq!(again, first, "the same request is answered from the record");
    let other = request(
        "edit-1",
        "document.transaction",
        json!({"document_id": id, "expected_revision": 1, "commands": [{"op": "rename", "title": "Twice"}]}),
    );
    assert_eq!(
        send(&app, &other)["diagnostics"][0]["code"],
        "request_id_reused"
    );
}

#[test]
fn invalid_parameters_of_a_known_operation_are_an_error_result_that_changes_nothing() {
    let (app, _data) = application();
    let reply = send(
        &app,
        &request(
            "create-1",
            "document.create",
            json!({"title": "x", "color": "red"}),
        ),
    );
    assert_eq!(reply["envelope"], "operation_result");
    assert_eq!(reply["outcome"], "error");
    assert_eq!(reply["diagnostics"][0]["code"], "invalid_request");
    assert_eq!(reply["diagnostics"][0]["path"], "parameters");
    assert_eq!(document_count(&app), 0);
}

#[test]
fn requests_that_cannot_be_read_get_a_protocol_error() {
    let (app, _data) = application();
    let cases: [(&str, Value, &str, &str); 11] = [
        (
            "not JSON",
            Value::String("{".into()),
            "malformed_request",
            "error",
        ),
        ("not an object", json!([1]), "malformed_request", "error"),
        (
            "another contract",
            json!({"contract": 2, "request_id": "r-1", "profile": "preserve-semantics", "operation": "document.create", "parameters": {}}),
            "unsupported_contract",
            "unsupported",
        ),
        (
            "no contract",
            json!({"request_id": "r-1", "profile": "preserve-semantics", "operation": "document.create", "parameters": {}}),
            "malformed_request",
            "error",
        ),
        (
            "an unknown field",
            json!({"contract": 1, "request_id": "r-1", "profile": "preserve-semantics", "operation": "document.create", "parameters": {}, "extra": 1}),
            "malformed_request",
            "error",
        ),
        (
            "an unknown operation",
            request("r-1", "document.teleport", json!({})),
            "unknown_operation",
            "unsupported",
        ),
        (
            "an unknown profile",
            json!({"contract": 1, "request_id": "r-1", "profile": "fast", "operation": "document.create", "parameters": {}}),
            "unknown_profile",
            "error",
        ),
        (
            "a malformed request ID",
            request("r 1", "document.create", json!({})),
            "invalid_request",
            "error",
        ),
        (
            "a repeated field",
            Value::String(r#"{"contract": 1, "request_id": "r-1", "profile": "preserve-semantics", "operation": "capabilities.inspect", "operation": "document.create", "parameters": {}}"#.into()),
            "malformed_request",
            "error",
        ),
        (
            "a repeated parameter",
            Value::String(r#"{"contract": 1, "request_id": "r-1", "profile": "preserve-semantics", "operation": "document.create", "parameters": {"title": "A", "title": "B"}}"#.into()),
            "malformed_request",
            "error",
        ),
        (
            "a contract that is not an integer",
            json!({"contract": 1.0, "request_id": "r-1", "profile": "preserve-semantics", "operation": "document.create", "parameters": {}}),
            "malformed_request",
            "error",
        ),
    ];
    for (case, value, code, outcome) in cases {
        let text = match &value {
            Value::String(raw) => raw.clone(),
            other => other.to_string(),
        };
        let reply = serde_json::to_value(app.execute(text.as_bytes())).unwrap();
        assert_eq!(reply["envelope"], "protocol_error", "{case}");
        assert_eq!(reply["diagnostics"][0]["code"], code, "{case}");
        assert_eq!(reply["diagnostics"][0]["severity"], "error", "{case}");
        assert_eq!(reply["outcome"], outcome, "{case}");
        assert_eq!(reply["result"], Value::Null, "{case}");
        assert_eq!(reply["profile"], Value::Null, "{case}");
    }
    assert_eq!(document_count(&app), 0, "nothing was created");

    let unknown = send(&app, &request("r-2", "document.teleport", json!({})));
    assert_eq!(unknown["request_id"], "r-2");
    assert_eq!(
        unknown["operation"], "document.teleport",
        "the unknown name is echoed"
    );
    let future = send(
        &app,
        &json!({"contract": 2, "request_id": "r-3", "operation": "x"}),
    );
    assert_eq!(
        future["diagnostics"][0]["details"],
        json!({"supported": [1]})
    );
}

#[test]
fn nothing_runs_under_the_legacy_profile_except_inspecting_capabilities() {
    let (app, _data) = application();
    let mut create = request("create-1", "document.create", json!({}));
    create["profile"] = json!("legacy-observed");
    let reply = send(&app, &create);
    assert_eq!(reply["outcome"], "unsupported");
    assert_eq!(reply["diagnostics"][0]["code"], "profile_unavailable");
    assert_eq!(reply["result"], Value::Null);
    assert_eq!(
        document_count(&app),
        0,
        "the refused request created nothing"
    );

    let mut inspect = request("caps-1", "capabilities.inspect", json!({}));
    inspect["profile"] = json!("legacy-observed");
    let catalog = send(&app, &inspect);
    assert_eq!(catalog["outcome"], "ok");
    let profiles = &catalog["result"]["profiles"];
    assert_eq!(profiles[1]["id"], "legacy-observed");
    assert_eq!(profiles[1]["execution"], "not_implemented");
}

#[test]
fn an_unavailable_prediction_model_returns_no_scientific_result() {
    let (app, _data) = application();
    let reply = send(
        &app,
        &request(
            "predict",
            "prediction.evaluate",
            json!({"model_id": "missing"}),
        ),
    );
    assert_eq!(reply["outcome"], "unsupported");
    assert_eq!(reply["result"], Value::Null);
    assert_eq!(reply["diagnostics"][0]["code"], "model_unavailable");
}

/// Whatever the catalog says runs does run, and whatever it says does not run is refused as
/// unsupported, with no result and no change. This keeps the catalog and the dispatcher from
/// drifting apart.
#[test]
fn the_catalog_says_exactly_which_operations_run() {
    let (app, _data) = application();
    let id = created(&app);
    let catalog = serde_json::to_value(app.operation_catalog()).unwrap();
    let operations = catalog["operations"].as_array().unwrap();
    assert_eq!(operations.len(), 21);
    let minimal = |operation: &str| match operation {
        "capabilities.inspect" => json!({}),
        "document.create" => json!({}),
        "document.snapshot" | "document.close" | "chem.validate" => json!({"document_id": id}),
        "chem.properties" => json!({"document_id": id, "scope": "molecule", "property": "formula"}),
        "document.transaction" => {
            json!({"document_id": id, "expected_revision": 0, "commands": []})
        }
        _ => json!({"document_id": id, "expected_revision": 0}),
    };
    let mut executable = Vec::new();
    for (index, entry) in operations.iter().enumerate() {
        let operation = entry["id"].as_str().unwrap();
        let reply = send(
            &app,
            &request(&format!("probe-{index}"), operation, minimal(operation)),
        );
        assert_eq!(reply["envelope"], "operation_result", "{operation}");
        match entry["execution"].as_str().unwrap() {
            "executable" => {
                assert_ne!(
                    reply["outcome"], "unsupported",
                    "{operation} is marked executable: {reply}"
                );
                executable.push(operation);
            }
            other => {
                assert_eq!(
                    reply["outcome"], "unsupported",
                    "{operation} is marked {other}"
                );
                assert_eq!(
                    reply["result"],
                    Value::Null,
                    "{operation}: no made-up result"
                );
                let code = if operation == "prediction.evaluate" {
                    "model_unavailable"
                } else {
                    "operation_not_implemented"
                };
                assert_eq!(reply["diagnostics"][0]["code"], code, "{operation}");
                assert_eq!(
                    reply["diagnostics"][0]["details"]["followup"], entry["followup"],
                    "{operation}"
                );
                assert_eq!(entry["representation"], "not_supported", "{operation}");
            }
        }
    }
    assert_eq!(
        executable,
        [
            "capabilities.inspect",
            "document.create",
            "document.snapshot",
            "document.transaction",
            "history.undo",
            "history.redo",
            "document.save",
            "document.close",
            "chem.validate",
            "chem.properties",
        ]
    );
    assert_eq!(
        document_count(&app),
        2,
        "only document.create created a document"
    );
}

#[test]
fn the_catalog_separates_what_a_transaction_holds_from_what_it_cannot_yet() {
    let (app, _data) = application();
    let catalog = serde_json::to_value(app.operation_catalog()).unwrap();
    let transaction = catalog["operations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["id"] == "document.transaction")
        .unwrap();
    let parts: Vec<(&str, &str, &str)> = transaction["sub_operations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|part| {
            (
                part["id"].as_str().unwrap(),
                part["representation"].as_str().unwrap(),
                part["execution"].as_str().unwrap(),
            )
        })
        .collect();
    assert!(parts.contains(&("add_bond", "supported", "executable")));
    assert!(parts.contains(&("hydrogen_state", "supported", "executable")));
    assert!(parts.contains(&("aromatic_or_dative_bond", "supported", "executable")));
    let validate = catalog["operations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["id"] == "chem.validate")
        .unwrap();
    assert_eq!(
        (
            validate["representation"].clone(),
            validate["execution"].clone()
        ),
        (json!("supported"), json!("executable")),
        "the bounded valence inspector is available"
    );
    let prediction = catalog["operations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["id"] == "prediction.evaluate")
        .unwrap();
    assert_eq!(
        prediction["execution"], "not_implemented",
        "no engine is claimed to be waiting for data"
    );
    assert_eq!(catalog["contracts"], json!([1]));
}

#[test]
fn valence_returns_the_actual_snapshot_and_does_not_edit_history() {
    let (app, _data) = application();
    let id = created(&app);
    let edited = send(
        &app,
        &request(
            "add-valence",
            "document.transaction",
            json!({
                "document_id": id, "expected_revision": 0, "commands": [
                    {"op":"add_atom", "element":"C", "position":{"x":0,"y":0}},
                    {"op":"add_atom", "element":"Fe", "position":{"x":20,"y":0}}
                ]
            }),
        ),
    );
    let inspected = send(
        &app,
        &request("inspect", "chem.validate", json!({"document_id":id})),
    );
    assert_eq!(inspected["outcome"], "ok");
    assert_eq!(inspected["model_id"], "chemifree-valence-2");
    assert_eq!(
        inspected["result"]["snapshot"]["document"],
        edited["result"]["document"]
    );
    assert_eq!(inspected["result"]["snapshot"]["session"]["undo_depth"], 1);
    assert_eq!(
        inspected["result"]["atoms"],
        json!([
            {"atom_id":"a1", "bond_order_sum":0, "hydrogens":4, "status":"valid", "reason":null},
            {"atom_id":"a2", "bond_order_sum":0, "hydrogens":null, "status":"unsupported", "reason":"element_or_charge"}
        ])
    );
    assert_eq!(
        inspected["diagnostics"][0]["details"]["reason"],
        "element_or_charge"
    );
    let after = send(
        &app,
        &request(
            "after-inspect",
            "document.snapshot",
            json!({"document_id":id}),
        ),
    );
    assert_eq!(after["result"]["document"], edited["result"]["document"]);
    assert_eq!(after["result"]["session"]["undo_depth"], 1);
    assert_eq!(after["result"]["session"]["redo_depth"], 0);
}

/// A transaction accepts exactly the commands its catalog entry lists as executable: each one
/// is a command the parser knows, and each part that does not run is not one. Scene placement
/// is the group the instance commands belong to, not a command.
#[test]
fn the_transaction_catalog_lists_exactly_the_commands_that_run() {
    let (app, _data) = application();
    let id = created(&app);
    let catalog = serde_json::to_value(app.operation_catalog()).unwrap();
    let transaction = catalog["operations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["id"] == "document.transaction")
        .unwrap();
    let mut commands = Vec::new();
    let mut capabilities: Vec<(String, Vec<String>)> = Vec::new();
    for (index, part) in transaction["sub_operations"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        let op = part["id"].as_str().unwrap();
        let reply = send(
            &app,
            &request(
                &format!("part-{index}"),
                "document.transaction",
                json!({"document_id": id, "expected_revision": 0, "commands": [{"op": op}]}),
            ),
        );
        // A command whose fields are all optional (`update_settings`) runs as a no-op.
        let message = match reply["outcome"].as_str() {
            Some("ok") => "",
            _ => reply["diagnostics"][0]["message"].as_str().unwrap(),
        };
        let unknown = message.contains("unknown variant");
        let executable = part["execution"] == "executable";
        // A row with `commands` is a capability carried out by those commands; one without
        // is itself a command, which parses exactly when it is executable.
        match part.get("commands") {
            Some(listed) => {
                assert!(unknown, "the capability {op} is not a command: {message}");
                let listed: Vec<String> = serde_json::from_value(listed.clone())
                    .unwrap_or_else(|error| panic!("{op}: commands is a list of names: {error}"));
                assert_eq!(
                    !listed.is_empty(),
                    executable,
                    "{op}: an executable capability names its commands, another names none"
                );
                capabilities.push((op.to_owned(), listed));
            }
            None => {
                assert_eq!(!unknown, executable, "{op}: {message}");
                commands.push(op);
            }
        }
    }
    for (capability, listed) in &capabilities {
        for command in listed {
            assert!(
                commands.contains(&command.as_str()),
                "{capability} names {command}, which is not an executable command row"
            );
        }
    }
    assert_eq!(
        commands,
        [
            "add_atom",
            "update_atom",
            "move_atoms",
            "delete_atoms",
            "add_bond",
            "update_bond",
            "delete_bonds",
            "rename",
            "add_instance",
            "move_instance",
            "delete_instance",
            "add_graphic",
            "group",
            "ungroup",
            "reorder",
            "delete_objects",
            "move_objects",
            "add_reference",
            "remove_reference",
            "update_settings",
        ]
    );
    let placement = capabilities
        .iter()
        .find(|(id, _)| id == "scene_placement")
        .expect("scene_placement is a capability row");
    assert_eq!(
        placement.1,
        ["add_instance", "move_instance", "delete_instance"]
    );
    let carried_out_by = |id: &str| -> Vec<String> {
        capabilities
            .iter()
            .find(|(capability, _)| capability == id)
            .unwrap_or_else(|| panic!("{id} is a capability row"))
            .1
            .clone()
    };
    assert_eq!(
        carried_out_by("scene_tree"),
        [
            "add_graphic",
            "group",
            "ungroup",
            "reorder",
            "delete_objects",
            "move_objects",
            "add_reference",
            "remove_reference",
        ]
    );
    assert_eq!(
        carried_out_by("hydrogen_state"),
        ["add_atom", "update_atom"]
    );
    assert_eq!(carried_out_by("radical"), ["add_atom", "update_atom"]);
    assert_eq!(
        carried_out_by("query_atom_or_bond"),
        ["add_atom", "update_atom", "add_bond", "update_bond"]
    );
    assert_eq!(
        carried_out_by("aromatic_or_dative_bond"),
        ["add_atom", "add_bond", "update_bond"]
    );
    assert_eq!(
        carried_out_by("attachment_point"),
        ["add_atom", "update_atom"]
    );
    assert_eq!(carried_out_by("document_settings"), ["update_settings"]);
    assert_eq!(capabilities.len(), 8, "{capabilities:?}");
    assert_eq!(document_count(&app), 1, "no part changed anything");
}
