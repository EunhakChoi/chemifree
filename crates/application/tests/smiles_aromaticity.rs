//! The shared aromaticity model as the SMILES canonical form depends on it: saturated ring
//! atoms, pyrrole-type nitrogen in Kekule form, partially aromatic fused systems, and the
//! systems that must keep their non-aromatic answer.
use chemifree_application::Application;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities, SmilesTextCodec};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicUsize, Ordering};

type App = Application<LocalCapabilities, FileDocumentRepository>;

fn call(app: &App, operation: &str, parameters: Value) -> Value {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let id = format!("a{}", NEXT.fetch_add(1, Ordering::Relaxed));
    let request = json!({"contract":1,"request_id":id,"profile":"preserve-semantics","operation":operation,"parameters":parameters});
    serde_json::to_value(app.execute(request.to_string().as_bytes())).unwrap()
}

fn import(app: &App, text: &str) -> String {
    let id = call(app, "document.create", json!({"title":"A"}))["result"]["document"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let source = json!({"document_id":id,"expected_revision":0,"format":"smiles","source":text});
    let mut preview = source.clone();
    preview["preview"] = json!(true);
    let reply = call(app, "document.import", preview);
    let mut commit = source;
    commit["preview_digest"] = reply["result"]["preview_digest"].clone();
    commit["accept_loss"] = json!(true);
    let done = call(app, "document.import", commit);
    assert!(done["result"].is_object(), "{text}: {done}");
    id
}

/// `classification/reason` of every ring system, in order.
fn systems(app: &App, text: &str) -> Vec<String> {
    let id = import(app, text);
    let reply = call(app, "chem.aromaticity", json!({"document_id":id}));
    reply["result"]["systems"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            format!(
                "{}/{}",
                s["classification"].as_str().unwrap(),
                s["reason"].as_str().unwrap_or("-")
            )
        })
        .collect()
}

fn chemical_code(app: &App, text: &str) -> Result<String, String> {
    let id = import(app, text);
    let reply = call(
        app,
        "chem.identity",
        json!({"document_id":id,"mode":"chemical"}),
    );
    match reply["result"]["identity"]["code"].as_str() {
        Some(code) => Ok(code.to_owned()),
        None => Err(reply["diagnostics"][0]["details"]["reason"]
            .as_str()
            .unwrap()
            .to_owned()),
    }
}

fn app() -> (App, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let app = Application::new(LocalCapabilities, FileDocumentRepository::new(dir.path()))
        .with_smiles_codec(SmilesTextCodec);
    (app, dir)
}

#[test]
fn saturated_ring_atoms_make_the_ring_non_aromatic_instead_of_unsupported() {
    let (app, _dir) = app();
    for text in [
        "[CH2]1[CH2][CH2]1",
        "CC1(C)CC1",
        "C1CC1(C)C",
        "C1CCCCC1(C)C",
    ] {
        let found = systems(&app, text);
        assert_eq!(found.len(), 1, "{text}");
        assert!(found[0].starts_with("non_aromatic/"), "{text}: {found:?}");
        assert!(chemical_code(&app, text).is_ok(), "{text}");
    }
    // Spiro atoms are in two separate ring systems; both are saturated.
    let spiro = systems(&app, "C1CCC2(CC1)CCCC2");
    assert_eq!(spiro.len(), 2);
    assert!(
        spiro.iter().all(|s| s.starts_with("non_aromatic/")),
        "{spiro:?}"
    );
    // A saturated atom cannot contribute electrons: cycloheptatriene is not tropylium-like.
    assert_eq!(
        systems(&app, "[CH2]1C=CC=CC=C1"),
        ["non_aromatic/saturated_atom"]
    );
    assert_eq!(
        systems(&app, "C1=CC=CCC=C1"),
        ["non_aromatic/not_conjugated"]
    );
    // An aromatic bond on a saturated atom stays an error.
    assert_eq!(
        systems(&app, "c1cc[cH2]cc1"),
        ["unsupported/valence_not_in_model"]
    );
}

