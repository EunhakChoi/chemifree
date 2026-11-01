//! SMILES import and export through the one operation contract: preview, consent, commit,
//! replay, read-only writes, losses and the catalog wording for every connected combination.
use chemifree_application::Application;
use chemifree_providers::smiles::LocalSmilesCodec;
use chemifree_providers::{
    FileDocumentRepository, LocalCapabilities, V2000Codec, interchange_codec::LocalInterchangeCodec,
};
use serde_json::{Value, json};
use tempfile::TempDir;

type App = Application<LocalCapabilities, FileDocumentRepository>;

fn app(path: &std::path::Path) -> App {
    Application::new(LocalCapabilities, FileDocumentRepository::new(path)).with_smiles_codec(LocalSmilesCodec)
}

fn call(app: &App, id: &str, operation: &str, parameters: Value) -> Value {
    let request = json!({"contract":1,"request_id":id,"profile":"preserve-semantics","operation":operation,"parameters":parameters});
    serde_json::to_value(app.execute(request.to_string().as_bytes())).unwrap()
}

fn setup(title: Option<&str>) -> (App, TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    let application = app(dir.path());
    let parameters = title.map_or_else(|| json!({}), |t| json!({"title": t}));
    let created = call(&application, "create", "document.create", parameters);
    let id = created["result"]["document"]["id"].as_str().unwrap().to_owned();
    (application, dir, id)
}

fn snapshot(app: &App, id: &str) -> Value {
    call(app, "snap", "document.snapshot", json!({"document_id": id}))["result"].clone()
}

fn preview(app: &App, id: &str, source: &str, revision: u64) -> Value {
    call(
        app,
        "preview",
        "document.import",
        json!({"format":"smiles","document_id":id,"expected_revision":revision,"source":source,"preview":true}),
    )
}

fn commit(app: &App, request: &str, id: &str, source: &str, revision: u64, digest: &str, accept: bool) -> Value {
    call(
        app,
        request,
        "document.import",
        json!({"format":"smiles","document_id":id,"expected_revision":revision,"source":source,"preview_digest":digest,"accept_loss":accept}),
    )
}

fn features(reply: &Value) -> Vec<String> {
    reply["loss_report"]
        .as_array()
        .unwrap()
        .iter()
        .map(|loss| loss["feature"].as_str().unwrap().to_owned())
        .collect()
}

fn export(app: &App, request: &str, id: &str, style: &str, accepted: Option<&str>) -> Value {
    let mut parameters = json!({"format":"smiles","document_id":id,"style":style});
    if let Some(digest) = accepted {
        parameters["accepted_losses"] = json!(digest);
    }
    call(app, request, "document.export", parameters)
}

#[test]
fn import_previews_then_commits_against_the_shown_snapshot_without_consent_when_none_is_needed() {
    let (app, _dir, id) = setup(None);
    let shown = preview(&app, &id, "CCO", 0);
    assert_eq!(shown["outcome"], "partial", "{shown}");
    assert_eq!(shown["model_id"], "chemifree-smiles-1");
    assert_eq!(shown["result"]["atoms"], 3);
    assert_eq!(shown["result"]["bonds"], 2);
    assert_eq!(shown["result"]["snapshot"]["document"]["revision"], 0);
    assert_eq!(snapshot(&app, &id)["document"]["atoms"].as_array().unwrap().len(), 0, "a preview changes nothing");
    assert_eq!(features(&shown), ["generated_layout"]);
    assert!(shown["loss_report"][0].get("consent").is_none(), "consent is not part of the wire report");
    let digest = shown["result"]["preview_digest"].as_str().unwrap();
    // Nothing needs consent, so no accept_loss is required.
    let done = commit(&app, "commit", &id, "CCO", 0, digest, false);
    assert_eq!(done["outcome"], "partial", "{done}");
    assert_eq!(done["document_revision"], 1);
    assert_eq!(done["result"]["document"]["atoms"].as_array().unwrap().len(), 3);
    assert_eq!(done["result"]["import_mapping"]["atoms"].as_array().unwrap().len(), 3);
    assert_eq!(done["result"]["import_mapping"]["bonds"].as_array().unwrap().len(), 2);
    let undone = call(&app, "undo", "history.undo", json!({"document_id": id, "expected_revision": 1}));
    assert_eq!(undone["result"]["document"]["atoms"].as_array().unwrap().len(), 0, "one undoable edit");
}

