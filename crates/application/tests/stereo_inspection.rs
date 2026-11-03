//! `chem.stereo` over the operation contract: the reply shape, every agreement value, every
//! diagnostic code with its documented count, the non-atom label field, weak evidence, that it
//! only reads, instance selection, the entry limit, and that `chem.validate` is unchanged.
//! Every document is made by transactions sent to the Application, as HTTP and the CLI send
//! them. The expected configurations are the hand-computed E1 values of the domain tests
//! (F top, Cl lower right with the wedge, Br lower left: clockwise over `[F, Cl, Br, H]`).

use std::collections::BTreeMap;

use chemifree_application::{Application, CommandDto};
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Map, Value, json};
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
    app.create_document(Some("create"), Some("Stereo"))
        .unwrap()
        .document
        .id
}

fn call(app: &App, request: &str, operation: &str, parameters: &Value) -> Value {
    let body = json!({"contract": 1, "request_id": request, "profile": "preserve-semantics",
                      "operation": operation, "parameters": parameters});
    serde_json::to_value(app.execute(body.to_string().as_bytes())).unwrap()
}

fn stereo(app: &App, request: &str, parameters: &Value) -> Value {
    call(app, request, "chem.stereo", parameters)
}

fn transact(app: &App, id: &str, request: &str, revision: u64, commands: &[Value]) -> Value {
    let dtos: Vec<CommandDto> =
        serde_json::from_value(Value::Array(commands.to_vec())).expect("commands parse");
    match app.apply_transaction(id, Some(request), revision, dtos) {
        Ok(response) => serde_json::to_value(response).unwrap(),
        Err(error) => panic!(
            "refused: {}",
            serde_json::to_value(error.to_response()).unwrap()
        ),
    }
}

/// Commands that build a drawing on an empty document, with the ids the transaction creates.
#[derive(Default)]
struct Sheet {
    commands: Vec<Value>,
    /// The command index of each atom, for `$n` references.
    created_by: Vec<usize>,
    atoms: usize,
    bonds: usize,
}

impl Sheet {
    /// A new atom, bonded to atom number `to` (zero-based) with `order` when given. Returns the
    /// atom's number.
    fn atom(&mut self, element: &str, x: f64, y: f64, to: Option<(usize, &str)>) -> usize {
        let mut command = json!({
            "op": "add_atom", "element": element, "position": {"x": x, "y": y}
        });
        if let Some((target, order)) = to {
            command["bond_to"] =
                json!({"atom": format!("${}", self.created_by[target]), "order": order});
            self.bonds += 1;
        }
        self.created_by.push(self.commands.len());
        self.commands.push(command);
        self.atoms += 1;
        self.atoms - 1
    }

    /// The id of atom number `index`.
    fn id(index: usize) -> String {
        format!("a{}", index + 1)
    }

    fn mark(&mut self, bond: usize, mark: &Value) {
        self.commands
            .push(json!({"op": "set_bond_mark", "bond": format!("b{bond}"), "mark": mark}));
    }

    fn tetrahedral(&mut self, center: usize, neighbors: [String; 4], turn: &str) {
        self.commands.push(json!({
            "op": "set_atom_configuration", "atom": Self::id(center),
            "configuration": {"kind": "tetrahedral", "neighbors": neighbors, "turn": turn}}));
    }

    /// Carbon, F above, Cl lower right, Br lower left (E1); returns the carbon's number and the
    /// bond numbers of F, Cl and Br. The atoms are `center + 1..=center + 3`.
    fn halo(&mut self, x: f64) -> (usize, [usize; 3]) {
        let center = self.atom("C", x, 100.0, None);
        let first = self.bonds + 1;
        self.atom("F", x, 70.0, Some((center, "single")));
        self.atom("Cl", x + 26.0, 115.0, Some((center, "single")));
        self.atom("Br", x - 26.0, 115.0, Some((center, "single")));
        (center, [first, first + 1, first + 2])
    }

    fn halo_neighbors(center: usize) -> [String; 4] {
        [
            Self::id(center + 1),
            Self::id(center + 2),
            Self::id(center + 3),
            "implicit_hydrogen".into(),
        ]
    }
}

fn wedge() -> Value {
    json!({"kind": "wedge", "narrow": "begin"})
}

/// A document that shows every agreement and every finding. Returns its id and the atom ids
/// (and bond ids) the tests look at.
struct Kitchen {
    app: App,
    _directory: TempDir,
    id: String,
    agrees: &'static str,
    disagrees: &'static str,
    only_stored: &'static str,
    only_drawn: &'static str,
    neither: &'static str,
    weak: &'static str,
    sulfoxide: &'static str,
}