#[test]
fn pyrrole_type_nitrogen_in_kekule_form_matches_the_aromatic_spelling() {
    let (app, _dir) = app();
    assert_eq!(systems(&app, "N1C=CC=C1"), ["aromatic/-"]);
    assert_eq!(systems(&app, "[NH]1C=CC=C1"), ["aromatic/-"]);
    assert_eq!(
        chemical_code(&app, "N1C=CC=C1"),
        chemical_code(&app, "[nH]1cccc1")
    );
    assert!(chemical_code(&app, "N1C=CC=C1").is_ok());
    // Lowercase n without hydrogen is pyridine-type: five electrons, and no usable form.
    assert_eq!(systems(&app, "n1cccc1"), ["non_aromatic/electron_count"]);
    assert_eq!(
        chemical_code(&app, "n1cccc1"),
        Err("aromaticity_unsupported".into())
    );
    // Pyridine keeps its ring double bond.
    assert_eq!(systems(&app, "N1=CC=CC=C1"), ["aromatic/-"]);
    // A saturated amine is still non-aromatic.
    assert_eq!(systems(&app, "N1CC=CC=C1"), ["non_aromatic/not_conjugated"]);
}

#[test]
fn fused_systems_with_a_possibly_aromatic_cycle_are_refused_in_every_spelling() {
    let (app, _dir) = app();
    for text in [
        "c1ccc2c(c1)CCC2",
        "C1=CC=C2C(=C1)CCC2",
        "c1ccc2c(c1)CCCC2",
        "C1=CC=C2C(=C1)CCCC2",
        "c1ccc2c(c1)Cc1ccccc12",
        "C1=CC=C2C(=C1)CC1=CC=CC=C12",
    ] {
        let found = systems(&app, text);
        assert!(
            found.contains(&"unsupported/fused_partially_aromatic".to_owned()),
            "{text}: every spelling reports the same reason: {found:?}"
        );
        assert!(chemical_code(&app, text).is_err(), "{text}");
    }
}

#[test]
fn fused_and_bridged_systems_without_an_aromatic_cycle_stay_non_aromatic() {
    let (app, _dir) = app();
    for text in [
        "C1CC2C=CC1C2",
        "C1=CC2C=CC1C2",
        "CC1=CCC2CC1C2(C)C",
        "C1CCC2=CCCCC2C1",
        "C1CCC2CCCCC2C1",
        "C1CC2CCC1CC2",
        "CC(C)CCCC(C)C1CCC2C1(CCC3C2CC=C4C3(CCC(C4)O)C)C",
    ] {
        let found = systems(&app, text);
        assert!(
            found.iter().all(|s| s.starts_with("non_aromatic/")),
            "{text}: {found:?}"
        );
        assert!(chemical_code(&app, text).is_ok(), "{text}");
    }
}

#[test]
fn exocyclic_double_bonds_on_ring_atoms_remain_outside_the_model() {
    let (app, _dir) = app();
    for text in ["O=C1CCCCC1", "C=C1CCCC1", "O=C1C=CC=CN1"] {
        assert_eq!(
            systems(&app, text),
            ["unsupported/unsupported_external_bond"],
            "{text}"
        );
        assert_eq!(
            chemical_code(&app, text),
            Err("unsupported_external_bond".into()),
            "{text}"
        );
    }
}

