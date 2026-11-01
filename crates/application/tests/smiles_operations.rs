//! SMILES operations through the shared application: import consent, writing, canonical form.
use chemifree_application::Application;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities, SmilesTextCodec};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicUsize, Ordering};
use tempfile::TempDir;

type App = Application<LocalCapabilities, FileDocumentRepository>;

fn setup() -> (App, TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let app = Application::new(LocalCapabilities, FileDocumentRepository::new(dir.path()))
        .with_smiles_codec(SmilesTextCodec);
    (app, dir)
}

fn call(app: &App, operation: &str, parameters: Value) -> Value {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let id = format!("r{}", NEXT.fetch_add(1, Ordering::Relaxed));
    let request = json!({"contract":1,"request_id":id,"profile":"preserve-semantics","operation":operation,"parameters":parameters});
    serde_json::to_value(app.execute(request.to_string().as_bytes())).unwrap()
}

fn call_id(app: &App, id: &str, operation: &str, parameters: Value) -> Value {
    let request = json!({"contract":1,"request_id":id,"profile":"preserve-semantics","operation":operation,"parameters":parameters});
    serde_json::to_value(app.execute(request.to_string().as_bytes())).unwrap()
}

fn new_document(app: &App) -> String {
    call(app, "document.create", json!({"title":"T"}))["result"]["document"]["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// Import a SMILES into a fresh document (preview, then consent) and return its id.
fn import(app: &App, source: &str) -> String {
    let id = new_document(app);
    let preview = call(
        app,
        "document.import",
        json!({"document_id":id,"expected_revision":0,"format":"smiles","source":source,"preview":true}),
    );
    assert_eq!(preview["outcome"], "partial", "{source}: {preview}");
    let digest = preview["result"]["preview_digest"].clone();
    let done = call_id(
        app,
        "commit",
        "document.import",
        json!({"document_id":id,"expected_revision":0,"format":"smiles","source":source,"preview_digest":digest,"accept_loss":true}),
    );
    assert!(done["outcome"] == "partial", "{source}: {done}");
    id
}

fn write(app: &App, id: &str, mode: &str) -> Value {
    call(
        app,
        "document.export",
        json!({"document_id":id,"format":"smiles","mode":mode}),
    )
}

fn text(app: &App, id: &str, mode: &str) -> String {
    let reply = write(app, id, mode);
    assert!(
        reply["outcome"] == "ok" || reply["outcome"] == "partial",
        "{mode}: {reply}"
    );
    reply["result"]["smiles"].as_str().unwrap().to_owned()
}

fn canonical_of(app: &App, source: &str) -> Result<String, String> {
    let id = import(app, source);
    let reply = write(app, &id, "canonical");
    match reply["outcome"].as_str().unwrap() {
        "ok" | "partial" => Ok(reply["result"]["smiles"].as_str().unwrap().to_owned()),
        _ => Err(reply["diagnostics"][0]["details"]["reason"]
            .as_str()
            .unwrap_or("?")
            .to_owned()),
    }
}

#[test]
fn capabilities_name_smiles_and_its_limits() {
    let (app, _dir) = setup();
    let catalog = call(&app, "capabilities.inspect", json!({}));
    let operations = catalog["result"]["operations"].as_array().unwrap();
    for name in ["document.import", "document.export"] {
        let entry = operations.iter().find(|o| o["id"] == name).unwrap();
        assert_eq!(entry["execution"], "executable", "{name}");
        assert!(entry["note"].as_str().unwrap().contains("SMILES"), "{name}");
    }
}

#[test]
fn preview_then_consent_appends_the_molecule_and_reports_losses() {
    let (app, _dir) = setup();
    let id = new_document(&app);
    let preview = call(
        &app,
        "document.import",
        json!({"document_id":id,"expected_revision":0,"format":"smiles","source":"CC(=O)[O-].[Na+]  acetate salt","preview":true}),
    );
    assert_eq!(preview["outcome"], "partial");
    assert_eq!(preview["model_id"], "chemifree-smiles-1");
    assert_eq!(preview["result"]["atoms"], 5);
    assert_eq!(preview["result"]["bonds"], 3);
    assert_eq!(preview["result"]["snapshot"]["document"]["revision"], 0);
    let features: Vec<_> = preview["loss_report"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["feature"].as_str().unwrap().to_owned())
        .collect();
    assert!(
        features.contains(&"ignored_text".to_owned()),
        "{features:?}"
    );
    assert!(features.contains(&"generated_coordinates".to_owned()));
    let ignored = preview["loss_report"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["feature"] == "ignored_text")
        .unwrap();
    assert_eq!(ignored["source_path"], "source[18..30]");
    // Previewing changed nothing.
    let snapshot = call(&app, "document.snapshot", json!({"document_id":id}));
    assert_eq!(
        snapshot["result"]["document"]["atoms"]
            .as_array()
            .unwrap()
            .len(),
        0
    );

    let digest = preview["result"]["preview_digest"].clone();
    let refused = call(
        &app,
        "document.import",
        json!({"document_id":id,"expected_revision":0,"format":"smiles","source":"CC(=O)[O-].[Na+]  acetate salt","preview_digest":digest}),
    );
    assert_eq!(refused["outcome"], "error");
    assert_eq!(refused["diagnostics"][0]["code"], "loss_consent_required");
    let done = call(
        &app,
        "document.import",
        json!({"document_id":id,"expected_revision":0,"format":"smiles","source":"CC(=O)[O-].[Na+]  acetate salt","preview_digest":digest,"accept_loss":true}),
    );
    assert_eq!(done["outcome"], "partial", "{done}");
    assert_eq!(done["document_revision"], 1);
    assert_eq!(
        done["result"]["import_mapping"]["atoms"]
            .as_array()
            .unwrap()
            .len(),
        5
    );
    let atoms = done["result"]["document"]["atoms"].as_array().unwrap();
    assert_eq!(atoms[3]["charge"], -1);
    assert_eq!(atoms[3]["hydrogens"]["mode"], "specified");
}

#[test]
fn a_stale_preview_digest_is_refused() {
    let (app, _dir) = setup();
    let id = new_document(&app);
    let preview = call(
        &app,
        "document.import",
        json!({"document_id":id,"expected_revision":0,"format":"smiles","source":"CCO","preview":true}),
    );
    let digest = preview["result"]["preview_digest"].clone();
    {
        let source = "CCN";
        let reply = call(
            &app,
            "document.import",
            json!({"document_id":id,"expected_revision":0,"format":"smiles","source":source,"preview_digest":digest,"accept_loss":true}),
        );
        assert_eq!(
            reply["diagnostics"][0]["code"], "preview_changed",
            "{source}"
        );
    }
    let snapshot = call(&app, "document.snapshot", json!({"document_id":id}));
    assert_eq!(snapshot["result"]["document"]["revision"], 0);
}

#[test]
fn syntax_errors_carry_code_and_position_and_unsupported_text_is_unsupported() {
    let (app, _dir) = setup();
    let id = new_document(&app);
    let go = |source: &str| {
        call(
            &app,
            "document.import",
            json!({"document_id":id,"expected_revision":0,"format":"smiles","source":source,"preview":true}),
        )
    };
    let bad = go("CC(C");
    assert_eq!(bad["outcome"], "error");
    assert_eq!(bad["diagnostics"][0]["code"], "invalid_smiles");
    assert_eq!(
        bad["diagnostics"][0]["details"]["reason"],
        "unclosed_branch"
    );
    assert_eq!(bad["diagnostics"][0]["details"]["start"], 2);
    assert_eq!(bad["diagnostics"][0]["details"]["end"], 3);
    let stereo = go("F/C=C/F");
    assert_eq!(stereo["outcome"], "unsupported");
    assert_eq!(
        stereo["diagnostics"][0]["details"]["reason"],
        "stereo_unsupported"
    );
    let long = go(&"C".repeat(20_000));
    assert_eq!(long["diagnostics"][0]["code"], "smiles_limit");
    assert_eq!(
        go(" CC")["diagnostics"][0]["details"]["reason"],
        "leading_whitespace"
    );
}

#[test]
fn writing_does_not_change_the_document_or_its_history() {
    let (app, _dir) = setup();
    let id = import(&app, "OC(=O)C");
    let before = call(&app, "document.snapshot", json!({"document_id":id}));
    let recorded = write(&app, &id, "recorded");
    let canonical = write(&app, &id, "canonical");
    let after = call(&app, "document.snapshot", json!({"document_id":id}));
    assert_eq!(
        before["result"], after["result"],
        "session and history are untouched"
    );
    assert_eq!(recorded["model_id"], "chemifree-smiles-1");
    assert_eq!(canonical["model_id"], "chemifree-smiles-canonical-1");
    assert_eq!(recorded["result"]["snapshot"], before["result"]);
    assert_eq!(canonical["result"]["snapshot"], before["result"]);
    assert_eq!(recorded["document_revision"], 1);
    assert_eq!(
        canonical["result"]["implementation"],
        "chemifree-smiles-canonical"
    );
    assert_eq!(canonical["result"]["version"], "1");
    assert_eq!(
        canonical["result"]["models"]["identity"],
        "chemifree-chemical-graph-1"
    );
    assert_eq!(recorded["result"]["smiles"], "OC(=O)C");
}

#[test]
fn recorded_output_states_hydrogens_for_bracket_atoms() {
    let (app, _dir) = setup();
    // Bracket-requiring atoms drawn with inferred hydrogens get the model's count.
    let id = new_document(&app);
    let edit = call(
        &app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":0,"commands":[
            {"op":"add_atom","element":"N","position":{"x":0,"y":0}},
            {"op":"add_atom","element":"C","position":{"x":20,"y":0},"bond_to":{"atom":"$0","order":"single"}},
        ]}),
    );
    assert_eq!(edit["outcome"], "ok", "{edit}");
    let update = call(
        &app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":1,"commands":[
            {"op":"update_atom","atom":"a1","charge":1},
            {"op":"update_atom","atom":"a2","isotope":13},
        ]}),
    );
    assert_eq!(update["outcome"], "ok", "{update}");
    assert_eq!(text(&app, &id, "recorded"), "[NH3+][13CH3]");
}

