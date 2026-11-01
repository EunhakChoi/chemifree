//! Property-style checks of the canonical and recorded SMILES over a broad, seeded corpus.
//!
//! For every molecule: all spellings (including random atom permutations and the model's own
//! Kekule/aromatic conversions) give one canonical text or are all refused; the canonical
//! text is a fixpoint; and the recorded text re-reads to the same recorded graph.
use chemifree_application::Application;
use chemifree_application::smiles::{SmilesBond, SmilesCodec, SmilesMolecule};
use chemifree_providers::{FileDocumentRepository, LocalCapabilities, SmilesTextCodec};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicUsize, Ordering};

type App = Application<LocalCapabilities, FileDocumentRepository>;

fn call(app: &App, operation: &str, parameters: Value) -> Value {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let id = format!("q{}", NEXT.fetch_add(1, Ordering::Relaxed));
    let request = json!({"contract":1,"request_id":id,"profile":"preserve-semantics","operation":operation,"parameters":parameters});
    serde_json::to_value(app.execute(request.to_string().as_bytes())).unwrap()
}

fn open(app: &App, text: &str) -> Result<String, String> {
    let created = call(app, "document.create", json!({"title":"P"}));
    let id = created["result"]["document"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("no document: {created}"))
        .to_owned();
    let source = json!({"document_id":id,"expected_revision":0,"format":"smiles","source":text});
    let mut preview = source.clone();
    preview["preview"] = json!(true);
    let reply = call(app, "document.import", preview);
    if reply["result"].is_null() {
        close(app, &id);
        return Err(format!(
            "{text}: {}",
            reply["diagnostics"][0]["details"]["reason"]
        ));
    }
    let mut commit = source;
    commit["preview_digest"] = reply["result"]["preview_digest"].clone();
    commit["accept_loss"] = json!(true);
    let done = call(app, "document.import", commit);
    assert!(done["result"].is_object(), "{text}: {done}");
    Ok(id)
}

fn close(app: &App, id: &str) {
    let revision =
        call(app, "document.snapshot", json!({"document_id":id}))["document_revision"].clone();
    let reply = call(
        app,
        "document.close",
        json!({"document_id":id,"discard_unsaved":true,"expected_revision":revision}),
    );
    assert_eq!(reply["outcome"], "ok", "{reply}");
}

/// The canonical text, or the reason it was refused.
fn canonical_id(app: &App, id: &str) -> Result<String, String> {
    let reply = call(
        app,
        "document.export",
        json!({"document_id":id,"format":"smiles","mode":"canonical"}),
    );
    match reply["result"]["smiles"].as_str() {
        Some(text) => Ok(text.to_owned()),
        None => Err(reply["diagnostics"][0]["details"]["reason"]
            .as_str()
            .unwrap_or("?")
            .to_owned()),
    }
}

fn canonical(app: &App, text: &str) -> Result<String, String> {
    let id = open(app, text)?;
    let result = canonical_id(app, &id);
    close(app, &id);
    result
}

fn recorded_code(app: &App, id: &str) -> String {
    let reply = call(
        app,
        "chem.identity",
        json!({"document_id":id,"mode":"recorded"}),
    );
    reply["result"]["identity"]["code"]
        .as_str()
        .unwrap()
        .to_owned()
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

/// The same molecule with its atoms in a random order, written in that order.
fn permuted(text: &str, seed: u64) -> String {
    let molecule = SmilesTextCodec.read(text).unwrap().molecule;
    let mut rng = Lcg(seed);
    let mut order: Vec<usize> = (0..molecule.atoms.len()).collect();
    for i in (1..order.len()).rev() {
        order.swap(i, rng.next(i + 1));
    }
    let mut position = vec![0; order.len()];
    for (new, &old) in order.iter().enumerate() {
        position[old] = new;
    }
    let atoms = order
        .iter()
        .map(|&old| molecule.atoms[old].clone())
        .collect();
    let mut bonds: Vec<SmilesBond> = molecule
        .bonds
        .iter()
        .map(|b| SmilesBond {
            begin: position[b.begin],
            end: position[b.end],
            kind: b.kind,
        })
        .collect();
    for i in (1..bonds.len()).rev() {
        bonds.swap(i, rng.next(i + 1));
    }
    SmilesTextCodec
        .write(&SmilesMolecule { atoms, bonds })
        .expect("a permutation of a read molecule is writable")
        .text
}

enum Expect {
    Ok,
    /// Every spelling is refused, with one of these reasons.
    Refused(&'static [&'static str]),
}
use Expect::{Ok as Accepted, Refused};

