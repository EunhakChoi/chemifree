//! SMILES stereo through `document.import`: stored configurations with no marks, CXSMILES groups
//! as stereo groups, and the all-or-nothing rule when the text is refused.

use chemifree_application::Application;
use chemifree_providers::interchange_codec::LocalInterchangeCodec;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities, OpenSmilesCodec, V2000Codec};
use serde_json::{Value, json};
use tempfile::TempDir;

type App = Application<LocalCapabilities, FileDocumentRepository>;

fn setup() -> (App, TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    let app = Application::new(LocalCapabilities, FileDocumentRepository::new(dir.path()))
        .with_ctfile_codec(V2000Codec)
        .with_interchange_codec(LocalInterchangeCodec)
        .with_smiles_codec(OpenSmilesCodec);
    let created = call(&app, "create", "document.create", json!({}));
    let id = created["result"]["document"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    (app, dir, id)
}

fn call(app: &App, id: &str, operation: &str, parameters: Value) -> Value {
    let request = json!({"contract":1,"request_id":id,"profile":"preserve-semantics","operation":operation,"parameters":parameters});
    serde_json::to_value(app.execute(request.to_string().as_bytes())).unwrap()
}

fn import(app: &App, id: &str, revision: u64, source: &str) -> Value {
    let params = |extra: Value| {
        let mut base = json!({"document_id":id,"expected_revision":revision,"format":"smiles","source":source});
        base.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        base
    };
    let preview = call(
        app,
        &format!("p{revision}"),
        "document.import",
        params(json!({"preview": true})),
    );
    let Some(digest) = preview["result"]["preview_digest"].as_str() else {
        return preview;
    };
    call(
        app,
        &format!("c{revision}"),
        "document.import",
        params(json!({"preview_digest": digest, "accept_loss": true})),
    )
}

fn document(app: &App, id: &str) -> Value {
    call(app, "s", "document.snapshot", json!({"document_id":id}))["result"]["document"].clone()
}

#[test]
fn a_tagged_center_is_stored_as_a_configuration_with_no_mark() {
    let (app, _dir, id) = setup();
    let done = import(&app, &id, 0, "F[C@H](Cl)[C@H](Br)I");
    assert_eq!(done["document_revision"], 1, "{done}");
    let doc = document(&app, &id);
    let stereo = &doc["stereo"];
    // F0 C1 Cl2 C3 Br4 I5 are a1..a6; each center reads (preceding, H, ...), ACW, which the
    // store keeps with the hydrogen last (two steps, even).
    assert_eq!(
        stereo["atoms"],
        json!([
            {"atom":"a2","configuration":{"kind":"tetrahedral","neighbors":["a1","a3","a4","implicit_hydrogen"],"turn":"anticlockwise"}},
            {"atom":"a4","configuration":{"kind":"tetrahedral","neighbors":["a2","a5","a6","implicit_hydrogen"],"turn":"anticlockwise"}}
        ]),
        "{stereo}"
    );
    assert!(
        doc["bonds"]
            .as_array()
            .unwrap()
            .iter()
            .all(|bond| bond["mark"].is_null() || bond.get("mark").is_none()),
        "no bond carries a mark: {doc}"
    );
}

#[test]
fn a_cx_block_becomes_stereo_groups_and_a_loss_free_import() {
    let (app, _dir, id) = setup();
    let done = import(&app, &id, 0, "F[C@H](Cl)[C@H](Br)I |o1:1,3|");
    assert_eq!(done["document_revision"], 1, "{done}");
    let groups = document(&app, &id)["stereo"]["groups"].clone();
    let groups = groups.as_array().unwrap();
    assert_eq!(groups.len(), 1, "{groups:?}");
    assert_eq!(groups[0]["kind"], "or");
    assert_eq!(groups[0]["number"], 1);
    assert_eq!(groups[0]["members"], json!(["a2", "a4"]));
    let features: Vec<&str> = done["loss_report"]
        .as_array()
        .unwrap()
        .iter()
        .map(|loss| loss["feature"].as_str().unwrap())
        .collect();
    assert!(!features.contains(&"ignored_text"), "{features:?}");
}

#[test]
fn a_second_import_with_the_same_group_number_is_renumbered_and_reported() {
    let (app, _dir, id) = setup();
    import(&app, &id, 0, "F[C@H](Cl)Br |o1:1|");
    let done = import(&app, &id, 1, "I[C@H](Cl)Br |o1:1|");
    assert_eq!(done["document_revision"], 2, "{done}");
    let groups = document(&app, &id)["stereo"]["groups"].clone();
    let numbers: Vec<i64> = groups
        .as_array()
        .unwrap()
        .iter()
        .map(|group| group["number"].as_i64().unwrap())
        .collect();
    assert_eq!(numbers, [1, 2]);
    assert!(
        done["loss_report"]
            .as_array()
            .unwrap()
            .iter()
            .any(|loss| loss["feature"] == "stereo_group_number"),
        "{done}"
    );
}

#[test]
fn a_refused_text_leaves_the_document_unchanged() {
    let (app, _dir, id) = setup();
    import(&app, &id, 0, "CCO");
    let before = document(&app, &id);
    for (source, family) in [
        ("F[C@H](Cl)Br |a:0|", "invalid_chemistry"),
        ("F[C@H](Cl)Br |r|", "unsupported_feature"),
        ("F[C@H](Cl)Br |a:1", "malformed_input"),
        ("F[C@TH1H](Cl)Br", "unsupported_feature"),
        ("[S@](=O)(C)CC", "unsupported_feature"),
        ("F/C=C/F", "unsupported_feature"),
        ("C[C@H](F)", "unsupported_feature"),
    ] {
        let reply = import(&app, &id, 1, source);
        assert_eq!(reply["outcome"], "error", "{source}: {reply}");
        assert_eq!(
            reply["diagnostics"][0]["details"]["family"], family,
            "{source}: {reply}"
        );
        assert_eq!(document(&app, &id), before, "{source}");
    }
}

#[test]
fn a_replayed_import_reports_the_group_losses_of_the_original() {
    let (app, _dir, id) = setup();
    let source = "F[C@H](Cl)[C@H](Br)I |o1:1,a:3|";
    import(&app, &id, 0, source);
    // The document now has or 1 and an absolute group: the same text collides with both.
    let base = json!({"document_id":id,"expected_revision":1,"format":"smiles","source":source});
    let mut preview = base.clone();
    preview["preview"] = json!(true);
    let preview = call(&app, "again-p", "document.import", preview);
    let digest = preview["result"]["preview_digest"].clone();
    let mut commit = base;
    commit["preview_digest"] = digest;
    commit["accept_loss"] = json!(true);
    let first = call(&app, "again", "document.import", commit.clone());
    assert_eq!(first["document_revision"], 2, "{first}");
    let features = |reply: &Value| -> Vec<String> {
        reply["loss_report"]
            .as_array()
            .unwrap()
            .iter()
            .map(|loss| loss["feature"].as_str().unwrap().to_owned())
            .collect()
    };
    let original = features(&first);
    assert!(
        original.contains(&"stereo_group_number".to_owned())
            && original.contains(&"stereo_group_merged".to_owned()),
        "{original:?}"
    );
    // Same request id and parameters: the committed reply is replayed, not applied again.
    let replay = call(&app, "again", "document.import", commit);
    assert_eq!(replay["document_revision"], 2, "{replay}");
    assert_eq!(replay["loss_report"], first["loss_report"], "{replay}");
}

#[test]
fn a_text_of_exactly_the_command_bound_commits_with_its_groups_and_one_more_is_refused() {
    // 70 units of `C[C@H](F)` and `C[C@H](F)Cl` are 214 atoms, 213 bonds and 71 configurations,
    // 498 commands; each group is one more. Two groups make 500, the bound of one transaction.
    let text = |groups: &str| format!("{}C[C@H](F)Cl |{groups}|", "C[C@H](F)".repeat(70));
    let (app, _dir, id) = setup();
    let done = import(&app, &id, 0, &text("o1:1,o2:4"));
    assert_eq!(done["document_revision"], 1, "{done}");
    assert_eq!(
        document(&app, &id)["stereo"]["groups"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );
    // A third group is the 501st command: refused by the reader, nothing imported.
    let (app, _dir, id) = setup();
    let refused = import(&app, &id, 0, &text("o1:1,o2:4,a:7"));
    assert_eq!(refused["outcome"], "error", "{refused}");
    assert_eq!(refused["diagnostics"][0]["code"], "command_limit");
    assert_eq!(document(&app, &id)["revision"], 0);
}

#[test]
fn an_import_next_to_an_existing_absolute_group_still_fits_the_bound_it_was_counted_against() {
    // Joining the document's absolute group is one command, as creating one is.
    let (app, _dir, id) = setup();
    import(&app, &id, 0, "F[C@H](Cl)Br |a:1|");
    let text = format!("{}C[C@H](F)Cl |a:1,o1:4|", "C[C@H](F)".repeat(70));
    let done = import(&app, &id, 1, &text);
    assert_eq!(done["document_revision"], 2, "{done}");
}

#[test]
fn recorded_export_preserves_imported_centers_and_groups_without_stereo_consent() {
    let (app, _dir, id) = setup();
    let source = "F[C@H](Cl)[C@@H](Br)I |o1:1,3|";
    let imported = import(&app, &id, 0, source);
    assert_eq!(imported["document_revision"], 1, "{imported}");
    let before = document(&app, &id);
    let written = call(
        &app,
        "recorded",
        "document.export",
        json!({"document_id": id, "format": "smiles", "style": "recorded"}),
    );
    assert_eq!(written["outcome"], "partial", "{written}");
    assert_eq!(written["result"]["snapshot"]["document"], before);
    assert_eq!(written["result"]["content"], source);
    assert!(
        written["loss_report"]
            .as_array()
            .unwrap()
            .iter()
            .all(|loss| {
                !["stereo_configuration", "stereo_marks", "stereo_group"]
                    .contains(&loss["feature"].as_str().unwrap())
            }),
        "preserved stereo must not be reported as lost: {written}"
    );
    assert_eq!(document(&app, &id), before);
}

#[test]
fn accepting_attachment_loss_preserves_the_same_snapshots_stereo() {
    let (app, _dir, id) = setup();
    let source = "F[C@H](Cl)Br |a:1|";
    let imported = import(&app, &id, 0, source);
    assert_eq!(imported["document_revision"], 1, "{imported}");
    let edited = call(
        &app,
        "attachment",
        "document.transaction",
        json!({"document_id": id, "expected_revision": 1, "commands": [
            {"op": "update_atom", "atom": "a2", "attachment_points": [1]}
        ]}),
    );
    assert_eq!(edited["outcome"], "ok", "{edited}");
    let before = document(&app, &id);
    let parameters = json!({"document_id": id, "format": "smiles", "style": "recorded"});
    let refused = call(&app, "refused", "document.export", parameters.clone());
    let details = &refused["diagnostics"][0]["details"];
    assert_eq!(
        refused["diagnostics"][0]["code"], "loss_consent_required",
        "{refused}"
    );
    assert_eq!(details["features"], json!(["attachment_points"]));
    let mut accepted = parameters;
    accepted["accepted_losses"] = details["loss_digest"].clone();
    let written = call(&app, "accepted", "document.export", accepted);
    assert_eq!(written["outcome"], "partial", "{written}");
    assert_eq!(written["result"]["snapshot"]["document"], before);
    assert_eq!(written["result"]["content"], source);
    assert_eq!(document(&app, &id), before);
}
