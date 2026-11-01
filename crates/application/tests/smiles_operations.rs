//! SMILES import and export through the operation contract.
use chemifree_application::Application;
use chemifree_providers::{
    FileDocumentRepository, LocalCapabilities, V2000Codec, interchange_codec::LocalInterchangeCodec,
};
use serde_json::{Value, json};
use tempfile::TempDir;

type App = Application<LocalCapabilities, FileDocumentRepository>;

fn app(path: &std::path::Path) -> App {
    Application::new(LocalCapabilities, FileDocumentRepository::new(path)).with_smiles()
}

fn setup() -> (App, TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    let application = app(dir.path());
    let created = call(
        &application,
        "create",
        "document.create",
        json!({"title":"Target"}),
    );
    let id = created["result"]["document"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    (application, dir, id)
}

fn call(app: &App, id: &str, operation: &str, parameters: Value) -> Value {
    let request = json!({"contract":1,"request_id":id,"profile":"preserve-semantics","operation":operation,"parameters":parameters});
    serde_json::to_value(app.execute(request.to_string().as_bytes())).unwrap()
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

fn preview(app: &App, id: &str, revision: u64, source: &str) -> Value {
    call(
        app,
        "preview",
        "document.import",
        json!({"document_id":id,"expected_revision":revision,"format":"smiles","source":source,"preview":true}),
    )
}

fn create(app: &App, request: &str, id: &str, revision: u64, source: &str, digest: &str) -> Value {
    call(
        app,
        request,
        "document.import",
        json!({"document_id":id,"expected_revision":revision,"format":"smiles","source":source,"preview_digest":digest,"accept_loss":true}),
    )
}

fn import(app: &App, request: &str, id: &str, revision: u64, source: &str) -> Value {
    let shown = preview(app, id, revision, source);
    let digest = shown["result"]["preview_digest"]
        .as_str()
        .expect("a preview")
        .to_owned();
    create(app, request, id, revision, source, &digest)
}

fn export(app: &App, id: &str, form: &str) -> Value {
    call(
        app,
        &format!("export-{form}"),
        "document.export",
        json!({"format":"smiles","document_id":id,"form":form}),
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

#[test]
fn a_preview_changes_nothing_and_binds_the_snapshot() {
    let (application, _dir, id) = setup();
    let before = snapshot(&application, &id);
    let shown = preview(&application, &id, 0, "c1ccccc1O");
    assert_eq!(shown["outcome"], "partial", "{shown}");
    assert_eq!(shown["document_revision"], 0);
    assert_eq!(shown["result"]["snapshot"], before);
    assert_eq!(
        shown["result"]["document"]["atoms"]
            .as_array()
            .unwrap()
            .len(),
        7
    );
    assert_eq!(
        shown["result"]["document"]["bonds"]
            .as_array()
            .unwrap()
            .len(),
        7
    );
    assert_eq!(shown["result"]["components"], 1);
    assert_eq!(
        snapshot(&application, &id),
        before,
        "a preview must not touch the drawing"
    );
    assert!(features(&shown).contains(&"generated_layout".to_owned()));
}

#[test]
fn creating_needs_the_preview_digest_and_consent() {
    let (application, _dir, id) = setup();
    let shown = preview(&application, &id, 0, "CCO");
    let digest = shown["result"]["preview_digest"].as_str().unwrap();

    let wrong = create(&application, "wrong", &id, 0, "CCO", &"0".repeat(64));
    assert_eq!(wrong["diagnostics"][0]["code"], "preview_changed");
    let other_source = create(&application, "other", &id, 0, "CCC", digest);
    assert_eq!(other_source["diagnostics"][0]["code"], "preview_changed");

    let no_consent = call(
        &application,
        "consent",
        "document.import",
        json!({"document_id":id,"expected_revision":0,"format":"smiles","source":"CCO","preview_digest":digest}),
    );
    assert_eq!(
        no_consent["diagnostics"][0]["code"],
        "loss_consent_required"
    );
    assert_eq!(snapshot(&application, &id)["document"]["revision"], 0);

    let stale = create(&application, "stale", &id, 7, "CCO", digest);
    assert_eq!(stale["outcome"], "error");
    assert_eq!(snapshot(&application, &id)["document"]["revision"], 0);

    let created = create(&application, "ok", &id, 0, "CCO", digest);
    assert_eq!(created["outcome"], "partial", "{created}");
    assert_eq!(created["document_revision"], 1);
    let atoms = created["result"]["document"]["atoms"].as_array().unwrap();
    assert_eq!(atoms.len(), 3);
    assert_eq!(
        created["result"]["import_mapping"]["atoms"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        created["result"]["import_mapping"]["bonds"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(created["result"]["session"]["undo_depth"], 1);

    // The preview is bound to the revision: it no longer matches the changed drawing.
    let again = create(&application, "again", &id, 1, "CCO", digest);
    assert_eq!(again["diagnostics"][0]["code"], "preview_changed");
}

#[test]
fn a_second_structure_is_placed_beside_the_first() {
    let (application, _dir, id) = setup();
    import(&application, "one", &id, 0, "CC");
    let second = import(&application, "two", &id, 1, "NN");
    let atoms = second["result"]["document"]["atoms"].as_array().unwrap();
    assert_eq!(atoms.len(), 4);
    let x = |i: usize| atoms[i]["position"]["x"].as_f64().unwrap();
    assert!(
        x(2) > x(1),
        "the second structure starts right of the first: {atoms:?}"
    );
    // Reading and writing it back keeps the recorded order.
    assert_eq!(
        export(&application, &id, "recorded")["result"]["content"],
        "CC.NN"
    );
}

#[test]
fn a_replayed_import_returns_the_committed_reply_without_placing_again() {
    let (application, _dir, id) = setup();
    let shown = preview(&application, &id, 0, "OCC");
    let digest = shown["result"]["preview_digest"]
        .as_str()
        .unwrap()
        .to_owned();
    let first = create(&application, "same-request", &id, 0, "OCC", &digest);
    assert_eq!(first["document_revision"], 1);

    // The drawing changes so that placing the import again would differ, and then so that it
    // could not be placed at all (two placements).
    let moved = call(
        &application,
        "move",
        "document.transaction",
        json!({"document_id":id,"expected_revision":1,"commands":[{"op":"move_atoms","atoms":["a1"],"delta":{"x":500,"y":0}}]}),
    );
    assert_eq!(moved["outcome"], "ok", "{moved}");
    let instanced = call(
        &application,
        "instance",
        "document.transaction",
        json!({"document_id":id,"expected_revision":2,"commands":[{"op":"add_instance","offset":{"x":0,"y":100}}]}),
    );
    assert_eq!(instanced["outcome"], "ok", "{instanced}");

    let replay = create(&application, "same-request", &id, 0, "OCC", &digest);
    assert_eq!(
        replay, first,
        "the replay is the committed reply, byte for byte"
    );
    // The drawing was not touched by the replay.
    assert_eq!(snapshot(&application, &id)["document"]["revision"], 3);

    // A new request with the same parameters is a new import, and now it cannot be placed.
    let fresh = preview(&application, &id, 3, "OCC");
    assert_eq!(fresh["diagnostics"][0]["code"], "unsupported_scene");
}

#[test]
fn read_errors_report_their_position_and_class() {
    let (application, _dir, id) = setup();
    let reply = preview(&application, &id, 0, "CC=(O)");
    assert_eq!(reply["outcome"], "error");
    let problem = &reply["diagnostics"][0];
    assert_eq!(problem["code"], "dangling_bond");
    assert_eq!(problem["details"]["offset"], 2);
    assert_eq!(problem["details"]["end"], 3);
    assert_eq!(problem["details"]["class"], "syntax");

    for (source, code, class, outcome) in [
        ("C@C", "chirality_unsupported", "unsupported", "unsupported"),
        (
            "[C@H](F)(Cl)Br",
            "chirality_unsupported",
            "unsupported",
            "unsupported",
        ),
        (
            "C/C=C/C",
            "directional_bond_unsupported",
            "unsupported",
            "unsupported",
        ),
        (
            "[0H]",
            "isotope_zero_unsupported",
            "unsupported",
            "unsupported",
        ),
        (
            "c1ccc2[nH]ccc2c1",
            "aromatic_system_unsupported",
            "unsupported",
            "unsupported",
        ),
        (" CC", "leading_whitespace", "syntax", "error"),
        ("", "empty_input", "syntax", "error"),
    ] {
        let reply = preview(&application, &id, 0, source);
        assert_eq!(reply["outcome"], outcome, "{source}: {reply}");
        assert_eq!(reply["diagnostics"][0]["code"], code, "{source}");
        assert_eq!(
            reply["diagnostics"][0]["details"]["class"], class,
            "{source}"
        );
    }
    let long = "C".repeat(300);
    let limit = preview(&application, &id, 0, &long);
    assert_eq!(limit["diagnostics"][0]["code"], "command_limit");
    assert_eq!(limit["diagnostics"][0]["details"]["class"], "limit");
    assert_eq!(limit["outcome"], "error");
    assert_eq!(snapshot(&application, &id)["document"]["revision"], 0);
}

#[test]
fn text_after_the_smiles_is_reported_not_dropped_silently() {
    let (application, _dir, id) = setup();
    let reply = preview(&application, &id, 0, "CCO ethanol, the drink");
    let loss = reply["loss_report"]
        .as_array()
        .unwrap()
        .iter()
        .find(|loss| loss["feature"] == "ignored_text")
        .expect("an ignored-text loss");
    assert_eq!(loss["source_path"], "source[4..22]");
    assert_eq!(loss["disposition"], "dropped");
    assert_eq!(
        reply["result"]["document"]["atoms"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
}

#[test]
fn radical_inference_is_reported_as_a_loss() {
    let (application, _dir, id) = setup();
    let reply = preview(&application, &id, 0, "[CH3].[CH2].[Na+]");
    let radicals: Vec<_> = reply["loss_report"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|loss| loss["feature"] == "radical_state")
        .collect();
    assert_eq!(radicals.len(), 3, "{reply}");
    assert_eq!(radicals[0]["disposition"], "normalized");
    assert_eq!(radicals[1]["disposition"], "unknown");
    let atoms = reply["result"]["document"]["atoms"].as_array().unwrap();
    assert_eq!(atoms[0]["radical"], "doublet");
    assert_eq!(atoms[1]["radical"], "unknown");
    // A closed-shell bracket atom reports nothing of the kind.
    let quiet = preview(&application, &id, 0, "[NH4+]");
    assert!(!features(&quiet).contains(&"radical_state".to_owned()));
}

#[test]
fn export_writes_the_recorded_and_the_canonical_form_and_changes_nothing() {
    let (application, _dir, id) = setup();
    import(&application, "kekule", &id, 0, "C1=CC=CC=C1O");
    let before = snapshot(&application, &id);

    let recorded = export(&application, &id, "recorded");
    assert_eq!(recorded["result"]["content"], "C1=CC=CC=C1O", "{recorded}");
    assert_eq!(recorded["model_id"], "chemifree-smiles-recorded-1");
    assert_eq!(recorded["document_revision"], 1);
    assert_eq!(recorded["result"]["snapshot"], before);
    assert!(recorded["result"]["canonical"].is_null());
    assert_eq!(recorded["result"]["atoms"].as_array().unwrap().len(), 7);

    let canonical = export(&application, &id, "canonical");
    assert_eq!(canonical["result"]["content"], "Oc1ccccc1");
    assert_eq!(canonical["model_id"], "chemifree-smiles-canonical-1");
    assert_eq!(
        canonical["result"]["canonical"],
        json!({"implementation":"chemifree-smiles-canonical-1","graph_model":"chemifree-chemical-graph-2"})
    );
    assert_eq!(canonical["result"]["snapshot"], before);
    let reported = features(&canonical);
    assert!(
        reported.contains(&"aromatic_bonds".to_owned()),
        "{reported:?}"
    );
    assert!(reported.contains(&"coordinates".to_owned()));

    // Reads: nothing about the drawing or its history moved.
    assert_eq!(snapshot(&application, &id), before);
    let undone = call(
        &application,
        "undo",
        "history.undo",
        json!({"document_id":id,"expected_revision":1}),
    );
    assert_eq!(
        undone["result"]["document"]["atoms"]
            .as_array()
            .unwrap()
            .len(),
        0,
        "{undone}"
    );
}

#[test]
fn canonical_output_does_not_depend_on_how_the_structure_was_written() {
    let spellings = ["OCC", "CCO", "C(O)C", "[H]OC([H])([H])C([H])([H])[H]"];
    let mut outputs = Vec::new();
    for (index, source) in spellings.iter().enumerate() {
        let (application, _dir, id) = setup();
        let created = import(&application, &format!("import-{index}"), &id, 0, source);
        assert_eq!(created["outcome"], "partial", "{source}: {created}");
        outputs.push(export(&application, &id, "canonical"));
    }
    for reply in &outputs {
        assert_eq!(reply["result"]["content"], outputs[0]["result"]["content"]);
    }
    // The explicit hydrogens are folded, listed apart, and not part of the atom order.
    let folded = &outputs[3]["result"];
    assert_eq!(folded["atoms"].as_array().unwrap().len(), 3);
    assert_eq!(folded["folded_hydrogens"].as_array().unwrap().len(), 6);
    assert!(features(&outputs[3]).contains(&"explicit_hydrogen_atoms".to_owned()));
    assert!(!features(&outputs[0]).contains(&"explicit_hydrogen_atoms".to_owned()));
}

#[test]
fn export_losses_use_the_shared_vocabulary_and_say_only_what_is_true() {
    let (application, _dir, id) = setup();
    // An empty, untitled-style drawing has nothing to lose but its settings.
    let empty = call(&application, "empty", "document.create", json!({}));
    let empty_id = empty["result"]["document"]["id"].as_str().unwrap();
    let nothing = export(&application, empty_id, "recorded");
    assert_eq!(
        nothing["outcome"], "unsupported",
        "no atoms, no SMILES: {nothing}"
    );
    assert_eq!(nothing["diagnostics"][0]["code"], "empty_graph");

    import(&application, "chain", &id, 0, "CC");
    let titled = export(&application, &id, "recorded");
    let list = features(&titled);
    for expected in [
        "coordinates",
        "title",
        "native_ids",
        "placement_id",
        "drawing_settings",
    ] {
        assert!(
            list.contains(&expected.to_owned()),
            "{expected} in {list:?}"
        );
    }
    for absent in [
        "extra_placement",
        "selection",
        "scene_object",
        "scene_references",
        "hydrogen_state",
    ] {
        assert!(
            !list.contains(&absent.to_owned()),
            "{absent} was reported but nothing exists: {list:?}"
        );
    }
    let wording = titled["loss_report"].to_string();
    assert!(
        !wording.contains("first placement") && !wording.contains("V2000"),
        "{wording}"
    );

    // A second placement, a graphic and a selection each appear once they exist.
    call(
        &application,
        "more",
        "document.transaction",
        json!({"document_id":id,"expected_revision":1,"commands":[{"op":"add_instance","offset":{"x":0,"y":100}}],"selection_after":{"atoms":["a1"],"bonds":[],"objects":[]}}),
    );
    let richer = export(&application, &id, "recorded");
    let list = features(&richer);
    assert!(list.contains(&"extra_placement".to_owned()), "{list:?}");
    assert!(list.contains(&"selection".to_owned()), "{list:?}");
    let wording = richer["loss_report"].to_string();
    assert!(!wording.contains("first placement"), "{wording}");

    // The default title is not a real title.
    let untitled = call(&application, "u", "document.create", json!({}));
    let untitled_id = untitled["result"]["document"]["id"].as_str().unwrap();
    import(&application, "chain2", untitled_id, 0, "CC");
    assert!(
        !features(&export(&application, untitled_id, "recorded")).contains(&"title".to_owned())
    );
}

#[test]
fn attachment_points_need_consent_bound_to_the_losses_and_the_content() {
    let (application, _dir, id) = setup();
    let edit = call(
        &application,
        "atom",
        "document.transaction",
        json!({"document_id":id,"expected_revision":0,"commands":[{"op":"add_atom","element":"C","position":{"x":0,"y":0},"attachment_points":[1]}]}),
    );
    assert_eq!(edit["outcome"], "ok", "{edit}");
    // Unknown hydrogens or attachment atoms may not write; this one does and drops the point.
    let first = export(&application, &id, "recorded");
    assert_eq!(first["outcome"], "error", "{first}");
    let problem = &first["diagnostics"][0];
    assert_eq!(problem["code"], "loss_consent_required");
    assert_eq!(problem["details"]["features"], json!(["attachment_points"]));
    let digest = problem["details"]["loss_digest"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        problem["details"]["losses"][0]["feature"],
        "attachment_points"
    );

    let accepted = call(
        &application,
        "accepted",
        "document.export",
        json!({"format":"smiles","document_id":id,"form":"recorded","accepted_losses":digest}),
    );
    assert_eq!(accepted["result"]["content"], "C", "{accepted}");

    let stale = call(
        &application,
        "stale",
        "document.export",
        json!({"format":"smiles","document_id":id,"form":"recorded","accepted_losses":"0".repeat(64)}),
    );
    assert_eq!(stale["diagnostics"][0]["code"], "loss_consent_changed");

    // A different drawing gives a different digest: an earlier consent does not carry over.
    call(
        &application,
        "more",
        "document.transaction",
        json!({"document_id":id,"expected_revision":1,"commands":[{"op":"add_atom","element":"O","position":{"x":30,"y":0},"bond_to":{"atom":"a1","order":"single"}}]}),
    );
    let changed = call(
        &application,
        "changed",
        "document.export",
        json!({"format":"smiles","document_id":id,"form":"recorded","accepted_losses":digest}),
    );
    assert_eq!(
        changed["diagnostics"][0]["code"], "loss_consent_changed",
        "{changed}"
    );
}

#[test]
fn a_drawn_ion_is_written_with_its_assumptions_named_and_needing_consent() {
    let (application, _dir, id) = setup();
    let edit = call(
        &application,
        "q",
        "document.transaction",
        json!({"document_id":id,"expected_revision":0,"commands":[{"op":"add_atom","element":"Na","position":{"x":0,"y":0},"hydrogens":{"mode":"inferred"}}]}),
    );
    assert_eq!(edit["outcome"], "ok", "{edit}");
    call(
        &application,
        "charge",
        "document.transaction",
        json!({"document_id":id,"expected_revision":1,"commands":[{"op":"update_atom","atom":"a1","charge":1}]}),
    );
    let first = export(&application, &id, "recorded");
    assert_eq!(
        first["outcome"], "error",
        "consent is asked for, not a refusal: {first}"
    );
    assert_eq!(first["document_revision"], 2);
    assert_eq!(first["model_id"], "chemifree-smiles-recorded-1");
    let problem = &first["diagnostics"][0];
    assert_eq!(problem["code"], "loss_consent_required");
    assert_eq!(
        problem["details"]["features"],
        json!(["radical_state", "hydrogen_assumption"])
    );
    assert_eq!(
        problem["details"]["losses"][0]["source_path"],
        "document.atoms[a1].radical"
    );
    assert_eq!(
        problem["details"]["losses"][1]["source_path"],
        "document.atoms[a1].hydrogens"
    );
    let digest = problem["details"]["loss_digest"].as_str().unwrap();
    let accepted = call(
        &application,
        "accepted",
        "document.export",
        json!({"format":"smiles","document_id":id,"form":"recorded","accepted_losses":digest}),
    );
    assert_eq!(accepted["result"]["content"], "[Na+]", "{accepted}");
    assert!(features(&accepted).contains(&"hydrogen_assumption".to_owned()));
    // The canonical form still has no answer, and says why.
    let canonical = export(&application, &id, "canonical");
    assert_eq!(canonical["outcome"], "unsupported");
    assert_eq!(canonical["diagnostics"][0]["code"], "canonical_unsupported");
    assert_eq!(
        canonical["diagnostics"][0]["details"]["reason"],
        "element_or_charge"
    );
}

#[test]
fn bracket_atoms_without_a_consistent_radical_are_unknown_and_reported() {
    let (application, _dir, id) = setup();
    let reply = preview(&application, &id, 0, "[C].[CH].[N].[CH2]");
    let atoms = reply["result"]["document"]["atoms"].as_array().unwrap();
    assert!(
        atoms.iter().all(|atom| atom["radical"] == "unknown"),
        "{atoms:?}"
    );
    let radicals = features(&reply)
        .iter()
        .filter(|f| *f == "radical_state")
        .count();
    assert_eq!(radicals, 4, "every inference is reported: {reply}");
}

#[test]
fn smiles_is_not_available_unless_it_is_connected() {
    let dir = tempfile::tempdir().unwrap();
    let bare = Application::new(LocalCapabilities, FileDocumentRepository::new(dir.path()));
    let created = call(&bare, "create", "document.create", json!({"title":"T"}));
    let id = created["result"]["document"]["id"].as_str().unwrap();
    let reply = call(
        &bare,
        "p",
        "document.import",
        json!({"document_id":id,"expected_revision":0,"format":"smiles","source":"C","preview":true}),
    );
    assert!(reply["result"].is_null());
    let reply = call(
        &bare,
        "e",
        "document.export",
        json!({"format":"smiles","document_id":id,"form":"recorded"}),
    );
    assert!(reply["result"].is_null());

    // With only SMILES connected, other formats are refused as formats, not as a missing feature.
    let (application, _dir, id) = setup();
    for format in ["mol", "cdxml", "inchi"] {
        let reply = call(
            &application,
            format,
            "document.import",
            json!({"document_id":id,"expected_revision":0,"format":format,"source":"x"}),
        );
        assert_eq!(
            reply["diagnostics"][0]["code"], "unsupported_format",
            "{format}: {reply}"
        );
    }
}

fn capability_text(ctfile: bool, rich: bool, smiles: bool) -> (Value, Value, Value) {
    let dir = tempfile::tempdir().unwrap();
    let mut application =
        Application::new(LocalCapabilities, FileDocumentRepository::new(dir.path()));
    if ctfile {
        application = application.with_ctfile_codec(V2000Codec);
    }
    if rich {
        application = application.with_interchange_codec(LocalInterchangeCodec);
    }
    if smiles {
        application = application.with_smiles();
    }
    let caps = serde_json::to_value(application.capabilities()).unwrap();
    let conversion = caps["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == "format_conversion")
        .unwrap()
        .clone();
    let operation = |id: &str| {
        caps["operation_catalog"]["operations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == id)
            .unwrap()
            .clone()
    };
    (
        conversion,
        operation("document.import"),
        operation("document.export"),
    )
}

#[test]
fn capability_and_catalog_text_is_consistent_for_every_combination_of_codecs() {
    for bits in 0..8 {
        let (ctfile, rich, smiles) = (bits & 1 != 0, bits & 2 != 0, bits & 4 != 0);
        let (conversion, import, export) = capability_text(ctfile, rich, smiles);
        let any = ctfile || rich || smiles;
        assert_eq!(
            conversion["status"],
            if any { "available" } else { "not_implemented" }
        );
        assert_eq!(
            import["execution"],
            if any { "executable" } else { "not_implemented" }
        );
        assert_eq!(
            export["execution"],
            if any { "executable" } else { "not_implemented" }
        );
        if !any {
            continue;
        }
        let description = conversion["description"].as_str().unwrap();
        let label = conversion["label"].as_str().unwrap();
        for (name, present) in [("MOL/SDF", ctfile), ("CDXML", rich), ("SMILES", smiles)] {
            assert_eq!(
                description.contains(name),
                present,
                "{name} in {description}"
            );
            assert_eq!(label.contains(name), present, "{name} in {label}");
        }
        assert!(
            label.ends_with(if ctfile as u8 + rich as u8 * 2 + smiles as u8 == 1 {
                "subset"
            } else {
                "subsets"
            }),
            "{label}"
        );
        // The unsupported claim is the last sentence, and the SMILES limits are the domain's.
        assert!(
            description.ends_with("other formats are unsupported."),
            "{description}"
        );
        if smiles {
            assert!(
                description.contains("4096 bytes") && description.contains("500 atoms and bonds"),
                "{description}"
            );
            assert!(import["note"].as_str().unwrap().contains("SMILES"));
            assert!(
                export["note"]
                    .as_str()
                    .unwrap()
                    .contains("canonical SMILES")
            );
        }
        if rich {
            assert!(description.contains("1 MiB"));
        }
    }
}
