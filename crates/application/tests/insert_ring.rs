//! The bounded recipe shares ordinary transaction replay, persistence and refusal semantics.

use chemifree_application::Application;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};

type App = Application<LocalCapabilities, FileDocumentRepository>;

fn request(app: &App, key: &str, operation: &str, parameters: Value) -> Value {
    let body = json!({"contract":1,"request_id":key,"profile":"preserve-semantics",
        "operation":operation,"parameters":parameters});
    serde_json::to_value(app.execute(body.to_string().as_bytes())).unwrap()
}

fn snapshot(app: &App, id: &str) -> Value {
    request(
        app,
        "snapshot",
        "document.snapshot",
        json!({"document_id":id}),
    )["result"]
        .clone()
}

#[test]
fn ring_replay_history_and_reopen_preserve_existing_drawing_and_selection() {
    for size in [None, Some(5), Some(6)] {
        ring_persistence(size, None);
    }
    ring_persistence(Some(5), Some("single"));
    ring_persistence(None, Some("single"));
    ring_persistence(None, Some("kekule"));
    ring_persistence(Some(6), Some("kekule"));
}

fn ring_persistence(size: Option<u8>, pattern: Option<&str>) {
    let directory = tempfile::tempdir().unwrap();
    let app = Application::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    );
    let created = request(
        &app,
        "create",
        "document.create",
        json!({"title":"Ring persistence"}),
    );
    let id = created["result"]["document"]["id"].as_str().unwrap();
    let seeded = request(
        &app,
        "seed",
        "document.transaction",
        json!({
        "document_id":id,"expected_revision":0,"commands":[
            {"op":"add_atom","element":"C","position":{"x":-40,"y":-30}},
            {"op":"add_atom","element":"O","position":{"x":-10,"y":-30},"bond_to":{"atom":"a1","order":"double"}},
            {"op":"set_bond_mark","bond":"b1","mark":{"kind":"crossed"}},
            {"op":"set_bond_configuration","bond":"b1","configuration":{"kind":"unknown"}},
            {"op":"add_graphic","shape":"arrow","points":[{"x":0,"y":0},{"x":5,"y":10}]},
            {"op":"add_reference","source":"s2","target":{"kind":"atom","id":"a1"}}
        ]}),
    );
    assert_eq!(seeded["outcome"], "ok", "{seeded}");
    let selection = json!({"objects":["s2"],"atoms":["a1"],"bonds":["b1"]});
    let mut command = json!({"op":"insert_ring","instance":"s1","center":{"x":100,"y":100}});
    if let Some(size) = size {
        command["size"] = json!(size);
    }
    if let Some(pattern) = pattern {
        command["pattern"] = json!(pattern);
    }
    let count = usize::from(size.unwrap_or(6));
    let parameters = json!({"document_id":id,"expected_revision":1,"commands":[command],
        "selection_before":selection});
    let inserted = request(&app, "ring", "document.transaction", parameters.clone());
    assert_eq!(inserted["outcome"], "ok", "{inserted}");
    assert_eq!(inserted["result"]["session"]["undo_depth"], 2);
    assert_eq!(
        inserted["result"]["document"]["atoms"]
            .as_array()
            .unwrap()
            .len(),
        2 + count
    );
    assert_eq!(
        inserted["result"]["document"]["bonds"]
            .as_array()
            .unwrap()
            .len(),
        1 + count
    );
    assert_eq!(inserted["result"]["document"]["selection"], selection);
    let orders: Vec<_> = inserted["result"]["document"]["bonds"]
        .as_array()
        .unwrap()
        .iter()
        .skip(1)
        .map(|bond| bond["order"].as_str().unwrap())
        .collect();
    let expected = if pattern == Some("kekule") {
        vec!["double", "single", "double", "single", "double", "single"]
    } else {
        vec!["single"; count]
    };
    assert_eq!(orders, expected);
    for field in ["title", "settings", "stereo"] {
        assert_eq!(
            inserted["result"]["document"][field], seeded["result"]["document"][field],
            "{field}"
        );
    }
    for field in ["roots", "objects", "references"] {
        assert_eq!(
            inserted["result"]["document"]["scene"][field],
            seeded["result"]["document"]["scene"][field],
            "{field}"
        );
    }
    assert_eq!(
        inserted["result"]["document"]["atoms"][0],
        seeded["result"]["document"]["atoms"][0]
    );
    assert_eq!(
        inserted["result"]["document"]["bonds"][0],
        seeded["result"]["document"]["bonds"][0]
    );
    assert_eq!(
        request(&app, "ring", "document.transaction", parameters.clone()),
        inserted
    );
    let mut changed_pattern = parameters.clone();
    changed_pattern["commands"][0]["pattern"] = json!(if pattern == Some("kekule") {
        "single"
    } else {
        "kekule"
    });
    let reused = request(&app, "ring", "document.transaction", changed_pattern);
    assert_eq!(
        reused["diagnostics"][0]["code"], "request_id_reused",
        "{reused}"
    );
    assert_eq!(
        snapshot(&app, id)["document"],
        inserted["result"]["document"]
    );
    let undone = request(
        &app,
        "undo",
        "history.undo",
        json!({"document_id":id,"expected_revision":2}),
    );
    assert_eq!(undone["outcome"], "ok");
    assert_eq!(
        undone["result"]["document"]["atoms"],
        seeded["result"]["document"]["atoms"]
    );
    assert_eq!(undone["result"]["document"]["selection"], selection);
    // A replay returns its immutable original response after a later history edit.
    assert_eq!(
        request(&app, "ring", "document.transaction", parameters),
        inserted
    );
    assert_eq!(snapshot(&app, id)["document"]["revision"], 3);
    let redone = request(
        &app,
        "redo",
        "history.redo",
        json!({"document_id":id,"expected_revision":3}),
    );
    assert_eq!(
        redone["result"]["document"]["atoms"],
        inserted["result"]["document"]["atoms"]
    );
    assert_eq!(
        redone["result"]["document"]["bonds"],
        inserted["result"]["document"]["bonds"]
    );
    assert_eq!(
        request(
            &app,
            "save",
            "document.save",
            json!({"document_id":id,"expected_revision":4})
        )["outcome"],
        "ok"
    );
    let stored = snapshot(&app, id)["document"].clone();
    drop(app);
    let reopened = Application::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    );
    assert_eq!(snapshot(&reopened, id)["document"], stored);
}

