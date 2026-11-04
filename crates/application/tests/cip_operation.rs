//! `chem.cip` over the operation contract.
//!
//! Documents are made by transactions sent to the Application, as HTTP and the CLI send them.
//! Most molecules are read from SMILES by the small test reader of the domain's CIP tests
//! (the application's own SMILES provider states no stereo), then turned into commands.

#[path = "../../domain/tests/cip/support.rs"]
#[allow(
    dead_code,
    reason = "this target uses a subset of the domain test reader"
)]
mod support;

use chemifree_application::{Application, CommandDto};
use chemifree_domain::document::BondOrder;
use chemifree_domain::states::{AtomKind, AttachmentPoints, Hydrogens, QueryAtom};
use chemifree_domain::stereo::{Relation, Turn};
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};
use support::{Molecule, Neighbor, parse};
use tempfile::TempDir;

type App = Application<LocalCapabilities, FileDocumentRepository>;

fn application() -> (App, TempDir) {
    let directory = tempfile::tempdir().unwrap();
    let app = Application::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    );
    (app, directory)
}

fn created(app: &App) -> String {
    app.create_document(None, Some("Cip")).unwrap().document.id
}

fn call(app: &App, request: &str, parameters: &Value) -> Value {
    let body = json!({"contract": 1, "request_id": request, "profile": "preserve-semantics",
                      "operation": "chem.cip", "parameters": parameters});
    serde_json::to_value(app.execute(body.to_string().as_bytes())).unwrap()
}

fn transact(
    app: &App,
    id: &str,
    request: Option<&str>,
    revision: u64,
    commands: &[Value],
) -> Value {
    let dtos: Vec<CommandDto> =
        serde_json::from_value(Value::Array(commands.to_vec())).expect("commands parse");
    match app.apply_transaction(id, request, revision, dtos) {
        Ok(response) => serde_json::to_value(response).unwrap(),
        Err(error) => panic!(
            "refused: {}",
            serde_json::to_value(error.to_response()).unwrap()
        ),
    }
}

fn slot(neighbor: Neighbor, ids: &[String]) -> String {
    match neighbor {
        Neighbor::Atom(index) => ids[index].clone(),
        Neighbor::Hydrogen => "implicit_hydrogen".into(),
        Neighbor::LonePair => "lone_pair".into(),
    }
}

/// The commands that build `molecule` on an empty document, atoms and bonds added in the
/// given orders, with the atom ids (`a1`...) and bond ids (`b1`...) each index gets.
fn commands(
    molecule: &Molecule,
    atom_order: &[usize],
    bond_order: &[usize],
) -> (Vec<Value>, Vec<String>, Vec<String>) {
    let mut ids = vec![String::new(); molecule.atoms.len()];
    let mut out = Vec::new();
    for (serial, &index) in atom_order.iter().enumerate() {
        ids[index] = format!("a{}", serial + 1);
        let atom = &molecule.atoms[index];
        let mut command =
            json!({"op": "add_atom", "position": {"x": 10.0 * index as f64, "y": 0.0}});
        match (atom.kind.element(), &atom.kind) {
            (Some(element), _) => command["element"] = json!(element.symbol()),
            (None, AtomKind::Query(QueryAtom::Any)) => command["query"] = json!({"kind": "any"}),
            (None, other) => panic!("not in these tests: {other:?}"),
        }
        match atom.hydrogens {
            Hydrogens::Specified(count) => {
                command["hydrogens"] = json!({"mode": "specified", "count": count});
            }
            Hydrogens::Unknown => command["hydrogens"] = json!({"mode": "unknown"}),
            Hydrogens::Inferred => {}
        }
        if !atom.attachment_points.is_empty() {
            command["attachment_points"] =
                json!(atom.attachment_points.values().collect::<Vec<_>>());
        }
        out.push(command);
    }
    for (serial, &index) in atom_order.iter().enumerate() {
        let atom = &molecule.atoms[index];
        if atom.isotope.is_some() || atom.charge != 0 {
            let mut update = json!({"op": "update_atom", "atom": format!("a{}", serial + 1)});
            if let Some(isotope) = atom.isotope {
                update["isotope"] = json!(isotope);
            }
            if atom.charge != 0 {
                update["charge"] = json!(atom.charge);
            }
            out.push(update);
        }
    }
    let mut bond_ids = vec![String::new(); molecule.bonds.len()];
    for (serial, &index) in bond_order.iter().enumerate() {
        bond_ids[index] = format!("b{}", serial + 1);
        let (begin, end, order) = &molecule.bonds[index];
        let order = match order {
            BondOrder::Single => "single",
            BondOrder::Double => "double",
            BondOrder::Triple => "triple",
            BondOrder::Aromatic => "aromatic",
            BondOrder::Dative => "dative",
            BondOrder::Unknown => "unknown",
            other => panic!("not in these tests: {other:?}"),
        };
        out.push(json!({"op": "add_bond", "begin": ids[*begin], "end": ids[*end], "order": order}));
    }
    for double in &molecule.doubles {
        let configuration = match double.relation {
            Some(relation) => json!({"kind": "relative",
            "begin_neighbor": slot(double.begin, &ids),
            "end_neighbor": slot(double.end, &ids),
            "relation": match relation {
                Relation::SameSide => "same_side",
                Relation::OppositeSides => "opposite_sides",
            }}),
            None => json!({"kind": "unknown"}),
        };
        out.push(
            json!({"op": "set_bond_configuration", "bond": bond_ids[double.bond],
            "configuration": configuration}),
        );
    }
    for center in &molecule.centers {
        // The command takes the neighbors in any order and the turn as seen in that order, which
        // is how the SMILES reader states them.
        let neighbors: Vec<String> = center.neighbors.iter().map(|&n| slot(n, &ids)).collect();
        let turn = match center.turn {
            Turn::Clockwise => "clockwise",
            Turn::Anticlockwise => "anticlockwise",
        };
        out.push(
            json!({"op": "set_atom_configuration", "atom": ids[center.atom],
            "configuration": {"kind": "tetrahedral", "neighbors": neighbors, "turn": turn}}),
        );
    }
    (out, ids, bond_ids)
}

struct Built {
    id: String,
    ids: Vec<String>,
    bond_ids: Vec<String>,
    revision: u64,
}

fn build(app: &App, molecule: &Molecule) -> Built {
    let atoms: Vec<usize> = (0..molecule.atoms.len()).collect();
    let bonds: Vec<usize> = (0..molecule.bonds.len()).collect();
    build_ordered(app, molecule, &atoms, &bonds)
}

fn build_ordered(app: &App, molecule: &Molecule, atoms: &[usize], bonds: &[usize]) -> Built {
    let id = created(app);
    let (commands, ids, bond_ids) = commands(molecule, atoms, bonds);
    let reply = transact(app, &id, None, 0, &commands);
    Built {
        id,
        ids,
        bond_ids,
        revision: reply["document"]["revision"].as_u64().unwrap(),
    }
}

// ---------------------------------------------------------------------------------------------
// Helpers for reading replies
// ---------------------------------------------------------------------------------------------

fn smiles(app: &App, text: &str) -> (Built, Molecule) {
    let molecule = parse(text);
    (build(app, &molecule), molecule)
}

/// The id of the bond between atoms `a` and `b` (indices of the molecule).
fn bond_id(built: &Built, molecule: &Molecule, a: usize, b: usize) -> String {
    let index = molecule
        .bonds
        .iter()
        .position(|&(begin, end, _)| (begin, end) == (a, b) || (begin, end) == (b, a))
        .unwrap_or_else(|| panic!("no bond {a}-{b}"));
    built.bond_ids[index].clone()
}

fn items<'a>(reply: &'a Value, list: &str) -> &'a Vec<Value> {
    reply["result"][list]
        .as_array()
        .unwrap_or_else(|| panic!("no result.{list} in {reply}"))
}

/// The item for `id` in `result.atoms` or `result.bonds`.
fn item<'a>(reply: &'a Value, list: &str, id: &str) -> &'a Value {
    let key = if list == "atoms" { "atom" } else { "bond" };
    items(reply, list)
        .iter()
        .find(|entry| entry[key] == id)
        .unwrap_or_else(|| panic!("{id} is not in result.{list}: {reply}"))
}

/// The label in the fixture file's spelling: `R`, `none`, `unsupported:cip_x`, ...
fn text(item: &Value) -> String {
    let state = item["state"].as_str().unwrap();
    match (state, item["descriptor"].as_str(), item["reason"].as_str()) {
        ("assigned", Some(descriptor), None) => descriptor.to_owned(),
        ("unsupported" | "ambiguous", None, Some(reason)) => format!("{state}:{reason}"),
        (_, None, None) => state.to_owned(),
        other => panic!("state, descriptor and reason do not pair: {other:?}"),
    }
}

fn ids(reply: &Value, list: &str) -> Vec<String> {
    let key = if list == "atoms" { "atom" } else { "bond" };
    items(reply, list)
        .iter()
        .map(|entry| entry[key].as_str().unwrap().to_owned())
        .collect()
}

fn codes(reply: &Value) -> Vec<&str> {
    reply["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|diagnostic| diagnostic["code"].as_str().unwrap())
        .collect()
}

fn diagnostic<'a>(reply: &'a Value, code: &str) -> &'a Value {
    reply["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|diagnostic| diagnostic["code"] == code)
        .unwrap_or_else(|| panic!("no {code} in {reply}"))
}

fn flipped(label: &str) -> String {
    match label {
        "R" => "S".to_owned(),
        "S" => "R".to_owned(),
        other => other.to_owned(),
    }
}

fn fixtures() -> Vec<Value> {
    let file = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../domain/tests/cip-fixtures.json"
    ))
    .expect("the domain's fixture file");
    let document: Value = serde_json::from_str(&file).expect("valid JSON");
    document["fixtures"].as_array().expect("fixtures").clone()
}

