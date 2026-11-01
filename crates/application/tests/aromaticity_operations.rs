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
    assert_eq!(view["model_id"], "chemifree-aromaticity-2");
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
    assert_eq!(failed["model_id"], "chemifree-aromaticity-2");
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

type App = Application<LocalCapabilities, FileDocumentRepository>;

fn new_app(directory: &tempfile::TempDir) -> App {
    Application::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    )
}

/// A ring written Kekulé: `atoms` in ring order, double bonds on every pair listed by
/// `doubles` (indexes of the bond from atom i to atom i+1).
struct Ring {
    atoms: Vec<&'static str>,
    doubles: Vec<usize>,
    hydrogens: Vec<(usize, u8)>,
    aromatic: bool,
}

fn pyrrole() -> Ring {
    Ring {
        atoms: vec!["N", "C", "C", "C", "C"],
        doubles: vec![1, 3],
        hydrogens: Vec::new(),
        aromatic: false,
    }
}

fn furan() -> Ring {
    Ring {
        atoms: vec!["O", "C", "C", "C", "C"],
        doubles: vec![1, 3],
        hydrogens: Vec::new(),
        aromatic: false,
    }
}

fn benzene() -> Ring {
    Ring {
        atoms: vec!["C"; 6],
        doubles: vec![0, 2, 4],
        hydrogens: Vec::new(),
        aromatic: false,
    }
}

fn ring_atoms_and_bonds(ring: &Ring, first: usize) -> Vec<Value> {
    let n = ring.atoms.len();
    let mut commands: Vec<Value> = ring
        .atoms
        .iter()
        .enumerate()
        .map(|(i, symbol)| {
            let mut command =
                json!({"op":"add_atom","element":symbol,"position":{"x":(first + i) * 20,"y":0}});
            if let Some((_, count)) = ring.hydrogens.iter().find(|(index, _)| *index == i) {
                command["hydrogens"] = json!({"mode":"specified","count":count});
            }
            command
        })
        .collect();
    for i in 0..n {
        let order = if ring.aromatic {
            "aromatic"
        } else if ring.doubles.contains(&i) {
            "double"
        } else {
            "single"
        };
        commands.push(json!({"op":"add_bond","begin":format!("${}", i),"end":format!("${}", (i + 1) % n),"order":order}));
    }
    commands
}

/// Draw rings as separate molecules; each transaction stays within the 500-command bound.
fn draw_rings(app: &App, title: &str, rings: &[Ring]) -> String {
    let id = app.create_document(None, Some(title)).unwrap().document.id;
    for (batch_index, batch) in rings.chunks(40).enumerate() {
        let mut commands: Vec<Value> = Vec::new();
        for ring in batch {
            // Each ring's `$i` references restart in its own transaction, so a batch draws
            // exactly one ring when it holds several: use references relative to atoms made
            // so far by offsetting them.
            let offset = commands.iter().filter(|c| c["op"] == "add_atom").count();
            let part = ring_atoms_and_bonds(ring, offset);
            for mut command in part {
                if command["op"] == "add_bond" {
                    for key in ["begin", "end"] {
                        let local: usize = command[key].as_str().unwrap()[1..].parse().unwrap();
                        command[key] = json!(format!("${}", local + offset));
                    }
                }
                commands.push(command);
            }
        }
        // Atoms must precede the bonds that reference them in a single transaction.
        commands.sort_by_key(|c| c["op"] == "add_bond");
        let result = call(
            app,
            &format!("draw-{batch_index}"),
            "document.transaction",
            json!({"document_id":id,"expected_revision":batch_index,"commands":commands}),
        );
        assert_eq!(result["outcome"], "ok", "{result}");
    }
    id
}

fn convert(app: &App, id: &str, request: &str, revision: u64, to: &str) -> Value {
    call(
        app,
        request,
        "chem.aromaticity_convert",
        json!({"document_id":id,"expected_revision":revision,"to":to}),
    )
}

fn chemical_code(app: &App, id: &str, request: &str) -> Value {
    let identity = call(
        app,
        request,
        "chem.identity",
        json!({"document_id":id,"mode":"chemical"}),
    );
    assert_eq!(identity["outcome"], "ok", "{identity}");
    identity["result"]["identity"]["code"].clone()
}