fn new_document(app: &App) -> String {
    call(app, "document.create", json!({"title":"A"}))["result"]["document"]["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn close_document(app: &App, id: &str) {
    let revision =
        call(app, "document.snapshot", json!({"document_id":id}))["document_revision"].clone();
    let reply = call(
        app,
        "document.close",
        json!({"document_id":id,"discard_unsaved":true,"expected_revision":revision}),
    );
    assert_eq!(reply["outcome"], "ok", "{reply}");
}

/// Convert in place; returns the reply.
fn convert(app: &App, id: &str, to: &str) -> Value {
    let revision =
        call(app, "document.snapshot", json!({"document_id":id}))["document_revision"].clone();
    call(
        app,
        "chem.aromaticity_convert",
        json!({"document_id":id,"expected_revision":revision,"to":to}),
    )
}

fn code_of(app: &App, id: &str) -> Result<String, String> {
    let reply = call(
        app,
        "chem.identity",
        json!({"document_id":id,"mode":"chemical"}),
    );
    match reply["result"]["identity"]["code"].as_str() {
        Some(code) => Ok(code.to_owned()),
        None => Err(reply["diagnostics"][0]["details"]["reason"]
            .as_str()
            .unwrap()
            .to_owned()),
    }
}

fn classes(app: &App, id: &str) -> Vec<String> {
    call(app, "chem.aromaticity", json!({"document_id":id}))["result"]["systems"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["classification"].as_str().unwrap().to_owned())
        .collect()
}

/// Converting a ring the model calls aromatic must not change what the model says about it.
fn assert_round_trip(app: &App, text: &str) {
    let id = import(app, text);
    let before = code_of(app, &id);
    assert_eq!(classes(app, &id), ["aromatic"], "{text}");
    let to_aromatic = convert(app, &id, "aromatic");
    assert_eq!(to_aromatic["outcome"], "ok", "{text}: {to_aromatic}");
    assert_eq!(classes(app, &id), ["aromatic"], "{text}: aromatic form");
    assert_eq!(
        code_of(app, &id),
        before,
        "{text}: identity of the aromatic form"
    );
    let back = convert(app, &id, "kekule");
    assert_eq!(back["outcome"], "ok", "{text}: {back}");
    assert_eq!(classes(app, &id), ["aromatic"], "{text}: Kekule form");
    assert_eq!(
        code_of(app, &id),
        before,
        "{text}: identity after converting back"
    );
}

#[test]
fn converting_kekule_pyrrole_type_rings_to_aromatic_keeps_their_interpretation() {
    let (app, _dir) = app();
    for text in [
        "N1C=CC=C1",
        "N1C=NC=C1",
        "N1C=CN=C1",
        "N1N=CC=C1",
        "N1SN1",
        "O1ON1",
        "[NH]1[NH]N1",
    ] {
        assert_round_trip(&app, text);
    }
}

struct Lcg(u64);
impl Lcg {
    fn next(&mut self, bound: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) as usize) % bound
    }
}

#[test]
fn every_kekule_ring_the_model_calls_aromatic_survives_conversion_both_ways() {
    let (app, _dir) = app();
    let atoms = [
        "C", "C", "C", "N", "N", "O", "S", "[CH-]", "[CH+]", "[NH]", "[N+]", "[NH+]", "[N-]",
    ];
    let mut tested = 0;
    for seed in 0..3000_u64 {
        let mut rng = Lcg(seed + 1);
        let size = 3 + rng.next(5);
        let mut text = String::new();
        for i in 0..size {
            if i > 0 && rng.next(2) == 0 {
                text.push('=');
            }
            text.push_str(atoms[rng.next(atoms.len())]);
            if i == 0 {
                text.push('1');
            }
        }
        if rng.next(2) == 0 {
            text.push('=');
        }
        text.push('1');
        let id = new_document(&app);
        let preview = call(
            &app,
            "document.import",
            json!({"document_id":id,"expected_revision":0,"format":"smiles","source":text,"preview":true}),
        );
        if preview["result"].is_null() {
            close_document(&app, &id);
            continue;
        }
        let digest = preview["result"]["preview_digest"].clone();
        call(
            &app,
            "document.import",
            json!({"document_id":id,"expected_revision":0,"format":"smiles","source":text,"preview_digest":digest,"accept_loss":true}),
        );
        if classes(&app, &id) == ["aromatic"] && code_of(&app, &id).is_ok() {
            tested += 1;
            assert_round_trip_in_place(&app, &id, &text, seed);
        }
        close_document(&app, &id);
    }
    assert!(tested > 30, "only {tested} aromatic rings were generated");
}