#[test]
fn a_missing_stale_or_foreign_preview_is_refused_and_the_drawing_stays() {
    let (app, _dir, id) = setup(None);
    let shown = preview(&app, &id, "CCO", 0);
    let digest = shown["result"]["preview_digest"].as_str().unwrap().to_owned();
    for (name, source, token) in [("none", "CCO", ""), ("other-source", "CCN", digest.as_str())] {
        let refused = commit(&app, name, &id, source, 0, token, false);
        assert_eq!(refused["diagnostics"][0]["code"], "preview_changed", "{name}: {refused}");
    }
    // The drawing changes after the preview: the digest no longer matches.
    let edit = call(&app, "edit", "document.transaction", json!({"document_id": id, "expected_revision": 0, "commands":[{"op":"add_atom","element":"N","position":{"x":0,"y":0}}]}));
    assert_eq!(edit["outcome"], "ok", "{edit}");
    let stale = commit(&app, "stale", &id, "CCO", 0, &digest, false);
    assert_eq!(stale["diagnostics"][0]["code"], "revision_conflict", "{stale}");
    let again = commit(&app, "again", &id, "CCO", 1, &digest, false);
    assert_eq!(again["diagnostics"][0]["code"], "preview_changed", "{again}");
    assert_eq!(snapshot(&app, &id)["document"]["atoms"].as_array().unwrap().len(), 1);
}

#[test]
fn text_after_whitespace_needs_consent_and_is_never_dropped_silently() {
    let (app, _dir, id) = setup(None);
    let source = "CCO ethanol";
    let shown = preview(&app, &id, source, 0);
    assert!(features(&shown).contains(&"ignored_text".to_owned()), "{shown}");
    let ignored = shown["loss_report"].as_array().unwrap().iter().find(|l| l["feature"] == "ignored_text").unwrap();
    assert_eq!(ignored["source_path"], "source.bytes[4..=10]", "the last byte, not the exclusive end");
    let digest = shown["result"]["preview_digest"].as_str().unwrap().to_owned();
    let refused = commit(&app, "no", &id, source, 0, &digest, false);
    let diagnostic = &refused["diagnostics"][0];
    assert_eq!(diagnostic["code"], "loss_consent_required", "{refused}");
    assert_eq!(diagnostic["details"]["features"], json!(["ignored_text"]));
    assert_eq!(diagnostic["details"]["losses"][0]["feature"], "ignored_text");
    assert_eq!(snapshot(&app, &id)["document"]["atoms"].as_array().unwrap().len(), 0);
    let accepted = commit(&app, "yes", &id, source, 0, &digest, true);
    assert_eq!(accepted["document_revision"], 1, "{accepted}");
}

#[test]
fn a_replayed_request_returns_the_committed_reply_without_placing_again() {
    let (app, _dir, id) = setup(None);
    let shown = preview(&app, &id, "CCO", 0);
    let digest = shown["result"]["preview_digest"].as_str().unwrap().to_owned();
    let first = commit(&app, "once", &id, "CCO", 0, &digest, false);
    assert_eq!(first["outcome"], "partial", "{first}");
    // A second placement makes any new placement for this import impossible (the import
    // needs exactly one), and the drawing has moved on; the replay must not look at either.
    let moved = call(&app, "second", "document.transaction", json!({"document_id": id, "expected_revision": 1, "commands":[{"op":"add_instance","offset":{"x":100,"y":0}}]}));
    assert_eq!(moved["outcome"], "ok", "{moved}");
    let replay = commit(&app, "once", &id, "CCO", 0, &digest, false);
    assert_eq!(replay["outcome"], first["outcome"], "{replay}");
    assert_eq!(replay["document_revision"], first["document_revision"]);
    assert_eq!(replay["result"]["import_mapping"], first["result"]["import_mapping"]);
    assert_eq!(replay["loss_report"], first["loss_report"]);
    // A new request with the same parameters is not a replay and is refused by the scene.
    let fresh = commit(&app, "other", &id, "CCO", 2, &digest, false);
    assert_ne!(fresh["outcome"], "partial", "{fresh}");
    // The same request ID with other parameters is refused.
    let reused = commit(&app, "once", &id, "CCN", 0, &digest, false);
    assert_ne!(reused["outcome"], "partial", "{reused}");
}