#[test]
fn canonical_output_agrees_across_spellings_and_orders() {
    let (app, _dir) = setup();
    let groups: &[&[&str]] = &[
        &["OCC", "CCO", "C(O)C", "C(C)O"],
        &["c1ccccc1", "C1=CC=CC=C1", "C1=CC=CC=C1", "c1ccc(cc1)"],
        &["[nH]1cccc1", "N1C=CC=C1", "c1cc[nH]c1", "C1=CNC=C1"],
        &["o1cccc1", "O1C=CC=C1", "c1ccoc1"],
        &["s1cccc1", "S1C=CC=C1", "c1ccsc1"],
        &["n1ccccc1", "N1=CC=CC=C1", "C1=CC=NC=C1"],
        &[
            "[nH]1cncc1",
            "N1C=NC=C1",
            "c1c[nH]cn1",
            "C1=CN=CN1",
            "N1C=CN=C1",
        ],
        &[
            "c1ccc2ccccc2c1",
            "C1=CC=C2C=CC=CC2=C1",
            "C1=CC2=CC=CC=C2C=C1",
        ],
        &[
            "CC(=O)[O-].[NH4+]",
            "[NH4+].[O-]C(C)=O",
            "O=C([O-])C.[NH4+]",
        ],
        &["[13CH3]C", "C[13CH3]", "[13CH3][CH3]"],
        &["C1CCCCC1", "[CH2]1[CH2][CH2][CH2][CH2][CH2]1"],
    ];
    for group in groups {
        let results: Vec<_> = group.iter().map(|s| canonical_of(&app, s)).collect();
        assert!(
            results.windows(2).all(|w| w[0] == w[1]),
            "{group:?} gave {results:?}"
        );
        assert!(results[0].is_ok(), "{group:?} gave {results:?}");
    }
}