const FUSED_PARTIAL: &[&str] = &["mixed_ring_bonds", "fused_partially_aromatic"];
const FUSED_OUTSIDE: &[&str] = &["fused_system_outside_model"];
const EXTERNAL: &[&str] = &["unsupported_external_bond"];

fn corpus() -> Vec<(&'static str, Vec<&'static str>, Expect)> {
    vec![
        // acyclic
        ("ethanol", vec!["CCO", "OCC", "C(O)C"], Accepted),
        ("isobutane", vec!["CC(C)C", "C(C)(C)C"], Accepted),
        (
            "acetic acid",
            vec!["CC(=O)O", "OC(C)=O", "C(=O)(C)O", "C(O)(=O)C"],
            Accepted,
        ),
        ("nitro", vec!["C[N+](=O)[O-]", "[O-][N+](=O)C"], Accepted),
        ("nitrile", vec!["CC#N", "N#CC"], Accepted),
        ("sulfone", vec!["CS(=O)(=O)C", "O=S(=O)(C)C"], Accepted),
        (
            "methane explicit H",
            vec!["[H]C([H])([H])[H]", "[CH4]", "C"],
            Accepted,
        ),
        ("hydrogen molecule", vec!["[H][H]"], Accepted),
        ("chloroform", vec!["ClC(Cl)Cl", "C(Cl)(Cl)Cl"], Accepted),
        (
            "quaternary acyclic",
            vec!["CC(C)(C)C", "C(C)(C)(C)C"],
            Accepted,
        ),
        // charged
        (
            "ammonium",
            vec!["[NH4+]", "[N+]([H])([H])([H])[H]"],
            Accepted,
        ),
        (
            "tetramethylammonium",
            vec!["C[N+](C)(C)C", "[N+](C)(C)(C)C"],
            Accepted,
        ),
        ("hydroxide", vec!["[OH-]", "[O-][H]"], Accepted),
        (
            "acetate salt",
            vec!["CC(=O)[O-].[NH4+]", "[NH4+].[O-]C(C)=O"],
            Accepted,
        ),
        ("carbanion", vec!["[CH3-]", "[CH3-]"], Accepted),
        // isotopes
        (
            "labelled ethane",
            vec!["[13CH3]C", "C[13CH3]", "[13CH3][CH3]"],
            Accepted,
        ),
        (
            "deuterated methanol",
            vec!["[2H]C([2H])([2H])O", "OC([2H])([2H])[2H]"],
            Accepted,
        ),
        ("heavy water", vec!["[2H]O[2H]", "[2H][O][2H]"], Accepted),
        ("oxygen 18", vec!["[18OH2]", "[18O]([H])[H]"], Accepted),
        // multi-component
        (
            "ethanol and water",
            vec!["CCO.O", "O.CCO", "OCC.[OH2]"],
            Accepted,
        ),
        ("two ethanes", vec!["CC.CC", "C(C).C(C)"], Accepted),
        (
            "ammonium chloride",
            vec!["[NH4+].[Cl-]", "[Cl-].[NH4+]"],
            Accepted,
        ),
        (
            "sodium chloride",
            vec!["[Na+].[Cl-]", "[Cl-].[Na+]"],
            Refused(&["element_or_charge"]),
        ),
        // saturated rings, explicit and inferred hydrogens, quaternary, spiro
        ("cyclopropane", vec!["C1CC1", "[CH2]1[CH2][CH2]1"], Accepted),
        (
            "cyclohexane",
            vec!["C1CCCCC1", "[CH2]1[CH2][CH2][CH2][CH2][CH2]1"],
            Accepted,
        ),
        (
            "gem-dimethylcyclopropane",
            vec!["CC1(C)CC1", "C1CC1(C)C"],
            Accepted,
        ),
        (
            "methylcyclohexane",
            vec!["CC1CCCCC1", "C1CCCCC1C"],
            Accepted,
        ),
        (
            "spiro[4.5]decane",
            vec!["C1CCC2(CC1)CCCC2", "C1CCCC12CCCCC2"],
            Accepted,
        ),
        (
            "spiro with stated H",
            vec![
                "[CH2]1[CH2][CH2]C2([CH2][CH2]1)[CH2][CH2][CH2][CH2]2",
                "C1CCC2(CC1)CCCC2",
            ],
            Accepted,
        ),
        (
            "quaternary ring atom",
            vec!["CC1(C)CCCCC1", "C1CCCCC1(C)C"],
            Accepted,
        ),
        (
            "piperidine",
            vec!["C1CCNCC1", "N1CCCCC1", "[NH]1CCCCC1"],
            Accepted,
        ),
        ("morpholine", vec!["C1COCCN1", "O1CCNCC1"], Accepted),
        ("tetrahydrofuran", vec!["C1CCOC1", "O1CCCC1"], Accepted),
        (
            "pyrrolidinium",
            vec!["C1CC[NH2+]C1", "[NH2+]1CCCC1"],
            Accepted,
        ),
        (
            "quaternary ammonium ring",
            vec!["C1CC[N+](C)(C)C1", "C[N+]1(C)CCCC1"],
            Accepted,
        ),
        // bridged and caged
        ("norbornane", vec!["C1CC2CCC1C2", "C1CC2CC1CC2"], Accepted),
        (
            "bicyclo[2.2.2]octane",
            vec!["C1CC2CCC1CC2", "C12CCC(CC1)CC2"],
            Accepted,
        ),
        (
            "adamantane",
            vec!["C1C2CC3CC1CC(C2)C3", "C1C2CC3CC1CC(C3)C2"],
            Accepted,
        ),
        ("cubane", vec!["C12C3C4C1C5C2C3C45"], Accepted),
        // non-aromatic ring alkenes, bridged and fused
        ("cyclohexene", vec!["C1=CCCCC1", "C1CCC=CC1"], Accepted),
        (
            "cyclopentadiene",
            vec!["C1C=CC=C1", "[CH2]1C=CC=C1", "C1=CC=CC1"],
            Accepted,
        ),
        (
            "cycloheptatriene",
            vec!["C1=CC=CCC=C1", "[CH2]1C=CC=CC=C1"],
            Accepted,
        ),
        (
            "cyclooctatetraene",
            vec!["C1=CC=CC=CC=C1", "C1=CC=CC=CC=C1"],
            Accepted,
        ),
        ("norbornene", vec!["C1CC2C=CC1C2", "C1=CC2CCC1C2"], Accepted),
        (
            "norbornadiene",
            vec!["C1=CC2C=CC1C2", "C1C2C=CC1C=C2"],
            Accepted,
        ),
        (
            "alpha-pinene",
            vec!["CC1=CCC2CC1C2(C)C", "CC1=CCC2CC1C2(C)C"],
            Accepted,
        ),
        (
            "octalin",
            vec!["C1CCC2=CCCCC2C1", "C1CC=C2CCCCC2C1"],
            Accepted,
        ),
        (
            "decalin",
            vec!["C1CCC2CCCCC2C1", "C1CCCC2CCCCC12"],
            Accepted,
        ),
        (
            "cholesterol skeleton",
            vec![
                "CC(C)CCCC(C)C1CCC2C1(CCC3C2CC=C4C3(CCC(C4)O)C)C",
                "OC1CCC2(C)C(C1)=CCC1C2CCC2(C)C(CCC12)C(C)CCCC(C)C",
            ],
            Accepted,
        ),
        // heteroaromatics (single ring)
        (
            "benzene",
            vec!["c1ccccc1", "C1=CC=CC=C1", "C1=CC=CC=C1", "c1ccc[cH]c1"],
            Accepted,
        ),
        (
            "toluene",
            vec!["Cc1ccccc1", "c1ccccc1C", "CC1=CC=CC=C1", "C1=CC=CC=C1C"],
            Accepted,
        ),
        (
            "phenol",
            vec!["Oc1ccccc1", "c1ccc(O)cc1", "OC1=CC=CC=C1"],
            Accepted,
        ),
        (
            "benzoic acid",
            vec!["OC(=O)c1ccccc1", "c1ccccc1C(=O)O", "OC(=O)C1=CC=CC=C1"],
            Accepted,
        ),
        (
            "biphenyl",
            vec![
                "c1ccccc1-c1ccccc1",
                "c1ccccc1c1ccccc1",
                "C1=CC=CC=C1C1=CC=CC=C1",
            ],
            Accepted,
        ),
        (
            "pyridine",
            vec!["n1ccccc1", "N1=CC=CC=C1", "C1=CC=NC=C1", "c1ccncc1"],
            Accepted,
        ),
        (
            "pyrrole",
            vec![
                "[nH]1cccc1",
                "N1C=CC=C1",
                "c1cc[nH]c1",
                "C1=CNC=C1",
                "[NH]1C=CC=C1",
            ],
            Accepted,
        ),
        (
            "N-methylpyrrole",
            vec!["Cn1cccc1", "CN1C=CC=C1", "c1ccn(C)c1"],
            Accepted,
        ),
        ("furan", vec!["o1cccc1", "O1C=CC=C1", "c1ccoc1"], Accepted),
        (
            "thiophene",
            vec!["s1cccc1", "S1C=CC=C1", "c1ccsc1"],
            Accepted,
        ),
        (
            "imidazole",
            vec![
                "[nH]1cncc1",
                "N1C=NC=C1",
                "c1c[nH]cn1",
                "C1=CN=CN1",
                "N1C=CN=C1",
            ],
            Accepted,
        ),
        (
            "pyrazole",
            vec!["c1cn[nH]c1", "N1N=CC=C1", "C1=CNN=C1", "c1c[nH]nc1"],
            Accepted,
        ),
        (
            "oxazole",
            vec!["c1cocn1", "O1C=NC=C1", "C1=COC=N1"],
            Accepted,
        ),
        (
            "thiazole",
            vec!["c1cscn1", "S1C=NC=C1", "C1=CSC=N1"],
            Accepted,
        ),
        (
            "pyrimidine",
            vec!["c1cncnc1", "N1=CN=CC=C1", "C1=CN=CN=C1"],
            Accepted,
        ),
        ("pyrazine", vec!["c1cnccn1", "N1=CC=NC=C1"], Accepted),
        (
            "N-methylpyridinium",
            vec!["C[n+]1ccccc1", "C[N+]1=CC=CC=C1"],
            Accepted,
        ),
        (
            "pyridine N-oxide",
            vec!["[O-][n+]1ccccc1", "[O-][N+]1=CC=CC=C1"],
            Accepted,
        ),
        (
            "pyridinium",
            vec!["[nH+]1ccccc1", "[NH+]1=CC=CC=C1"],
            Accepted,
        ),
        (
            "cyclopentadienyl anion",
            vec!["[cH-]1cccc1", "[CH-]1C=CC=C1"],
            Accepted,
        ),
        (
            "tropylium",
            vec!["c1ccc[cH+]cc1", "C1=CC=C[CH+]C=C1"],
            Accepted,
        ),
        // fused aromatic carbocycles
        (
            "naphthalene",
            vec![
                "c1ccc2ccccc2c1",
                "C1=CC=C2C=CC=CC2=C1",
                "C1=CC2=CC=CC=C2C=C1",
                "c1cccc2ccccc12",
            ],
            Accepted,
        ),
        (
            "anthracene",
            vec!["c1ccc2cc3ccccc3cc2c1", "C1=CC=C2C=C3C=CC=CC3=CC2=C1"],
            Accepted,
        ),
        (
            "phenanthrene",
            vec!["c1ccc2c(c1)ccc1ccccc12", "c1ccc2c(c1)ccc1ccccc21"],
            Accepted,
        ),
        (
            "methylnaphthalene",
            vec!["Cc1cccc2ccccc12", "c1ccc2c(C)cccc2c1"],
            Accepted,
        ),
        // fused heteroaromatics are outside the model: refused in every spelling
        (
            "indole",
            vec!["c1ccc2[nH]ccc2c1", "C1=CC=C2NC=CC2=C1"],
            Refused(FUSED_OUTSIDE),
        ),
        (
            "quinoline",
            vec!["c1ccc2ncccc2c1", "C1=CC=C2N=CC=CC2=C1"],
            Refused(FUSED_OUTSIDE),
        ),
        (
            "azulene",
            vec!["c1ccc2cccc2cc1", "C1=CC=C2C=CC=C2C=C1"],
            Refused(FUSED_OUTSIDE),
        ),
        // fused, partly saturated: a cycle could be aromatic, so every spelling is refused
        (
            "indane",
            vec!["c1ccc2c(c1)CCC2", "C1=CC=C2C(=C1)CCC2", "C1CCc2ccccc12"],
            Refused(FUSED_PARTIAL),
        ),
        (
            "tetralin",
            vec!["c1ccc2c(c1)CCCC2", "C1=CC=C2C(=C1)CCCC2", "C1CCc2ccccc2C1"],
            Refused(FUSED_PARTIAL),
        ),
        (
            "fluorene",
            vec!["c1ccc2c(c1)Cc1ccccc12", "C1=CC=C2C(=C1)CC1=CC=CC=C12"],
            Refused(FUSED_PARTIAL),
        ),
        (
            "indene",
            vec!["c1ccc2c(c1)C=CC2", "C1=CC=C2C(=C1)C=CC2"],
            Refused(FUSED_PARTIAL),
        ),
        (
            "dihydronaphthalene",
            vec!["C1=Cc2ccccc2CC1", "C1=CC2=CC=CC=C2CC1"],
            Refused(FUSED_PARTIAL),
        ),
        // exocyclic double bonds on ring atoms are outside the model
        (
            "cyclohexanone",
            vec!["O=C1CCCCC1", "C1CCC(=O)CC1"],
            Refused(EXTERNAL),
        ),
        (
            "methylenecyclopentane",
            vec!["C=C1CCCC1", "C1CCC(=C)C1"],
            Refused(EXTERNAL),
        ),
        (
            "2-pyridone",
            vec!["O=c1cccc[nH]1", "O=C1C=CC=CN1"],
            Refused(&[
                "unsupported_external_bond",
                "aromatic_bond_outside_ring",
                "mixed_ring_bonds",
            ]),
        ),
        // radicals
        ("methyl radical", vec!["[CH3]", "[CH3]"], Accepted),
        (
            "carbene",
            vec!["[CH2]", "[CH2]"],
            Refused(&["radical_state"]),
        ),
    ]
}