#[test]
fn import_appends_to_the_right_of_existing_content() {
    let (app, _dir, id) = setup(None);
    let shown = preview(&app, &id, "CC", 0);
    let digest = shown["result"]["preview_digest"].as_str().unwrap().to_owned();
    commit(&app, "a", &id, "CC", 0, &digest, false);
    let before = snapshot(&app, &id);
    let rightmost = before["document"]["atoms"].as_array().unwrap().iter().map(|a| a["position"]["x"].as_f64().unwrap()).fold(f64::MIN, f64::max);
    let shown = preview(&app, &id, "O", 1);
    let digest = shown["result"]["preview_digest"].as_str().unwrap().to_owned();
    let done = commit(&app, "b", &id, "O", 1, &digest, false);
    let atoms = done["result"]["document"]["atoms"].as_array().unwrap();
    assert_eq!(atoms.len(), 3);
    assert!(atoms[2]["position"]["x"].as_f64().unwrap() > rightmost, "{done}");
}

#[test]
fn reads_report_their_class_position_and_outcome() {
    let (app, _dir, id) = setup(None);
    let syntax = preview(&app, &id, "C=(O)", 0);
    assert_eq!(syntax["outcome"], "error", "{syntax}");
    let diagnostic = &syntax["diagnostics"][0];
    assert_eq!(diagnostic["code"], "dangling_bond");
    assert_eq!(diagnostic["details"]["class"], "syntax");
    assert_eq!(diagnostic["details"]["byte_start"], 1);
    assert_eq!(diagnostic["details"]["byte_end_exclusive"], 2);
    let stereo = preview(&app, &id, "C[C@H](O)N", 0);
    assert_eq!(stereo["outcome"], "unsupported", "{stereo}");
    assert_eq!(stereo["diagnostics"][0]["code"], "stereo_unsupported");
    assert_eq!(stereo["diagnostics"][0]["details"]["class"], "unsupported");
    let limit = preview(&app, &id, &"C".repeat(9000), 0);
    assert_eq!(limit["outcome"], "error");
    assert_eq!(limit["diagnostics"][0]["details"]["class"], "limit");
    let aromatic = preview(&app, &id, "c1ccccc1.c1cccc1", 0);
    assert_eq!(aromatic["diagnostics"][0]["details"]["byte_start"], 9);
    assert_eq!(aromatic["diagnostics"][0]["details"]["reason"], "electron_count");
    assert_eq!(snapshot(&app, &id)["document"]["revision"], 0);
}

#[test]
fn undetermined_radicals_are_recorded_unknown_and_reported() {
    let (app, _dir, id) = setup(None);
    let shown = preview(&app, &id, "[CH2]", 0);
    assert!(features(&shown).contains(&"radical_state".to_owned()), "{shown}");
    assert_eq!(shown["result"]["document"]["atoms"][0]["radical"], "unknown");
    let digest = shown["result"]["preview_digest"].as_str().unwrap().to_owned();
    // The uncertainty is reported but needs no consent.
    let done = commit(&app, "c", &id, "[CH2]", 0, &digest, false);
    assert_eq!(done["document_revision"], 1, "{done}");
}

#[test]
fn recorded_and_canonical_export_are_read_only_and_name_their_snapshot_and_model() {
    let (app, _dir, id) = setup(Some("Target"));
    let shown = preview(&app, &id, "OC(C)=O.[Cl-]", 0);
    let digest = shown["result"]["preview_digest"].as_str().unwrap().to_owned();
    commit(&app, "c", &id, "OC(C)=O.[Cl-]", 0, &digest, false);
    let before = snapshot(&app, &id);
    let recorded = export(&app, "r", &id, "recorded", None);
    assert_eq!(recorded["outcome"], "partial", "{recorded}");
    assert_eq!(recorded["result"]["content"], "OC(C)=O.[Cl-]");
    assert_eq!(recorded["model_id"], "chemifree-smiles-1");
    assert_eq!(recorded["result"]["snapshot"]["document"], before["document"]);
    assert_eq!(recorded["document_revision"], before["document"]["revision"]);
    assert_eq!(recorded["result"]["atom_order"].as_array().unwrap().len(), 5);
    let canonical = export(&app, "c", &id, "canonical", None);
    // [Cl-] is inside the valence table, so this molecule has a canonical form.
    assert_eq!(canonical["outcome"], "partial", "{canonical}");
    assert_eq!(canonical["model_id"], "chemifree-smiles-canonical-1");
    assert_eq!(canonical["result"]["canonical"]["graph_model"], "chemifree-chemical-graph-2");
    assert_eq!(canonical["result"]["canonical"]["implementation"], "chemifree-smiles-canonical-1");
    let after = snapshot(&app, &id);
    assert_eq!(after, before, "writing changes neither the drawing nor its history");
    assert_eq!(after["session"]["undo_depth"], 1);
}