fn reason(reply: &Value) -> &str {
    reply["diagnostics"][0]["details"]["reason"]
        .as_str()
        .unwrap_or("?")
}

#[test]
fn radical_states_come_from_the_shared_valence_model() {
    let (app, _dir) = setup();
    // A methyl radical has one fewer bond order than the closed shell: a doublet.
    let id = import(&app, "[CH3]");
    let atoms =
        call(&app, "document.snapshot", json!({"document_id":id}))["result"]["document"]["atoms"]
            .clone();
    assert_eq!(atoms[0]["radical"], "doublet");
    assert_eq!(text(&app, &id, "canonical"), "[CH3]");
    // Two fewer bond orders cannot be told apart as singlet or triplet: undetermined, and said so.
    let id = new_document(&app);
    let preview = call(
        &app,
        "document.import",
        json!({"document_id":id,"expected_revision":0,"format":"smiles","source":"[CH2]","preview":true}),
    );
    let loss = preview["loss_report"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["feature"] == "radical_state")
        .unwrap();
    assert!(
        loss["reason"]
            .as_str()
            .unwrap()
            .contains("singlet from a triplet"),
        "{loss}"
    );
    assert!(
        !loss["reason"].as_str().unwrap().contains("incomplete"),
        "{loss}"
    );
    let id = import(&app, "[CH2]");
    let atoms =
        call(&app, "document.snapshot", json!({"document_id":id}))["result"]["document"]["atoms"]
            .clone();
    assert_eq!(atoms[0]["radical"], "unknown");
    assert_eq!(reason(&write(&app, &id, "canonical")), "radical_state");
    // An element outside the valence table is undetermined, with outside-the-model wording.
    let id = new_document(&app);
    let preview = call(
        &app,
        "document.import",
        json!({"document_id":id,"expected_revision":0,"format":"smiles","source":"[Na+]","preview":true}),
    );
    let loss = preview["loss_report"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["feature"] == "radical_state")
        .unwrap();
    assert!(
        loss["reason"]
            .as_str()
            .unwrap()
            .contains("outside the valence model"),
        "{loss}"
    );
    assert_eq!(text(&app, &import(&app, "[Na+]"), "recorded"), "[Na+]");
}