fn kitchen() -> Kitchen {
    let (app, directory) = application();
    let id = created(&app);
    let mut sheet = Sheet::default();
    // a1-a4: a stored turn the drawing shows. A hash whose narrow end is the terminal F states
    // nothing about the center: a misplaced mark with a hint.
    let (agrees, bonds) = sheet.halo(100.0);
    sheet.mark(bonds[1], &wedge());
    sheet.tetrahedral(agrees, Sheet::halo_neighbors(agrees), "clockwise");
    sheet.mark(bonds[0], &json!({"kind": "hash", "narrow": "end"}));
    // a5-a8: the stored turn is the opposite of the drawing.
    let (disagrees, bonds) = sheet.halo(300.0);
    sheet.mark(bonds[1], &wedge());
    sheet.tetrahedral(disagrees, Sheet::halo_neighbors(disagrees), "anticlockwise");
    // a9-a12: stored, no mark at all.
    let (only_stored, _) = sheet.halo(500.0);
    sheet.tetrahedral(only_stored, Sheet::halo_neighbors(only_stored), "clockwise");
    // a13-a16: marked, nothing stored.
    let (only_drawn, bonds) = sheet.halo(700.0);
    sheet.mark(bonds[1], &wedge());
    // a17-a20: a wedge and a wavy bond to the terminal F conflict.
    let (neither, bonds) = sheet.halo(900.0);
    sheet.mark(bonds[1], &wedge());
    sheet.mark(bonds[0], &json!({"kind": "wavy"}));
    // a21-a24: F up, Cl down but off the line by 0.0125 rad, Br wedge: a nearly flat drawing.
    let weak = sheet.atom("C", 1100.0, 100.0, None);
    sheet.atom("F", 1100.0, 70.0, Some((weak, "single")));
    sheet.atom("Cl", 1100.375, 129.995, Some((weak, "single")));
    sheet.atom("Br", 1126.0, 115.0, Some((weak, "single")));
    sheet.mark(sheet.bonds, &wedge());
    // a25-a28: S(=O)(C)C drawn as E1, a lone pair; the stored record names a hydrogen.
    let sulfoxide = sheet.atom("S", 1300.0, 100.0, None);
    sheet.atom("O", 1300.0, 70.0, Some((sulfoxide, "double")));
    sheet.atom("C", 1326.0, 115.0, Some((sulfoxide, "single")));
    sheet.mark(sheet.bonds, &wedge());
    sheet.atom("C", 1274.0, 115.0, Some((sulfoxide, "single")));
    sheet.tetrahedral(sulfoxide, Sheet::halo_neighbors(sulfoxide), "clockwise");
    // a29-a33: four neighbors, one aromatic, one wedge.
    let aromatic = sheet.atom("C", 1500.0, 100.0, None);
    sheet.atom("C", 1500.0, 70.0, Some((aromatic, "single")));
    sheet.atom("C", 1526.0, 115.0, Some((aromatic, "single")));
    sheet.mark(sheet.bonds, &wedge());
    sheet.atom("C", 1474.0, 115.0, Some((aromatic, "aromatic")));
    sheet.atom("C", 1500.0, 130.0, Some((aromatic, "single")));
    // a34-a38: two centers joined by a wavy bond.
    let left = sheet.atom("C", 1700.0, 100.0, None);
    sheet.atom("C", 1700.0, 70.0, Some((left, "single")));
    sheet.atom("C", 1674.0, 115.0, Some((left, "single")));
    let right = sheet.atom("C", 1760.0, 100.0, Some((left, "single")));
    sheet.mark(sheet.bonds, &json!({"kind": "wavy"}));
    sheet.atom("C", 1760.0, 70.0, Some((right, "single")));
    sheet.atom("C", 1786.0, 115.0, Some((right, "single")));
    // a40-a44: an alkene, trans as drawn and as stored, with a second substituent on the side
    // of the first.
    let first = sheet.atom("C", 1900.0, 115.0, None);
    let begin = sheet.atom("C", 1930.0, 100.0, Some((first, "single")));
    let end = sheet.atom("C", 1960.0, 100.0, Some((begin, "double")));
    let alkene_bond = sheet.bonds;
    let last = sheet.atom("C", 1990.0, 85.0, Some((end, "single")));
    sheet.atom("C", 1910.0, 130.0, Some((begin, "single")));
    sheet.commands.push(json!({
        "op": "set_bond_configuration", "bond": format!("b{alkene_bond}"),
        "configuration": {"kind": "relative", "begin_neighbor": Sheet::id(first),
                          "end_neighbor": Sheet::id(last), "relation": "opposite_sides"}}));
    // a45-a47: a crossed bond next to another double bond: the model cannot hold it.
    let cumulated = sheet.atom("C", 2100.0, 100.0, None);
    let middle = sheet.atom("C", 2130.0, 100.0, Some((cumulated, "double")));
    sheet.atom("C", 2160.0, 100.0, Some((middle, "double")));
    sheet.mark(sheet.bonds, &json!({"kind": "crossed"}));

    let reply = transact(&app, &id, "kitchen", 0, &sheet.commands);
    assert_eq!(reply["document"]["revision"], 1);
    let name = |index: usize| -> &'static str { Box::leak(Sheet::id(index).into_boxed_str()) };
    Kitchen {
        app,
        _directory: directory,
        id,
        agrees: name(agrees),
        disagrees: name(disagrees),
        only_stored: name(only_stored),
        only_drawn: name(only_drawn),
        neither: name(neither),
        weak: name(weak),
        sulfoxide: name(sulfoxide),
    }
}

