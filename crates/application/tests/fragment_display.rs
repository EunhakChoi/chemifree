//! Public operations retain graph science and report fragment display loss explicitly.

use std::sync::atomic::{AtomicU64, Ordering};

use chemifree_application::Application;
use chemifree_providers::interchange_codec::LocalInterchangeCodec;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities, OpenSmilesCodec, V2000Codec};
use serde_json::{Value, json};

type App = Application<LocalCapabilities, FileDocumentRepository>;
fn send(app: &App, request_id: &str, operation: &str, parameters: Value) -> Value {
    serde_json::to_value(app.execute(json!({"contract":1,"request_id":request_id,"profile":"preserve-semantics","operation":operation,"parameters":parameters}).to_string().as_bytes())).unwrap()
}
fn call(app: &App, operation: &str, parameters: Value) -> Value {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    send(
        app,
        &format!("fragment-{}", NEXT.fetch_add(1, Ordering::Relaxed)),
        operation,
        parameters,
    )
}
fn setup() -> (App, tempfile::TempDir, String) {
    let root = tempfile::tempdir().unwrap();
    let app = Application::new(LocalCapabilities, FileDocumentRepository::new(root.path()))
        .with_ctfile_codec(V2000Codec)
        .with_smiles_codec(OpenSmilesCodec)
        .with_interchange_codec(LocalInterchangeCodec);
    let created = call(
        &app,
        "document.create",
        json!({"title":"Explicit branched graph"}),
    );
    let id = created["result"]["document"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let drawn = call(
        &app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":0,"commands":[
            {"op":"add_atom","element":"O","position":{"x":0,"y":0}},
            {"op":"add_atom","element":"C","position":{"x":30,"y":0},"bond_to":{"atom":"$0","order":"single"}},
            {"op":"add_atom","element":"C","position":{"x":45,"y":25},"bond_to":{"atom":"$1","order":"single"}},
            {"op":"add_atom","element":"C","position":{"x":45,"y":-25},"bond_to":{"atom":"$1","order":"single"}}
        ]}),
    );
    assert_eq!(drawn["outcome"], "ok", "{drawn}");
    (app, root, id)
}
fn snapshot(app: &App, id: &str) -> Value {
    call(app, "document.snapshot", json!({"document_id":id}))["result"].clone()
}
fn contract(id: &str) -> Value {
    json!({"document_id":id,"expected_revision":1,"commands":[{"op":"contract_fragment","instance":"s1","anchor":"a2","members":["a2","a3","a4"],"text":"iPr"}]})
}

#[test]
fn replay_save_reopen_recovery_and_undo_retain_metadata_and_exact_graph() {
    let (app, root, id) = setup();
    let before = snapshot(&app, &id);
    let parameters = contract(&id);
    let result = send(
        &app,
        "contract-once",
        "document.transaction",
        parameters.clone(),
    );
    assert_eq!(result["outcome"], "ok", "{result}");
    assert_eq!(
        send(&app, "contract-once", "document.transaction", parameters),
        result
    );
    let display = json!([{"instance":"s1","anchor":"a2","members":["a2","a3","a4"],"text":"iPr","runs":[{"text":"iPr","script":"baseline"}]}]);
    assert_eq!(result["result"]["document"]["fragment_displays"], display);
    for field in [
        "atoms",
        "bonds",
        "scene",
        "stereo",
        "settings",
        "abbreviations",
    ] {
        assert_eq!(
            result["result"]["document"][field],
            before["document"][field]
        );
    }
    app.save(&id, Some("save-fragment"), 2).unwrap();
    app.close(&id, Some("close-fragment"), false, Some(2))
        .unwrap();
    assert_eq!(
        serde_json::to_value(app.document(&id).unwrap()).unwrap()["document"]["fragment_displays"],
        display
    );
    let edited = call(
        &app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":2,"commands":[{"op":"set_fragment_display_text","instance":"s1","anchor":"a2","text":"한 e\u{301} 🧪"}]}),
    );
    assert_eq!(edited["outcome"], "ok", "{edited}");
    app.autosave(&id, Some("draft-fragment")).unwrap();
    drop(app);
    let app = Application::new(LocalCapabilities, FileDocumentRepository::new(root.path()));
    app.document(&id).unwrap();
    let recovered = app.recover(&id, Some("recover-fragment")).unwrap();
    assert_eq!(
        recovered.document.fragment_displays[0].text,
        "한 e\u{301} 🧪"
    );
    let undo = call(
        &app,
        "history.undo",
        json!({"document_id":id,"expected_revision":recovered.document.revision}),
    );
    assert_eq!(undo["result"]["document"]["fragment_displays"], display);
}