#[test]
fn refusals_name_the_real_cause() {
    let (app, _dir) = setup();
    // Aromaticity is checked before atoms are formatted: indane drawn aromatic names its rings.
    let id = import(&app, "c1ccc2c(c1)CCC2");
    assert_eq!(reason(&write(&app, &id, "recorded")), "mixed_ring_bonds");
    assert_eq!(reason(&write(&app, &id, "canonical")), "mixed_ring_bonds");
    // A charged atom with inferred hydrogens in an unusual valence has no count to state.
    let id = new_document(&app);
    let drawn = call(
        &app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":0,"commands":[
            {"op":"add_atom","element":"N","position":{"x":0,"y":0}},
            {"op":"add_atom","element":"C","position":{"x":20,"y":0},"bond_to":{"atom":"$0","order":"single"}},
            {"op":"add_atom","element":"C","position":{"x":0,"y":20},"bond_to":{"atom":"$0","order":"single"}},
            {"op":"add_atom","element":"C","position":{"x":-20,"y":0},"bond_to":{"atom":"$0","order":"single"}},
            {"op":"add_atom","element":"C","position":{"x":0,"y":-20},"bond_to":{"atom":"$0","order":"single"}},
            {"op":"add_atom","element":"C","position":{"x":20,"y":20},"bond_to":{"atom":"$0","order":"single"}},
        ]}),
    );
    assert_eq!(drawn["outcome"], "ok", "{drawn}");
    let charged = call(
        &app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":1,"commands":[{"op":"update_atom","atom":"a1","charge":1}]}),
    );
    assert_eq!(charged["outcome"], "ok", "{charged}");
    let refused = write(&app, &id, "recorded");
    assert_eq!(refused["outcome"], "unsupported");
    assert_eq!(
        refused["diagnostics"][0]["code"],
        "smiles_write_unsupported"
    );
    assert_eq!(reason(&refused), "hydrogens_undetermined");
    assert_eq!(refused["diagnostics"][0]["details"]["atom_id"], "a1");
    assert_eq!(
        refused["diagnostics"][0]["details"]["snapshot"]["document"]["revision"],
        2
    );
}

#[test]
fn output_the_reader_would_refuse_is_a_limit_error_not_an_invalid_graph() {
    let (app, _dir) = setup();
    let id = new_document(&app);
    for (revision, chunk) in (0..3).enumerate() {
        let mut commands = Vec::new();
        for i in 0..100 {
            let mut command = json!({"op":"add_atom","element":"C","position":{"x":(i * 20) as f64,"y":(chunk * 20) as f64}});
            if i > 0 {
                command["bond_to"] = json!({"atom": format!("${}", i - 1), "order": "single"});
            }
            commands.push(command);
        }
        let reply = call(
            &app,
            "document.transaction",
            json!({"document_id":id,"expected_revision":revision,"commands":commands}),
        );
        assert_eq!(reply["outcome"], "ok", "{reply}");
    }
    // 300 atoms and 297 bonds are over the reader's 500 items, and over the identity limit of 128.
    for (mode, expected) in [("recorded", "size_limit"), ("canonical", "size_limit")] {
        let reply = write(&app, &id, mode);
        assert_eq!(reply["outcome"], "error", "{mode}");
        assert_eq!(reply["diagnostics"][0]["code"], "smiles_limit", "{mode}");
        assert_eq!(reason(&reply), expected, "{mode}");
    }
    // Within the reader's limit but over the canonical search's 128 atoms.
    let id = new_document(&app);
    let mut commands = Vec::new();
    for i in 0..130 {
        let mut command =
            json!({"op":"add_atom","element":"C","position":{"x":(i * 20) as f64,"y":0}});
        if i > 0 {
            command["bond_to"] = json!({"atom": format!("${}", i - 1), "order": "single"});
        }
        commands.push(command);
    }
    let reply = call(
        &app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":0,"commands":commands}),
    );
    assert_eq!(reply["outcome"], "ok");
    assert_eq!(write(&app, &id, "recorded")["outcome"], "ok");
    let reply = write(&app, &id, "canonical");
    assert_eq!(reply["diagnostics"][0]["code"], "smiles_limit");
    assert_eq!(reason(&reply), "atom_limit");
}
