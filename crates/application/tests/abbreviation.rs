//! The public operation path retains chemistry while Et is compacted and reports format limits.

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
        &format!("ethyl-{}", NEXT.fetch_add(1, Ordering::Relaxed)),
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
    let created = call(&app, "document.create", json!({"title":"Et drawing"}));
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
            {"op":"add_atom","element":"C","position":{"x":45,"y":25},"bond_to":{"atom":"$1","order":"single"}}
        ]}),
    );
    assert_eq!(drawn["outcome"], "ok", "{drawn}");
    (app, root, id)
}
fn collapse(id: &str) -> Value {
    json!({"document_id":id,"expected_revision":1,"commands":[{"op":"collapse_ethyl","instance":"s1","anchor":"a2","terminal":"a3"}]})
}
fn snapshot(app: &App, id: &str) -> Value {
    call(app, "document.snapshot", json!({"document_id":id}))["result"].clone()
}

#[test]
fn replay_save_reopen_recovery_and_history_keep_the_exact_graph_and_display() {
    let (app, root, id) = setup();
    let original = snapshot(&app, &id);
    let parameters = collapse(&id);
    let collapsed = send(
        &app,
        "collapse-et",
        "document.transaction",
        parameters.clone(),
    );
    assert_eq!(collapsed["outcome"], "ok", "{collapsed}");
    assert_eq!(
        collapsed["result"]["document"]["abbreviations"],
        json!([{"instance":"s1","anchor":"a2","terminal":"a3","display_label":"Et"}])
    );
    assert_eq!(
        collapsed["result"]["document"]["atoms"],
        original["document"]["atoms"]
    );
    assert_eq!(
        collapsed["result"]["document"]["bonds"],
        original["document"]["bonds"]
    );
    assert_eq!(
        collapsed["result"]["document"]["scene"],
        original["document"]["scene"]
    );
    assert_eq!(
        send(&app, "collapse-et", "document.transaction", parameters),
        collapsed
    );
    app.save(&id, Some("save-et"), 2).unwrap();
    app.close(&id, Some("close-et"), false, Some(2)).unwrap();
    assert_eq!(
        serde_json::to_value(app.document(&id).unwrap()).unwrap()["document"],
        collapsed["result"]["document"]
    );
    let expanded = call(
        &app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":2,"commands":[{"op":"expand_ethyl","instance":"s1","anchor":"a2"}]}),
    );
    assert_eq!(expanded["result"]["document"]["abbreviations"], json!([]));
    app.autosave(&id, Some("draft-et")).unwrap();
    drop(app);
    let app = Application::new(LocalCapabilities, FileDocumentRepository::new(root.path()));
    app.document(&id).unwrap();
    let recovered = app.recover(&id, Some("recover-et")).unwrap();
    assert_eq!(recovered.document.abbreviations.len(), 0);
    let undone = call(
        &app,
        "history.undo",
        json!({"document_id":id,"expected_revision":recovered.document.revision}),
    );
    assert_eq!(
        undone["result"]["document"]["abbreviations"],
        collapsed["result"]["document"]["abbreviations"]
    );
    assert_eq!(
        undone["result"]["document"]["atoms"],
        original["document"]["atoms"]
    );
}

#[test]
fn all_writers_emit_the_full_graph_and_a_rendering_loss_without_mutating_the_document() {
    let (app, _root, id) = setup();
    let mol = call(
        &app,
        "document.export",
        json!({"records":[{"document_id":id}],"format":"mol"}),
    );
    let smiles = call(
        &app,
        "document.export",
        json!({"document_id":id,"format":"smiles","style":"recorded"}),
    );
    assert_eq!(mol["outcome"], "partial", "{mol}");
    assert_eq!(smiles["outcome"], "partial", "{smiles}");
    assert!(
        mol["result"]["content"]
            .as_str()
            .is_some_and(|text| text.contains("  3  2")),
        "{mol}"
    );
    assert!(
        smiles["result"]["content"]
            .as_str()
            .is_some_and(|text| text.contains('O') && text.matches('C').count() == 2),
        "{smiles}"
    );
    let collapsed = call(&app, "document.transaction", collapse(&id));
    assert_eq!(collapsed["outcome"], "ok", "{collapsed}");
    let labeled = call(
        &app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":2,"commands":[{"op":"update_abbreviation_label","instance":"s1","anchor":"a2","text":"Ph"}]}),
    );
    assert_eq!(labeled["outcome"], "ok", "{labeled}");
    let formula = call(
        &app,
        "chem.properties",
        json!({"document_id":id,"scope":"molecule","property":"formula"}),
    );
    assert_eq!(formula["outcome"], "ok", "{formula}");
    assert_eq!(formula["result"]["components"][0]["formula"], "C2H6O");
    let before = snapshot(&app, &id);
    for (format, parameters) in [
        (
            "mol",
            json!({"format":"mol","records":[{"document_id":id}]}),
        ),
        (
            "sdf",
            json!({"format":"sdf","records":[{"document_id":id}]}),
        ),
        (
            "smiles",
            json!({"format":"smiles","document_id":id,"style":"recorded"}),
        ),
        ("cdxml", json!({"format":"cdxml","document_id":id})),
        ("cdx", json!({"format":"cdx","document_id":id})),
    ] {
        let exported = call(&app, "document.export", parameters);
        assert_eq!(exported["outcome"], "partial", "{format}: {exported}");
        let losses: Vec<&Value> = exported["loss_report"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|loss| loss["feature"] == "abbreviation_display")
            .collect();
        assert_eq!(losses.len(), 1, "{format}: {exported}");
        assert_eq!(losses[0]["semantic_impact"], "rendering");
        assert_eq!(losses[0]["disposition"], "dropped");
        if format == "mol" {
            assert_eq!(exported["result"]["content"], mol["result"]["content"]);
        }
        if format == "smiles" {
            assert_eq!(exported["result"]["content"], smiles["result"]["content"]);
        }
        assert_eq!(snapshot(&app, &id), before);
    }
}

