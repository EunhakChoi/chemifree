//! Caption wire, replay, persistence, recovery, and explicit unsupported export boundaries.

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
        &format!("caption-{}", NEXT.fetch_add(1, Ordering::Relaxed)),
        operation,
        parameters,
    )
}
fn snapshot(app: &App, id: &str) -> Value {
    call(app, "document.snapshot", json!({"document_id":id}))["result"].clone()
}
fn setup() -> (App, tempfile::TempDir, String) {
    let root = tempfile::tempdir().unwrap();
    let app = Application::new(LocalCapabilities, FileDocumentRepository::new(root.path()))
        .with_ctfile_codec(V2000Codec)
        .with_smiles_codec(OpenSmilesCodec)
        .with_interchange_codec(LocalInterchangeCodec);
    let created = call(&app, "document.create", json!({"title":"Captions"}));
    let id = created["result"]["document"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    (app, root, id)
}
fn runs() -> Value {
    json!([{"text":"한글 e\u{301} 👩‍🔬 H","script":"baseline"},{"text":"2","script":"subscript"},{"text":"+","script":"superscript"}])
}
fn commands() -> Value {
    json!([
        {"op":"add_atom","element":"C","position":{"x":0,"y":0}},
        {"op":"add_caption","anchor":{"x":40,"y":50},"runs":runs()}
    ])
}

#[test]
fn caption_replay_save_reopen_and_crash_draft_preserve_text_scripts_and_history() {
    let (app, root, id) = setup();
    let parameters = json!({"document_id":id,"expected_revision":0,"commands":commands(),"selection_after":{"objects":["$1"],"atoms":[],"bonds":[]}});
    let added = send(
        &app,
        "caption-add",
        "document.transaction",
        parameters.clone(),
    );
    assert_eq!(added["outcome"], "ok", "{added}");
    assert_eq!(
        added["result"]["document"]["scene"]["objects"][1]["runs"],
        runs()
    );
    assert_eq!(
        send(&app, "caption-add", "document.transaction", parameters),
        added
    );
    let first_revision = added["result"]["document"]["revision"].as_u64().unwrap();
    app.save(&id, Some("save-caption"), first_revision).unwrap();
    app.close(&id, Some("close-caption"), false, Some(first_revision))
        .unwrap();
    let reopened = app.document(&id).unwrap();
    assert_eq!(
        serde_json::to_value(&reopened.document).unwrap()["scene"]["objects"][1]["runs"],
        runs()
    );
    let changed = call(
        &app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":reopened.document.revision,"commands":[{"op":"update_caption","object":"s2","anchor":{"x":-20,"y":10},"runs":[{"text":"🧪 가나다 NFC é / NFD e\u{301}","script":"baseline"}]}]}),
    );
    assert_eq!(changed["outcome"], "ok", "{changed}");
    app.autosave(&id, Some("draft-caption")).unwrap();
    let expected = changed["result"]["document"]["scene"].clone();
    drop(app);
    let app = Application::new(LocalCapabilities, FileDocumentRepository::new(root.path()))
        .with_ctfile_codec(V2000Codec)
        .with_smiles_codec(OpenSmilesCodec)
        .with_interchange_codec(LocalInterchangeCodec);
    let recovered = app.recover(&id, Some("recover-caption")).unwrap();
    assert_eq!(
        serde_json::to_value(&recovered.document).unwrap()["scene"],
        expected
    );
    let undone = app
        .undo(&id, Some("undo-caption"), recovered.document.revision)
        .unwrap();
    assert_eq!(
        serde_json::to_value(&undone.document).unwrap()["scene"]["objects"][1]["runs"],
        runs()
    );
    let redone = app
        .redo(&id, Some("redo-caption"), undone.document.revision)
        .unwrap();
    assert_eq!(
        serde_json::to_value(&redone.document).unwrap()["scene"],
        expected
    );
}

#[test]
fn malformed_runs_and_unsupported_caption_operations_leave_the_snapshot_unchanged() {
    let (app, _root, id) = setup();
    let added = call(
        &app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":0,"commands":commands()}),
    );
    assert_eq!(added["outcome"], "ok", "{added}");
    let before = snapshot(&app, &id);
    for (script, code) in [
        (json!(null), "invalid_request"),
        (json!(["baseline"]), "invalid_request"),
        (json!("automatic"), "invalid_request"),
    ] {
        let refused = call(
            &app,
            "document.transaction",
            json!({"document_id":id,"expected_revision":1,"commands":[{"op":"update_caption","object":"s2","anchor":{"x":0,"y":0},"runs":[{"text":"A","script":script}]}]}),
        );
        assert_eq!(refused["diagnostics"][0]["code"], code, "{refused}");
        assert_eq!(snapshot(&app, &id), before);
    }
    for command in [
        json!({"op":"transform_selection","selection":{"objects":["s1","s2"],"atoms":[],"bonds":[]},"pivot":{"x":0,"y":0},"transform":{"type":"rotate","degrees":90}}),
        json!({"op":"arrange_objects","objects":["s1","s2"],"mode":"align_left"}),
    ] {
        let refused = call(
            &app,
            "document.transaction",
            json!({"document_id":id,"expected_revision":1,"commands":[{"op":"rename","title":"must roll back"},command]}),
        );
        assert_eq!(
            refused["diagnostics"][0]["details"]["reason"], "caption_unsupported",
            "{refused}"
        );
        assert_eq!(refused["diagnostics"][0]["details"]["command_index"], 1);
        assert_eq!(snapshot(&app, &id), before);
    }
    let copied = call(
        &app,
        "document.clipboard_scene_copy",
        json!({"document_id":id,"selection":{"objects":["s2"]}}),
    );
    assert_eq!(
        copied["diagnostics"][0]["code"], "clipboard_scene_caption_unsupported",
        "{copied}"
    );
    assert_eq!(snapshot(&app, &id), before);
}

#[test]
fn caption_coordinate_conversion_matches_existing_scene_command_errors() {
    let (app, _root, id) = setup();
    let added = call(
        &app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":0,"commands":commands()}),
    );
    assert_eq!(added["outcome"], "ok", "{added}");
    let before = snapshot(&app, &id);
    let bad_anchor = json!({"x":100001,"y":0});
    let graphic = call(
        &app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":1,"commands":[
            {"op":"rename","title":"must stay unchanged"},
            {"op":"add_graphic","shape":"line","points":[bad_anchor,{"x":0,"y":0}]}
        ]}),
    );
    assert_eq!(
        graphic["diagnostics"][0]["code"], "invalid_command",
        "{graphic}"
    );
    assert_eq!(graphic["diagnostics"][0]["details"]["command_index"], 1);
    assert_eq!(graphic["diagnostics"][0]["details"].get("reason"), None);
    for command in [
        json!({"op":"add_caption","anchor":bad_anchor,"runs":runs()}),
        json!({"op":"update_caption","object":"s2","anchor":bad_anchor,"runs":runs()}),
    ] {
        let refused = call(
            &app,
            "document.transaction",
            json!({"document_id":id,"expected_revision":1,"commands":[
                {"op":"rename","title":"must stay unchanged"},command
            ]}),
        );
        assert_eq!(refused["diagnostics"], graphic["diagnostics"]);
        assert_eq!(snapshot(&app, &id), before);
    }
    let invalid_content = call(
        &app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":1,"commands":[
            {"op":"rename","title":"must stay unchanged"},
            {"op":"update_caption","object":"s2","anchor":{"x":0,"y":0},"runs":[{"text":"","script":"baseline"}]}
        ]}),
    );
    assert_eq!(
        invalid_content["diagnostics"][0]["details"]["reason"],
        "caption_invalid"
    );
    assert_eq!(
        invalid_content["diagnostics"][0]["details"]["command_index"],
        1
    );
    assert_eq!(snapshot(&app, &id), before);
}

