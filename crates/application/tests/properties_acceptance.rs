//! Formula acceptance matrix. Each case draws its molecule in one `document.transaction` and reads
//! it through `chem.properties` with the formula property, so every case runs the path a client
//! uses. No mass data is registered; formula needs none.

use chemifree_application::Application;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};

type App = Application<LocalCapabilities, FileDocumentRepository>;

fn call(app: &App, id: &str, operation: &str, parameters: Value) -> Value {
    let request = json!({"contract":1,"request_id":id,"profile":"preserve-semantics","operation":operation,"parameters":parameters});
    serde_json::to_value(app.execute(request.to_string().as_bytes())).unwrap()
}

fn new_app(data: &tempfile::TempDir) -> App {
    Application::new(LocalCapabilities, FileDocumentRepository::new(data.path()))
}

/// Creates a document and applies `commands` as its one transaction, which must succeed.
fn drawn(app: &App, title: &str, commands: Value) -> String {
    let id = app.create_document(None, Some(title)).unwrap().document.id;
    let edit = call(
        app,
        "draw",
        "document.transaction",
        json!({"document_id": id, "expected_revision": 0, "commands": commands}),
    );
    assert_eq!(edit["outcome"], "ok", "{edit}");
    id
}

fn document_json(app: &App, id: &str) -> Value {
    serde_json::to_value(app.document(id).unwrap()).unwrap()
}

/// Runs a formula request that must succeed. Checks what every successful formula reply shares:
/// the model identifier, zero display decimals, and a document left exactly as it was.
fn formula_reply(app: &App, id: &str, parameters: Value) -> Value {
    let before = document_json(app, id);
    let reply = call(app, "formula", "chem.properties", parameters);
    assert_eq!(reply["outcome"], "ok", "{reply}");
    assert_eq!(reply["model_id"], "chemifree-properties-1", "{reply}");
    assert_eq!(reply["result"]["display_decimals"], 0, "{reply}");
    assert_eq!(
        document_json(app, id),
        before,
        "a formula read must not change the document"
    );
    reply
}

#[test]
fn salt_formula_by_scope() {
    let data = tempfile::tempdir().unwrap();
    let app = new_app(&data);
    // Acetate CH3-C(=O)-O(-) and a separate sodium cation. The sodium states zero hydrogens; the
    // charged oxygen keeps inferred hydrogens, which the valence model gives as zero.
    let id = drawn(
        &app,
        "Sodium acetate",
        json!([
            {"op":"add_atom","element":"C","position":{"x":0,"y":0}},
            {"op":"add_atom","element":"C","position":{"x":20,"y":0},"bond_to":{"atom":"$0","order":"single"}},
            {"op":"add_atom","element":"O","position":{"x":40,"y":0},"bond_to":{"atom":"$1","order":"double"}},
            {"op":"add_atom","element":"O","position":{"x":20,"y":20},"bond_to":{"atom":"$1","order":"single"}},
            {"op":"add_atom","element":"Na","position":{"x":100,"y":0}},
            {"op":"update_atom","atom":"a4","charge":-1},
            {"op":"update_atom","atom":"a5","charge":1}
        ]),
    );

    let molecule = formula_reply(
        &app,
        &id,
        json!({"document_id": id, "scope": "molecule", "property": "formula"}),
    );
    assert_eq!(
        molecule["result"]["components"].as_array().unwrap().len(),
        1,
        "{molecule}"
    );
    assert_eq!(
        molecule["result"]["components"][0]["formula"], "C2H3NaO2",
        "{molecule}"
    );
    assert_eq!(
        molecule["result"]["components"][0]["formal_charge"], 0,
        "{molecule}"
    );

    let components = formula_reply(
        &app,
        &id,
        json!({"document_id": id, "scope": "components", "property": "formula"}),
    );
    let mut parts: Vec<(&str, i64)> = components["result"]["components"]
        .as_array()
        .unwrap()
        .iter()
        .map(|part| {
            (
                part["formula"].as_str().unwrap(),
                part["formal_charge"].as_i64().unwrap(),
            )
        })
        .collect();
    parts.sort();
    assert_eq!(parts, [("C2H3O2-", -1), ("Na+", 1)], "{components}");
}

#[test]
fn radical_hydrogen_reduction() {
    let data = tempfile::tempdir().unwrap();
    let app = new_app(&data);
    // A lone uncharged atom with inferred hydrogens: each unpaired electron removes one hydrogen
    // from the element's neutral target, as in the reduction table of chemistry.rs.
    for (element, radical, expected) in [
        ("C", "doublet", "CH3"),
        ("C", "singlet", "CH2"),
        ("C", "triplet", "CH2"),
        // No carbon: Composition::formula sorts all symbols alphabetically (Hill rule), so H2N.
        ("N", "doublet", "H2N"),
    ] {
        let id = drawn(
            &app,
            "Radical",
            json!([{"op":"add_atom","element":element,"position":{"x":0,"y":0},"radical":radical}]),
        );
        let reply = formula_reply(
            &app,
            &id,
            json!({"document_id": id, "scope": "molecule", "property": "formula"}),
        );
        assert_eq!(
            reply["result"]["components"][0]["formula"], expected,
            "{element} with a {radical} radical: {reply}"
        );
    }
}