#[test]
fn native_copy_and_bad_commands_have_typed_refusals_and_no_side_effects() {
    let (app, _root, id) = setup();
    let before = snapshot(&app, &id);
    let invalid = call(
        &app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":1,"commands":[{"op":"rename","title":"roll back"},{"op":"collapse_ethyl","instance":"s1","anchor":"a3","terminal":"a2"}]}),
    );
    assert_eq!(
        invalid["diagnostics"][0]["details"]["reason"], "abbreviation_invalid",
        "{invalid}"
    );
    assert_eq!(invalid["diagnostics"][0]["details"]["command_index"], 1);
    assert_eq!(snapshot(&app, &id), before);
    let malformed = call(
        &app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":1,"commands":[{"op":"collapse_ethyl","instance":null,"anchor":"a2","terminal":"a3"}]}),
    );
    assert_eq!(malformed["diagnostics"][0]["code"], "invalid_request");
    let collapsed = call(&app, "document.transaction", collapse(&id));
    assert_eq!(collapsed["outcome"], "ok", "{collapsed}");
    let before = snapshot(&app, &id);
    let copied = call(
        &app,
        "document.clipboard_copy",
        json!({"document_id":id,"instance":"s1","selection":{"atoms":["a2","a3"],"bonds":["b2"]}}),
    );
    assert_eq!(copied["outcome"], "error", "{copied}");
    assert_eq!(
        copied["diagnostics"][0]["code"], "invalid_command",
        "{copied}"
    );
    assert_eq!(
        copied["diagnostics"][0]["details"]["reason"], "clipboard_abbreviation_display_unsupported",
        "{copied}"
    );
    assert_eq!(snapshot(&app, &id), before);
}

#[test]
fn chemical_label_weight_preserves_ethyl_graph_inspections_and_all_export_payloads() {
    let (app, _root, id) = setup();
    assert_eq!(
        call(&app, "document.transaction", collapse(&id))["outcome"],
        "ok"
    );
    let original = snapshot(&app, &id);
    let operations = [
        ("chem.validate", json!({"document_id":id})),
        ("chem.aromaticity", json!({"document_id":id})),
        ("chem.identity", json!({"document_id":id,"mode":"recorded"})),
        ("chem.identity", json!({"document_id":id,"mode":"chemical"})),
        (
            "chem.properties",
            json!({"document_id":id,"scope":"molecule","property":"formula"}),
        ),
    ];
    let before: Vec<Value> = operations
        .iter()
        .map(|(operation, parameters)| {
            let mut result = call(&app, operation, parameters.clone());
            assert_eq!(result["outcome"], "ok", "{result}");
            result["result"].as_object_mut().unwrap().remove("snapshot");
            result["result"].as_object_mut().unwrap().remove("revision");
            result["result"].clone()
        })
        .collect();
    let exports = [
        json!({"format":"mol","records":[{"document_id":id}]}),
        json!({"format":"sdf","records":[{"document_id":id}]}),
        json!({"format":"smiles","document_id":id,"style":"recorded"}),
        json!({"format":"cdxml","document_id":id}),
        json!({"format":"cdx","document_id":id}),
    ];
    let payloads: Vec<Value> = exports
        .iter()
        .map(|parameters| {
            call(&app, "document.export", parameters.clone())["result"]["content"].clone()
        })
        .collect();
    let params = json!({"document_id":id,"expected_revision":2,"commands":[{"op":"update_settings","label_weight":"bold"}]});
    let changed = send(
        &app,
        "chemical-label-bold",
        "document.transaction",
        params.clone(),
    );
    assert_eq!(changed["outcome"], "ok", "{changed}");
    assert_eq!(
        send(&app, "chemical-label-bold", "document.transaction", params),
        changed
    );
    let now = snapshot(&app, &id);
    for field in [
        "atoms",
        "bonds",
        "scene",
        "stereo",
        "abbreviations",
        "molecule",
        "selection",
    ] {
        assert_eq!(
            now["document"][field], original["document"][field],
            "{field}"
        );
    }
    assert_eq!(now["document"]["settings"]["label_weight"], "bold");
    for ((operation, parameters), expected) in operations.iter().zip(before) {
        let mut result = call(&app, operation, parameters.clone());
        result["result"].as_object_mut().unwrap().remove("snapshot");
        result["result"].as_object_mut().unwrap().remove("revision");
        assert_eq!(result["result"], expected, "{operation}");
    }
    for (parameters, payload) in exports.iter().zip(payloads) {
        let result = call(&app, "document.export", parameters.clone());
        assert_eq!(
            result["result"]["content"], payload,
            "{parameters}: {result}"
        );
        assert!(
            result["loss_report"]
                .as_array()
                .unwrap()
                .iter()
                .any(|loss| loss["feature"] == "drawing_settings"
                    && loss["semantic_impact"] == "rendering"),
            "{result}"
        );
    }
    assert_eq!(
        snapshot(&app, &id),
        now,
        "read-only results and exports leave the style edit intact"
    );
}