#[test]
fn every_chemical_export_requires_explicit_consent_for_caption_loss() {
    let (app, _root, id) = setup();
    let added = call(
        &app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":0,"commands":commands()}),
    );
    assert_eq!(added["outcome"], "ok", "{added}");
    let before = snapshot(&app, &id);
    for format in ["mol", "sdf", "smiles", "cdxml", "cdx"] {
        let refused = call(
            &app,
            "document.export",
            json!({"document_id":id,"format":format}),
        );
        assert_eq!(
            refused["diagnostics"][0]["code"], "loss_consent_required",
            "{format}: {refused}"
        );
        let losses = refused["diagnostics"][0]["details"]["losses"]
            .as_array()
            .unwrap();
        let caption: Vec<_> = losses
            .iter()
            .filter(|loss| loss["feature"] == "caption")
            .collect();
        assert_eq!(caption.len(), 1, "{format}: {refused}");
        assert_eq!(caption[0]["disposition"], "dropped");
        assert_eq!(caption[0]["semantic_impact"], "rendering");
        let digest = refused["diagnostics"][0]["details"]["loss_digest"].clone();
        let accepted = call(
            &app,
            "document.export",
            json!({"document_id":id,"format":format,"accepted_losses":digest}),
        );
        assert_eq!(accepted["outcome"], "partial", "{format}: {accepted}");
        assert_eq!(snapshot(&app, &id), before);
    }
}