#[test]
fn charged_radical_is_refused_without_result() {
    let data = tempfile::tempdir().unwrap();
    let app = new_app(&data);
    let id = drawn(
        &app,
        "Charged radical",
        json!([
            {"op":"add_atom","element":"C","position":{"x":0,"y":0},"radical":"doublet"},
            {"op":"update_atom","atom":"a1","charge":1}
        ]),
    );
    let before = document_json(&app, &id);

    let reply = call(
        &app,
        "charged",
        "chem.properties",
        json!({"document_id": id, "scope": "molecule", "property": "formula"}),
    );
    // A refused reply carries no model_id: the unsupported path never sets one.
    assert_eq!(reply["outcome"], "unsupported", "{reply}");
    assert_eq!(
        reply["diagnostics"][0]["code"], "hydrogens_unavailable",
        "{reply}"
    );
    assert_eq!(
        reply["diagnostics"][0]["details"]["atom"], "a1",
        "the refusal names the charged radical: {reply}"
    );
    assert!(
        reply["result"].is_null(),
        "a refused formula must carry no partial result: {reply}"
    );
    assert_eq!(
        document_json(&app, &id),
        before,
        "a refused formula read must not change the document"
    );
}

#[test]
fn hydrogen_modes() {
    let data = tempfile::tempdir().unwrap();
    let app = new_app(&data);
    // A stated count is recorded as given and is not inferred.
    let stated = drawn(
        &app,
        "Stated count",
        json!([{"op":"add_atom","element":"C","position":{"x":0,"y":0},"hydrogens":{"mode":"specified","count":2}}]),
    );
    let reply = formula_reply(
        &app,
        &stated,
        json!({"document_id": stated, "scope": "molecule", "property": "formula"}),
    );
    assert_eq!(
        reply["result"]["components"][0]["formula"], "CH2",
        "{reply}"
    );

    // Methanol with the hydroxyl hydrogen drawn as an explicit H vertex bonded to O (a3).
    let methanol = drawn(
        &app,
        "Methanol",
        json!([
            {"op":"add_atom","element":"C","position":{"x":0,"y":0}},
            {"op":"add_atom","element":"O","position":{"x":20,"y":0},"bond_to":{"atom":"$0","order":"single"}},
            {"op":"add_atom","element":"H","position":{"x":40,"y":0},"bond_to":{"atom":"$1","order":"single"}}
        ]),
    );
    let molecule = formula_reply(
        &app,
        &methanol,
        json!({"document_id": methanol, "scope": "molecule", "property": "formula"}),
    );
    assert_eq!(
        molecule["result"]["components"][0]["formula"], "CH4O",
        "{molecule}"
    );
    // The explicit H vertex is counted once. Oxygen's two bonds fill its valence, so its inferred
    // count is zero: O alone gives "O", and O with the vertex gives "HO".
    let oxygen = formula_reply(
        &app,
        &methanol,
        json!({"document_id": methanol, "scope": "selection", "selection": {"atoms": ["a2"]}, "property": "formula"}),
    );
    assert_eq!(
        oxygen["result"]["components"][0]["formula"], "O",
        "{oxygen}"
    );
    let oxygen_and_vertex = formula_reply(
        &app,
        &methanol,
        json!({"document_id": methanol, "scope": "selection", "selection": {"atoms": ["a2", "a3"]}, "property": "formula"}),
    );
    assert_eq!(
        oxygen_and_vertex["result"]["components"][0]["formula"], "HO",
        "{oxygen_and_vertex}"
    );
}

#[test]
fn isotope_hill_order() {
    let data = tempfile::tempdir().unwrap();
    let app = new_app(&data);
    // Ethane with one carbon labelled 13. Carbon leads and hydrogen follows; the labelled carbon
    // is bracketed after the unlabelled carbon, per the Composition::formula sort key.
    let id = drawn(
        &app,
        "Labelled ethane",
        json!([
            {"op":"add_atom","element":"C","position":{"x":0,"y":0}},
            {"op":"add_atom","element":"C","position":{"x":20,"y":0},"bond_to":{"atom":"$0","order":"single"}},
            {"op":"update_atom","atom":"a2","isotope":13}
        ]),
    );
    let reply = formula_reply(
        &app,
        &id,
        json!({"document_id": id, "scope": "molecule", "property": "formula"}),
    );
    assert_eq!(
        reply["result"]["components"][0]["formula"], "C[13C]H6",
        "{reply}"
    );
}

#[test]
fn hill_order_puts_carbon_and_hydrogen_first() {
    let data = tempfile::tempdir().unwrap();
    let app = new_app(&data);
    // With carbon present, Composition::formula puts C, then H, then the other symbols (Hill).
    // An alphabetical order would give "CClH3" here.
    let chloromethane = drawn(
        &app,
        "Chloromethane",
        json!([
            {"op":"add_atom","element":"C","position":{"x":0,"y":0}},
            {"op":"add_atom","element":"Cl","position":{"x":20,"y":0},"bond_to":{"atom":"$0","order":"single"}}
        ]),
    );
    let reply = formula_reply(
        &app,
        &chloromethane,
        json!({"document_id": chloromethane, "scope": "molecule", "property": "formula"}),
    );
    assert_eq!(
        reply["result"]["components"][0]["formula"], "CH3Cl",
        "{reply}"
    );

    // No carbon: every symbol sorts alphabetically, so a lone chlorine with its one inferred
    // hydrogen reads "ClH".
    let hydrogen_chloride = drawn(
        &app,
        "Hydrogen chloride",
        json!([{"op":"add_atom","element":"Cl","position":{"x":0,"y":0}}]),
    );
    let reply = formula_reply(
        &app,
        &hydrogen_chloride,
        json!({"document_id": hydrogen_chloride, "scope": "molecule", "property": "formula"}),
    );
    assert_eq!(
        reply["result"]["components"][0]["formula"], "ClH",
        "{reply}"
    );
}