#[test]
fn chemical_label_style_preserves_ethyl_graph_inspections_and_all_export_payloads() {
    let (app, _root, id) = setup();
    assert_eq!(
        call(&app, "document.transaction", collapse(&id))["outcome"],
        "ok"
    );
    let original = snapshot(&app, &id);
    let operations = [
        ("chem.validate", json!({"document_id":id})),
        ("chem.aromaticity", json!({"document_id":id})),
        ("chem.identity", json!({"document_id":id,"mode":"recorded"})),
        ("chem.identity", json!({"document_id":id,"mode":"chemical"})),
        (
            "chem.properties",
            json!({"document_id":id,"scope":"molecule","property":"formula"}),
        ),
    ];
    let before: Vec<Value> = operations
        .iter()
        .map(|(operation, parameters)| {
            let mut result = call(&app, operation, parameters.clone());
            assert_eq!(result["outcome"], "ok", "{result}");
            result["result"].as_object_mut().unwrap().remove("snapshot");
            result["result"].as_object_mut().unwrap().remove("revision");
            result["result"].clone()
        })
        .collect();
    let exports = [
        json!({"format":"mol","records":[{"document_id":id}]}),
        json!({"format":"sdf","records":[{"document_id":id}]}),
        json!({"format":"smiles","document_id":id,"style":"recorded"}),
        json!({"format":"cdxml","document_id":id}),
        json!({"format":"cdx","document_id":id}),
    ];
    let payloads: Vec<Value> = exports
        .iter()
        .map(|parameters| {
            let result = call(&app, "document.export", parameters.clone());
            assert_eq!(result["outcome"], "partial", "{result}");
            assert!(result["result"]["content"].is_string(), "{result}");
            result["result"]["content"].clone()
        })
        .collect();
    let params = json!({"document_id":id,"expected_revision":2,"commands":[{"op":"update_settings","label_style":"italic"}]});
    let changed = send(
        &app,
        "chemical-label-italic",
        "document.transaction",
        params.clone(),
    );
    assert_eq!(changed["outcome"], "ok", "{changed}");
    assert_eq!(
        send(
            &app,
            "chemical-label-italic",
            "document.transaction",
            params
        ),
        changed
    );
    let now = snapshot(&app, &id);
    for field in [
        "atoms",
        "bonds",
        "scene",
        "stereo",
        "abbreviations",
        "molecule",
        "selection",
    ] {
        assert_eq!(
            now["document"][field], original["document"][field],
            "{field}"
        );
    }
    assert_eq!(now["document"]["settings"]["label_style"], "italic");
    for ((operation, parameters), expected) in operations.iter().zip(before) {
        let mut result = call(&app, operation, parameters.clone());
        result["result"].as_object_mut().unwrap().remove("snapshot");
        result["result"].as_object_mut().unwrap().remove("revision");
        assert_eq!(result["result"], expected, "{operation}");
    }
    for (parameters, payload) in exports.iter().zip(payloads) {
        let result = call(&app, "document.export", parameters.clone());
        assert_eq!(result["outcome"], "partial", "{result}");
        assert_eq!(
            result["result"]["content"], payload,
            "{parameters}: {result}"
        );
        assert!(
            result["loss_report"]
                .as_array()
                .unwrap()
                .iter()
                .any(|loss| loss["feature"] == "drawing_settings"
                    && loss["semantic_impact"] == "rendering"),
            "{result}"
        );
    }
    assert_eq!(
        snapshot(&app, &id),
        now,
        "read-only results and exports leave the style edit intact"
    );
}