#[test]
fn export_reports_exactly_what_smiles_drops() {
    let (app, _dir, id) = setup(Some("Target"));
    let shown = preview(&app, &id, "CCO", 0);
    let digest = shown["result"]["preview_digest"].as_str().unwrap().to_owned();
    commit(&app, "c", &id, "CCO", 0, &digest, false);
    let out = export(&app, "r", &id, "recorded", None);
    let all = features(&out);
    for expected in ["coordinates", "title", "native_ids", "drawing_settings"] {
        assert!(all.contains(&expected.to_owned()), "{expected} in {all:?}");
    }
    for absent in ["selection", "extra_placement", "scene_object", "scene_references", "placement_id"] {
        assert!(!all.contains(&absent.to_owned()), "{absent} is not present in this drawing: {all:?}");
    }
    for loss in out["loss_report"].as_array().unwrap() {
        let reason = loss["reason"].as_str().unwrap();
        assert!(!reason.contains("first placement"), "SMILES writes no placement: {reason}");
    }
    // A second placement and a graphic are reported when they exist.
    call(&app, "e", "document.transaction", json!({"document_id": id, "expected_revision": 1, "commands":[{"op":"add_instance","offset":{"x":100,"y":0}}]}));
    let two = export(&app, "r2", &id, "recorded", None);
    let all = features(&two);
    assert!(all.contains(&"extra_placement".to_owned()), "{all:?}");
    let extra = two["loss_report"].as_array().unwrap().iter().find(|l| l["feature"] == "extra_placement").unwrap();
    assert_eq!(extra["reason"], "SMILES writes no placement; this one is not written.");
    // The default title is not a real title.
    let (app, _dir, id) = setup(None);
    let shown = preview(&app, &id, "C", 0);
    let digest = shown["result"]["preview_digest"].as_str().unwrap().to_owned();
    commit(&app, "c", &id, "C", 0, &digest, false);
    assert!(!features(&export(&app, "r", &id, "recorded", None)).contains(&"title".to_owned()));
}

#[test]
fn losses_that_need_consent_follow_the_digest_mechanism() {
    let (app, _dir, id) = setup(None);
    let drawn = call(&app, "d", "document.transaction", json!({"document_id": id, "expected_revision": 0, "commands":[
        {"op":"add_atom","element":"Na","position":{"x":0,"y":0}}]}));
    assert_eq!(drawn["outcome"], "ok", "{drawn}");
    let charged = call(&app, "d1", "document.transaction", json!({"document_id": id, "expected_revision": 1, "commands":[
        {"op":"update_atom","atom":"a1","charge":1}]}));
    assert_eq!(charged["outcome"], "ok", "{charged}");
    let refused = export(&app, "x1", &id, "recorded", None);
    let diagnostic = &refused["diagnostics"][0];
    assert_eq!(diagnostic["code"], "loss_consent_required", "{refused}");
    assert_eq!(refused["result"], Value::Null);
    let shown: Vec<&str> = diagnostic["details"]["features"].as_array().unwrap().iter().map(|f| f.as_str().unwrap()).collect();
    assert_eq!(shown, ["hydrogens_assumed", "radical_assumed"]);
    let digest = diagnostic["details"]["loss_digest"].as_str().unwrap().to_owned();
    let wrong = export(&app, "x2", &id, "recorded", Some("deadbeef"));
    assert_eq!(wrong["diagnostics"][0]["code"], "loss_consent_changed");
    let accepted = export(&app, "x3", &id, "recorded", Some(&digest));
    assert_eq!(accepted["result"]["content"], "[Na+]", "{accepted}");
    // The digest is bound to the output: another drawing needs its own.
    call(&app, "d2", "document.transaction", json!({"document_id": id, "expected_revision": 2, "commands":[
        {"op":"add_atom","element":"Fe","position":{"x":40,"y":0}}]}));
    let changed = export(&app, "x4", &id, "recorded", Some(&digest));
    assert_eq!(changed["diagnostics"][0]["code"], "loss_consent_changed", "{changed}");
}