fn entry<'a>(reply: &'a Value, key: &str, id: &str) -> &'a Value {
    reply["result"][if key == "atom" { "atoms" } else { "bonds" }]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry[key] == id)
        .unwrap_or_else(|| panic!("no {key} {id} in {reply}"))
}

fn counts(reply: &Value) -> BTreeMap<String, usize> {
    let mut found = BTreeMap::new();
    for diagnostic in reply["diagnostics"].as_array().unwrap() {
        *found
            .entry(diagnostic["code"].as_str().unwrap().to_owned())
            .or_default() += 1;
    }
    found
}

fn keys(value: &Value) -> Vec<&str> {
    let mut keys: Vec<&str> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    keys
}

#[test]
fn the_reply_has_exactly_the_documented_shape_and_is_ok_with_the_stereo_model() {
    let kitchen = kitchen();
    let reply = stereo(&kitchen.app, "shape", &json!({"document_id": kitchen.id}));
    assert_eq!(reply["envelope"], "operation_result");
    assert_eq!(reply["operation"], "chem.stereo");
    assert_eq!(
        reply["outcome"], "ok",
        "a disagreement is a finding, not a loss"
    );
    assert_eq!(reply["model_id"], "chemifree-stereo-1");
    assert_eq!(reply["loss_report"], json!([]));
    assert_eq!(reply["document_revision"], 1);
    let result = &reply["result"];
    assert_eq!(
        keys(result),
        [
            "atoms",
            "bonds",
            "instances",
            "snapshot",
            "stereogenicity_assessed",
            "valence_model"
        ]
    );
    assert_eq!(result["valence_model"], "chemifree-valence-3");
    assert_eq!(result["stereogenicity_assessed"], false);
    assert_eq!(result["instances"], json!(["s1"]));
    // The snapshot is the exact document compared.
    let document = serde_json::to_value(kitchen.app.document(&kitchen.id).unwrap()).unwrap();
    assert_eq!(result["snapshot"]["document"], document["document"]);
    assert_eq!(result["snapshot"]["document"]["revision"], 1);

    let atom = entry(&reply, "atom", kitchen.agrees);
    assert_eq!(keys(atom), ["atom", "drawn", "stored"]);
    assert_eq!(
        atom["stored"],
        json!({"kind": "tetrahedral", "neighbors": ["a2", "a3", "a4", "implicit_hydrogen"],
               "turn": "clockwise"})
    );
    let drawn = &atom["drawn"][0];
    assert_eq!(
        keys(drawn),
        [
            "agreement",
            "configuration",
            "instance",
            "label_differs",
            "reason"
        ]
    );
    assert_eq!(drawn["instance"], "s1");
    assert_eq!(
        drawn["configuration"], atom["stored"],
        "the same value pastes into a command"
    );
    assert_eq!(drawn["reason"], Value::Null);
    assert_eq!(drawn["agreement"], "agrees");
    assert_eq!(drawn["label_differs"], false);

    let bond = &result["bonds"][0];
    assert_eq!(keys(bond), ["bond", "drawn", "stored"]);
    assert_eq!(
        keys(&bond["drawn"][0]),
        ["agreement", "configuration", "instance", "reason"],
        "a bond has no label field"
    );
}