#[test]
fn display_text_replays_exactly_and_invalid_updates_are_atomic() {
    let (app, _root, id) = setup();
    call(&app, "document.transaction", collapse(&id));
    let before = snapshot(&app, &id);
    let params = json!({"document_id":id,"expected_revision":2,"commands":[{"op":"update_abbreviation_label","instance":"s1","anchor":"a2","text":"Me 한 e\u{301} 🧪"}]});
    let labeled = send(
        &app,
        "label-request",
        "document.transaction",
        params.clone(),
    );
    assert_eq!(labeled["outcome"], "ok", "{labeled}");
    assert_eq!(
        labeled["result"]["document"]["abbreviations"][0]["display_label"],
        "Me 한 e\u{301} 🧪"
    );
    assert_eq!(
        send(
            &app,
            "label-request",
            "document.transaction",
            params.clone()
        ),
        labeled
    );
    let mut reused = params;
    reused["commands"][0]["text"] = json!("Ph");
    assert_eq!(
        send(&app, "label-request", "document.transaction", reused)["diagnostics"][0]["code"],
        "request_id_reused"
    );
    for field in ["atoms", "bonds", "scene", "stereo", "settings"] {
        assert_eq!(
            labeled["result"]["document"][field],
            before["document"][field]
        );
    }
    for text in [json!(""), json!("a\nb"), json!("한".repeat(43)), json!(42)] {
        let snapshot = snapshot(&app, &id);
        let result = call(
            &app,
            "document.transaction",
            json!({"document_id":id,"expected_revision":3,"commands":[{"op":"rename","title":"must roll back"},{"op":"update_abbreviation_label","instance":"s1","anchor":"a2","text":text}]}),
        );
        assert_eq!(result["outcome"], "error", "{result}");
        assert_eq!(crate::snapshot(&app, &id), snapshot);
    }
    let reset = call(
        &app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":3,"commands":[{"op":"update_abbreviation_label","instance":"s1","anchor":"a2","text":null}]}),
    );
    assert_eq!(
        reset["result"]["document"]["abbreviations"][0]["display_label"],
        "Et"
    );
    let undo = call(
        &app,
        "history.undo",
        json!({"document_id":id,"expected_revision":4}),
    );
    assert_eq!(
        undo["result"]["document"]["abbreviations"],
        labeled["result"]["document"]["abbreviations"]
    );
}

#[test]
fn arbitrary_display_text_keeps_scientific_results_and_recovers_as_metadata() {
    let (app, root, id) = setup();
    call(&app, "document.transaction", collapse(&id));
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
    let results = |app: &App| -> Vec<Value> {
        inspections.iter().map(|(operation, parameters)| {
            let mut response = call(app, operation, parameters.clone());
            if let Some(result) = response["result"].as_object_mut() {
                result.remove("snapshot"); result.remove("revision");
            }
            json!({"outcome":response["outcome"],"model_id":response["model_id"],"result":response["result"],"diagnostics":response["diagnostics"]})
        }).collect()
    };
    let before = results(&app);
    for (revision, text) in [(2, "Ph"), (3, "Me")] {
        let edited = call(
            &app,
            "document.transaction",
            json!({"document_id":id,"expected_revision":revision,"commands":[{"op":"update_abbreviation_label","instance":"s1","anchor":"a2","text":text}]}),
        );
        assert_eq!(edited["outcome"], "ok", "{edited}");
        assert_eq!(
            results(&app),
            before,
            "the text {text} never defines chemistry"
        );
    }
    app.save(&id, Some("save-custom-label"), 4).unwrap();
    let reset = call(
        &app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":4,"commands":[{"op":"update_abbreviation_label","instance":"s1","anchor":"a2","text":null}]}),
    );
    assert_eq!(reset["outcome"], "ok", "{reset}");
    app.autosave(&id, Some("draft-display-reset")).unwrap();
    drop(app);
    let app = Application::new(LocalCapabilities, FileDocumentRepository::new(root.path()));
    app.document(&id).unwrap();
    let recovered = app.recover(&id, Some("recover-display-reset")).unwrap();
    assert_eq!(recovered.document.abbreviations[0].display_label, "Et");
    let restored = call(
        &app,
        "history.undo",
        json!({"document_id":id,"expected_revision":recovered.document.revision}),
    );
    assert_eq!(
        restored["result"]["document"]["abbreviations"][0]["display_label"],
        "Me"
    );
}