#[test]
fn invalid_ring_requests_leave_document_revision_selection_and_redo_unchanged() {
    let directory = tempfile::tempdir().unwrap();
    let app = Application::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    );
    let created = request(&app, "create", "document.create", json!({}));
    let id = created["result"]["document"]["id"].as_str().unwrap();
    let inserted = request(
        &app,
        "seed",
        "document.transaction",
        json!({"document_id":id,"expected_revision":0,
        "commands":[{"op":"insert_ring","instance":"s1","center":{"x":0,"y":0}}]}),
    );
    assert_eq!(inserted["outcome"], "ok", "{inserted}");
    assert_eq!(
        request(
            &app,
            "undo",
            "history.undo",
            json!({"document_id":id,"expected_revision":1})
        )["outcome"],
        "ok"
    );
    let before = snapshot(&app, id);
    for (index, (command, code)) in [
        (
            json!({"op":"insert_ring","center":{"x":0,"y":0}}),
            "invalid_request",
        ),
        (
            json!({"op":"insert_ring","instance":"s99","center":{"x":0,"y":0}}),
            "invalid_command",
        ),
        (
            json!({"op":"insert_ring","instance":"s1","center":{"x":99990,"y":0}}),
            "invalid_command",
        ),
        (
            json!({"op":"insert_ring","instance":"s1","center":{"x":0,"y":0},"attachment":"a1"}),
            "invalid_request",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let reply = request(
            &app,
            &format!("reject-{index}"),
            "document.transaction",
            json!({
            "document_id":id,"expected_revision":2,"commands":[{"op":"rename","title":"Must roll back"},command]}),
        );
        assert_eq!(reply["outcome"], "error", "{reply}");
        assert_eq!(reply["diagnostics"][0]["code"], code, "{reply}");
        assert_eq!(snapshot(&app, id), before);
    }
    let stale = request(
        &app,
        "stale",
        "document.transaction",
        json!({"document_id":id,"expected_revision":0,
        "commands":[{"op":"insert_ring","instance":"s1","center":{"x":0,"y":0}}]}),
    );
    assert_eq!(stale["diagnostics"][0]["code"], "revision_conflict");
    assert_eq!(snapshot(&app, id), before);
}