#[test]
fn attachment_points_reach_the_consent_path_in_both_styles() {
    let (app, _dir, id) = setup(None);
    let drawn = call(&app, "d", "document.transaction", json!({"document_id": id, "expected_revision": 0, "commands":[
        {"op":"add_atom","element":"C","attachment_points":[1],"position":{"x":0,"y":0}},
        {"op":"add_atom","element":"O","position":{"x":20,"y":0}},
        {"op":"add_bond","begin":"$0","end":"$1","order":"single"}]}));
    assert_eq!(drawn["outcome"], "ok", "{drawn}");
    for style in ["recorded", "canonical"] {
        let refused = export(&app, &format!("a-{style}"), &id, style, None);
        let diagnostic = &refused["diagnostics"][0];
        assert_eq!(diagnostic["code"], "loss_consent_required", "{style}: {refused}");
        assert_eq!(diagnostic["details"]["features"], json!(["attachment_points"]));
        let digest = diagnostic["details"]["loss_digest"].as_str().unwrap();
        let accepted = export(&app, &format!("b-{style}"), &id, style, Some(digest));
        assert_eq!(accepted["outcome"], "partial", "{style}: {accepted}");
        assert_eq!(accepted["result"]["content"].as_str().unwrap().len(), 2, "{accepted}");
    }
}

#[test]
fn canonical_export_reports_representation_changes_and_folds_hydrogens_apart() {
    let (app, _dir, id) = setup(None);
    let source = "[H]C([H])([H])C1=CC=CC=C1";
    let shown = preview(&app, &id, source, 0);
    let digest = shown["result"]["preview_digest"].as_str().unwrap().to_owned();
    commit(&app, "c", &id, source, 0, &digest, false);
    let out = export(&app, "e", &id, "canonical", None);
    let plain = {
        let (other, _keep, other_id) = setup(None);
        let shown = preview(&other, &other_id, "Cc1ccccc1", 0);
        let digest = shown["result"]["preview_digest"].as_str().unwrap().to_owned();
        commit(&other, "c", &other_id, "Cc1ccccc1", 0, &digest, false);
        export(&other, "e", &other_id, "canonical", None)["result"]["content"].clone()
    };
    assert_eq!(out["result"]["content"], plain, "explicit hydrogens and Kekulé bonds give the aromatic form");
    let all = features(&out);
    assert!(all.contains(&"explicit_hydrogens_folded".to_owned()), "{all:?}");
    assert!(all.contains(&"kekule_to_aromatic".to_owned()), "{all:?}");
    assert_eq!(out["result"]["folded_hydrogens"].as_array().unwrap().len(), 3);
    assert_eq!(out["result"]["atom_order"].as_array().unwrap().len(), 7);
    let folded = out["result"]["folded_hydrogens"].as_array().unwrap();
    assert!(out["result"]["atom_order"].as_array().unwrap().iter().all(|a| !folded.contains(a)));
}

#[test]
fn a_loss_consent_refusal_has_one_envelope_across_formats() {
    let (app, _dir, id) = setup(None);
    let anchored = call(&app, "d", "document.transaction", json!({"document_id": id, "expected_revision": 0, "commands":[
        {"op":"add_atom","element":"C","attachment_points":[1],"position":{"x":0,"y":0}}]}));
    assert_eq!(anchored["outcome"], "ok");
    let smiles_export = export(&app, "s-export", &id, "recorded", None);
    let (other, _keep, other_id) = setup(None);
    let shown = preview(&other, &other_id, "CCO ethanol", 0);
    let digest = shown["result"]["preview_digest"].as_str().unwrap().to_owned();
    let smiles_import = commit(&other, "s-import", &other_id, "CCO ethanol", 0, &digest, false);
    let dir = tempfile::tempdir().unwrap();
    let rich = Application::new(LocalCapabilities, FileDocumentRepository::new(dir.path()))
        .with_interchange_codec(LocalInterchangeCodec);
    let cdxml = r#"<CDXML xmlns="urn:example:cdxml"><page id="1"><fragment id="2"><n id="3" p="0.1 2"/></fragment></page></CDXML>"#;
    let read = serde_json::to_value(rich.execute(json!({"contract":1,"request_id":"r","profile":"preserve-semantics","operation":"document.import","parameters":{"mode":"read","format":"cdxml","source":cdxml}}).to_string().as_bytes())).unwrap();
    let cdx_export = serde_json::to_value(rich.execute(json!({"contract":1,"request_id":"x","profile":"preserve-semantics","operation":"document.export","parameters":{"format":"cdx","document":read["result"]["document"]}}).to_string().as_bytes())).unwrap();
    let shape = |reply: &Value| {
        let mut keys: Vec<&str> = reply.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        (
            keys.join(","),
            reply["envelope"].clone(),
            reply["outcome"].clone(),
            reply["result"].clone(),
            reply["diagnostics"][0]["code"].clone(),
            reply["diagnostics"][0]["severity"].clone(),
            reply["diagnostics"][0]["details"]["features"].is_array(),
            reply["diagnostics"][0]["details"]["losses"].is_array(),
        )
    };
    let expected = shape(&cdx_export);
    assert_eq!(expected.4, "loss_consent_required", "{cdx_export}");
    assert_eq!(shape(&smiles_export), expected, "{smiles_export}");
    assert_eq!(shape(&smiles_import), expected, "{smiles_import}");
}