#[test]
fn unknown_parameters_and_documents_are_refused_without_a_result() {
    let kitchen = kitchen();
    let extra = stereo(
        &kitchen.app,
        "extra",
        &json!({"document_id": kitchen.id, "all": true}),
    );
    assert_eq!(extra["outcome"], "error");
    assert_eq!(extra["diagnostics"][0]["code"], "invalid_request");
    assert_eq!(extra["result"], Value::Null);

    let missing = stereo(
        &kitchen.app,
        "missing",
        &json!({"document_id": "no-such-document"}),
    );
    assert_eq!(missing["outcome"], "error");
    assert_eq!(missing["diagnostics"][0]["code"], "document_not_found");
}

#[test]
fn every_agreement_value_is_reported_with_its_configuration_and_reason() {
    let kitchen = kitchen();
    let reply = stereo(&kitchen.app, "agree", &json!({"document_id": kitchen.id}));
    let drawn = |id: &str| entry(&reply, "atom", id)["drawn"][0].clone();

    let agrees = drawn(kitchen.agrees);
    assert_eq!(agrees["agreement"], "agrees");

    let disagrees = drawn(kitchen.disagrees);
    assert_eq!(disagrees["agreement"], "disagrees");
    assert_eq!(disagrees["configuration"]["turn"], "clockwise");
    assert_eq!(
        entry(&reply, "atom", kitchen.disagrees)["stored"]["turn"],
        "anticlockwise"
    );

    let only_stored = drawn(kitchen.only_stored);
    assert_eq!(only_stored["agreement"], "only_stored");
    assert_eq!(only_stored["configuration"], Value::Null);
    assert_eq!(only_stored["reason"], "stereo_not_derivable_no_marks");

    let only_drawn = drawn(kitchen.only_drawn);
    assert_eq!(only_drawn["agreement"], "only_drawn");
    assert_eq!(only_drawn["reason"], Value::Null);
    assert_eq!(
        entry(&reply, "atom", kitchen.only_drawn)["stored"],
        Value::Null
    );
    assert_eq!(only_drawn["configuration"]["turn"], "clockwise");

    let neither = drawn(kitchen.neither);
    assert_eq!(neither["agreement"], "neither");
    assert_eq!(neither["configuration"], Value::Null);
    assert_eq!(neither["reason"], "stereo_not_derivable_conflicting_marks");

    // Bonds: the trans alkene agrees; the crossed bond beside another double bond cannot be held.
    let bonds = reply["result"]["bonds"].as_array().unwrap();
    let by_agreement: Vec<(&str, &str)> = bonds
        .iter()
        .map(|b| {
            (
                b["bond"].as_str().unwrap(),
                b["drawn"][0]["agreement"].as_str().unwrap(),
            )
        })
        .collect();
    assert!(
        by_agreement.contains(&("b32", "agrees")),
        "{by_agreement:?}"
    );
    let cumulated = bonds.iter().find(|b| b["bond"] == "b36").unwrap();
    assert_eq!(cumulated["drawn"][0]["agreement"], "neither");
    assert_eq!(
        cumulated["drawn"][0]["reason"],
        "stereo_geometry_unsupported"
    );
}