fn assert_round_trip_in_place(app: &App, id: &str, text: &str, seed: u64) {
    let before = code_of(app, id);
    for target in ["aromatic", "kekule"] {
        let reply = convert(app, id, target);
        assert_eq!(
            reply["outcome"], "ok",
            "seed {seed} {text}: to {target}: {reply}"
        );
        assert_eq!(
            classes(app, id),
            ["aromatic"],
            "seed {seed} {text}: {target} form"
        );
        assert_eq!(
            code_of(app, id),
            before,
            "seed {seed} {text}: identity in {target} form"
        );
    }
}

#[test]
fn kekule_benzene_fused_to_saturated_rings_is_unsupported_never_silently_non_aromatic() {
    let (app, _dir) = app();
    // main reported each of these as `non_aromatic/not_conjugated` with chemical identity ok.
    for (name, text) in [
        ("tetralin", "C1=CC=C2CCCCC2=C1"),
        ("indane", "C1=CC=C2CCCC2=C1"),
        ("benzodioxole", "C1=CC=C2OCOC2=C1"),
        ("indoline", "C1=CC=C2NCCC2=C1"),
    ] {
        assert_eq!(
            systems(&app, text),
            ["unsupported/fused_partially_aromatic"],
            "{name}"
        );
        assert_eq!(
            chemical_code(&app, text),
            Err("fused_partially_aromatic".into()),
            "{name}"
        );
    }
}

/// A ladder of `squares` fused four-membered rings (rails single, rungs double) with one
/// saturated atom closing a three-membered ring on the first rung, so the system as a whole is
/// not conjugated and the model must decide whether some cycle of it could be aromatic.
fn ladder(app: &App, squares: usize) -> Vec<String> {
    let id = new_document(app);
    let columns = squares + 1;
    let mut commands = Vec::new();
    for row in 0..2 {
        for column in 0..columns {
            commands.push(json!({"op":"add_atom","element":"C","position":{"x":(column * 20) as f64,"y":(row * 20) as f64}}));
        }
    }
    commands.push(json!({"op":"add_atom","element":"C","position":{"x":-20.0,"y":10.0}}));
    let spur = 2 * columns;
    let bond = |a: usize, b: usize, order: &str| json!({"op":"add_bond","begin":format!("${a}"),"end":format!("${b}"),"order":order});
    for column in 0..columns {
        commands.push(bond(column, columns + column, "double"));
        if column + 1 < columns {
            commands.push(bond(column, column + 1, "single"));
            commands.push(bond(columns + column, columns + column + 1, "single"));
        }
    }
    commands.push(bond(spur, 0, "single"));
    commands.push(bond(spur, columns, "single"));
    let reply = call(
        app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":0,"commands":commands}),
    );
    assert_eq!(reply["outcome"], "ok", "{reply}");
    let found = call(app, "chem.aromaticity", json!({"document_id":id}))["result"]["systems"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            format!(
                "{}/{}",
                s["classification"].as_str().unwrap(),
                s["reason"].as_str().unwrap_or("-")
            )
        })
        .collect();
    close_document(app, &id);
    found
}

#[test]
fn large_fused_conjugated_systems_with_no_aromatic_cycle_stay_decided_up_to_the_size_limit() {
    let (app, _dir) = app();
    // Every cycle of a ladder of squares has 4n pi electrons, so none can be aromatic.
    for squares in [2, 6, 10] {
        assert_eq!(
            ladder(&app, squares),
            ["non_aromatic/not_conjugated"],
            "{squares} squares"
        );
    }
    // Up to the 127-atom size the model accepts, the cycle search stays within its budget.
    for squares in [14, 30, 62] {
        assert_eq!(
            ladder(&app, squares),
            ["non_aromatic/not_conjugated"],
            "{squares} squares"
        );
    }
}