#[test]
fn arbitrary_text_keeps_scientific_results_and_all_exports_write_the_complete_graph() {
    let (app, _root, id) = setup();
    let inspections = [
        ("chem.validate", json!({"document_id":id})),
        ("chem.aromaticity", json!({"document_id":id})),
        ("chem.identity", json!({"document_id":id,"mode":"recorded"})),
        ("chem.identity", json!({"document_id":id,"mode":"chemical"})),
        (
            "chem.properties",
            json!({"document_id":id,"scope":"molecule","property":"formula"}),
        ),
        (
            "chem.properties",
            json!({"document_id":id,"scope":"molecule","property":"masses"}),
        ),
    ];
    let inspect = |app: &App| -> Vec<Value> {
        inspections.iter().map(|(operation,parameters)| {
        let mut result=call(app,operation,parameters.clone());
        if let Some(body)=result["result"].as_object_mut() { body.remove("snapshot");body.remove("revision"); }
        json!({"outcome":result["outcome"],"model_id":result["model_id"],"result":result["result"],"diagnostics":result["diagnostics"]})
    }).collect()
    };
    let science = inspect(&app);
    let formats = [
        json!({"records":[{"document_id":id}],"format":"mol"}),
        json!({"records":[{"document_id":id}],"format":"sdf"}),
        json!({"document_id":id,"format":"smiles","style":"recorded"}),
        json!({"document_id":id,"format":"cdxml"}),
        json!({"document_id":id,"format":"cdx"}),
    ];
    let original: Vec<Value> = formats
        .iter()
        .map(|parameters| call(&app, "document.export", parameters.clone()))
        .collect();
    let contracted = call(&app, "document.transaction", contract(&id));
    assert_eq!(contracted["outcome"], "ok", "{contracted}");
    assert_eq!(inspect(&app), science);
    let mislabeled = call(
        &app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":2,"commands":[{"op":"set_fragment_display_text","instance":"s1","anchor":"a2","text":"OMe"}]}),
    );
    assert_eq!(mislabeled["outcome"], "ok", "{mislabeled}");
    assert_eq!(inspect(&app), science);
    let rich = call(
        &app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":3,"commands":[{"op":"set_fragment_display_runs","instance":"s1","anchor":"a2","runs":[{"text":"C","script":"baseline"},{"text":"3","script":"subscript"},{"text":"H7","script":"baseline"},{"text":"+","script":"superscript"}]}]}),
    );
    assert_eq!(rich["outcome"], "ok", "{rich}");
    assert_eq!(inspect(&app), science);
    let formula = call(
        &app,
        "chem.properties",
        json!({"document_id":id,"scope":"molecule","property":"formula"}),
    );
    assert_eq!(
        formula["result"]["components"][0]["formula"], "C3H8O",
        "display text never defines OMe chemistry"
    );
    for (parameters, original) in formats.iter().zip(&original) {
        let exported = call(&app, "document.export", parameters.clone());
        assert_eq!(
            exported["result"]["content"], original["result"]["content"],
            "{exported}"
        );
        let losses: Vec<_> = exported["loss_report"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|loss| loss["feature"] == "abbreviation_display")
            .collect();
        assert_eq!(losses.len(), 1, "{exported}");
        assert_eq!(losses[0]["semantic_impact"], "rendering");
    }
}