#[test]
fn every_diagnostic_code_appears_with_its_documented_count_and_details() {
    let kitchen = kitchen();
    let reply = stereo(&kitchen.app, "codes", &json!({"document_id": kitchen.id}));
    let expected: BTreeMap<String, usize> = [
        ("drawing_disagrees", 1),
        ("drawing_not_derivable", 2),
        ("marks_without_configuration", 5),
        ("configuration_without_marks", 1),
        ("mark_narrow_end_not_a_center", 1),
        ("center_has_aromatic_bond", 1),
        ("double_bond_substituents_same_side", 1),
        ("wavy_between_possible_centers", 1),
        ("drawing_weak_evidence", 1),
        ("stereo_not_checked", 1),
    ]
    .into_iter()
    .map(|(code, count)| (code.to_owned(), count))
    .collect();
    assert_eq!(counts(&reply), expected);

    let find = |code: &str| -> Vec<&Value> {
        reply["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|d| d["code"] == code)
            .collect()
    };
    let severity = |code: &str| find(code)[0]["severity"].as_str().unwrap().to_owned();
    for (code, expected) in [
        ("drawing_disagrees", "warning"),
        ("drawing_not_derivable", "warning"),
        ("marks_without_configuration", "info"),
        ("configuration_without_marks", "info"),
        ("mark_narrow_end_not_a_center", "warning"),
        ("center_has_aromatic_bond", "warning"),
        ("double_bond_substituents_same_side", "warning"),
        ("wavy_between_possible_centers", "info"),
        ("drawing_weak_evidence", "info"),
        ("stereo_not_checked", "info"),
    ] {
        assert_eq!(severity(code), expected, "{code}");
    }

    let disagrees = &find("drawing_disagrees")[0]["details"];
    assert_eq!(disagrees["item"], "atom");
    assert_eq!(disagrees["id"], kitchen.disagrees);
    assert_eq!(disagrees["instance"], "s1");
    assert_eq!(disagrees["stored"]["turn"], "anticlockwise");
    assert_eq!(disagrees["drawn"]["turn"], "clockwise");

    let reasons: Vec<&Value> = find("drawing_not_derivable")
        .into_iter()
        .map(|d| &d["details"]["reason"])
        .collect();
    assert_eq!(
        reasons,
        [
            &json!("stereo_not_derivable_conflicting_marks"),
            &json!("stereo_geometry_unsupported")
        ]
    );

    let misplaced = &find("mark_narrow_end_not_a_center")[0]["details"];
    assert_eq!(misplaced["bond"], "b1");
    assert_eq!(misplaced["narrow_atom"], "a2");
    assert_eq!(misplaced["other_atom"], "a1");
    assert!(misplaced["hint"].as_str().unwrap().contains("a1"));

    assert_eq!(
        find("configuration_without_marks")[0]["details"]["id"],
        kitchen.only_stored
    );
    assert_eq!(
        find("center_has_aromatic_bond")[0]["details"]["atom"],
        "a29"
    );
    assert_eq!(
        find("double_bond_substituents_same_side")[0]["details"]["ends"],
        json!(["a41"])
    );
    assert_eq!(
        find("wavy_between_possible_centers")[0]["details"]["atoms"],
        json!(["a34", "a37"])
    );
    // The statement of what is not judged names each thing.
    let message = find("stereo_not_checked")[0]["message"].as_str().unwrap();
    for word in [
        "CIP",
        "stereogenic",
        "ring",
        "chem.identity",
        "chem.substructure",
    ] {
        assert!(message.contains(word), "{word} in {message}");
    }
    assert_eq!(
        reply["diagnostics"].as_array().unwrap().last().unwrap()["code"],
        "stereo_not_checked"
    );
}

#[test]
fn the_non_atom_label_is_a_field_of_the_entry_and_not_a_finding() {
    let kitchen = kitchen();
    let reply = stereo(&kitchen.app, "label", &json!({"document_id": kitchen.id}));
    let atom = entry(&reply, "atom", kitchen.sulfoxide);
    assert_eq!(atom["stored"]["neighbors"][3], "implicit_hydrogen");
    let drawn = &atom["drawn"][0];
    assert_eq!(drawn["configuration"]["neighbors"][3], "lone_pair");
    assert_eq!(drawn["agreement"], "agrees", "the label is not compared");
    assert_eq!(drawn["label_differs"], true);
    let others = reply["result"]["atoms"].as_array().unwrap();
    let differing: Vec<&str> = others
        .iter()
        .filter(|a| a["drawn"][0]["label_differs"] == true)
        .map(|a| a["atom"].as_str().unwrap())
        .collect();
    assert_eq!(differing, [kitchen.sulfoxide]);
    assert!(!counts(&reply).contains_key("implicit_position_label_differs"));
}

#[test]
fn a_nearly_flat_drawing_reports_weak_evidence_with_its_strength_and_a_firm_one_does_not() {
    let kitchen = kitchen();
    let reply = stereo(&kitchen.app, "weak", &json!({"document_id": kitchen.id}));
    let weak: Vec<&Value> = reply["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|d| d["code"] == "drawing_weak_evidence")
        .collect();
    assert_eq!(weak.len(), 1);
    let details = &weak[0]["details"];
    assert_eq!(details["id"], kitchen.weak);
    assert_eq!(details["instance"], "s1");
    assert_eq!(details["limit"], 0.1);
    let strength = details["strength"].as_f64().unwrap();
    assert!((0.001..0.1).contains(&strength), "{strength}");
    // The derivation itself is not refused: the entry carries a configuration.
    assert_eq!(
        entry(&reply, "atom", kitchen.weak)["drawn"][0]["agreement"],
        "only_drawn"
    );
    // E1 (|D| = 3.46) and the other firm drawings raise no such finding.
    assert!(weak.iter().all(|d| d["details"]["id"] != kitchen.agrees));
}

#[test]
fn an_inspection_changes_nothing_and_is_not_replayed() {
    let kitchen = kitchen();
    let app = &kitchen.app;
    let parameters = json!({"document_id": kitchen.id});
    let before = serde_json::to_value(app.document(&kitchen.id).unwrap()).unwrap();
    let history =
        |app: &App| serde_json::to_value(app.document(&kitchen.id).unwrap().session).unwrap();
    let first = stereo(app, "same-id", &parameters);
    let again = stereo(app, "same-id", &parameters);
    assert_eq!(first, again, "unchanged input, equal reply");
    let after = serde_json::to_value(app.document(&kitchen.id).unwrap()).unwrap();
    assert_eq!(
        before, after,
        "revision, session, history depth and stereo are as they were"
    );
    assert_eq!(history(app)["undo_depth"], 1);
    assert_eq!(first["result"]["snapshot"]["document"], before["document"]);

    // The same request id after an edit is not replayed from before it.
    transact(
        app,
        &kitchen.id,
        "edit",
        1,
        &[json!({"op": "set_atom_configuration", "atom": kitchen.agrees, "configuration": null})],
    );
    let later = stereo(app, "same-id", &parameters);
    assert_eq!(later["document_revision"], 2);
    let atom = entry(&later, "atom", kitchen.agrees);
    assert_eq!(atom["stored"], Value::Null);
    assert_eq!(atom["drawn"][0]["agreement"], "only_drawn");
}

#[test]
fn validate_is_byte_identical_around_an_inspection_and_has_no_stereo_findings() {
    let kitchen = kitchen();
    let parameters = json!({"document_id": kitchen.id});
    let before = call(&kitchen.app, "validate", "chem.validate", &parameters);
    stereo(&kitchen.app, "between", &parameters);
    let after = call(&kitchen.app, "validate", "chem.validate", &parameters);
    assert_eq!(before.to_string(), after.to_string());
    assert_eq!(before["model_id"], "chemifree-valence-3");
    let diagnostic_codes: Vec<&str> = before["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["code"].as_str().unwrap())
        .collect();
    assert!(
        diagnostic_codes
            .iter()
            .all(|code| *code == "valence_diagnostic"),
        "{diagnostic_codes:?}"
    );
    assert!(!before.to_string().contains("drawing_disagrees"));
    // The catalog still says validate has no stereo check.
    let catalog = call(&kitchen.app, "catalog", "capabilities.inspect", &json!({}));
    let note = catalog["result"]["operations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["id"] == "document.transaction")
        .unwrap()["sub_operations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["id"] == "stereo_configuration")
        .unwrap()["note"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(note.contains("no stereo check in chem.validate"), "{note}");
    assert!(note.contains("chem.stereo compares"), "{note}");
}

/// The E1 halo with a stored clockwise turn and a second instance in which Br is moved across
/// the F-Cl gap (E3, anticlockwise).
fn two_instances() -> (App, TempDir, String) {
    let (app, directory) = application();
    let id = created(&app);
    let mut sheet = Sheet::default();
    let (center, bonds) = sheet.halo(100.0);
    sheet.mark(bonds[1], &wedge());
    sheet.tetrahedral(center, Sheet::halo_neighbors(center), "clockwise");
    sheet
        .commands
        .push(json!({"op": "add_instance", "offset": {"x": 200, "y": 0}}));
    sheet.commands.push(json!({
        "op": "move_atoms", "atoms": ["a4"], "delta": {"x": 52, "y": -30}, "instance": "s2"}));
    transact(&app, &id, "two", 0, &sheet.commands);
    (app, directory, id)
}

#[test]
fn an_omitted_instance_inspects_every_instance_and_a_named_one_only_that_one() {
    let (app, _directory, id) = two_instances();
    let both = stereo(&app, "both", &json!({"document_id": id}));
    assert_eq!(both["result"]["instances"], json!(["s1", "s2"]));
    let drawn = &entry(&both, "atom", "a1")["drawn"];
    assert_eq!(drawn[0]["instance"], "s1");
    assert_eq!(drawn[0]["agreement"], "agrees");
    assert_eq!(drawn[1]["instance"], "s2");
    assert_eq!(drawn[1]["agreement"], "disagrees");
    assert_eq!(drawn[1]["configuration"]["turn"], "anticlockwise", "E3");
    assert_eq!(counts(&both)["drawing_disagrees"], 1);

    for (name, instance, agreement) in [("s1", "s1", "agrees"), ("s2", "s2", "disagrees")] {
        let one = stereo(
            &app,
            name,
            &json!({"document_id": id, "instance": instance}),
        );
        assert_eq!(one["result"]["instances"], json!([instance]));
        let drawn = entry(&one, "atom", "a1")["drawn"]
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(drawn.len(), 1, "{instance}");
        assert_eq!(drawn[0]["agreement"], agreement, "{instance}");
    }
    // `null` is the same as leaving it out.
    let null = stereo(&app, "null", &json!({"document_id": id, "instance": null}));
    assert_eq!(null["result"]["instances"], json!(["s1", "s2"]));
}

#[test]
fn an_unknown_or_malformed_instance_is_an_invalid_request_naming_the_parameter() {
    let (app, _directory, id) = two_instances();
    for (label, instance) in [
        ("unknown", json!("s99")),
        ("malformed", json!("not-an-instance")),
        ("an atom id", json!("a1")),
        ("a number", json!(2)),
        ("an object", json!({"id": "s1"})),
    ] {
        let reply = stereo(
            &app,
            &label.replace(' ', "-"),
            &json!({"document_id": id, "instance": instance}),
        );
        assert_eq!(reply["outcome"], "error", "{label}");
        assert_eq!(reply["result"], Value::Null, "{label}");
        let diagnostic = &reply["diagnostics"][0];
        assert_eq!(diagnostic["code"], "invalid_request", "{label}");
        assert_eq!(diagnostic["path"], "parameters.instance", "{label}");
    }
}

#[test]
fn a_document_without_stereo_gives_empty_lists_and_the_statement_of_what_is_not_judged() {
    let (app, _directory) = application();
    let id = created(&app);
    let mut sheet = Sheet::default();
    sheet.halo(100.0);
    transact(&app, &id, "plain", 0, &sheet.commands);
    let reply = stereo(&app, "plain", &json!({"document_id": id}));
    assert_eq!(reply["outcome"], "ok");
    assert_eq!(reply["result"]["atoms"], json!([]));
    assert_eq!(reply["result"]["bonds"], json!([]));
    assert_eq!(reply["result"]["stereogenicity_assessed"], false);
    assert_eq!(
        counts(&reply),
        BTreeMap::from([("stereo_not_checked".to_owned(), 1)])
    );
}

/// A chain of `atoms` carbons joined by double bonds. Returns the document id.
fn double_bond_chain(app: &App, atoms: usize) -> String {
    let id = created(app);
    let mut commands = vec![json!({
        "op": "add_atom", "element": "C", "position": {"x": 0, "y": 0}})];
    for index in 1..atoms {
        commands.push(json!({
            "op": "add_atom", "element": "C", "position": {"x": index * 2, "y": 0},
            "bond_to": {"atom": format!("a{index}"), "order": "double"}}));
    }
    let mut revision = 0;
    for chunk in commands.chunks(500) {
        let reply = transact(app, &id, &format!("chain-{revision}"), revision, chunk);
        revision = reply["document"]["revision"].as_u64().unwrap();
    }
    id
}

fn cross(app: &App, id: &str, request: &str, bonds: std::ops::RangeInclusive<usize>) {
    let commands: Vec<Value> = bonds
        .map(|bond| json!({"op": "set_bond_mark", "bond": format!("b{bond}"), "mark": {"kind": "crossed"}}))
        .collect();
    let revision = serde_json::to_value(app.document(id).unwrap()).unwrap()["document"]["revision"]
        .as_u64()
        .unwrap();
    for (index, chunk) in commands.chunks(500).enumerate() {
        transact(
            app,
            id,
            &format!("{request}-{index}"),
            revision + index as u64,
            chunk,
        );
    }
}

#[test]
fn more_than_twenty_thousand_entries_is_a_resource_limit_that_naming_an_instance_avoids() {
    let (app, _directory) = application();
    // 1,002 carbons: 1,001 double bonds. Twenty instances is the document maximum.
    let id = double_bond_chain(&app, 1002);
    cross(&app, &id, "cross", 1..=1000);
    let instances: Vec<Value> = (0..19)
        .map(|_| json!({"op": "add_instance", "offset": {"x": 0, "y": 10}}))
        .collect();
    let revision =
        serde_json::to_value(app.document(&id).unwrap()).unwrap()["document"]["revision"]
            .as_u64()
            .unwrap();
    transact(&app, &id, "instances", revision, &instances);

    // 1,000 items in 20 instances is exactly the limit: answered.
    let at_limit = stereo(&app, "at-limit", &json!({"document_id": id}));
    assert_eq!(at_limit["outcome"], "ok");
    let entries: usize = at_limit["result"]["bonds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["drawn"].as_array().unwrap().len())
        .sum();
    assert_eq!(entries, 20_000);

    // One more marked bond: 1,001 items in 20 instances is 20,020.
    cross(&app, &id, "cross-more", 1001..=1001);
    let over = stereo(&app, "over", &json!({"document_id": id}));
    assert_eq!(over["outcome"], "error");
    assert_eq!(over["result"], Value::Null);
    let diagnostic = &over["diagnostics"][0];
    assert_eq!(diagnostic["code"], "resource_limit");
    assert_eq!(diagnostic["path"], "parameters.instance");
    assert_eq!(diagnostic["details"]["entries"], 20_020);
    assert_eq!(diagnostic["details"]["limit"], 20_000);
    assert_eq!(diagnostic["details"]["parameter"], "instance");
    assert!(
        diagnostic["message"]
            .as_str()
            .unwrap()
            .contains("`instance`")
    );

    // Naming one instance always fits.
    let one = stereo(&app, "one", &json!({"document_id": id, "instance": "s20"}));
    assert_eq!(one["outcome"], "ok");
    assert_eq!(one["result"]["bonds"].as_array().unwrap().len(), 1001);
}

/// The documented list of diagnostic codes and agreement values is exactly what is emitted, and
/// every one is in CONTRACT.
#[test]
fn contract_lists_every_agreement_value_and_diagnostic_code_the_operation_emits() {
    let contract = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/CONTRACT.md"),
    )
    .unwrap();
    let start = contract
        .find("## Stereo inspection")
        .expect("CONTRACT has a Stereo inspection section");
    let section = &contract[start..];
    let end = section[3..].find("\n## ").map_or(section.len(), |i| i + 3);
    let section = &section[..end];

    let kitchen = kitchen();
    let reply = stereo(&kitchen.app, "docs", &json!({"document_id": kitchen.id}));
    let mut emitted: Map<String, Value> = Map::new();
    for code in counts(&reply).keys() {
        emitted.insert(code.clone(), Value::Null);
    }
    for code in emitted.keys() {
        assert!(
            section.contains(&format!("`{code}`")),
            "{code} is not in CONTRACT"
        );
    }
    for agreement in [
        "agrees",
        "disagrees",
        "only_stored",
        "only_drawn",
        "neither",
    ] {
        assert!(section.contains(&format!("`{agreement}`")), "{agreement}");
    }
    for field in ["stereogenicity_assessed", "label_differs", "MAX_ENTRIES"] {
        assert!(section.contains(field), "{field}");
    }
    assert!(!section.contains("implicit_position_label_differs"));
}

/// A stored, wedged center whose atom was later given hydrogens the fourth position cannot take
/// (the record stays, the drawing then names no position).
fn stored_center_with_refused_fourth_position() -> (App, TempDir, String) {
    let (app, directory) = application();
    let id = created(&app);
    let mut sheet = Sheet::default();
    let (center, bonds) = sheet.halo(100.0);
    sheet.mark(bonds[1], &wedge());
    sheet.tetrahedral(center, Sheet::halo_neighbors(center), "clockwise");
    sheet.commands.push(json!({
        "op": "update_atom", "atom": "a1", "hydrogens": {"mode": "specified", "count": 2}}));
    transact(&app, &id, "refused-fourth", 0, &sheet.commands);
    (app, directory, id)
}

#[test]
fn a_marked_stored_center_the_drawing_cannot_read_is_still_not_derivable() {
    let (app, _directory, id) = stored_center_with_refused_fourth_position();
    let reply = stereo(&app, "marked", &json!({"document_id": id}));
    let drawn = &entry(&reply, "atom", "a1")["drawn"][0];
    assert_eq!(drawn["agreement"], "only_stored");
    assert_eq!(drawn["reason"], "stereo_not_derivable_fourth_position");
    let found = counts(&reply);
    assert_eq!(found["drawing_not_derivable"], 1);
    assert_eq!(found.get("configuration_without_marks"), None);
}