#[test]
fn kekule_pyrrole_converts_with_a_stated_hydrogen_and_round_trips_exactly() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let id = draw_rings(&app, "Pyrrole", &[pyrrole()]);
    let drawn = serde_json::to_value(app.document(&id).unwrap()).unwrap();
    assert_eq!(
        drawn["document"]["atoms"][0]["hydrogens"],
        json!({"mode":"inferred"})
    );
    let inspected = call(
        &app,
        "inspect",
        "chem.aromaticity",
        json!({"document_id":id}),
    );
    assert_eq!(
        inspected["result"]["systems"][0]["classification"],
        "aromatic"
    );
    let code = chemical_code(&app, &id, "identity-before");

    let aromatic = convert(&app, &id, "to-aromatic", 1, "aromatic");
    assert_eq!(aromatic["outcome"], "ok", "{aromatic}");
    assert_eq!(
        aromatic["result"]["document"]["atoms"][0]["hydrogens"],
        json!({"mode":"specified","count":1})
    );
    assert_eq!(
        aromatic["result"]["session"]["undo_depth"], 2,
        "one undoable edit"
    );
    let reinspected = call(
        &app,
        "inspect-2",
        "chem.aromaticity",
        json!({"document_id":id}),
    );
    assert_eq!(
        reinspected["result"]["systems"][0]["classification"],
        "aromatic"
    );
    assert_eq!(chemical_code(&app, &id, "identity-aromatic"), code);

    let back = convert(&app, &id, "to-kekule", 2, "kekule");
    assert_eq!(back["outcome"], "ok", "{back}");
    assert_eq!(
        back["result"]["document"]["atoms"],
        drawn["document"]["atoms"]
    );
    assert_eq!(
        back["result"]["document"]["bonds"],
        drawn["document"]["bonds"]
    );
    assert_eq!(chemical_code(&app, &id, "identity-after"), code);

    let undone = call(
        &app,
        "undo",
        "history.undo",
        json!({"document_id":id,"expected_revision":3}),
    );
    assert_eq!(
        undone["result"]["document"]["atoms"][0]["hydrogens"],
        json!({"mode":"specified","count":1}),
        "undo restores the nitrogen statement with the bonds"
    );
}

#[test]
fn a_user_stated_aromatic_nitrogen_hydrogen_returns_to_inferred_in_kekule_form() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let mut ring = pyrrole();
    ring.aromatic = true;
    ring.hydrogens = vec![(0, 1)];
    let id = draw_rings(&app, "n-H pyrrole", &[ring]);
    let code = chemical_code(&app, &id, "identity-before");
    let kekule = convert(&app, &id, "to-kekule", 1, "kekule");
    assert_eq!(kekule["outcome"], "ok", "{kekule}");
    assert_eq!(
        kekule["result"]["document"]["atoms"][0]["hydrogens"],
        json!({"mode":"inferred"})
    );
    assert_eq!(chemical_code(&app, &id, "identity-after"), code);
}

#[test]
fn pyridine_style_nitrogen_stays_untouched_by_a_round_trip() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let ring = Ring {
        atoms: vec!["N", "C", "C", "C", "C", "C"],
        doubles: vec![0, 2, 4],
        hydrogens: Vec::new(),
        aromatic: false,
    };
    let id = draw_rings(&app, "Pyridine", &[ring]);
    let drawn = serde_json::to_value(app.document(&id).unwrap()).unwrap();
    let aromatic = convert(&app, &id, "to-aromatic", 1, "aromatic");
    assert_eq!(
        aromatic["result"]["document"]["atoms"], drawn["document"]["atoms"],
        "no hydrogen statement is added to a pyridine-type nitrogen"
    );
    let back = convert(&app, &id, "to-kekule", 2, "kekule");
    assert_eq!(
        back["result"]["document"]["atoms"],
        drawn["document"]["atoms"]
    );
}