#[test]
fn invalid_commands_and_native_copy_overlaps_refuse_atomically() {
    let (app, _root, id) = setup();
    let before = snapshot(&app, &id);
    let invalid = call(
        &app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":1,"commands":[{"op":"rename","title":"roll back"},{"op":"contract_fragment","instance":"s1","anchor":"a2","members":["a2","a3"],"text":"x"}]}),
    );
    assert_eq!(
        invalid["diagnostics"][0]["details"]["reason"], "fragment_display_invalid",
        "{invalid}"
    );
    assert_eq!(snapshot(&app, &id), before);
    let contracted = call(&app, "document.transaction", contract(&id));
    assert_eq!(contracted["outcome"], "ok");
    let before = snapshot(&app, &id);
    for member in ["a2", "a3", "a4"] {
        let copied = call(
            &app,
            "document.clipboard_copy",
            json!({"document_id":id,"instance":"s1","selection":{"atoms":[member]}}),
        );
        assert_eq!(
            copied["diagnostics"][0]["details"]["reason"],
            "clipboard_abbreviation_display_unsupported",
            "{copied}"
        );
        assert_eq!(snapshot(&app, &id), before);
    }
    for text in [
        json!(null),
        json!(""),
        json!("a\nb"),
        json!("한".repeat(43)),
        json!(42),
    ] {
        let edited = call(
            &app,
            "document.transaction",
            json!({"document_id":id,"expected_revision":2,"commands":[{"op":"set_fragment_display_text","instance":"s1","anchor":"a2","text":text}]}),
        );
        assert_eq!(edited["outcome"], "error", "{edited}");
        assert_eq!(snapshot(&app, &id), before);
    }
}

#[test]
fn rich_run_edit_replays_as_one_transaction_and_plain_text_resets_scripts() {
    let (app, _root, id) = setup();
    call(&app, "document.transaction", contract(&id));
    let original = snapshot(&app, &id);
    let runs = json!([{"text":"C","script":"baseline"},{"text":"3","script":"subscript"},{"text":"H","script":"baseline"},{"text":"7","script":"subscript"},{"text":"+","script":"superscript"}]);
    let parameters = json!({"document_id":id,"expected_revision":2,"commands":[{"op":"set_fragment_display_runs","instance":"s1","anchor":"a2","runs":runs}]});
    let result = send(
        &app,
        "rich-once",
        "document.transaction",
        parameters.clone(),
    );
    assert_eq!(result["outcome"], "ok", "{result}");
    assert_eq!(
        send(&app, "rich-once", "document.transaction", parameters),
        result
    );
    assert_eq!(
        result["result"]["document"]["fragment_displays"][0]["text"],
        "C3H7+"
    );
    assert_eq!(
        result["result"]["document"]["fragment_displays"][0]["runs"],
        runs
    );
    for field in ["atoms", "bonds", "scene", "stereo", "settings"] {
        assert_eq!(
            result["result"]["document"][field],
            original["document"][field]
        );
    }
    for invalid in [
        json!([]),
        json!([{"text":"x"}]),
        json!([{"text":"x","script":"unknown"}]),
        json!([{"text":"x","script":"baseline","color":"red"}]),
        json!([{"text":"","script":"baseline"}]),
        json!([{"text":"a\nb","script":"baseline"}]),
    ] {
        let before = snapshot(&app, &id);
        let result = call(
            &app,
            "document.transaction",
            json!({"document_id":id,"expected_revision":3,"commands":[{"op":"set_fragment_display_runs","instance":"s1","anchor":"a2","runs":invalid}]}),
        );
        assert_eq!(result["outcome"], "error", "{result}");
        assert_eq!(snapshot(&app, &id), before);
    }
    let reset = call(
        &app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":3,"commands":[{"op":"set_fragment_display_text","instance":"s1","anchor":"a2","text":"C3H7+"}]}),
    );
    assert_eq!(
        reset["result"]["document"]["fragment_displays"][0]["runs"],
        json!([{"text":"C3H7+","script":"baseline"}])
    );
    let undo = call(
        &app,
        "history.undo",
        json!({"document_id":id,"expected_revision":4}),
    );
    assert_eq!(
        undo["result"]["document"]["fragment_displays"][0]["runs"],
        runs
    );
}
