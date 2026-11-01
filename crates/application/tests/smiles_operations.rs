//! SMILES through `document.import` and `document.export`: the same preview, consent, replay and
//! read-only rules as the other formats, over the shared Application.

use chemifree_application::Application;
use chemifree_providers::interchange_codec::LocalInterchangeCodec;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities, OpenSmilesCodec, V2000Codec};
use serde_json::{Value, json};
use tempfile::TempDir;

type App = Application<LocalCapabilities, FileDocumentRepository>;

fn app(path: &std::path::Path) -> App {
    Application::new(LocalCapabilities, FileDocumentRepository::new(path))
        .with_ctfile_codec(V2000Codec)
        .with_interchange_codec(LocalInterchangeCodec)
        .with_smiles_codec(OpenSmilesCodec)
}

fn call(app: &App, id: &str, operation: &str, parameters: Value) -> Value {
    let request = json!({"contract":1,"request_id":id,"profile":"preserve-semantics","operation":operation,"parameters":parameters});
    serde_json::to_value(app.execute(request.to_string().as_bytes())).unwrap()
}

fn setup() -> (App, TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    let application = app(dir.path());
    let created = call(&application, "create", "document.create", json!({}));
    let id = created["result"]["document"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    (application, dir, id)
}

fn snapshot(app: &App, id: &str) -> Value {
    call(
        app,
        "snapshot",
        "document.snapshot",
        json!({"document_id":id}),
    )["result"]
        .clone()
}

fn preview(app: &App, request: &str, id: &str, revision: u64, source: &str) -> Value {
    call(
        app,
        request,
        "document.import",
        json!({"document_id":id,"expected_revision":revision,"format":"smiles","source":source,"preview":true}),
    )
}

fn commit(
    app: &App,
    request: &str,
    id: &str,
    revision: u64,
    source: &str,
    digest: &str,
    accept: bool,
) -> Value {
    call(
        app,
        request,
        "document.import",
        json!({"document_id":id,"expected_revision":revision,"format":"smiles","source":source,"preview_digest":digest,"accept_loss":accept}),
    )
}

fn features(report: &Value) -> Vec<&str> {
    report["loss_report"]
        .as_array()
        .unwrap()
        .iter()
        .map(|loss| loss["feature"].as_str().unwrap())
        .collect()
}

fn export(app: &App, request: &str, id: &str, style: &str) -> Value {
    call(
        app,
        request,
        "document.export",
        json!({"format":"smiles","document_id":id,"style":style}),
    )
}

fn atoms(document: &Value) -> usize {
    document["atoms"].as_array().unwrap().len()
}

#[test]
fn a_plain_string_previews_and_commits_without_accepting_a_loss() {
    let (application, _directory, id) = setup();
    let report = preview(&application, "preview", &id, 0, "CCO");
    assert_eq!(report["outcome"], "partial", "{report}");
    assert_eq!(report["model_id"], "chemifree-smiles-1");
    assert_eq!(features(&report), ["generated_layout"]);
    let result = &report["result"];
    assert_eq!(result["atoms"].as_array().unwrap().len(), 3);
    assert_eq!(result["bonds"].as_array().unwrap().len(), 2);
    assert_eq!(result["valence_model"], "chemifree-valence-2");
    assert_eq!(result["aromaticity_model"], "chemifree-aromaticity-2");
    assert_eq!(result["snapshot"]["document"]["revision"], 0);
    assert_eq!(
        atoms(&snapshot(&application, &id)["document"]),
        0,
        "a preview changes nothing"
    );
    let digest = result["preview_digest"].as_str().unwrap();
    // No loss needs consent, so the commit does not ask for accept_loss.
    let done = commit(&application, "commit", &id, 0, "CCO", digest, false);
    assert_eq!(done["outcome"], "partial", "{done}");
    assert_eq!(atoms(&done["result"]["document"]), 3);
    assert_eq!(
        done["result"]["import_mapping"]["atoms"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        done["result"]["import_mapping"]["bonds"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(done["document_revision"], 1);
    let undone = call(
        &application,
        "undo",
        "history.undo",
        json!({"document_id":id,"expected_revision":1}),
    );
    assert_eq!(atoms(&undone["result"]["document"]), 0, "{undone}");
}

#[test]
fn losses_that_change_the_chemistry_or_drop_text_need_consent() {
    for (source, feature) in [
        ("[Na+]", "radical_state"),
        ("[CH2]", "radical_state"),
        ("CCO\nCC", "ignored_text"),
        ("[CH4:3]", "atom_class"),
    ] {
        let (application, _directory, id) = setup();
        let report = preview(&application, "preview", &id, 0, source);
        assert!(features(&report).contains(&feature), "{source}: {report}");
        let digest = report["result"]["preview_digest"]
            .as_str()
            .unwrap()
            .to_owned();
        let refused = commit(&application, "refuse", &id, 0, source, &digest, false);
        assert_eq!(refused["outcome"], "error", "{refused}");
        let diagnostic = &refused["diagnostics"][0];
        assert_eq!(diagnostic["code"], "loss_consent_required", "{refused}");
        let listed: Vec<&str> = diagnostic["details"]["features"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f.as_str().unwrap())
            .collect();
        assert!(
            listed.contains(&feature) && !listed.contains(&"generated_layout"),
            "only losses that need consent are listed: {listed:?}"
        );
        assert!(
            !diagnostic["details"]["losses"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            atoms(&snapshot(&application, &id)["document"]),
            0,
            "{source}"
        );
        let accepted = commit(&application, "accept", &id, 0, source, &digest, true);
        assert_eq!(accepted["outcome"], "partial", "{source}: {accepted}");
        assert!(atoms(&accepted["result"]["document"]) >= 1, "{source}");
    }
}

#[test]
fn the_preview_reports_ethanol_and_the_ignored_text_of_a_pasted_second_line() {
    let (application, _directory, id) = setup();
    let report = preview(&application, "preview", &id, 0, "CCO\nCC");
    let result = &report["result"];
    assert_eq!(result["atoms"].as_array().unwrap().len(), 3);
    assert_eq!(result["ignored_text"]["offset"], 4);
    assert_eq!(result["ignored_text"]["length"], 2);
    assert_eq!(result["ignored_text"]["character_offset"], 4);
    let loss = report["loss_report"]
        .as_array()
        .unwrap()
        .iter()
        .find(|loss| loss["feature"] == "ignored_text")
        .unwrap();
    assert_eq!(loss["source_path"], "source[4..=5]");
}

#[test]
fn a_preview_is_bound_to_its_source_graph_snapshot_and_service() {
    let (application, _directory, id) = setup();
    let report = preview(&application, "p", &id, 0, "CCO");
    let digest = report["result"]["preview_digest"]
        .as_str()
        .unwrap()
        .to_owned();
    // Another string, a missing digest and a wrong digest are all a changed preview.
    for (source, digest) in [("CCN", digest.as_str()), ("CCO", "0"), ("CCO", "")] {
        let refused = commit(&application, "c", &id, 0, source, digest, true);
        assert_eq!(
            refused["diagnostics"][0]["code"], "preview_changed",
            "{refused}"
        );
    }
    let missing = call(
        &application,
        "m",
        "document.import",
        json!({"document_id":id,"expected_revision":0,"format":"smiles","source":"CCO"}),
    );
    assert_eq!(missing["diagnostics"][0]["code"], "preview_changed");
    // The drawing changed after the preview: the same digest no longer matches.
    let edit = call(
        &application,
        "edit",
        "document.transaction",
        json!({"document_id":id,"expected_revision":0,"commands":[{"op":"add_atom","element":"N","position":{"x":0,"y":0}}]}),
    );
    assert_eq!(edit["outcome"], "ok", "{edit}");
    let stale = commit(&application, "stale", &id, 0, "CCO", &digest, true);
    assert_eq!(
        stale["diagnostics"][0]["code"], "revision_conflict",
        "{stale}"
    );
    let fresh = commit(&application, "fresh", &id, 1, "CCO", &digest, true);
    assert_eq!(
        fresh["diagnostics"][0]["code"], "preview_changed",
        "{fresh}"
    );
    assert_eq!(atoms(&snapshot(&application, &id)["document"]), 1);
}

#[test]
fn a_new_service_rejects_a_preview_of_the_previous_one() {
    let (application, directory, id) = setup();
    let report = preview(&application, "p", &id, 0, "CCO");
    let digest = report["result"]["preview_digest"]
        .as_str()
        .unwrap()
        .to_owned();
    drop(application);
    let restarted = app(directory.path());
    let refused = commit(&restarted, "c", &id, 0, "CCO", &digest, true);
    assert_eq!(
        refused["diagnostics"][0]["code"], "preview_changed",
        "{refused}"
    );
}

#[test]
fn a_replayed_import_returns_the_committed_reply_without_placing_atoms_again() {
    let (application, _directory, id) = setup();
    let report = preview(&application, "p", &id, 0, "CCO");
    let digest = report["result"]["preview_digest"]
        .as_str()
        .unwrap()
        .to_owned();
    let first = commit(&application, "same-request", &id, 0, "CCO", &digest, false);
    assert_eq!(first["outcome"], "partial", "{first}");
    // Fill the space below the drawing so that placing the atoms again would be out of range.
    let edit = call(
        &application,
        "far",
        "document.transaction",
        json!({"document_id":id,"expected_revision":1,"commands":[{"op":"add_atom","element":"N","position":{"x":0,"y":99990}}]}),
    );
    assert_eq!(edit["outcome"], "ok", "{edit}");
    let fresh = preview(&application, "p2", &id, 2, "CCO");
    assert_eq!(
        fresh["diagnostics"][0]["code"], "coordinate_out_of_range",
        "a new import could not be placed there: {fresh}"
    );
    let replay = commit(&application, "same-request", &id, 0, "CCO", &digest, false);
    assert_eq!(replay["outcome"], "partial", "{replay}");
    assert_eq!(replay["result"]["document"], first["result"]["document"]);
    assert_eq!(
        replay["result"]["import_mapping"],
        first["result"]["import_mapping"]
    );
    assert_eq!(replay["loss_report"], first["loss_report"]);
    assert_eq!(
        atoms(&snapshot(&application, &id)["document"]),
        4,
        "nothing was added again"
    );
    // The same request ID for a different import is refused.
    let reused = commit(&application, "same-request", &id, 0, "CCN", &digest, false);
    assert_eq!(
        reused["diagnostics"][0]["code"], "request_id_reused",
        "{reused}"
    );
}

#[test]
fn new_atoms_are_placed_below_what_the_drawing_holds() {
    let (application, _directory, id) = setup();
    let edit = call(
        &application,
        "edit",
        "document.transaction",
        json!({"document_id":id,"expected_revision":0,"commands":[{"op":"add_atom","element":"N","position":{"x":50,"y":70}}]}),
    );
    assert_eq!(edit["outcome"], "ok");
    let report = preview(&application, "p", &id, 1, "CCO");
    let placed = report["result"]["document"]["atoms"].as_array().unwrap();
    assert_eq!(placed.len(), 4);
    for atom in &placed[1..] {
        assert!(atom["position"]["y"].as_f64().unwrap() > 70.0, "{atom}");
    }
    assert_eq!(placed[0]["position"]["x"], 50.0);
}

#[test]
fn errors_name_the_offending_part_in_bytes_and_in_characters() {
    let (application, _directory, id) = setup();
    let refused = preview(&application, "bad", &id, 0, "C\u{e9}O");
    assert_eq!(refused["outcome"], "error");
    let diagnostic = &refused["diagnostics"][0];
    assert_eq!(diagnostic["code"], "unexpected_character");
    let details = &diagnostic["details"];
    assert_eq!(details["class"], "syntax");
    assert_eq!(
        (details["offset"].as_u64(), details["length"].as_u64()),
        (Some(1), Some(2))
    );
    assert_eq!(
        (
            details["character_offset"].as_u64(),
            details["character_length"].as_u64()
        ),
        (Some(1), Some(1))
    );
    // A multibyte character before the fault moves the byte offset but not the character one.
    let later = preview(&application, "bad2", &id, 0, "[\u{e9}]");
    assert_eq!(later["diagnostics"][0]["details"]["offset"], 1);
    let after = preview(&application, "bad3", &id, 0, "\u{e9}");
    assert_eq!(after["diagnostics"][0]["details"]["length"], 2);
    let aromatic = preview(&application, "bad4", &id, 0, "c1cccc1");
    assert_eq!(
        aromatic["diagnostics"][0]["code"],
        "aromatic_system_unsupported"
    );
    assert!(aromatic["diagnostics"][0]["details"]["reason"].is_string());
    let stereo = preview(&application, "bad5", &id, 0, "F/C=C/F");
    assert_eq!(stereo["diagnostics"][0]["details"]["class"], "unsupported");
    let limit = preview(&application, "bad6", &id, 0, &"C".repeat(300));
    assert_eq!(limit["diagnostics"][0]["details"]["class"], "limit");
    assert_eq!(atoms(&snapshot(&application, &id)["document"]), 0);
}

#[test]
fn a_document_with_two_placements_is_not_imported_into() {
    let (application, _directory, id) = setup();
    let edit = call(
        &application,
        "two",
        "document.transaction",
        json!({"document_id":id,"expected_revision":0,"commands":[{"op":"add_instance","offset":{"x":10,"y":10}}]}),
    );
    assert_eq!(edit["outcome"], "ok", "{edit}");
    let refused = preview(&application, "p", &id, 1, "CCO");
    assert_eq!(
        refused["diagnostics"][0]["code"], "unsupported_scene",
        "{refused}"
    );
}

#[test]
fn export_is_read_only_and_names_its_snapshot_and_model() {
    let (application, _directory, id) = setup();
    let report = preview(&application, "p", &id, 0, "OC(C)=O");
    let digest = report["result"]["preview_digest"]
        .as_str()
        .unwrap()
        .to_owned();
    commit(&application, "c", &id, 0, "OC(C)=O", &digest, false);
    let before = snapshot(&application, &id);
    for (style, model, text) in [
        ("recorded", "chemifree-smiles-1", "OC(C)=O"),
        ("canonical", "chemifree-smiles-canonical-1", ""),
    ] {
        let written = export(&application, style, &id, style);
        assert_eq!(written["outcome"], "partial", "{written}");
        assert_eq!(written["model_id"], model);
        assert_eq!(written["document_revision"], 1);
        let result = &written["result"];
        assert_eq!(result["style"], style);
        assert_eq!(result["snapshot"], before, "the exact snapshot is returned");
        assert_eq!(result["valence_model"], "chemifree-valence-2");
        if style == "recorded" {
            assert_eq!(result["content"], text);
            assert_eq!(result["graph_model"], Value::Null);
            assert_eq!(result["atom_order"], json!(["a1", "a2", "a3", "a4"]));
        } else {
            assert_eq!(result["graph_model"], "chemifree-chemical-graph-2");
            assert_eq!(result["atom_order"].as_array().unwrap().len(), 4);
        }
        assert_eq!(
            snapshot(&application, &id),
            before,
            "an export changes nothing"
        );
    }
    let undo = call(
        &application,
        "u",
        "history.undo",
        json!({"document_id":id,"expected_revision":1}),
    );
    assert_eq!(
        undo["outcome"], "ok",
        "the import is still undoable: {undo}"
    );
}

#[test]
fn canonical_export_lists_folded_hydrogens_apart_from_the_atom_order() {
    let (application, _directory, id) = setup();
    let edit = call(
        &application,
        "edit",
        "document.transaction",
        json!({"document_id":id,"expected_revision":0,"commands":[
            {"op":"add_atom","element":"C","position":{"x":0,"y":0}},
            {"op":"add_atom","element":"H","position":{"x":20,"y":0},"bond_to":{"atom":"$0","order":"single"}}
        ]}),
    );
    assert_eq!(edit["outcome"], "ok", "{edit}");
    let written = export(&application, "w", &id, "canonical");
    let result = &written["result"];
    assert_eq!(result["content"], "C");
    assert_eq!(result["atom_order"], json!(["a1"]));
    assert_eq!(
        result["folded_hydrogens"],
        json!([{"hydrogen":"a2","attached_to":"a1"}])
    );
    assert!(features(&written).contains(&"folded_hydrogens"));
}

#[test]
fn the_drawings_own_content_is_reported_as_lost_only_when_it_exists() {
    let (application, _directory, id) = setup();
    let blank = export(&application, "blank-fails", &id, "recorded");
    assert_eq!(blank["diagnostics"][0]["code"], "empty_graph", "{blank}");
    let edit = call(
        &application,
        "e",
        "document.transaction",
        json!({"document_id":id,"expected_revision":0,"commands":[{"op":"add_atom","element":"C","position":{"x":0,"y":0}}]}),
    );
    assert_eq!(edit["outcome"], "ok");
    let plain = export(&application, "plain", &id, "recorded");
    let plain_features = features(&plain);
    for expected in [
        "coordinates",
        "native_ids",
        "placement_id",
        "drawing_settings",
    ] {
        assert!(
            plain_features.contains(&expected),
            "{expected} in {plain_features:?}"
        );
    }
    for absent in [
        "title",
        "selection",
        "extra_placement",
        "scene_object",
        "scene_references",
    ] {
        assert!(
            !plain_features.contains(&absent),
            "{absent} was reported without existing"
        );
    }
    // A real title, a second placement, a graphic and a selection exist and are reported.
    let rename = call(
        &application,
        "r",
        "document.transaction",
        json!({"document_id":id,"expected_revision":1,"commands":[
        {"op":"rename","title":"Caffeine study"},
        {"op":"add_instance","offset":{"x":5,"y":5}},
        {"op":"add_graphic","shape":"line","points":[{"x":0,"y":0},{"x":10,"y":10}]}
    ],"selection_after":{"atoms":["a1"],"bonds":[],"objects":[]}}),
    );
    assert_eq!(rename["outcome"], "ok", "{rename}");
    let rich = export(&application, "rich", &id, "recorded");
    assert_eq!(rich["outcome"], "partial", "{rich}");
    let rich_features = features(&rich);
    for expected in ["title", "selection", "extra_placement", "scene_object"] {
        assert!(
            rich_features.contains(&expected),
            "{expected} in {rich_features:?}"
        );
    }
    let wording: String = rich["loss_report"].to_string();
    assert!(
        !wording.contains("first placement"),
        "SMILES writes no placement: {wording}"
    );
    assert!(!wording.contains("V2000"), "{wording}");
}

#[test]
fn ions_and_metals_export_as_recorded_text_with_consent_required_assumptions() {
    let (application, _directory, id) = setup();
    let edit = call(
        &application,
        "e",
        "document.transaction",
        json!({"document_id":id,"expected_revision":0,"commands":[
            {"op":"add_atom","element":"Fe","position":{"x":0,"y":0}},
            {"op":"add_atom","element":"Na","position":{"x":30,"y":0}},
            {"op":"update_atom","atom":"a2","charge":1}
        ]}),
    );
    assert_eq!(edit["outcome"], "ok", "{edit}");
    let refused = export(&application, "first", &id, "recorded");
    let diagnostic = &refused["diagnostics"][0];
    assert_eq!(diagnostic["code"], "loss_consent_required", "{refused}");
    let listed: Vec<&str> = diagnostic["details"]["features"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f.as_str().unwrap())
        .collect();
    assert_eq!(listed, ["hydrogen_assumption", "radical_assumption"]);
    assert!(refused["result"].is_null());
    let digest = diagnostic["details"]["loss_digest"]
        .as_str()
        .unwrap()
        .to_owned();
    let accept = |request: &str, digest: &str| {
        call(
            &application,
            request,
            "document.export",
            json!({"format":"smiles","document_id":id,"style":"recorded","accepted_losses":digest}),
        )
    };
    let accepted = accept("second", &digest);
    assert_eq!(accepted["outcome"], "partial", "{accepted}");
    assert_eq!(accepted["result"]["content"], "[Fe].[Na+]");
    let wrong = accept("third", "0000");
    assert_eq!(wrong["diagnostics"][0]["code"], "loss_consent_changed");
    // The canonical form has no row for them and is refused with its reason.
    let canonical = export(&application, "canon", &id, "canonical");
    assert_eq!(canonical["diagnostics"][0]["code"], "canonical_unsupported");
    assert_eq!(
        canonical["diagnostics"][0]["details"]["reason"],
        "element_or_charge"
    );
    assert_eq!(
        canonical["diagnostics"][0]["details"]["class"],
        "not_writable"
    );
}

#[test]
fn attachment_points_reach_the_consent_path_in_both_styles() {
    let (application, _directory, id) = setup();
    let edit = call(
        &application,
        "e",
        "document.transaction",
        json!({"document_id":id,"expected_revision":0,"commands":[
            {"op":"add_atom","element":"C","position":{"x":0,"y":0},"attachment_points":[1]},
            {"op":"add_atom","element":"O","position":{"x":20,"y":0},"bond_to":{"atom":"$0","order":"single"}}
        ]}),
    );
    assert_eq!(edit["outcome"], "ok", "{edit}");
    for (style, text) in [("recorded", "CO"), ("canonical", "C(O)")] {
        let refused = export(&application, style, &id, style);
        let diagnostic = &refused["diagnostics"][0];
        assert_eq!(
            diagnostic["code"], "loss_consent_required",
            "{style}: {refused}"
        );
        assert_eq!(
            diagnostic["details"]["features"],
            json!(["attachment_points"])
        );
        let digest = diagnostic["details"]["loss_digest"].as_str().unwrap();
        let accepted = call(
            &application,
            &format!("{style}-ok"),
            "document.export",
            json!({"format":"smiles","document_id":id,"style":style,"accepted_losses":digest}),
        );
        assert_eq!(accepted["outcome"], "partial", "{style}: {accepted}");
        let content = accepted["result"]["content"].as_str().unwrap();
        assert!(
            content.contains('C') && content.contains('O'),
            "{style}: {content} vs {text}"
        );
    }
    // Without the attachment point there is nothing to consent to.
    let clear = call(
        &application,
        "c",
        "document.transaction",
        json!({"document_id":id,"expected_revision":1,"commands":[
            {"op":"update_atom","atom":"a1","attachment_points":[]}
        ]}),
    );
    assert_eq!(clear["outcome"], "ok", "{clear}");
    assert_eq!(
        export(&application, "ok", &id, "recorded")["outcome"],
        "partial"
    );
}

#[test]
fn a_drawing_the_writers_cannot_state_is_refused_with_the_real_cause() {
    let (application, _directory, id) = setup();
    let edit = call(
        &application,
        "e",
        "document.transaction",
        json!({"document_id":id,"expected_revision":0,"commands":[
            {"op":"add_atom","element":"C","position":{"x":0,"y":0},"hydrogens":{"mode":"unknown"}}
        ]}),
    );
    assert_eq!(edit["outcome"], "ok", "{edit}");
    for style in ["recorded", "canonical"] {
        let refused = export(&application, style, &id, style);
        assert_eq!(refused["outcome"], "error", "{refused}");
        assert!(refused["result"].is_null());
        let code = refused["diagnostics"][0]["code"].as_str().unwrap();
        assert!(
            ["hydrogens_not_writable", "canonical_unsupported"].contains(&code),
            "{code}"
        );
    }
    let unknown = call(
        &application,
        "style",
        "document.export",
        json!({"format":"smiles","document_id":id,"style":"fancy"}),
    );
    assert_eq!(unknown["diagnostics"][0]["code"], "invalid_request");
}

#[test]
fn a_consent_refusal_has_one_envelope_in_every_format() {
    let (application, _directory, id) = setup();
    // SMILES export.
    let edit = call(
        &application,
        "e",
        "document.transaction",
        json!({"document_id":id,"expected_revision":0,"commands":[{"op":"add_atom","element":"Fe","position":{"x":0,"y":0}}]}),
    );
    assert_eq!(edit["outcome"], "ok", "{edit}");
    let smiles = export(&application, "smiles", &id, "recorded");
    // CDX export of a tree whose coordinates would be rounded.
    let cdxml = r#"<CDXML xmlns="urn:example:cdxml"><page id="1"><fragment id="2"><n id="3" p="0.1 2"/></fragment></page></CDXML>"#;
    let read = call(
        &application,
        "read",
        "document.import",
        json!({"mode":"read","format":"cdxml","source":cdxml}),
    );
    let cdx = call(
        &application,
        "cdx",
        "document.export",
        json!({"format":"cdx","document":read["result"]["document"]}),
    );
    // Imports: SMILES, MOL records and CDXML creation each ask for acceptance.
    let report = preview(&application, "pre", &id, 1, "[Na+]");
    let digest = report["result"]["preview_digest"].as_str().unwrap();
    let smiles_import = commit(
        &application,
        "smiles-import",
        &id,
        1,
        "[Na+]",
        digest,
        false,
    );
    let mol = "Ammonium\n\n\n  2  1  0  0  0  0  0  0  0  0999 V2000\n    0.0000    1.0000    0.0000 N   0  0  0  0  0  0  0  0  0  0  0  0\n    1.0000    0.0000    0.0000 H   0  0  0  0  0  0  0  0  0  0  0  0\n  1  2  1  0  0  0  0\nM  CHG  1   1   1\nM  END\n";
    let mol_report = call(
        &application,
        "mol-preview",
        "document.import",
        json!({"document_id":id,"expected_revision":1,"format":"mol","source":mol,"record":1,"preview":true}),
    );
    let mol_digest = mol_report["result"]["preview_digest"].as_str().unwrap();
    let mol_import = call(
        &application,
        "mol-import",
        "document.import",
        json!({"document_id":id,"expected_revision":1,"format":"mol","source":mol,"record":1,"preview_digest":mol_digest,"accept_loss":false}),
    );
    let cdxml_preview = call(
        &application,
        "cdxml-preview",
        "document.import",
        json!({"mode":"preview","format":"cdxml","source":cdxml_with_loss(),"title":"T","page_id":1,"fragment_id":2}),
    );
    let cdxml_digest = cdxml_preview["result"]["preview_digest"].as_str().unwrap();
    let cdxml_import = call(
        &application,
        "cdxml-import",
        "document.import",
        json!({"mode":"create","format":"cdxml","source":cdxml_with_loss(),"title":"T","page_id":1,"fragment_id":2,"preview_digest":cdxml_digest,"accept_loss":false}),
    );
    // The envelope: every field of the reply and of its diagnostic, whichever format refused.
    let envelope = |response: &Value| {
        let keys = |value: &Value| -> Vec<String> {
            let mut keys: Vec<String> = value.as_object().unwrap().keys().cloned().collect();
            keys.sort();
            keys
        };
        let diagnostic = &response["diagnostics"][0];
        (
            keys(response),
            keys(diagnostic),
            response["envelope"].clone(),
            response["outcome"].clone(),
            response["result"].clone(),
            response["loss_report"].clone(),
            diagnostic["severity"].clone(),
            diagnostic["path"].clone(),
            diagnostic["code"].clone(),
            diagnostic["details"].is_object(),
        )
    };
    let reference = envelope(&smiles);
    assert_eq!(reference.8, "loss_consent_required");
    assert_eq!(reference.4, Value::Null);
    for (name, response) in [
        ("cdx export", &cdx),
        ("smiles import", &smiles_import),
        ("mol import", &mol_import),
        ("cdxml create", &cdxml_import),
    ] {
        assert_eq!(envelope(response), reference, "{name}: {response}");
    }
    // Both SMILES refusals name what the caller would accept, in the same two fields.
    for response in [&smiles, &smiles_import] {
        assert!(response["diagnostics"][0]["details"]["features"].is_array());
        assert!(response["diagnostics"][0]["details"]["losses"].is_array());
    }
}

fn cdxml_with_loss() -> &'static str {
    "\u{feff}<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!--outside-->\n<CDXML><page id=\"1\"><fragment id=\"2\"><n id=\"3\" p=\"1 2\"/><n id=\"4\" p=\"3 4\" Element=\"8\" Charge=\"-1\"/><b id=\"5\" B=\"3\" E=\"4\" Order=\"2\"/></fragment><t id=\"6\"><s>α &amp; β</s></t></page><page id=\"7\"><fragment id=\"8\"><n id=\"9\"/></fragment></page></CDXML>"
}