#[test]
fn conversion_commands_count_changed_bonds_plus_nitrogen_hydrogen_updates() {
    // 80 pyrroles change 5 bonds and 1 atom each; 4 furans change 5 bonds each: exactly 500.
    let mut rings: Vec<Ring> = (0..80).map(|_| pyrrole()).collect();
    rings.extend((0..4).map(|_| furan()));
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let id = draw_rings(&app, "At bound", &rings);
    let revision = 3;
    let before = serde_json::to_value(app.document(&id).unwrap()).unwrap();
    let aromatic = convert(&app, &id, "to-aromatic", revision, "aromatic");
    assert_eq!(aromatic["outcome"], "ok", "500 commands fit: {aromatic}");
    let back = convert(&app, &id, "to-kekule", revision + 1, "kekule");
    assert_eq!(back["outcome"], "ok", "500 commands fit: {back}");
    assert_eq!(
        back["result"]["document"]["atoms"],
        before["document"]["atoms"]
    );
    assert_eq!(
        back["result"]["document"]["bonds"],
        before["document"]["bonds"]
    );

    // One more furan makes 505 commands: refused as a whole, even though only 405 bonds change.
    let mut over: Vec<Ring> = (0..80).map(|_| pyrrole()).collect();
    over.extend((0..5).map(|_| furan()));
    let id = draw_rings(&app, "Over bound", &over);
    let before = serde_json::to_value(app.document(&id).unwrap()).unwrap();
    let refused = convert(&app, &id, "too-many", revision, "aromatic");
    assert_eq!(refused["outcome"], "error", "{refused}");
    assert_eq!(refused["diagnostics"][0]["code"], "invalid_command");
    assert!(
        refused["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("at most 500 commands"),
        "the refusal names the transaction bound: {refused}"
    );
    assert_eq!(
        serde_json::to_value(app.document(&id).unwrap()).unwrap(),
        before
    );
}

#[test]
fn one_unsupported_system_refuses_the_whole_document_conversion() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let id = app
        .create_document(None, Some("Benzene and tetralin"))
        .unwrap()
        .document
        .id;
    let mut commands: Vec<Value> = (0..16)
        .map(|i| json!({"op":"add_atom","element":"C","position":{"x":i * 20,"y":0}}))
        .collect();
    let mut bond = |a: usize, b: usize, order: &str| {
        commands.push(
            json!({"op":"add_bond","begin":format!("${a}"),"end":format!("${b}"),"order":order}),
        );
    };
    for i in 0..6 {
        bond(i, (i + 1) % 6, if i % 2 == 0 { "double" } else { "single" });
    }
    // Tetralin on atoms 6..16: benzene 6..12 sharing the edge 6-11 with a saturated ring 6,11,12..16.
    for i in 0..6 {
        bond(
            6 + i,
            6 + (i + 1) % 6,
            if i % 2 == 0 { "double" } else { "single" },
        );
    }
    bond(6, 12, "single");
    bond(12, 13, "single");
    bond(13, 14, "single");
    bond(14, 15, "single");
    bond(15, 11, "single");
    let drawn = call(
        &app,
        "draw",
        "document.transaction",
        json!({"document_id":id,"expected_revision":0,"commands":commands}),
    );
    assert_eq!(drawn["outcome"], "ok", "{drawn}");
    let before = serde_json::to_value(app.document(&id).unwrap()).unwrap();
    let inspected = call(
        &app,
        "inspect",
        "chem.aromaticity",
        json!({"document_id":id}),
    );
    let systems = inspected["result"]["systems"].as_array().unwrap();
    assert_eq!(systems.len(), 2);
    assert!(
        systems
            .iter()
            .any(|s| s["classification"] == "unsupported"
                && s["reason"] == "fused_partially_aromatic")
    );
    let refused = convert(&app, &id, "convert", 1, "aromatic");
    assert_eq!(refused["outcome"], "error", "{refused}");
    assert_eq!(
        refused["diagnostics"][0]["details"]["reason"],
        "fused_partially_aromatic"
    );
    assert_eq!(
        serde_json::to_value(app.document(&id).unwrap()).unwrap(),
        before,
        "benzene is not converted alone"
    );
    let identity = call(
        &app,
        "identity",
        "chem.identity",
        json!({"document_id":id,"mode":"chemical"}),
    );
    assert_eq!(identity["outcome"], "unsupported", "{identity}");
    assert_eq!(
        identity["diagnostics"][0]["details"]["reason"],
        "fused_partially_aromatic"
    );
}

#[test]
fn a_plain_benzene_still_converts_alone() {
    let directory = tempfile::tempdir().unwrap();
    let app = new_app(&directory);
    let id = draw_rings(&app, "Benzene", &[benzene()]);
    let converted = convert(&app, &id, "to-aromatic", 1, "aromatic");
    assert_eq!(converted["outcome"], "ok", "{converted}");
}
