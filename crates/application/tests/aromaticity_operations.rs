use chemifree_application::Application;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};

fn call(
    app: &Application<LocalCapabilities, FileDocumentRepository>,
    id: &str,
    operation: &str,
    parameters: Value,
) -> Value {
    let request = json!({"contract": 1, "request_id": id, "profile": "preserve-semantics", "operation": operation, "parameters": parameters});
    serde_json::to_value(app.execute(request.to_string().as_bytes())).unwrap()
}

fn ring_commands(aromatic: bool, add_orphan: bool) -> Vec<Value> {
    let count = if add_orphan { 8 } else { 6 };
    let mut commands: Vec<_> = (0..count)
        .map(|i| json!({"op":"add_atom", "element":"C", "position":{"x":i * 20,"y":0}}))
        .collect();
    for i in 0..6 {
        let order = if aromatic {
            "aromatic"
        } else if i % 2 == 0 {
            "double"
        } else {
            "single"
        };
        commands.push(json!({"op":"add_bond", "begin":format!("${i}"), "end":format!("${}",(i+1)%6), "order":order}));
    }
    if add_orphan {
        commands.push(json!({"op":"add_bond", "begin":"$6", "end":"$7", "order":"aromatic"}));
    }
    commands
}

#[test]
fn inspect_convert_replay_and_history_keep_the_source_separate() {
    let directory = tempfile::tempdir().unwrap();
    let app = Application::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    );
    let id = app
        .create_document(None, Some("Benzene"))
        .unwrap()
        .document
        .id;
    let created = call(
        &app,
        "draw",
        "document.transaction",
        json!({"document_id":id,"expected_revision":0,"commands":ring_commands(false, false)}),
    );
    assert_eq!(created["outcome"], "ok", "{created}");
    let before = serde_json::to_value(app.document(&id).unwrap()).unwrap();
    let view = call(
        &app,
        "inspect",
        "chem.aromaticity",
        json!({"document_id":id}),
    );
    assert_eq!(view["outcome"], "ok", "{view}");
    assert_eq!(view["model_id"], "chemifree-aromaticity-1");
    assert_eq!(view["result"]["systems"][0]["classification"], "aromatic");
    assert_eq!(view["result"]["systems"][0]["electrons"], 6);
    assert_eq!(view["result"]["snapshot"]["document"], before["document"]);
    assert_eq!(
        serde_json::to_value(app.document(&id).unwrap()).unwrap(),
        before
    );

    let parameters = json!({"document_id":id,"expected_revision":1,"to":"aromatic"});
    let converted = call(
        &app,
        "convert",
        "chem.aromaticity_convert",
        parameters.clone(),
    );
    assert_eq!(converted["outcome"], "ok", "{converted}");
    assert_eq!(converted["document_revision"], 2);
    assert!(
        converted["result"]["document"]["bonds"]
            .as_array()
            .unwrap()
            .iter()
            .all(|bond| bond["order"] == "aromatic")
    );
    assert_eq!(
        call(&app, "convert", "chem.aromaticity_convert", parameters),
        converted,
        "replay returns the original response"
    );
    let reused = call(
        &app,
        "convert",
        "chem.aromaticity_convert",
        json!({"document_id":id,"expected_revision":1,"to":"kekule"}),
    );
    assert_eq!(reused["diagnostics"][0]["code"], "request_id_reused");
    let undone = call(
        &app,
        "undo",
        "history.undo",
        json!({"document_id":id,"expected_revision":2}),
    );
    assert_eq!(
        undone["result"]["document"]["bonds"],
        before["document"]["bonds"]
    );
    let redone = call(
        &app,
        "redo",
        "history.redo",
        json!({"document_id":id,"expected_revision":3}),
    );
    assert_eq!(
        redone["result"]["document"]["bonds"],
        converted["result"]["document"]["bonds"]
    );
    let kekule = call(
        &app,
        "kekule",
        "chem.aromaticity_convert",
        json!({"document_id":id,"expected_revision":4,"to":"kekule"}),
    );
    assert_eq!(kekule["outcome"], "ok", "{kekule}");
    let bonds = kekule["result"]["document"]["bonds"].as_array().unwrap();
    assert_eq!(bonds.iter().filter(|b| b["order"] == "double").count(), 3);
    assert_eq!(bonds.iter().filter(|b| b["order"] == "single").count(), 3);
    let unchanged = call(
        &app,
        "again",
        "chem.aromaticity_convert",
        json!({"document_id":id,"expected_revision":5,"to":"kekule"}),
    );
    assert_eq!(
        unchanged["document_revision"], 5,
        "a repeated conversion is a no-op"
    );
}

#[test]
fn failed_conversion_is_atomic_even_after_a_convertible_system() {
    let directory = tempfile::tempdir().unwrap();
    let app = Application::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    );
    let id = app
        .create_document(None, Some("Bad aromatic bond"))
        .unwrap()
        .document
        .id;
    let created = call(
        &app,
        "draw",
        "document.transaction",
        json!({"document_id":id,"expected_revision":0,"commands":ring_commands(true, true)}),
    );
    assert_eq!(created["outcome"], "ok", "{created}");
    let before = serde_json::to_value(app.document(&id).unwrap()).unwrap();
    let failed = call(
        &app,
        "bad",
        "chem.aromaticity_convert",
        json!({"document_id":id,"expected_revision":1,"to":"kekule"}),
    );
    assert_eq!(failed["outcome"], "error", "{failed}");
    assert_eq!(failed["model_id"], "chemifree-aromaticity-1");
    assert_eq!(
        failed["diagnostics"][0]["details"]["reason"],
        "aromatic_bond_outside_ring"
    );
    assert!(failed["result"].is_null());
    assert_eq!(
        serde_json::to_value(app.document(&id).unwrap()).unwrap(),
        before
    );
}