#[test]
fn spellings_orders_fixpoints_and_recorded_round_trips_agree() {
    let dir = tempfile::tempdir().unwrap();
    let app = Application::new(LocalCapabilities, FileDocumentRepository::new(dir.path()))
        .with_smiles_codec(SmilesTextCodec);
    let mut round_trips = 0;
    let mut converted = 0;
    for (name, spellings, expect) in corpus() {
        let base = canonical(&app, spellings[0]);
        // Every spelling, and random orders of each, give the same result.
        let mut texts: Vec<String> = spellings.iter().map(|s| (*s).to_owned()).collect();
        for (i, spelling) in spellings.iter().enumerate() {
            for seed in 0..3_u64 {
                texts.push(permuted(spelling, seed * 31 + i as u64 + 1));
            }
        }
        for text in &texts {
            let result = canonical(&app, text);
            match &expect {
                Accepted => assert_eq!(result, base, "{name}: {text:?}"),
                Refused(reasons) => assert!(
                    result
                        .as_ref()
                        .is_err_and(|r| reasons.contains(&r.as_str())),
                    "{name}: {text:?} gave {result:?}, expected one of {reasons:?}"
                ),
            }
        }
        // The model's own conversions are further spellings.
        if let Ok(id) = open(&app, spellings[0]) {
            for target in ["kekule", "aromatic"] {
                let reply = call(
                    &app,
                    "chem.aromaticity_convert",
                    json!({"document_id":id,"expected_revision":1,"to":target}),
                );
                if reply["result"].is_object() {
                    converted += 1;
                    let result = canonical_id(&app, &id);
                    match &expect {
                        Accepted => {
                            assert_eq!(result, base, "{name}: after converting to {target}")
                        }
                        Refused(reasons) => assert!(
                            result
                                .as_ref()
                                .is_err_and(|r| reasons.contains(&r.as_str())),
                            "{name}: after converting to {target}: {result:?}"
                        ),
                    }
                    call(
                        &app,
                        "history.undo",
                        json!({"document_id":id,"expected_revision":reply["document_revision"]}),
                    );
                }
            }
            close(&app, &id);
        }
        match (&expect, &base) {
            (Accepted, Ok(text)) => {
                assert_eq!(canonical(&app, text).as_ref(), Ok(text), "{name}: fixpoint");
            }
            (Refused(reasons), Err(reason)) => {
                assert!(
                    reasons.contains(&reason.as_str()),
                    "{name}: refused with {reason}"
                );
            }
            (Accepted, Err(reason)) => {
                panic!("{name}: expected a canonical text, refused: {reason}")
            }
            (Refused(_), Ok(text)) => panic!("{name}: expected a refusal, got {text}"),
        }
        // Recorded writing re-reads to the same recorded graph.
        for spelling in &spellings {
            let Ok(id) = open(&app, spelling) else {
                continue;
            };
            let reply = call(
                &app,
                "document.export",
                json!({"document_id":id,"format":"smiles","mode":"recorded"}),
            );
            if let Some(text) = reply["result"]["smiles"].as_str() {
                let again = open(&app, text).unwrap_or_else(|e| panic!("{name}: {text}: {e}"));
                assert_eq!(
                    recorded_code(&app, &again),
                    recorded_code(&app, &id),
                    "{name}: {spelling:?} wrote {text:?}"
                );
                round_trips += 1;
                close(&app, &again);
            }
            close(&app, &id);
        }
    }
    assert!(
        round_trips > 150,
        "only {round_trips} recorded round trips ran"
    );
    assert!(converted > 20, "only {converted} model conversions ran");
}
