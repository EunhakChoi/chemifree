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
    json!([{"text":"한글 e\u{301} 👩‍🔬 H","script":"baseline","weight":"bold","style":"normal","font":"serif"},{"text":"2","script":"subscript","weight":"normal","style":"italic","font":"monospace"},{"text":"+","script":"superscript","weight":"bold","style":"italic","font":"sans-serif"}])
}
fn commands() -> Value {
    json!([
        {"op":"add_atom","element":"C","position":{"x":0,"y":0}},
        {"op":"add_caption","anchor":{"x":40,"y":50},"alignment":"center","runs":runs()}
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
    assert_eq!(
        added["result"]["document"]["scene"]["objects"][1]["alignment"],
        "center"
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
        json!({"document_id":id,"expected_revision":reopened.document.revision,"commands":[{"op":"update_caption","object":"s2","anchor":{"x":-20,"y":10},"alignment":"right","runs":[{"text":"🧪 가나다 NFC é / NFD e\u{301}","script":"baseline"}]}]}),
    );
    assert_eq!(changed["outcome"], "ok", "{changed}");
    assert_eq!(
        changed["result"]["document"]["scene"]["objects"][1]["alignment"],
        "right"
    );
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
        copied["diagnostics"][0]["code"], "invalid_command",
        "{copied}"
    );
    assert_eq!(copied["outcome"], "error");
    assert_eq!(copied["result"], Value::Null);
    assert_eq!(
        copied["diagnostics"][0]["details"]["reason"],
        "clipboard_scene_caption_unsupported"
    );
    assert_eq!(copied["diagnostics"][0]["details"]["current_revision"], 1);
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
    for (case, mut parameters, source_path) in [
        (
            "MOL V2000",
            json!({"format":"mol","ctab":"v2000","records":[{"document_id":id}]}),
            "records[1].scene.objects[s2]",
        ),
        (
            "MOL V3000",
            json!({"format":"mol","ctab":"v3000","records":[{"document_id":id}]}),
            "records[1].scene.objects[s2]",
        ),
        (
            "SDF V2000",
            json!({"format":"sdf","ctab":"v2000","records":[{"document_id":id}]}),
            "records[1].scene.objects[s2]",
        ),
        (
            "SDF V3000",
            json!({"format":"sdf","ctab":"v3000","records":[{"document_id":id}]}),
            "records[1].scene.objects[s2]",
        ),
        (
            "recorded SMILES",
            json!({"format":"smiles","style":"recorded","document_id":id}),
            "document.scene.objects[s2]",
        ),
        (
            "canonical SMILES",
            json!({"format":"smiles","style":"canonical","document_id":id}),
            "document.scene.objects[s2]",
        ),
        (
            "CDXML drawing",
            json!({"format":"cdxml","document_id":id}),
            "document.scene.objects[s2]",
        ),
        (
            "CDX drawing",
            json!({"format":"cdx","document_id":id}),
            "document.scene.objects[s2]",
        ),
    ] {
        let refused = call(&app, "document.export", parameters.clone());
        assert_eq!(refused["outcome"], "error", "{case}: {refused}");
        assert_eq!(
            refused["result"],
            Value::Null,
            "no bytes without consent: {case}"
        );
        assert_eq!(
            refused["diagnostics"][0]["code"], "loss_consent_required",
            "{case}: {refused}"
        );
        let losses = refused["diagnostics"][0]["details"]["losses"]
            .as_array()
            .unwrap();
        let caption: Vec<_> = losses
            .iter()
            .filter(|loss| loss["feature"] == "caption")
            .collect();
        let expected = json!({
            "feature":"caption", "source_path":source_path, "disposition":"dropped", "semantic_impact":"rendering",
            "reason":"Independent caption text, script, weight and style are not written by this export profile."
        });
        assert_eq!(caption, vec![&expected], "{case}: {refused}");
        let digest = refused["diagnostics"][0]["details"]["loss_digest"].clone();
        assert_eq!(digest.as_str().unwrap().len(), 64, "{case}");
        parameters["accepted_losses"] = json!("0".repeat(64));
        let wrong_digest = call(&app, "document.export", parameters.clone());
        assert_eq!(
            wrong_digest["diagnostics"][0]["code"], "loss_consent_changed",
            "{case}: {wrong_digest}"
        );
        assert_eq!(wrong_digest["result"], Value::Null, "{case}");
        parameters["accepted_losses"] = digest;
        let accepted = call(&app, "document.export", parameters);
        assert_eq!(accepted["outcome"], "partial", "{case}: {accepted}");
        assert!(
            !accepted["result"]["content"].as_str().unwrap().is_empty(),
            "{case}"
        );
        let accepted_caption: Vec<_> = accepted["loss_report"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|loss| loss["feature"] == "caption")
            .collect();
        assert_eq!(accepted_caption, vec![&expected], "{case}");
        assert_eq!(snapshot(&app, &id), before);
    }
}

#[test]
fn a_small_wire_request_cannot_create_an_oversized_caption_history_entry() {
    use chemifree_application::DocumentRepository;
    use chemifree_domain::document::{Command, Document, DocumentId, ObjectId, Point};
    use chemifree_domain::scene::{Caption, CaptionRun, CaptionScript};

    let root = tempfile::tempdir().unwrap();
    let repository = FileDocumentRepository::new(root.path());
    let mut document =
        Document::new(DocumentId::parse("caption-wire-budget").unwrap(), "Budget").unwrap();
    let captions: Vec<_> = (0..117)
        .map(|_| Command::AddCaption {
            caption: Caption {
                alignment: Default::default(),
                anchor: Point::new(0.0, 0.0).unwrap(),
                runs: vec![
                    CaptionRun {
                        font: None,
                        text: "\"".repeat(512),
                        script: CaptionScript::Superscript,
                        weight: Default::default(),
                        style: Default::default(),
                    };
                    32
                ],
            },
            parent: None,
            index: None,
        })
        .collect();
    document.apply(&captions).unwrap();
    document
        .apply(&[Command::Group {
            objects: (2..119)
                .map(|serial| ObjectId::from_serial(serial).unwrap())
                .collect(),
        }])
        .unwrap();
    repository.save(&document, None).unwrap();
    let app = Application::new(LocalCapabilities, repository);
    let before = snapshot(&app, "caption-wire-budget");
    // One short group move duplicates the stored complete states of all its caption leaves.
    let refused = call(
        &app,
        "document.transaction",
        json!({"document_id":"caption-wire-budget",
        "expected_revision":before["document"]["revision"], "commands":[
            {"op":"rename","title":"must roll back"},
            {"op":"move_objects","objects":["s119"],"delta":{"x":1,"y":0}}
        ]}),
    );
    assert_eq!(refused["outcome"], "error", "{refused}");
    assert_eq!(refused["diagnostics"][0]["code"], "invalid_command");
    assert_eq!(
        refused["diagnostics"][0]["details"]["reason"],
        "caption_persistence_limit"
    );
    assert_eq!(
        refused["diagnostics"][0]["details"].get("command_index"),
        None,
        "whole-transaction budget"
    );
    assert_eq!(refused["result"], Value::Null);
    assert_eq!(snapshot(&app, "caption-wire-budget"), before);
}

#[test]
fn invalid_caption_styles_refuse_the_whole_transaction_and_omissions_default_to_normal() {
    let (app, _root, id) = setup();
    for (field, value) in [
        ("weight", json!("heavy")),
        ("weight", json!(700)),
        ("style", json!("oblique")),
        ("style", json!(null)),
    ] {
        let mut run = json!({"text":"H","script":"baseline"});
        run[field] = value;
        let before = snapshot(&app, &id);
        let refused = call(
            &app,
            "document.transaction",
            json!({"document_id":id,"expected_revision":0,"commands":[
                {"op":"rename","title":"must not commit"},
                {"op":"add_caption","anchor":{"x":0,"y":0},"runs":[run]}
            ]}),
        );
        assert_eq!(
            refused["diagnostics"][0]["code"], "invalid_request",
            "{refused}"
        );
        assert_eq!(snapshot(&app, &id), before);
    }
    let added = call(
        &app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":0,"commands":[
            {"op":"add_caption","anchor":{"x":0,"y":0},"runs":[{"text":"H","script":"baseline"}]}
        ]}),
    );
    assert_eq!(added["outcome"], "ok", "{added}");
    assert_eq!(
        added["result"]["document"]["scene"]["objects"][1]["runs"],
        json!([
            {"text":"H","script":"baseline","weight":"normal","style":"normal","font":null}
        ])
    );
}