#[test]
fn malformed_ring_sizes_preserve_revision_history_and_redo() {
    let directory = tempfile::tempdir().unwrap();
    let app = Application::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    );
    let created = request(&app, "create", "document.create", json!({}));
    let id = created["result"]["document"]["id"].as_str().unwrap();
    request(
        &app,
        "seed",
        "document.transaction",
        json!({"document_id":id,
        "expected_revision":0,"commands":[{"op":"insert_ring","instance":"s1",
        "center":{"x":0,"y":0},"size":5}]}),
    );
    let undone = request(
        &app,
        "undo",
        "history.undo",
        json!({"document_id":id,"expected_revision":1}),
    );
    assert_eq!(undone["outcome"], "ok", "{undone}");
    let before = snapshot(&app, id);
    for (index, size) in [
        json!(null),
        json!(0),
        json!(4),
        json!(7),
        json!(-5),
        json!(5.5),
        json!("5"),
        json!(true),
    ]
    .into_iter()
    .enumerate()
    {
        let reply = request(
            &app,
            &format!("size-{index}"),
            "document.transaction",
            json!({
            "document_id":id,"expected_revision":2,"commands":[{"op":"rename","title":"Must roll back"},
            {"op":"insert_ring","instance":"s1","center":{"x":0,"y":0},"size":size}]}),
        );
        assert_eq!(
            reply["diagnostics"][0]["code"], "invalid_request",
            "{reply}"
        );
        assert_eq!(snapshot(&app, id), before, "size {size}");
    }
}

#[test]
fn omitted_and_explicit_six_have_identical_command_semantics() {
    use chemifree_application::CommandDto;
    let omitted: CommandDto =
        serde_json::from_value(json!({"op":"insert_ring","instance":"s1","center":{"x":0,"y":0}}))
            .unwrap();
    let explicit: CommandDto = serde_json::from_value(
        json!({"op":"insert_ring","instance":"s1","center":{"x":0,"y":0},"size":6}),
    )
    .unwrap();
    assert_eq!(omitted, explicit);
    let explicit_single: CommandDto = serde_json::from_value(
        json!({"op":"insert_ring","instance":"s1","center":{"x":0,"y":0},"size":6,"pattern":"single"}),
    )
    .unwrap();
    assert_eq!(omitted, explicit_single);
}

#[test]
fn unsupported_ring_patterns_preserve_revision_selection_and_redo() {
    let directory = tempfile::tempdir().unwrap();
    let app = Application::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    );
    let created = request(&app, "create", "document.create", json!({}));
    let id = created["result"]["document"]["id"].as_str().unwrap();
    let seeded = request(
        &app,
        "seed",
        "document.transaction",
        json!({
        "document_id":id,"expected_revision":0,"commands":[
        {"op":"insert_ring","instance":"s1","center":{"x":0,"y":0},"pattern":"kekule"}]}),
    );
    assert_eq!(seeded["outcome"], "ok", "{seeded}");
    let undone = request(
        &app,
        "undo",
        "history.undo",
        json!({"document_id":id,"expected_revision":1}),
    );
    assert_eq!(undone["outcome"], "ok", "{undone}");
    let before = snapshot(&app, id);
    for (index, pattern) in [
        json!(null),
        json!("aromatic"),
        json!("Kekule"),
        json!(""),
        json!(5),
        json!(true),
        json!({}),
        json!([]),
    ]
    .into_iter()
    .enumerate()
    {
        let refused = request(
            &app,
            &format!("pattern-{index}"),
            "document.transaction",
            json!({
            "document_id":id,"expected_revision":2,"commands":[
            {"op":"rename","title":"Must roll back"},
            {"op":"insert_ring","instance":"s1","center":{"x":0,"y":0},"pattern":pattern}]}),
        );
        assert_eq!(
            refused["diagnostics"][0]["code"], "invalid_request",
            "{refused}"
        );
        assert_eq!(snapshot(&app, id), before, "pattern {pattern}");
    }
    let five = request(
        &app,
        "five-kekule",
        "document.transaction",
        json!({
        "document_id":id,"expected_revision":2,"commands":[
        {"op":"rename","title":"Must roll back"},
        {"op":"insert_ring","instance":"s1","center":{"x":0,"y":0},"size":5,"pattern":"kekule"}]}),
    );
    assert_eq!(five["diagnostics"][0]["code"], "invalid_command", "{five}");
    assert_eq!(
        five["diagnostics"][0]["details"]["reason"],
        "ring_pattern_invalid"
    );
    assert_eq!(snapshot(&app, id), before);
    let attached = request(
        &app,
        "attach-pattern",
        "document.transaction",
        json!({
        "document_id":id,"expected_revision":2,"commands":[
        {"op":"attach_ring","instance":"s1","atom":"a1","direction":{"dx":1,"dy":0},"pattern":"kekule"}]}),
    );
    assert_eq!(
        attached["diagnostics"][0]["code"], "invalid_request",
        "{attached}"
    );
    assert_eq!(snapshot(&app, id), before);
}