/// Every expectation of a fixture as `(list, id, label)` for a document built from `molecule`.
fn expectations(
    fixture: &Value,
    built: &Built,
    molecule: &Molecule,
) -> Vec<(&'static str, String, String)> {
    fixture["expect"]
        .as_array()
        .unwrap()
        .iter()
        .map(|expectation| {
            let label = expectation["label"].as_str().unwrap().to_owned();
            match expectation.get("bond") {
                Some(ends) => {
                    let end = |i: usize| usize::try_from(ends[i].as_u64().unwrap()).unwrap();
                    ("bonds", bond_id(built, molecule, end(0), end(1)), label)
                }
                None => {
                    let atom = usize::try_from(expectation["atom"].as_u64().unwrap()).unwrap();
                    ("atoms", built.ids[atom].clone(), label)
                }
            }
        })
        .collect()
}

fn name_all(expected: &[(&'static str, String, String)]) -> Value {
    let pick = |list: &str| -> Vec<&String> {
        expected
            .iter()
            .filter(|(kind, _, _)| *kind == list)
            .map(|(_, id, _)| id)
            .collect()
    };
    let mut parameters = serde_json::Map::new();
    for list in ["atoms", "bonds"] {
        let names = pick(list);
        if !names.is_empty() {
            parameters.insert(list.into(), json!(names));
        }
    }
    Value::Object(parameters)
}

fn request(built: &Built, extra: &Value) -> Value {
    let mut parameters = extra.as_object().cloned().unwrap_or_default();
    parameters.insert("document_id".into(), json!(built.id));
    Value::Object(parameters)
}

// ---------------------------------------------------------------------------------------------
// The labels the operation gives
// ---------------------------------------------------------------------------------------------

/// Every fixture of the domain's CIP tests, whose values are hand-derived, public, or checked
/// against RDKit by `scripts/verify-cip-rdkit.py`, is asked for through the operation, and so is
/// its mirror image: stored turns reversed flip R and S and leave every other label alone.
#[test]
fn the_operation_gives_every_fixture_its_label_and_a_mirror_image_flips_r_and_s() {
    let mut checked = 0;
    for fixture in fixtures() {
        // A service holds 64 documents with unsaved changes at most, so each molecule gets its own.
        let (app, _directory) = application();
        let name = fixture["name"].as_str().unwrap();
        let molecule = parse(fixture["smiles"].as_str().unwrap());
        for (image, mirror) in [(&molecule, false), (&molecule.mirrored(), true)] {
            let built = build(&app, image);
            let expected = expectations(&fixture, &built, image);
            let reply = call(&app, "fixture", &request(&built, &name_all(&expected)));
            for (list, id, label) in expected {
                let want = if mirror { flipped(&label) } else { label };
                assert_eq!(
                    text(item(&reply, list, &id)),
                    want,
                    "{name} {id} (mirror {mirror})"
                );
                checked += 1;
            }
        }
    }
    assert!(checked >= 140, "only {checked} labels were compared");
}

/// The same molecule built with its atoms and bonds added in another order has other
/// identifiers and other stored slot orders; each atom and bond still gets the same label.
#[test]
fn labels_do_not_depend_on_the_order_atoms_and_bonds_were_added() {
    for fixture in fixtures() {
        let (app, _directory) = application();
        let name = fixture["name"].as_str().unwrap();
        let molecule = parse(fixture["smiles"].as_str().unwrap());
        let forward = build(&app, &molecule);
        let atoms: Vec<usize> = (0..molecule.atoms.len()).rev().collect();
        let bonds: Vec<usize> = (0..molecule.bonds.len()).rev().collect();
        let reversed = build_ordered(&app, &molecule, &atoms, &bonds);
        let expected = expectations(&fixture, &forward, &molecule);
        let other = expectations(&fixture, &reversed, &molecule);
        let first = call(&app, "forward", &request(&forward, &name_all(&expected)));
        let second = call(&app, "reversed", &request(&reversed, &name_all(&other)));
        for ((list, id, _), (_, other_id, _)) in expected.iter().zip(&other) {
            let (one, two) = (item(&first, list, id), item(&second, list, other_id));
            assert_eq!(text(one), text(two), "{name} {id}");
            // The ranking is the same up to the names of the neighbors.
            let ranks = |item: &Value, ids: &[String], other: &[String]| -> Vec<(usize, u64)> {
                let _ = other;
                item["priority"]
                    .as_array()
                    .map(|priority| {
                        priority
                            .iter()
                            .map(|entry| {
                                let name = entry["neighbor"].as_str().unwrap();
                                let index = ids.iter().position(|id| id == name);
                                (index.unwrap_or(usize::MAX), entry["rank"].as_u64().unwrap())
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            };
            let mut a = ranks(one, &forward.ids, &reversed.ids);
            let mut b = ranks(two, &reversed.ids, &forward.ids);
            a.sort_unstable();
            b.sort_unstable();
            assert_eq!(a, b, "{name} {id}: priority by atom index");
        }
    }
}

/// The values the public sources and the hand derivation give, asked for by name. L-alanine is
/// (S), D-glyceraldehyde (R), `F[C@](Cl)(Br)I` is S and its mirror R, and `F[C@H](Cl)[C@H](Br)I`
/// is (R,R) with the mirror (S,S). They are also in the fixture file; here each is read from the
/// reply's own fields so that a swapped descriptor cannot hide in the generic comparison.
#[test]
fn public_hand_checked_molecules_have_their_known_descriptors() {
    for (text, atom, expected) in [
        ("N[C@@H](C)C(=O)O", 1, "S"),
        ("C([C@H](C=O)O)O", 1, "R"),
        ("F[C@](Cl)(Br)I", 1, "S"),
        ("F[C@@](Cl)(Br)I", 1, "R"),
        ("F[C@H](Cl)[C@H](Br)I", 1, "R"),
        ("F[C@H](Cl)[C@H](Br)I", 3, "R"),
        ("F[C@@H](Cl)[C@@H](Br)I", 1, "S"),
        ("F[C@@H](Cl)[C@@H](Br)I", 3, "S"),
    ] {
        let (app, _directory) = application();
        let (built, _) = smiles(&app, text);
        let reply = call(
            &app,
            "named",
            &request(&built, &json!({"atoms": [built.ids[atom]]})),
        );
        let atom = item(&reply, "atoms", &built.ids[atom]);
        assert_eq!(atom["state"], "assigned", "{text}");
        assert_eq!(atom["descriptor"], expected, "{text}");
    }
}

#[test]
fn an_assigned_center_is_reported_with_its_priority_and_stored_configuration() {
    let (app, _directory) = application();
    let (built, _) = smiles(&app, "N[C@@H](C)C(=O)O");
    let reply = call(&app, "wire", &request(&built, &json!({})));
    assert_eq!(reply["outcome"], "ok");
    assert_eq!(reply["model_id"], "chemifree-cip-1");
    assert_eq!(reply["document_revision"], 1);
    assert_eq!(reply["loss_report"], json!([]));
    assert_eq!(reply["result"]["model"], "chemifree-cip-1");
    assert_eq!(reply["result"]["rules"], json!(["1a", "1b", "2", "3"]));
    assert_eq!(reply["result"]["scope"], "stored");
    assert_eq!(reply["result"]["bonds"], json!([]));
    assert_eq!(reply["result"]["snapshot"]["document"]["revision"], 1);
    assert_eq!(
        reply["result"]["atoms"],
        json!([{
            "atom": "a2", "state": "assigned", "descriptor": "S", "reason": null,
            "stored": {"kind": "tetrahedral", "neighbors": ["a1", "a3", "a4", "implicit_hydrogen"],
                       "turn": "clockwise"},
            "priority": [{"neighbor": "a1", "rank": 1}, {"neighbor": "a4", "rank": 2},
                         {"neighbor": "a3", "rank": 3}, {"neighbor": "implicit_hydrogen", "rank": 4}],
            "decided_by": "1a"
        }])
    );
    assert_eq!(codes(&reply), ["cip_scope", "cip_not_checked"]);
}

#[test]
fn an_assigned_double_bond_names_the_higher_substituent_at_each_end() {
    for (text, descriptor) in [("Cl/C=C\\F", "Z"), ("Cl/C=C/F", "E")] {
        let (app, _directory) = application();
        let (built, molecule) = smiles(&app, text);
        let bond = bond_id(&built, &molecule, 1, 2);
        let reply = call(&app, "bond", &request(&built, &json!({})));
        assert_eq!(reply["outcome"], "ok", "{text}");
        assert_eq!(reply["result"]["atoms"], json!([]), "{text}");
        let entry = item(&reply, "bonds", &bond);
        assert_eq!(entry["descriptor"], descriptor, "{text}");
        // Chlorine outranks carbon at one end and fluorine hydrogen at the other.
        assert_eq!(
            entry["ends"],
            json!([{"atom": "a2", "higher": "a1", "decided_by": "1a"},
                   {"atom": "a3", "higher": "a4", "decided_by": "1a"}]),
            "{text}"
        );
        assert_eq!(entry["stored"]["kind"], "relative", "{text}");
    }
}

// ---------------------------------------------------------------------------------------------
// Which items a request lists
// ---------------------------------------------------------------------------------------------

/// `F[C@H](Cl)C(Br)(I)C=CC`: a2 is a stored center, a4 a stereocenter with nothing stored, b7
/// (`a7=a8`) a double bond with nothing stored; a1, a3, a5, a6, a9 are terminal.
const SELECTION: &str = "F[C@H](Cl)C(Br)(I)C=CC";

#[test]
fn the_default_scope_lists_the_stored_configurations_and_nothing_else() {
    let (app, _directory) = application();
    let (built, _) = smiles(&app, SELECTION);
    let reply = call(&app, "default", &request(&built, &json!({})));
    assert_eq!(reply["result"]["scope"], "stored");
    assert_eq!(ids(&reply, "atoms"), ["a2"]);
    assert_eq!(ids(&reply, "bonds"), Vec::<String>::new());
    let explicit = call(
        &app,
        "explicit",
        &request(&built, &json!({"scope": "stored"})),
    );
    assert_eq!(explicit["result"]["atoms"], reply["result"]["atoms"]);
    assert_eq!(explicit["result"]["scope"], "stored");
    let said = diagnostic(&reply, "cip_scope");
    assert_eq!(said["severity"], "info");
    assert_eq!(said["details"], json!({"scope": "stored"}));
}

#[test]
fn the_stereogenic_scope_adds_units_found_stereogenic_with_nothing_stored() {
    let (app, _directory) = application();
    let (built, _) = smiles(&app, SELECTION);
    let reply = call(
        &app,
        "scan",
        &request(&built, &json!({"scope": "stereogenic"})),
    );
    assert_eq!(reply["result"]["scope"], "stereogenic");
    assert_eq!(reply["outcome"], "ok");
    assert_eq!(ids(&reply, "atoms"), ["a2", "a4"]);
    assert_eq!(text(item(&reply, "atoms", "a2")), "R");
    let unspecified = item(&reply, "atoms", "a4");
    assert_eq!(text(unspecified), "unspecified");
    assert_eq!(unspecified["stored"], Value::Null);
    // Iodine, bromine, then the two carbons: the ranking is there although nothing is stored.
    assert_eq!(
        unspecified["priority"],
        json!([{"neighbor": "a6", "rank": 1}, {"neighbor": "a5", "rank": 2},
               {"neighbor": "a2", "rank": 3}, {"neighbor": "a7", "rank": 4}])
    );
    assert_eq!(ids(&reply, "bonds"), ["b7"]);
    assert_eq!(text(item(&reply, "bonds", "b7")), "unspecified");
    assert_eq!(item(&reply, "bonds", "b7")["ends"][0]["higher"], "a4");
    // The scan lists what it found; the diagnostic says what it did not look at.
    assert!(
        diagnostic(&reply, "cip_scope")["message"]
            .as_str()
            .unwrap()
            .contains("ring system"),
        "the scan says ring double bonds are not scanned"
    );
}

#[test]
fn the_stereogenic_scan_leaves_out_units_found_not_stereogenic_but_keeps_a_stored_one() {
    let (app, _directory) = application();
    // Isopropanol's C2 ties two methyls; the second document stores a configuration on it.
    let (plain, _) = smiles(&app, "CC(C)O");
    let reply = call(
        &app,
        "scan",
        &request(&plain, &json!({"scope": "stereogenic"})),
    );
    assert_eq!(ids(&reply, "atoms"), Vec::<String>::new());
    assert!(!codes(&reply).contains(&"cip_stored_not_stereogenic"));
    let (stored, _) = smiles(&app, "C[C@H](C)O");
    let reply = call(
        &app,
        "scan",
        &request(&stored, &json!({"scope": "stereogenic"})),
    );
    assert_eq!(ids(&reply, "atoms"), ["a2"]);
    assert_eq!(text(item(&reply, "atoms", "a2")), "none");
}

#[test]
fn naming_items_without_a_scope_lists_only_those() {
    let (app, _directory) = application();
    let (built, _) = smiles(&app, SELECTION);
    let reply = call(&app, "named", &request(&built, &json!({"atoms": ["a4"]})));
    assert_eq!(reply["result"]["scope"], "named");
    // The stored a2 is not listed: naming narrows.
    assert_eq!(ids(&reply, "atoms"), ["a4"]);
    assert_eq!(ids(&reply, "bonds"), Vec::<String>::new());
    assert_eq!(text(item(&reply, "atoms", "a4")), "unspecified");
    let said = diagnostic(&reply, "cip_scope");
    assert_eq!(said["details"], json!({"scope": "named"}));
    // Naming a bond alone narrows the atoms too.
    let reply = call(&app, "bond", &request(&built, &json!({"bonds": ["b7"]})));
    assert_eq!(ids(&reply, "atoms"), Vec::<String>::new());
    assert_eq!(ids(&reply, "bonds"), ["b7"]);
}

#[test]
fn a_named_item_is_always_evaluated_and_listed_with_its_true_state() {
    let (app, _directory) = application();
    let (built, _) = smiles(&app, SELECTION);
    // a1 (fluorine) and a bond that is not double cannot be units: they are listed, not refused.
    let reply = call(
        &app,
        "named",
        &request(
            &built,
            &json!({"atoms": ["a1", "a2"], "bonds": ["b1", "b7"]}),
        ),
    );
    assert_eq!(reply["outcome"], "partial");
    assert_eq!(
        text(item(&reply, "atoms", "a1")),
        "unsupported:cip_not_a_candidate"
    );
    assert_eq!(
        text(item(&reply, "bonds", "b1")),
        "unsupported:cip_not_a_candidate"
    );
    assert_eq!(text(item(&reply, "atoms", "a2")), "R");
    assert_eq!(text(item(&reply, "bonds", "b7")), "unspecified");
    // A unit found not stereogenic is listed when it is named, whatever the scope.
    let (plain, _) = smiles(&app, "CC(C)O");
    for extra in [
        json!({}),
        json!({"scope": "stored"}),
        json!({"scope": "stereogenic"}),
    ] {
        let mut parameters = extra.clone();
        parameters["atoms"] = json!(["a2"]);
        let reply = call(&app, "none", &request(&plain, &parameters));
        assert_eq!(text(item(&reply, "atoms", "a2")), "none", "{extra}");
        assert_eq!(reply["outcome"], "ok", "{extra}");
    }
}

#[test]
fn an_explicit_scope_and_names_list_both_in_ascending_order_with_repeats_merged() {
    let (app, _directory) = application();
    let (built, _) = smiles(&app, SELECTION);
    let reply = call(
        &app,
        "both",
        &request(
            &built,
            &json!({"scope": "stored", "atoms": ["a4", "a4", "a1"], "bonds": ["b7"]}),
        ),
    );
    assert_eq!(reply["result"]["scope"], "stored");
    assert_eq!(ids(&reply, "atoms"), ["a1", "a2", "a4"]);
    assert_eq!(ids(&reply, "bonds"), ["b7"]);
}

#[test]
fn a_request_that_names_nothing_valid_is_refused_with_the_path_of_the_first_fault() {
    let (app, _directory) = application();
    let (built, _) = smiles(&app, SELECTION);
    let refused = |parameters: Value| -> Value {
        let reply = call(&app, "bad", &request(&built, &parameters));
        assert_eq!(reply["outcome"], "error", "{parameters}");
        assert_eq!(reply["result"], Value::Null, "{parameters}");
        assert_eq!(reply["document_revision"], Value::Null, "{parameters}");
        assert_eq!(reply["model_id"], "chemifree-cip-1", "{parameters}");
        let diagnostic = reply["diagnostics"][0].clone();
        assert_eq!(diagnostic["code"], "invalid_request", "{parameters}");
        assert_eq!(
            reply["diagnostics"].as_array().unwrap().len(),
            1,
            "{parameters}"
        );
        diagnostic
    };
    for (parameters, path, value) in [
        (
            json!({"atoms": ["a2", "a4", "b3"]}),
            "parameters.atoms[2]",
            json!("b3"),
        ),
        (
            json!({"atoms": ["a99"]}),
            "parameters.atoms[0]",
            json!("a99"),
        ),
        (
            json!({"atoms": ["a2", "x"]}),
            "parameters.atoms[1]",
            json!("x"),
        ),
        (
            json!({"atoms": ["a01"]}),
            "parameters.atoms[0]",
            json!("a01"),
        ),
        (json!({"atoms": [7]}), "parameters.atoms[0]", json!(7)),
        (
            json!({"bonds": ["b7", "a1"]}),
            "parameters.bonds[1]",
            json!("a1"),
        ),
        (
            json!({"bonds": ["b99"]}),
            "parameters.bonds[0]",
            json!("b99"),
        ),
        (json!({"atoms": "a2"}), "parameters.atoms", json!("a2")),
        (
            json!({"scope": "everything"}),
            "parameters.scope",
            json!("everything"),
        ),
        (json!({"scope": 3}), "parameters.scope", json!(3)),
    ] {
        let diagnostic = refused(parameters.clone());
        assert_eq!(diagnostic["path"], path, "{parameters}");
        assert_eq!(diagnostic["details"]["value"], value, "{parameters}");
    }
    for (parameters, path) in [
        (json!({"atoms": []}), "parameters.atoms"),
        (json!({"bonds": []}), "parameters.bonds"),
    ] {
        assert_eq!(refused(parameters.clone())["path"], path, "{parameters}");
    }
    // A null list is the same as no list.
    let reply = call(
        &app,
        "null",
        &request(
            &built,
            &json!({"atoms": null, "bonds": null, "scope": null}),
        ),
    );
    assert_eq!(reply["outcome"], "ok");
    assert_eq!(reply["result"]["scope"], "stored");
}

#[test]
fn unknown_parameters_and_unknown_documents_are_refused_as_for_any_operation() {
    let (app, _directory) = application();
    let (built, _) = smiles(&app, SELECTION);
    let reply = call(&app, "extra", &request(&built, &json!({"instance": "s1"})));
    assert_eq!(reply["outcome"], "error");
    assert_eq!(reply["diagnostics"][0]["code"], "invalid_request");
    assert_eq!(reply["diagnostics"][0]["path"], "parameters");
    let reply = call(&app, "missing", &json!({"document_id": "no-such-document"}));
    assert_eq!(reply["outcome"], "error");
    assert_eq!(reply["diagnostics"][0]["code"], "document_not_found");
}

// ---------------------------------------------------------------------------------------------
// States, diagnostics and the outcome
// ---------------------------------------------------------------------------------------------

fn edit(app: &App, built: &mut Built, commands: &[Value]) -> Value {
    let reply = transact(app, &built.id, None, built.revision, commands);
    built.revision = reply["document"]["revision"].as_u64().unwrap();
    reply
}

#[test]
fn a_center_or_bond_stored_unknown_is_unknown_and_still_carries_its_ranking() {
    let (app, _directory) = application();
    let (mut built, molecule) = smiles(&app, "Br[C@H](Cl)F");
    edit(
        &app,
        &mut built,
        &[
            json!({"op": "set_atom_configuration", "atom": "a2", "configuration": {
            "kind": "unknown", "neighbors": ["a1", "a3", "a4", "implicit_hydrogen"]}}),
        ],
    );
    let reply = call(&app, "unknown", &request(&built, &json!({})));
    assert_eq!(reply["outcome"], "ok", "unknown is a state, not a loss");
    let center = item(&reply, "atoms", "a2");
    assert_eq!(center["state"], "unknown");
    assert_eq!(center["descriptor"], Value::Null);
    assert_eq!(center["reason"], Value::Null);
    assert_eq!(center["stored"]["kind"], "unknown");
    assert_eq!(center["priority"].as_array().unwrap().len(), 4);
    let _ = molecule;

    let (mut built, molecule) = smiles(&app, "F/C=C\\Cl");
    let bond = bond_id(&built, &molecule, 1, 2);
    edit(
        &app,
        &mut built,
        &[
            json!({"op": "set_bond_configuration", "bond": bond, "configuration": {"kind": "unknown"}}),
        ],
    );
    let reply = call(&app, "unknown", &request(&built, &json!({})));
    let entry = item(&reply, "bonds", &bond);
    assert_eq!(text(entry), "unknown");
    assert_eq!(entry["stored"], json!({"kind": "unknown"}));
    assert_eq!(entry["ends"].as_array().unwrap().len(), 2);
}

#[test]
fn a_configuration_stored_on_a_unit_that_is_not_stereogenic_is_a_warning_naming_the_item() {
    let (app, _directory) = application();
    let (built, _) = smiles(&app, "C[C@H](C)O");
    let reply = call(&app, "center", &request(&built, &json!({})));
    assert_eq!(text(item(&reply, "atoms", "a2")), "none");
    assert_eq!(reply["outcome"], "ok");
    let warning = diagnostic(&reply, "cip_stored_not_stereogenic");
    assert_eq!(warning["severity"], "warning");
    assert_eq!(warning["details"], json!({"item": "atom", "id": "a2"}));
    assert_eq!(warning["path"], Value::Null);

    let (mut built, molecule) = smiles(&app, "CC(C)=CCl");
    let bond = bond_id(&built, &molecule, 1, 3);
    edit(
        &app,
        &mut built,
        &[
            json!({"op": "set_bond_configuration", "bond": bond, "configuration": {"kind": "unknown"}}),
        ],
    );
    let reply = call(&app, "bond", &request(&built, &json!({})));
    assert_eq!(text(item(&reply, "bonds", &bond)), "none");
    assert_eq!(
        diagnostic(&reply, "cip_stored_not_stereogenic")["details"],
        json!({"item": "bond", "id": bond})
    );
    // Named without a stored record, the same units are `none` and warn of nothing.
    let (plain, _) = smiles(&app, "CC(C)O");
    let reply = call(&app, "plain", &request(&plain, &json!({"atoms": ["a2"]})));
    assert_eq!(codes(&reply), ["cip_scope", "cip_not_checked"]);
}

#[test]
fn a_stored_fourth_position_the_valence_model_would_not_name_is_used_and_flagged() {
    let (app, _directory) = application();
    let mut molecule = parse("C[N@](CC)C(C)C");
    molecule.centers[0].neighbors[1] = Neighbor::Hydrogen;
    let built = build(&app, &molecule);
    let reply = call(&app, "slot", &request(&built, &json!({})));
    assert_eq!(text(item(&reply, "atoms", "a2")), "R");
    assert_eq!(
        item(&reply, "atoms", "a2")["stored"]["neighbors"],
        json!(["a1", "a3", "a5", "implicit_hydrogen"])
    );
    let warning = diagnostic(&reply, "cip_slot_unusual");
    assert_eq!(warning["severity"], "warning");
    assert_eq!(warning["details"], json!({"item": "atom", "id": "a2"}));
    // The warnings come first, in item order, then the two statements.
    assert_eq!(
        codes(&reply),
        ["cip_slot_unusual", "cip_scope", "cip_not_checked"]
    );
}

#[test]
fn ambiguous_and_unsupported_items_make_the_outcome_partial_with_one_loss_entry_each() {
    let (app, _directory) = application();
    // Three disconnected molecules in one drawing: a center that is labeled, one that depends on
    // an unspecified ligand double bond, and one only Rule 4 or 5 could settle.
    let mut molecule = parse("N[C@@H](C)C(=O)O");
    let ambiguous = molecule.append(&parse("F[C@H](/C=C\\C)C=CC"));
    let unsupported = molecule.append(&parse("C[C@H]1CC[C@@H](C)CC1"));
    let built = build(&app, &molecule);
    let reply = call(&app, "mixed", &request(&built, &json!({})));
    let id = |index: usize| built.ids[index].clone();
    assert_eq!(reply["outcome"], "partial");
    assert_eq!(text(item(&reply, "atoms", &id(1))), "S");
    assert_eq!(
        text(item(&reply, "atoms", &id(ambiguous + 1))),
        "ambiguous:cip_ligand_stereo_unspecified"
    );
    assert_eq!(
        text(item(&reply, "atoms", &id(unsupported + 1))),
        "unsupported:cip_rule_4_or_5_needed"
    );
    // The refused items carry no ranking; the labeled item is untouched by their refusal.
    assert_eq!(
        item(&reply, "atoms", &id(ambiguous + 1))["priority"],
        Value::Null
    );
    assert_eq!(
        item(&reply, "atoms", &id(unsupported + 1))["decided_by"],
        Value::Null
    );
    assert_eq!(
        item(&reply, "atoms", &id(1))["priority"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    let losses = reply["loss_report"].as_array().unwrap();
    assert_eq!(
        losses.len(),
        3,
        "the ambiguous center and the two centers of the ring"
    );
    let entry = |path: &str| -> &Value {
        losses
            .iter()
            .find(|loss| loss["source_path"] == path)
            .unwrap_or_else(|| panic!("no loss for {path}: {losses:?}"))
    };
    for (path, code) in [
        (
            format!("atoms[{}]", id(ambiguous + 1)),
            "cip_ligand_stereo_unspecified",
        ),
        (
            format!("atoms[{}]", id(unsupported + 1)),
            "cip_rule_4_or_5_needed",
        ),
        (
            format!("atoms[{}]", id(unsupported + 4)),
            "cip_rule_4_or_5_needed",
        ),
    ] {
        let loss = entry(&path);
        assert_eq!(loss["feature"], "cip_descriptor", "{path}");
        assert_eq!(loss["disposition"], "unknown", "{path}");
        assert_eq!(loss["semantic_impact"], "stereo", "{path}");
        assert!(
            loss["reason"].as_str().unwrap().starts_with(code),
            "{path}: {loss}"
        );
    }
}

#[test]
fn an_unsupported_double_bond_is_a_loss_entry_on_its_bond_path() {
    let (app, _directory) = application();
    // The ligand propenyl group of the second end is unspecified: the bond depends on it.
    let molecule = parse("F/C=C(/C=C\\C)C=CC");
    let built = build(&app, &molecule);
    let bond = bond_id(&built, &molecule, 1, 2);
    let reply = call(&app, "bond", &request(&built, &json!({})));
    assert_eq!(reply["outcome"], "partial");
    assert_eq!(
        text(item(&reply, "bonds", &bond)),
        "ambiguous:cip_ligand_stereo_unspecified"
    );
    let losses = reply["loss_report"].as_array().unwrap();
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0]["source_path"], format!("bonds[{bond}]"));
}

#[test]
fn every_state_the_outcome_calls_ok_leaves_the_loss_report_empty() {
    let (app, _directory) = application();
    // assigned, none (stored), unknown and unspecified together.
    let mut molecule = parse("N[C@@H](C)C(=O)O");
    let none = molecule.append(&parse("C[C@H](C)O"));
    let free = molecule.append(&parse("F[C@H](Cl)C(Br)(I)C=CC"));
    let mut built = build(&app, &molecule);
    let unknown = molecule.atoms.len();
    let reply = edit(
        &app,
        &mut built,
        &[
            json!({"op": "add_atom", "element": "C", "position": {"x": 0, "y": 50}}),
            json!({"op": "add_atom", "element": "F", "position": {"x": 10, "y": 50}, "bond_to": {"atom": "$0", "order": "single"}}),
            json!({"op": "add_atom", "element": "Cl", "position": {"x": 20, "y": 50}, "bond_to": {"atom": "$0", "order": "single"}}),
            json!({"op": "add_atom", "element": "Br", "position": {"x": 30, "y": 50}, "bond_to": {"atom": "$0", "order": "single"}}),
            json!({"op": "set_atom_configuration", "atom": format!("a{}", unknown + 1), "configuration": {
                "kind": "unknown", "neighbors": [format!("a{}", unknown + 2), format!("a{}", unknown + 3), format!("a{}", unknown + 4), "implicit_hydrogen"]}}),
        ],
    );
    assert!(reply.get("code").is_none());
    let id = |index: usize| format!("a{}", index + 1);
    let reply = call(
        &app,
        "ok",
        &request(
            &built,
            &json!({"scope": "stereogenic", "atoms": [id(none + 1)]}),
        ),
    );
    assert_eq!(text(item(&reply, "atoms", &id(1))), "S");
    assert_eq!(text(item(&reply, "atoms", &id(none + 1))), "none");
    assert_eq!(text(item(&reply, "atoms", &id(unknown))), "unknown");
    assert_eq!(text(item(&reply, "atoms", &id(free + 3))), "unspecified");
    assert_eq!(reply["outcome"], "ok");
    assert_eq!(reply["loss_report"], json!([]));
}

// ---------------------------------------------------------------------------------------------
// A label follows the stored configuration and the graph, and nothing else
// ---------------------------------------------------------------------------------------------

/// The whole state a document has, as the snapshot operation reports it.
fn whole(app: &App, built: &Built) -> Value {
    serde_json::to_value(app.document(&built.id).unwrap()).unwrap()
}

#[test]
fn the_operation_changes_nothing_a_document_holds_and_answers_a_repeated_request_afresh() {
    let (app, _directory) = application();
    let (mut built, molecule) = smiles(&app, "F[C@H](/C=C\\C)/C=C/C");
    // A wedge on the center's bond, a selection, an undo step: all must be as they were.
    edit(
        &app,
        &mut built,
        &[
            json!({"op": "set_bond_mark", "bond": "b1", "mark": {"kind": "wedge", "narrow": "begin"}}),
        ],
    );
    let before = whole(&app, &built);
    let validate = |request: &str| {
        serde_json::to_value(
            app.execute(
                json!({"contract": 1, "request_id": request, "profile": "preserve-semantics",
                   "operation": "chem.validate", "parameters": {"document_id": built.id}})
                .to_string()
                .as_bytes(),
            ),
        )
        .unwrap()
    };
    let validate_before = validate("v1");
    let first = call(
        &app,
        "same",
        &request(&built, &json!({"scope": "stereogenic"})),
    );
    let second = call(
        &app,
        "same",
        &request(&built, &json!({"scope": "stereogenic"})),
    );
    assert_eq!(
        first, second,
        "a repeated request id is computed again, identically"
    );
    assert_eq!(
        whole(&app, &built),
        before,
        "document, revision, history, selection, dirty state"
    );
    assert_eq!(validate("v1"), validate_before);
    assert_eq!(first["document_revision"], before["document"]["revision"]);
    assert_eq!(first["result"]["snapshot"]["document"], before["document"]);
    assert_eq!(first["result"]["snapshot"]["session"], before["session"]);
    // Undo and redo take the same step back and forth; the label is the same each time.
    let labels = |reply: &Value| reply["result"]["atoms"].clone();
    app.undo(
        &built.id,
        Some("undo"),
        before["document"]["revision"].as_u64().unwrap(),
    )
    .unwrap();
    let undone = call(
        &app,
        "undone",
        &request(&built, &json!({"scope": "stereogenic"})),
    );
    assert_eq!(labels(&undone), labels(&first), "a mark is not an input");
    let _ = molecule;
}

#[test]
fn a_mark_does_not_change_a_label_and_a_stored_turn_does() {
    let (app, _directory) = application();
    let (mut built, _) = smiles(&app, "Br[C@H](Cl)F");
    let labels = |app: &App, request_id: &str, built: &Built| {
        call(app, request_id, &request(built, &json!({})))["result"]["atoms"].clone()
    };
    let first = labels(&app, "a", &built);
    assert_eq!(first[0]["descriptor"], "S");
    // Drawing marks every way round does not move the label of the stored statement.
    for (index, mark) in [
        json!({"kind": "wedge", "narrow": "begin"}),
        json!({"kind": "hash", "narrow": "begin"}),
        json!({"kind": "wavy"}),
    ]
    .into_iter()
    .enumerate()
    {
        edit(
            &app,
            &mut built,
            &[json!({"op": "set_bond_mark", "bond": "b2", "mark": mark})],
        );
        assert_eq!(
            labels(&app, &format!("m{index}"), &built),
            first,
            "mark {index}"
        );
    }
    // Stating the other turn changes it, and only the descriptor and the stored turn change.
    edit(
        &app,
        &mut built,
        &[
            json!({"op": "set_atom_configuration", "atom": "a2", "configuration": {
            "kind": "tetrahedral", "neighbors": ["a1", "a3", "a4", "implicit_hydrogen"],
            "turn": "clockwise"}}),
        ],
    );
    let after = labels(&app, "after", &built);
    assert_eq!(after[0]["descriptor"], "R");
    assert_eq!(after[0]["priority"], first[0]["priority"]);
    assert_eq!(after[0]["stored"]["turn"], "clockwise");
    assert_eq!(first[0]["stored"]["turn"], "anticlockwise");
}

#[test]
fn an_isotope_a_bond_order_and_a_charge_edited_after_a_first_label_are_read_afresh() {
    let (app, _directory) = application();

    // Two methyls tie; marking one as carbon-13 breaks the tie by Rule 2. The expected label is
    // the fixture's `Cl[C@H]([13CH3])C`, S.
    let (mut built, _) = smiles(&app, "Cl[C@H](C)C");
    let first = call(&app, "iso0", &request(&built, &json!({})));
    assert_eq!(text(item(&first, "atoms", "a2")), "none");
    edit(
        &app,
        &mut built,
        &[json!({"op": "update_atom", "atom": "a3", "isotope": 13})],
    );
    let second = call(&app, "iso1", &request(&built, &json!({})));
    assert_eq!(text(item(&second, "atoms", "a2")), "S");
    assert_eq!(item(&second, "atoms", "a2")["decided_by"], "2");
    edit(
        &app,
        &mut built,
        &[json!({"op": "update_atom", "atom": "a3", "isotope": null})],
    );
    let third = call(&app, "iso2", &request(&built, &json!({})));
    assert_eq!(text(item(&third, "atoms", "a2")), "none");

    // A vinyl group outranks an ethyl one; making the double bond single ties them.
    let (mut built, molecule) = smiles(&app, "Cl[C@H](CC)C=C");
    let first = call(&app, "bond0", &request(&built, &json!({})));
    assert_eq!(text(item(&first, "atoms", "a2")), "R");
    let vinyl = bond_id(&built, &molecule, 4, 5);
    edit(
        &app,
        &mut built,
        &[json!({"op": "update_bond", "bond": vinyl, "order": "single"})],
    );
    let second = call(&app, "bond1", &request(&built, &json!({})));
    assert_eq!(text(item(&second, "atoms", "a2")), "none");
    assert!(codes(&second).contains(&"cip_stored_not_stereogenic"));

    // A charge on the center takes it out of the structure the model reads: the stored record
    // stays and is reported unsupported, not dropped and not labeled.
    let (mut built, _) = smiles(&app, "C[N@](CC)C(C)C");
    let first = call(&app, "charge0", &request(&built, &json!({})));
    assert_eq!(text(item(&first, "atoms", "a2")), "R");
    edit(
        &app,
        &mut built,
        &[json!({"op": "update_atom", "atom": "a2", "charge": 1})],
    );
    let second = call(&app, "charge1", &request(&built, &json!({})));
    assert_eq!(
        text(item(&second, "atoms", "a2")),
        "unsupported:cip_center_structure"
    );
    assert_eq!(second["outcome"], "partial");
}

#[test]
fn moving_rotating_and_reflecting_atoms_do_not_change_a_label() {
    let (app, _directory) = application();
    let (mut built, molecule) = smiles(&app, "N[C@@H](C)C(=O)O");
    let labels = |app: &App, request_id: &str, built: &Built| {
        call(app, request_id, &request(built, &json!({})))["result"]["atoms"].clone()
    };
    let first = labels(&app, "before", &built);
    assert_eq!(first[0]["descriptor"], "S");
    // The reader puts atom `i` at (10 i, 0). Each edit is one transaction on every atom.
    let per_atom = |delta: &dyn Fn(f64) -> (f64, f64)| -> Vec<Value> {
        (0..molecule.atoms.len())
            .map(|i| {
                let (x, y) = delta(10.0 * i as f64);
                json!({"op": "move_atoms", "atoms": [format!("a{}", i + 1)], "delta": {"x": x, "y": y}})
            })
            .collect()
    };
    for (name, delta) in [
        (
            "translation",
            Box::new(|_| (35.0, -12.0)) as Box<dyn Fn(f64) -> (f64, f64)>,
        ),
        ("mirror", Box::new(|x| (-2.0 * x, 0.0))),
        ("quarter-turn", Box::new(|x| (-x, x))),
    ] {
        edit(&app, &mut built, &per_atom(&*delta));
        let reply = call(&app, name, &request(&built, &json!({})));
        assert_eq!(reply["result"]["atoms"], first, "{name}: {reply}");
    }
}

// ---------------------------------------------------------------------------------------------
// Work limits, interrupts and a replaced labeler
// ---------------------------------------------------------------------------------------------

use chemifree_application::cip::{BuiltinCip, CipLabeler};
use chemifree_domain::budget::{CancelFlag, Interrupt};
use chemifree_domain::cip::{
    BondReport, CenterReport, Descriptor, Limits, Reason, State, Stop, Warning,
};
use chemifree_domain::document::{AtomId, BondId};
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::stereo::Slot;

/// The built-in rules under other limits, another name and a shorter rule list.
struct Limited {
    limits: Limits,
}

impl CipLabeler for Limited {
    fn model_id(&self) -> &'static str {
        "limited-test-1"
    }

    fn rules(&self) -> &'static [&'static str] {
        &["1a"]
    }

    fn limits(&self) -> Limits {
        self.limits
    }

    fn label_centers(
        &self,
        graph: &MoleculeGraph,
        atoms: &[AtomId],
        limits: Limits,
        interrupt: &dyn Interrupt,
    ) -> Result<Vec<CenterReport>, Stop> {
        BuiltinCip.label_centers(graph, atoms, limits, interrupt)
    }

    fn label_double_bonds(
        &self,
        graph: &MoleculeGraph,
        bonds: &[BondId],
        limits: Limits,
        interrupt: &dyn Interrupt,
    ) -> Result<Vec<BondReport>, Stop> {
        BuiltinCip.label_double_bonds(graph, bonds, limits, interrupt)
    }
}

fn application_with(labeler: impl CipLabeler + 'static) -> (App, TempDir) {
    let directory = tempfile::tempdir().unwrap();
    let app = Application::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    )
    .with_cip_labeler(labeler);
    (app, directory)
}

/// A square grid of carbons: every inner atom has four neighbors and an exploding digraph.
fn grid(side: usize) -> Molecule {
    let mut molecule = Molecule::default();
    for _ in 0..side * side {
        molecule.atoms.push(support::element("C"));
    }
    for row in 0..side {
        for column in 0..side {
            let atom = row * side + column;
            if column + 1 < side {
                molecule.bonds.push((atom, atom + 1, BondOrder::Single));
            }
            if row + 1 < side {
                molecule.bonds.push((atom, atom + side, BondOrder::Single));
            }
        }
    }
    molecule
}

#[test]
fn a_request_over_the_node_limit_is_refused_whole_with_the_numbers() {
    let (app, _directory) = application();
    let side = 13;
    let molecule = grid(side);
    let built = build(&app, &molecule);
    let inner: Vec<String> = (1..side - 1)
        .flat_map(|row| (1..side - 1).map(move |column| row * side + column))
        .map(|atom| built.ids[atom].clone())
        .collect();
    let before = whole(&app, &built);
    let reply = call(&app, "big", &request(&built, &json!({"atoms": inner})));
    assert_eq!(reply["outcome"], "error");
    assert_eq!(reply["result"], Value::Null, "no partial list");
    assert_eq!(reply["loss_report"], json!([]));
    assert_eq!(reply["model_id"], "chemifree-cip-1");
    let refusal = &reply["diagnostics"][0];
    assert_eq!(refusal["code"], "resource_limit");
    assert_eq!(refusal["severity"], "error");
    assert_eq!(refusal["path"], "parameters.atoms");
    assert_eq!(refusal["details"]["limit"], 2_000_000);
    assert_eq!(refusal["details"]["items"], 121);
    assert_eq!(refusal["details"]["item_limit"], 20_000);
    assert_eq!(refusal["details"]["parameter"], "atoms");
    assert_eq!(reply["diagnostics"].as_array().unwrap().len(), 1);
    assert_eq!(whole(&app, &built), before);
    // Naming fewer atoms is the way out, and each of them is a limit of its own, not a label.
    let reply = call(
        &app,
        "small",
        &request(&built, &json!({"atoms": [inner_first(&built, side)]})),
    );
    assert_eq!(reply["outcome"], "partial");
    assert_eq!(
        text(item(&reply, "atoms", &inner_first(&built, side))),
        "unsupported:cip_digraph_limit"
    );
}

fn inner_first(built: &Built, side: usize) -> String {
    built.ids[side + 1].clone()
}

#[test]
fn the_limits_are_those_of_the_labeler_and_a_reached_item_cap_is_unsupported_never_none() {
    let tiny = Limits {
        item_nodes: 3,
        request_nodes: usize::MAX,
    };
    let (app, _directory) = application_with(Limited { limits: tiny });
    for (text_, uncapped) in [
        ("C[C@H](C)O", "none"),
        ("O[C@H]1CCCCC1", "none"),
        ("O[C@H]([C@H](O)C(=O)O)C(=O)O", "R"),
    ] {
        let (built, _) = smiles(&app, text_);
        let reply = call(&app, "capped", &request(&built, &json!({})));
        assert_eq!(reply["outcome"], "partial", "{text_}");
        assert_eq!(reply["model_id"], "limited-test-1");
        assert_eq!(reply["result"]["model"], "limited-test-1");
        assert_eq!(reply["result"]["rules"], json!(["1a"]));
        let first = &reply["result"]["atoms"][0];
        assert_eq!(
            text(first),
            "unsupported:cip_digraph_limit",
            "{text_} (uncapped {uncapped})"
        );
        assert_eq!(first["priority"], Value::Null);
    }
    // The request limit is shared by centers and double bonds: each alone fits, together they do not.
    let molecule = parse("F[C@H](/C=C\\C)/C=C/C");
    let (_, ids_only) = (0, build(&application().0, &molecule).ids);
    let _ = ids_only;
    let per_item = Limits {
        item_nodes: 20_000,
        request_nodes: 1,
    };
    let (app, _directory) = application_with(Limited { limits: per_item });
    let built = build(&app, &molecule);
    let reply = call(&app, "request", &request(&built, &json!({})));
    assert_eq!(reply["diagnostics"][0]["code"], "resource_limit");
    assert_eq!(reply["diagnostics"][0]["details"]["limit"], 1);
}

#[test]
fn a_cancelled_request_changes_nothing_and_says_so() {
    let (app, _directory) = application();
    let (built, _) = smiles(&app, "N[C@@H](C)C(=O)O");
    let before = whole(&app, &built);
    let flag = CancelFlag::new();
    flag.cancel();
    let body = json!({"contract": 1, "request_id": "stop", "profile": "preserve-semantics",
                      "operation": "chem.cip", "parameters": request(&built, &json!({}))});
    let reply = serde_json::to_value(app.execute_with(body.to_string().as_bytes(), &flag)).unwrap();
    assert_eq!(reply["outcome"], "error");
    assert_eq!(reply["result"], Value::Null);
    assert_eq!(reply["diagnostics"][0]["code"], "cancelled");
    assert_eq!(whole(&app, &built), before);
    // Without the flag the same request runs.
    let reply = call(&app, "go", &request(&built, &json!({})));
    assert_eq!(reply["outcome"], "ok");
    let scan = body_with_scope(&built);
    let reply = serde_json::to_value(app.execute_with(scan.to_string().as_bytes(), &flag)).unwrap();
    assert_eq!(
        reply["diagnostics"][0]["code"], "cancelled",
        "the candidate scan is interruptible too"
    );
}

fn body_with_scope(built: &Built) -> Value {
    json!({"contract": 1, "request_id": "scan", "profile": "preserve-semantics",
           "operation": "chem.cip", "parameters": request(built, &json!({"scope": "stereogenic"}))})
}

type CenterFix = Box<dyn Fn(&mut Vec<CenterReport>) + Send + Sync>;
type BondFix = Box<dyn Fn(&mut Vec<BondReport>) + Send + Sync>;

/// The built-in rules, then whatever a test does to their answer: a labeler that lies.
struct Tampered {
    centers: CenterFix,
    bonds: BondFix,
}

impl Tampered {
    fn honest() -> Self {
        Self {
            centers: Box::new(|_| {}),
            bonds: Box::new(|_| {}),
        }
    }
}

impl CipLabeler for Tampered {
    fn model_id(&self) -> &'static str {
        "tampered-test-1"
    }

    fn rules(&self) -> &'static [&'static str] {
        &["1a", "1b"]
    }

    fn limits(&self) -> Limits {
        Limits::default()
    }

    fn label_centers(
        &self,
        graph: &MoleculeGraph,
        atoms: &[AtomId],
        limits: Limits,
        interrupt: &dyn Interrupt,
    ) -> Result<Vec<CenterReport>, Stop> {
        let mut reports = BuiltinCip.label_centers(graph, atoms, limits, interrupt)?;
        (self.centers)(&mut reports);
        Ok(reports)
    }

    fn label_double_bonds(
        &self,
        graph: &MoleculeGraph,
        bonds: &[BondId],
        limits: Limits,
        interrupt: &dyn Interrupt,
    ) -> Result<Vec<BondReport>, Stop> {
        let mut reports = BuiltinCip.label_double_bonds(graph, bonds, limits, interrupt)?;
        (self.bonds)(&mut reports);
        Ok(reports)
    }
}

/// L-alanine (a2, assigned S), isopropanol with a stored configuration (a8, `none` with a
/// warning), `Cl/C=C\F` (a12=a13 is `b?`, assigned Z) and 2-methylbut-... `CC(C)=CCl` with a
/// stored unknown (none with a warning). The atoms and bonds are named in one request.
struct Liar {
    app: App,
    _directory: TempDir,
    parameters: Value,
    alanine: String,
    isopropanol: String,
    z_bond: String,
    none_bond: String,
}

fn liar(tamper: Tampered) -> Liar {
    let (app, directory) = application_with(tamper);
    let mut molecule = parse("N[C@@H](C)C(=O)O");
    let propanol = molecule.append(&parse("C[C@H](C)O"));
    let alkene = molecule.append(&parse("Cl/C=C\\F"));
    let tied = molecule.append(&parse("CC(C)=CCl"));
    let mut built = build(&app, &molecule);
    let none_bond = built.bond_ids[molecule
        .bonds
        .iter()
        .position(|&(a, b, _)| (a, b) == (tied + 1, tied + 3))
        .unwrap()]
    .clone();
    edit(
        &app,
        &mut built,
        &[
            json!({"op": "set_bond_configuration", "bond": none_bond, "configuration": {"kind": "unknown"}}),
        ],
    );
    let z_bond = built.bond_ids[molecule
        .bonds
        .iter()
        .position(|&(a, b, _)| (a, b) == (alkene + 1, alkene + 2))
        .unwrap()]
    .clone();
    let alanine = built.ids[1].clone();
    let isopropanol = built.ids[propanol + 1].clone();
    let parameters = request(
        &built,
        &json!({"atoms": [alanine, isopropanol], "bonds": [z_bond, none_bond]}),
    );
    Liar {
        app,
        _directory: directory,
        parameters,
        alanine,
        isopropanol,
        z_bond,
        none_bond,
    }
}

impl Liar {
    fn ask(&self) -> Value {
        call(&self.app, "liar", &self.parameters)
    }
}

fn refused(reply: &Value) -> (&str, &str) {
    assert_eq!(reply["outcome"], "error", "{reply}");
    assert_eq!(
        reply["result"],
        Value::Null,
        "nothing of a refused answer is shown"
    );
    assert_eq!(reply["loss_report"], json!([]));
    assert_eq!(reply["model_id"], "tampered-test-1");
    let only = &reply["diagnostics"];
    assert_eq!(only.as_array().unwrap().len(), 1, "{reply}");
    assert_eq!(only[0]["code"], "cip_labeler_invalid", "{reply}");
    assert_eq!(only[0]["severity"], "error");
    (
        only[0]["details"]["problem"].as_str().unwrap(),
        only[0]["details"]["unit"].as_str().unwrap(),
    )
}

fn replace_center(reports: &mut [CenterReport], atom: &str, change: impl Fn(&mut CenterReport)) {
    let report = reports
        .iter_mut()
        .find(|report| report.atom.to_string() == atom)
        .unwrap();
    change(report);
}

fn replace_bond(reports: &mut [BondReport], bond: &str, change: impl Fn(&mut BondReport)) {
    let report = reports
        .iter_mut()
        .find(|report| report.bond.to_string() == bond)
        .unwrap();
    change(report);
}

#[test]
fn an_honest_replacement_labeler_is_used_and_named_in_the_reply() {
    let liar = liar(Tampered::honest());
    let reply = liar.ask();
    assert_eq!(reply["outcome"], "ok", "{reply}");
    assert_eq!(reply["model_id"], "tampered-test-1");
    assert_eq!(reply["result"]["model"], "tampered-test-1");
    assert_eq!(reply["result"]["rules"], json!(["1a", "1b"]));
    assert_eq!(text(item(&reply, "atoms", &liar.alanine)), "S");
    assert_eq!(text(item(&reply, "atoms", &liar.isopropanol)), "none");
    assert_eq!(text(item(&reply, "bonds", &liar.z_bond)), "Z");
    assert_eq!(text(item(&reply, "bonds", &liar.none_bond)), "none");
}

#[test]
fn the_built_in_labeler_is_there_without_being_installed() {
    let (app, _directory) = application();
    let (built, _) = smiles(&app, "N[C@@H](C)C(=O)O");
    let reply = call(&app, "default", &request(&built, &json!({})));
    assert_eq!(reply["model_id"], "chemifree-cip-1");
    assert_eq!(reply["result"]["rules"], json!(["1a", "1b", "2", "3"]));
}

#[test]
fn an_answer_that_does_not_name_each_requested_unit_once_in_order_is_refused() {
    type Fix = fn(&mut Vec<CenterReport>);
    let fixes: [(&str, Fix); 3] = [
        ("one short", |reports| {
            reports.pop();
        }),
        ("swapped", |reports| reports.swap(0, 1)),
        ("a unit twice", |reports| {
            let again = reports[0].clone();
            reports.push(again);
        }),
    ];
    for (name, fix) in fixes {
        let liar = liar(Tampered {
            centers: Box::new(fix),
            bonds: Box::new(|_| {}),
        });
        assert_eq!(refused(&liar.ask()), ("units", ""), "{name}");
    }
    let liar = liar_with_bonds(|reports| reports.reverse());
    assert_eq!(refused(&liar.ask()), ("units", ""), "bonds reversed");
}

fn liar_with_bonds(fix: impl Fn(&mut Vec<BondReport>) + Send + Sync + 'static) -> Liar {
    liar(Tampered {
        centers: Box::new(|_| {}),
        bonds: Box::new(fix),
    })
}

fn liar_with_centers(fix: impl Fn(&mut Vec<CenterReport>) + Send + Sync + 'static) -> Liar {
    liar(Tampered {
        centers: Box::new(fix),
        bonds: Box::new(|_| {}),
    })
}

#[test]
fn a_state_that_does_not_fit_the_unit_or_its_stored_record_is_refused() {
    let alanine = "a2";
    // A descriptor of the other kind of unit.
    let reply = liar_with_centers(move |reports| {
        replace_center(reports, alanine, |report| {
            report.state = State::Assigned(Descriptor::E)
        });
    })
    .ask();
    assert_eq!(refused(&reply), ("descriptor", alanine));
    let reply = liar_with_bonds({
        let bond = liar(Tampered::honest()).z_bond;
        move |reports| {
            replace_bond(reports, &bond, |report| {
                report.state = State::Assigned(Descriptor::R)
            })
        }
    })
    .ask();
    assert_eq!(refused(&reply).0, "descriptor");
    // Each state against the record the document holds for it (alanine's is a stated turn).
    for (name, state) in [
        ("unknown", State::Unknown),
        ("unspecified", State::Unspecified),
    ] {
        let reply = liar_with_centers(move |reports| {
            replace_center(reports, alanine, |report| report.state = state);
        })
        .ask();
        assert_eq!(
            refused(&reply),
            ("stored", alanine),
            "{name} over a stated turn"
        );
    }
    // A record the document does not hold, or none where it holds one.
    let reply = liar_with_centers(move |reports| {
        replace_center(reports, alanine, |report| report.stored = None);
    })
    .ask();
    assert_eq!(refused(&reply), ("stored", alanine));
    let reply = liar_with_bonds({
        let bond = liar(Tampered::honest()).none_bond;
        move |reports| replace_bond(reports, &bond, |report| report.stored = None)
    })
    .ask();
    assert_eq!(refused(&reply).0, "stored");
    // The unit that is `assigned` while the document holds `unknown`.
    let reply = liar_with_bonds({
        let bond = liar(Tampered::honest()).none_bond;
        move |reports| {
            replace_bond(reports, &bond, |report| {
                report.state = State::Assigned(Descriptor::Z)
            });
        }
    })
    .ask();
    assert_eq!(refused(&reply).0, "stored");
}

#[test]
fn a_reason_that_does_not_fit_its_state_is_refused() {
    let alanine = "a2";
    for (name, state) in [
        (
            "ambiguous for another reason",
            State::Ambiguous(Reason::Rule4Or5Needed),
        ),
        (
            "unsupported for the one reason that is ambiguity",
            State::Unsupported(Reason::LigandStereoUnspecified),
        ),
    ] {
        let reply = liar_with_centers(move |reports| {
            replace_center(reports, alanine, |report| {
                report.state = state;
                report.priority = None;
                report.decided_by = None;
            });
        })
        .ask();
        assert_eq!(refused(&reply), ("reason", alanine), "{name}");
    }
    // The honest pairings pass.
    for state in [
        State::Ambiguous(Reason::LigandStereoUnspecified),
        State::Unsupported(Reason::Rule4Or5Needed),
    ] {
        let reply = liar_with_centers(move |reports| {
            replace_center(reports, alanine, |report| {
                report.state = state;
                report.priority = None;
                report.decided_by = None;
            });
        })
        .ask();
        assert_eq!(reply["outcome"], "partial", "{state:?}");
    }
}

#[test]
fn a_ranking_that_is_not_four_positions_ranked_one_to_four_is_refused() {
    let alanine = "a2";
    type Fix = fn(&mut CenterReport);
    let fixes: [(&str, Fix); 9] = [
        ("a tied rank", |report| {
            report.priority.as_mut().unwrap()[1].1 = 1;
        }),
        ("rank zero", |report| {
            report.priority.as_mut().unwrap()[0].1 = 0;
        }),
        ("rank five", |report| {
            report.priority.as_mut().unwrap()[3].1 = 5;
        }),
        ("three positions", |report| {
            report.priority.as_mut().unwrap().pop();
        }),
        ("a position twice", |report| {
            let priority = report.priority.as_mut().unwrap();
            priority[3].0 = priority[2].0;
        }),
        ("a position the record does not hold", |report| {
            report.priority.as_mut().unwrap()[3].0 = Slot::LonePair;
        }),
        ("an atom that is not in the document", |report| {
            report.priority.as_mut().unwrap()[0].0 = Slot::Atom(AtomId::from_serial(999).unwrap());
        }),
        ("no rule that decided", |report| report.decided_by = None),
        ("no ranking for an assigned unit", |report| {
            report.priority = None;
            report.decided_by = None;
        }),
    ];
    for (name, fix) in fixes {
        let reply = liar_with_centers(move |reports| replace_center(reports, alanine, fix)).ask();
        assert_eq!(refused(&reply), ("ranking", alanine), "{name}");
    }
    // A ranking on a unit the model could not decide.
    let reply = liar_with_centers(|reports| {
        let ranked = reports[0].priority.clone();
        replace_center(reports, "a8", |report| report.priority = ranked.clone());
    })
    .ask();
    assert_eq!(refused(&reply), ("ranking", "a8"));
}

#[test]
fn ends_that_are_not_the_ends_of_the_bond_are_refused() {
    let z_bond = liar(Tampered::honest()).z_bond;
    type Fix = fn(&mut BondReport);
    let fixes: [(&str, Fix); 3] = [
        ("ends in the other order", |report| {
            report.ends.as_mut().unwrap().swap(0, 1);
        }),
        ("an assigned bond without ends", |report| report.ends = None),
        ("an atom that is not in the document", |report| {
            report.ends.as_mut().unwrap()[0].higher = Slot::Atom(AtomId::from_serial(999).unwrap());
        }),
    ];
    for (name, fix) in fixes {
        let bond = z_bond.clone();
        let reply = liar_with_bonds(move |reports| replace_bond(reports, &bond, fix)).ask();
        assert_eq!(refused(&reply), ("ranking", z_bond.as_str()), "{name}");
    }
    let none_bond = liar(Tampered::honest()).none_bond;
    let ends = liar(Tampered::honest()).ask()["result"]["bonds"].clone();
    let _ = ends;
    let bond = none_bond.clone();
    let reply = liar_with_bonds(move |reports| {
        let ends = reports
            .iter()
            .find(|report| report.ends.is_some())
            .and_then(|report| report.ends);
        replace_bond(reports, &bond, |report| report.ends = ends);
    })
    .ask();
    assert_eq!(
        refused(&reply),
        ("ranking", none_bond.as_str()),
        "ends on a bond that is not stereogenic"
    );
}

#[test]
fn a_warning_the_state_and_record_do_not_allow_is_refused() {
    let alanine = "a2";
    let reply = liar_with_centers(move |reports| {
        replace_center(reports, alanine, |report| {
            report.warnings.push(Warning::StoredNotStereogenic)
        });
    })
    .ask();
    assert_eq!(refused(&reply), ("warnings", alanine));
    let reply = liar_with_centers(move |reports| {
        replace_center(reports, alanine, |report| {
            report.warnings = vec![Warning::SlotUnusual, Warning::SlotUnusual];
        });
    })
    .ask();
    assert_eq!(refused(&reply), ("warnings", alanine));
    // The warning a stored record on a unit that is not stereogenic must carry cannot be left out.
    let reply = liar_with_centers(|reports| {
        replace_center(reports, "a8", |report| report.warnings.clear())
    })
    .ask();
    assert_eq!(refused(&reply), ("warnings", "a8"));
    let none_bond = liar(Tampered::honest()).none_bond;
    let bond = none_bond.clone();
    let reply = liar_with_bonds(move |reports| {
        replace_bond(reports, &bond, |report| report.warnings.clear())
    })
    .ask();
    assert_eq!(refused(&reply), ("warnings", none_bond.as_str()));
    let z_bond = liar(Tampered::honest()).z_bond;
    let bond = z_bond.clone();
    let reply = liar_with_bonds(move |reports| {
        replace_bond(reports, &bond, |report| {
            report.warnings.push(Warning::SlotUnusual)
        });
    })
    .ask();
    assert_eq!(
        refused(&reply),
        ("warnings", z_bond.as_str()),
        "a bond has no fourth position"
    );
}

// ---------------------------------------------------------------------------------------------
// Every reason a unit can be refused under, reached through the operation
// ---------------------------------------------------------------------------------------------

/// One molecule per reason the operation can give, with the unit asked for and the label the
/// reason belongs to. `cip_ranking_inconsistent` is the one reason with no case: the model
/// reports it for pairwise results that are not an order, which no input has produced.
#[test]
fn each_reason_the_operation_can_give_is_pinned_by_a_molecule_that_reaches_it() {
    /// The unit asked for: an atom (`second` unused) or the bond between two atoms.
    struct Case {
        molecule: Molecule,
        bond: bool,
        first: usize,
        second: usize,
    }
    let atom = |molecule: Molecule, first: usize| Case {
        molecule,
        bond: false,
        first,
        second: 0,
    };
    let bond = |smiles: &str, first: usize, second: usize| Case {
        molecule: parse(smiles),
        bond: true,
        first,
        second,
    };
    // Two CH2 arms tie at the first sphere, so the comparison must expand their atoms.
    let arm = |edit: &dyn Fn(&mut Molecule)| {
        let mut molecule = parse("Cl[C@H](CC)CC");
        edit(&mut molecule);
        atom(molecule, 1)
    };
    let plain = |smiles: &str, first: usize| atom(parse(smiles), first);
    let mut seen = Vec::new();
    let cases = vec![
        ("cip_center_structure", {
            let mut molecule = parse("N[C@@H](C)C(=O)O");
            molecule.atoms[1].hydrogens = Hydrogens::Specified(2);
            atom(molecule, 1)
        }),
        ("cip_double_bond_structure", bond("C/C=C=C/C", 1, 2)),
        (
            "cip_query_atom",
            arm(&|m| m.atoms[2].kind = AtomKind::Query(QueryAtom::Any)),
        ),
        ("cip_bond_kind", arm(&|m| m.bonds[2].2 = BondOrder::Dative)),
        (
            "cip_attachment_point",
            arm(&|m| m.atoms[2].attachment_points = AttachmentPoints::new(&[1]).unwrap()),
        ),
        (
            "cip_hydrogens_unknown",
            arm(&|m| m.atoms[2].hydrogens = Hydrogens::Unknown),
        ),
        ("cip_aromatic_system", plain("C[C@H](O)c1ccccn1", 1)),
        ("cip_mancude_rule_1b", plain("O[C@H](c1ccccc1)c1ccccc1", 1)),
        (
            "cip_rule_1b_duplicate_distance",
            plain("C12=CC[C@@]1(C)C(C=C)=C2", 3),
        ),
        ("cip_mass_number", plain("Cl[C@H]([12CH3])C", 1)),
        ("cip_rule_4_or_5_needed", plain("C[C@H]1CC[C@@H](C)CC1", 1)),
        ("cip_stereo_cycle", bond("CC=C1C(=CC)C(=CC)C1=CC", 1, 2)),
        (
            "cip_rule3_nested_descriptor",
            bond("C/C=C(/C/C(CC(C)C)=C/C)\\C/C(CC(C)C)=C\\C", 1, 2),
        ),
        (
            "cip_ligand_stereo_unspecified",
            plain("F[C@H](/C=C\\C)C=CC", 1),
        ),
        // A terminal atom cannot be a center.
        ("cip_not_a_candidate", plain("CCO", 0)),
    ];
    for (reason, case) in cases {
        let (app, _directory) = application();
        let built = build(&app, &case.molecule);
        let (list, parameters) = if case.bond {
            (
                "bonds",
                json!({"bonds": [bond_id(&built, &case.molecule, case.first, case.second)]}),
            )
        } else {
            ("atoms", json!({"atoms": [built.ids[case.first]]}))
        };
        let reply = call(&app, "reason", &request(&built, &parameters));
        let entry = &reply["result"][list][0];
        assert_eq!(entry["reason"], reason, "{reply}");
        let state = if reason == "cip_ligand_stereo_unspecified" {
            "ambiguous"
        } else {
            "unsupported"
        };
        assert_eq!(entry["state"], state, "{reason}");
        assert_eq!(reply["outcome"], "partial", "{reason}");
        assert_eq!(
            reply["loss_report"].as_array().unwrap().len(),
            1,
            "{reason}"
        );
        seen.push(reason);
    }
    // The digraph limit needs more nodes than the default allows or a smaller cap (see the
    // limits tests); the grid reaches the default through the operation.
    let (app, _directory) = application();
    let side = 13;
    let built = build(&app, &grid(side));
    let reply = call(
        &app,
        "limit",
        &request(&built, &json!({"atoms": [inner_first(&built, side)]})),
    );
    assert_eq!(reply["result"]["atoms"][0]["reason"], "cip_digraph_limit");
    seen.push("cip_digraph_limit");
    let mut listed: Vec<&str> = chemifree_application::CIP_REASONS
        .iter()
        .map(|r| r.code())
        .collect();
    listed.retain(|code| *code != "cip_ranking_inconsistent");
    seen.sort_unstable();
    listed.sort_unstable();
    assert_eq!(seen, listed, "every reason of the closed list has a case");
}

// ---------------------------------------------------------------------------------------------
// The catalog
// ---------------------------------------------------------------------------------------------

#[test]
fn the_catalog_advertises_chem_cip_with_its_limits_and_its_missing_time_limit() {
    let (app, _directory) = application();
    let catalog = serde_json::to_value(app.operation_catalog()).unwrap();
    let operations = catalog["operations"].as_array().unwrap();
    let entry = operations
        .iter()
        .find(|entry| entry["id"] == "chem.cip")
        .unwrap();
    assert_eq!(entry["execution"], "executable");
    assert_eq!(entry["representation"], "supported");
    let note = entry["note"].as_str().unwrap();
    // The numbers in the note are those the labeler runs under.
    let limits = chemifree_domain::cip::Limits::default();
    for number in [
        limits.item_nodes,
        limits.request_nodes,
        chemifree_domain::cip::MAX_SPHERES,
        chemifree_domain::cip::MAX_NESTED_SPHERES,
        chemifree_domain::cip::MAX_RULE_3_NESTING,
    ] {
        assert!(
            note.contains(&number.to_string()),
            "{number} is not in: {note}"
        );
    }
    for words in [
        "Rules 1a, 1b, 2 and 3 only",
        "no time limit",
        "unsupported or ambiguous",
    ] {
        assert!(note.contains(words), "{words:?} is not in: {note}");
    }
    // The stereo rows keep saying what chem.stereo does not do and where labels come from.
    let stereo = operations
        .iter()
        .find(|entry| entry["id"] == "chem.stereo")
        .unwrap();
    assert!(
        stereo["note"]
            .as_str()
            .unwrap()
            .contains("computes no CIP label (chem.cip does)")
    );
    let transaction = operations
        .iter()
        .find(|entry| entry["id"] == "document.transaction")
        .unwrap();
    let configuration = transaction["sub_operations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|part| part["id"] == "stereo_configuration")
        .unwrap();
    let note = configuration["note"].as_str().unwrap();
    assert!(note.contains("chem.stereo compares"), "{note}");
    assert!(note.contains("does not compute labels"), "{note}");
    assert!(
        note.contains("chem.cip computes CIP labels from stored configurations"),
        "{note}"
    );
}