#[test]
fn alignment_defaults_are_whole_state_replacements_and_invalid_values_preserve_history() {
    let (app, _root, id) = setup();
    let added = call(
        &app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":0,"commands":[{"op":"add_caption","anchor":{"x":7,"y":9},"runs":runs()}]}),
    );
    assert_eq!(
        added["result"]["document"]["scene"]["objects"][1]["alignment"],
        "left"
    );
    let parameters = json!({"document_id":id,"expected_revision":1,"commands":[{"op":"update_caption","object":"s2","anchor":{"x":7,"y":9},"alignment":"center","runs":runs()}]});
    let centered = send(
        &app,
        "align-center",
        "document.transaction",
        parameters.clone(),
    );
    assert_eq!(
        centered["result"]["document"]["scene"]["objects"][1]["alignment"],
        "center"
    );
    assert_eq!(
        send(&app, "align-center", "document.transaction", parameters),
        centered
    );
    let changed_replay = send(
        &app,
        "align-center",
        "document.transaction",
        json!({"document_id":id,"expected_revision":1,"commands":[{"op":"update_caption","object":"s2","anchor":{"x":7,"y":9},"alignment":"right","runs":runs()}]}),
    );
    assert_eq!(
        changed_replay["diagnostics"][0]["code"],
        "request_id_reused"
    );
    let before = snapshot(&app, &id);
    for alignment in [
        json!(null),
        json!("start"),
        json!(false),
        json!(3),
        json!(["left"]),
    ] {
        let refused = call(
            &app,
            "document.transaction",
            json!({"document_id":id,"expected_revision":2,"commands":[{"op":"rename","title":"rollback"},{"op":"update_caption","object":"s2","anchor":{"x":7,"y":9},"alignment":alignment,"runs":runs()}]}),
        );
        assert_eq!(
            refused["diagnostics"][0]["code"], "invalid_request",
            "{refused}"
        );
        assert_eq!(snapshot(&app, &id), before);
    }
    // Legacy update requests replace a complete caption: omission resets alignment to left.
    let omitted = call(
        &app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":2,"commands":[{"op":"update_caption","object":"s2","anchor":{"x":7,"y":9},"runs":runs()}]}),
    );
    assert_eq!(
        omitted["result"]["document"]["scene"]["objects"][1]["alignment"],
        "left"
    );
    let undone = app.undo(&id, Some("align-undo"), 3).unwrap();
    assert_eq!(
        serde_json::to_value(undone.document).unwrap()["scene"]["objects"][1]["alignment"],
        "center"
    );
}

