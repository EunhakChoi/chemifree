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
            found
                .iter()
                .any(|s| s == "unsupported/fused_partially_aromatic"
                    || s == "unsupported/mixed_ring_bonds"),
            "{text}: {found:?}"
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