#[test]
fn the_catalog_and_capabilities_say_one_consistent_thing_for_every_combination() {
    for (ct, rich, smiles) in [
        (false, false, false), (true, false, false), (false, true, false), (true, true, false),
        (false, false, true), (true, false, true), (false, true, true), (true, true, true),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let mut application = Application::new(LocalCapabilities, FileDocumentRepository::new(dir.path()));
        if ct {
            application = application.with_ctfile_codec(V2000Codec);
        }
        if rich {
            application = application.with_interchange_codec(LocalInterchangeCodec);
        }
        if smiles {
            application = application.with_smiles_codec(LocalSmilesCodec);
        }
        let capabilities = serde_json::to_value(application.capabilities()).unwrap();
        let conversion = capabilities["capabilities"].as_array().unwrap().iter().find(|c| c["id"] == "format_conversion").unwrap();
        let operation = |name: &str| capabilities["operation_catalog"]["operations"].as_array().unwrap().iter().find(|o| o["id"] == name).unwrap().clone();
        let (import, export_row) = (operation("document.import"), operation("document.export"));
        let any = ct || rich || smiles;
        assert_eq!(conversion["status"], if any { "available" } else { "not_implemented" }, "{ct} {rich} {smiles}");
        assert_eq!(import["execution"], if any { "executable" } else { "not_implemented" });
        assert_eq!(export_row["execution"], if any { "executable" } else { "not_implemented" });
        if smiles {
            let description = conversion["description"].as_str().unwrap();
            assert!(description.contains("SMILES"), "{description}");
            assert!(!description.contains("Other formats"), "no 'other formats are unsupported' beside a supported one: {description}");
            let label = conversion["label"].as_str().unwrap();
            assert!(label.ends_with(if ct || rich { "subsets" } else { "subset" }), "plural follows the count: {label}");
            for row in [&import, &export_row] {
                let note = row["note"].as_str().unwrap();
                assert!(note.contains("SMILES"), "{note}");
                assert!(!note.contains("other formats") && !note.contains("Other formats"), "{note}");
            }
            // The quoted limits come from the codec's own constants.
            assert!(description.contains(&chemifree_providers::smiles::MAX_INPUT_BYTES.to_string()), "{description}");
            assert!(description.contains(&chemifree_providers::smiles::MAX_COMMANDS.to_string()), "{description}");
        } else {
            assert!(!conversion["description"].as_str().unwrap().contains("SMILES"));
            assert!(!import["note"].as_str().unwrap().contains("SMILES"));
        }
        // The dispatcher runs exactly what the catalog says it runs.
        let probe = call(&application, "p", "document.import", json!({"format":"smiles","document_id":"x","expected_revision":0,"source":"C","preview":true}));
        if smiles {
            assert_ne!(probe["outcome"], "unsupported", "{probe}");
        } else {
            let code = if any { "unsupported_format" } else { "operation_not_implemented" };
            assert_eq!(probe["diagnostics"][0]["code"], code, "{probe}");
        }
    }
}

#[test]
fn smiles_is_refused_when_the_provider_is_not_connected_and_other_formats_when_only_smiles_is() {
    let dir = tempfile::tempdir().unwrap();
    let only = app(dir.path());
    let created = call(&only, "c", "document.create", json!({}));
    let id = created["result"]["document"]["id"].as_str().unwrap().to_owned();
    for format in ["mol", "sdf", "cdxml", "cdx", "inchi"] {
        let reply = call(&only, format, "document.export", json!({"format":format,"document_id":id,"style":"recorded"}));
        assert_eq!(reply["diagnostics"][0]["code"], "unsupported_format", "{format}: {reply}");
    }
}