#[test]
fn font_only_wire_edit_replays_and_omission_resets_to_inheritance() {
    let (app, _root, id) = setup();
    let mut run = json!({"text":" H ","script":"baseline","font":"serif"});
    let added = call(
        &app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":0,"commands":[{"op":"add_caption","anchor":{"x":3,"y":4},"alignment":"right","runs":[run]}]}),
    );
    assert_eq!(added["outcome"], "ok", "{added}");
    run["font"] = json!("monospace");
    let params = json!({"document_id":id,"expected_revision":1,"commands":[{"op":"update_caption","object":"s2","anchor":{"x":3,"y":4},"alignment":"right","runs":[run]}]});
    let changed = send(&app, "family-only", "document.transaction", params.clone());
    assert_eq!(changed["outcome"], "ok", "{changed}");
    assert_eq!(
        changed["result"]["document"]["scene"]["objects"][1]["runs"][0]["font"],
        "monospace"
    );
    assert_eq!(
        send(&app, "family-only", "document.transaction", params.clone()),
        changed
    );
    let mut different = params;
    different["commands"][0]["runs"][0]["font"] = json!(null);
    assert_eq!(
        send(&app, "family-only", "document.transaction", different)["diagnostics"][0]["code"],
        "request_id_reused"
    );
    for font in [
        json!("Arial"),
        json!("inherit"),
        json!(4),
        json!({}),
        json!([]),
    ] {
        let before = snapshot(&app, &id);
        let refused = call(
            &app,
            "document.transaction",
            json!({"document_id":id,"expected_revision":2,"commands":[{"op":"rename","title":"must roll back"},{"op":"update_caption","object":"s2","anchor":{"x":3,"y":4},"runs":[{"text":"H","script":"baseline","font":font}]}]}),
        );
        assert_eq!(refused["diagnostics"][0]["code"], "invalid_request");
        assert_eq!(snapshot(&app, &id), before);
    }
    run.as_object_mut().unwrap().remove("font");
    let reset = call(
        &app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":2,"commands":[{"op":"update_caption","object":"s2","anchor":{"x":3,"y":4},"alignment":"right","runs":[run]}]}),
    );
    assert_eq!(reset["outcome"], "ok", "{reset}");
    assert_eq!(
        reset["result"]["document"]["scene"]["objects"][1]["runs"][0]["font"],
        Value::Null
    );
}
