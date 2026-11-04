//! The stereo write side over the wire: the three commands, their refusals and reasons, the
//! notes a cascade clear leaves in the reply, and what the neighboring operations then say.
//!
//! Every document here is made by transactions sent to the Application, as HTTP and the CLI
//! send them. The tests pin the wire shapes (`null` clears, required keys, `$N` naming the bond
//! of an `add_atom`), the closed list of `reason` codes, atomic refusals with the failing
//! command index, and the rule that `partial` goes with exactly the replies that carry notes.

use chemifree_application::{Application, CommandDto, TransactionRequest};
use chemifree_domain::stereo::StereoError;
use chemifree_domain::stereo_drawing::NotDerivable;
use chemifree_providers::interchange_codec::LocalInterchangeCodec;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities, OpenSmilesCodec, V2000Codec};
use serde_json::{Value, json};
use tempfile::TempDir;

type App = Application<LocalCapabilities, FileDocumentRepository>;

fn application_at(directory: &TempDir) -> App {
    Application::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    )
    .with_ctfile_codec(V2000Codec)
    .with_interchange_codec(LocalInterchangeCodec)
    .with_smiles_codec(OpenSmilesCodec)
}

fn application() -> (App, TempDir) {
    let directory = tempfile::tempdir().unwrap();
    (application_at(&directory), directory)
}

fn created(app: &App) -> String {
    app.create_document(Some("create"), Some("Stereo"))
        .unwrap()
        .document
        .id
}

fn dtos(commands: &Value) -> Vec<CommandDto> {
    serde_json::from_value(commands.clone()).unwrap_or_else(|error| panic!("{error}: {commands}"))
}

/// A transaction reply as the HTTP adapter would send it, or the error envelope.
fn transact(app: &App, id: &str, request: &str, revision: u64, commands: &Value) -> Value {
    match app.apply_transaction(id, Some(request), revision, dtos(commands)) {
        Ok(response) => serde_json::to_value(response).unwrap(),
        Err(error) => serde_json::to_value(error.to_response()).unwrap(),
    }
}

fn ok(reply: Value) -> Value {
    assert!(reply.get("code").is_none(), "refused: {reply}");
    reply
}

fn atom_at(x: f64, y: f64, element: &str, bond_to: Option<&str>) -> Value {
    let mut command = json!({
        "op": "add_atom", "element": element, "position": {"x": x, "y": y}
    });
    if let Some(to) = bond_to {
        command["bond_to"] = json!({"atom": to, "order": "single"});
    }
    command
}

/// Carbon `a1` with F `a2`, Cl `a3`, Br `a4` (bonds `b1`-`b3`), command indices 0-3.
fn halo_commands() -> Vec<Value> {
    vec![
        atom_at(100.0, 100.0, "C", None),
        atom_at(100.0, 70.0, "F", Some("$0")),
        atom_at(126.0, 115.0, "Cl", Some("$0")),
        atom_at(74.0, 115.0, "Br", Some("$0")),
    ]
}

/// `a1=a2` (bond `b1`) with `a3` on `a1` and `a4` on `a2`, command indices 0-3.
fn alkene_commands() -> Vec<Value> {
    vec![
        atom_at(0.0, 0.0, "C", None),
        json!({"op": "add_atom", "element": "C", "position": {"x": 30, "y": 0},
               "bond_to": {"atom": "$0", "order": "double"}}),
        atom_at(-20.0, 20.0, "C", Some("$0")),
        atom_at(50.0, 20.0, "C", Some("$1")),
    ]
}

fn with(mut commands: Vec<Value>, more: Vec<Value>) -> Value {
    commands.extend(more);
    Value::Array(commands)
}

fn wedge_and_center() -> Vec<Value> {
    vec![
        json!({"op": "set_bond_mark", "bond": "$2", "mark": {"kind": "wedge", "narrow": "begin"}}),
        json!({"op": "set_atom_configuration", "atom": "$0", "configuration": {
            "kind": "tetrahedral",
            "neighbors": ["$3", "$1", "$2", "implicit_hydrogen"], "turn": "clockwise"}}),
    ]
}

fn notes(reply: &Value) -> &Vec<Value> {
    reply["notes"]
        .as_array()
        .unwrap_or_else(|| panic!("a reply carries notes: {reply}"))
}

// ---------------------------------------------------------------------------------------------
// The commands
// ---------------------------------------------------------------------------------------------

#[test]
fn a_mark_and_a_center_are_set_read_back_in_canonical_form_and_kept_by_history_and_reopen() {
    let (app, directory) = application();
    let id = created(&app);
    let reply = ok(transact(
        &app,
        &id,
        "r1",
        0,
        &with(halo_commands(), wedge_and_center()),
    ));
    // `$2` is the bond made with the third atom (`a3`), so it is `b2`.
    let bonds = &reply["document"]["bonds"];
    assert_eq!(bonds[1]["id"], "b2");
    assert_eq!(
        bonds[1]["mark"],
        json!({"kind": "wedge", "narrow": "begin"})
    );
    assert_eq!(
        (&bonds[0]["mark"], &bonds[2]["mark"]),
        (&Value::Null, &Value::Null)
    );
    // Named as [a4, a2, a3, H]: an even permutation of the sorted order, so the turn stays.
    assert_eq!(
        reply["document"]["stereo"],
        json!({"atoms": [{"atom": "a1", "configuration": {
            "kind": "tetrahedral",
            "neighbors": ["a2", "a3", "a4", "implicit_hydrogen"], "turn": "clockwise"}}],
            "bonds": [], "groups": []})
    );
    assert_eq!(notes(&reply), &Vec::<Value>::new());

    // An odd permutation flips the stored turn.
    let odd = ok(transact(
        &app,
        &id,
        "r2",
        1,
        &json!([{"op": "set_atom_configuration", "atom": "a1", "configuration": {
            "kind": "tetrahedral",
            "neighbors": ["a3", "a2", "a4", "implicit_hydrogen"], "turn": "clockwise"}}]),
    ));
    assert_eq!(
        odd["document"]["stereo"]["atoms"][0]["configuration"]["turn"],
        "anticlockwise"
    );

    // Undo, redo and a reopened service agree.
    let undone = app.undo(&id, Some("u"), 2).unwrap();
    let undone = serde_json::to_value(undone).unwrap();
    assert_eq!(
        undone["document"]["stereo"]["atoms"][0]["configuration"]["turn"],
        "clockwise"
    );
    let undone = serde_json::to_value(app.undo(&id, Some("u2"), 3).unwrap()).unwrap();
    assert_eq!(
        undone["document"]["stereo"],
        json!({"atoms": [], "bonds": [], "groups": []})
    );
    assert_eq!(undone["document"]["bonds"][1]["mark"], Value::Null);
    app.redo(&id, Some("re"), 4).unwrap();
    let redone = serde_json::to_value(app.redo(&id, Some("re2"), 5).unwrap()).unwrap();
    assert_eq!(redone["document"]["bonds"][1]["mark"]["kind"], "wedge");
    let saved = app.save(&id, Some("save"), 6).unwrap();
    let before = serde_json::to_value(&saved.document).unwrap();
    drop(app);
    let reopened = application_at(&directory);
    let after = serde_json::to_value(reopened.document(&id).unwrap().document).unwrap();
    assert_eq!(
        after, before,
        "a reopened service shows the same marks and stereo"
    );
}

#[test]
fn a_value_read_from_a_snapshot_is_a_valid_command_that_changes_nothing() {
    let (app, _directory) = application();
    let id = created(&app);
    let mut commands = with(
        alkene_commands(),
        vec![
            json!({"op": "set_bond_configuration", "bond": "$1", "configuration": {
            "kind": "relative", "begin_neighbor": "a3", "end_neighbor": "a4",
            "relation": "opposite_sides"}}),
        ],
    );
    commands = with(
        commands.as_array().unwrap().clone(),
        vec![
            json!({"op": "set_bond_mark", "bond": "$2", "mark": {"kind": "hash", "narrow": "end"}}),
        ],
    );
    let reply = ok(transact(&app, &id, "r1", 0, &commands));
    let document = &reply["document"];
    let mut echo = Vec::new();
    for bond in document["bonds"].as_array().unwrap() {
        echo.push(json!({"op": "set_bond_mark", "bond": bond["id"], "mark": bond["mark"]}));
    }
    for record in document["stereo"]["atoms"].as_array().unwrap() {
        echo.push(
            json!({"op": "set_atom_configuration", "atom": record["atom"],
                         "configuration": record["configuration"]}),
        );
    }
    for record in document["stereo"]["bonds"].as_array().unwrap() {
        echo.push(
            json!({"op": "set_bond_configuration", "bond": record["bond"],
                         "configuration": record["configuration"]}),
        );
    }
    assert_eq!(
        echo.len(),
        3 + 1,
        "three marks and one double-bond record: {echo:?}"
    );
    let again = ok(transact(&app, &id, "r2", 1, &Value::Array(echo)));
    assert_eq!(again["document"]["revision"], 1, "nothing changed: {again}");
    assert_eq!(again["document"]["stereo"], document["stereo"]);
    assert_eq!(notes(&again), &Vec::<Value>::new());
}

#[test]
fn a_double_bond_end_may_name_its_implicit_hydrogen_and_the_same_bond_stated_either_way_is_one_state()
 {
    let (app, _directory) = application();
    let id = created(&app);
    let reply = ok(transact(
        &app,
        &id,
        "r1",
        0,
        &with(
            alkene_commands(),
            vec![
                json!({"op": "set_bond_configuration", "bond": "b1", "configuration": {
                "kind": "relative", "begin_neighbor": "a3",
                "end_neighbor": "implicit_hydrogen", "relation": "same_side"}}),
            ],
        ),
    ));
    // The end `a2` has one other atom, `a4`; the hydrogen is the position opposite it.
    assert_eq!(
        reply["document"]["stereo"]["bonds"][0]["configuration"],
        json!({"kind": "relative", "begin_neighbor": "a3", "end_neighbor": "a4",
               "relation": "opposite_sides"})
    );
    let restated = ok(transact(
        &app,
        &id,
        "r2",
        1,
        &json!([{"op": "set_bond_configuration", "bond": "b1", "configuration": {
            "kind": "relative", "begin_neighbor": "a3", "end_neighbor": "a4",
            "relation": "opposite_sides"}}]),
    ));
    assert_eq!(
        restated["document"]["revision"], 1,
        "no new revision: {restated}"
    );
}

#[test]
fn null_clears_and_a_missing_key_is_a_malformed_command() {
    let (app, _directory) = application();
    let id = created(&app);
    ok(transact(
        &app,
        &id,
        "r1",
        0,
        &with(halo_commands(), wedge_and_center()),
    ));
    let cleared = ok(transact(
        &app,
        &id,
        "r2",
        1,
        &json!([
            {"op": "set_bond_mark", "bond": "b2", "mark": null},
            {"op": "set_atom_configuration", "atom": "a1", "configuration": null}
        ]),
    ));
    assert_eq!(cleared["document"]["bonds"][1]["mark"], Value::Null);
    assert_eq!(
        cleared["document"]["stereo"],
        json!({"atoms": [], "bonds": [], "groups": []})
    );
    assert_eq!(
        notes(&cleared),
        &Vec::<Value>::new(),
        "the client asked for it"
    );

    // The key is required: leaving it out must not mean "clear".
    let malformed: [Value; 8] = [
        json!({"op": "set_bond_mark", "bond": "b1"}),
        json!({"op": "set_atom_configuration", "atom": "a1"}),
        json!({"op": "set_bond_configuration", "bond": "b1"}),
        // Wavy and crossed have no narrow end; unknown words and fields are refused.
        json!({"op": "set_bond_mark", "bond": "b1", "mark": {"kind": "wavy", "narrow": "begin"}}),
        json!({"op": "set_bond_mark", "bond": "b1", "mark": {"kind": "wedge"}}),
        json!({"op": "set_bond_mark", "bond": "b1", "mark": {"kind": "bold"}}),
        json!({"op": "set_atom_configuration", "atom": "a1", "configuration": {
            "kind": "tetrahedral", "neighbors": ["a2", "a3", "a4"], "turn": "clockwise"}}),
        json!({"op": "set_bond_configuration", "bond": "b1", "configuration": {
            "kind": "relative", "begin_neighbor": "a3", "end_neighbor": "a4",
            "relation": "opposite_side"}}),
    ];
    for command in malformed {
        let request = json!({"request_id": "x", "expected_revision": 2, "commands": [command]});
        assert!(
            serde_json::from_value::<TransactionRequest>(request).is_err(),
            "{command} is read as a command"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Refusals
// ---------------------------------------------------------------------------------------------

/// What a refusal looks like on the wire.
struct Refused {
    reason: Option<&'static str>,
    index: usize,
}

/// A document with `setup` applied, then `commands` sent: the reply must be an
/// `invalid_command` refusal that names `expected`, changes nothing, and is the same when the
/// request is sent again.
fn assert_refusal(name: &str, setup: &Value, commands: &Value, expected: &Refused) {
    let (app, _directory) = application();
    let id = created(&app);
    let revision = ok(transact(&app, &id, "setup", 0, setup))["document"]["revision"]
        .as_u64()
        .unwrap();
    let before = serde_json::to_value(app.document(&id).unwrap()).unwrap();
    let reply = transact(&app, &id, "refused", revision, commands);
    assert_eq!(reply["code"], "invalid_command", "{name}: {reply}");
    assert_eq!(reply["command_index"], expected.index, "{name}: {reply}");
    match expected.reason {
        Some(reason) => assert_eq!(reply["reason"], reason, "{name}: {reply}"),
        None => assert!(reply.get("reason").is_none(), "{name}: no reason: {reply}"),
    }
    assert_eq!(
        serde_json::to_value(app.document(&id).unwrap()).unwrap(),
        before,
        "{name}: a refused transaction changes nothing"
    );
    assert_eq!(
        transact(&app, &id, "refused-2", revision, commands),
        reply,
        "{name}"
    );
}

fn at(index: usize, reason: &'static str) -> Refused {
    Refused {
        reason: Some(reason),
        index,
    }
}

fn bond_with(order: &str) -> Value {
    json!([
        atom_at(0.0, 0.0, "C", None),
        {"op": "add_atom", "element": "C", "position": {"x": 30, "y": 0},
         "bond_to": {"atom": "$0", "order": order}}
    ])
}

fn center_commands(neighbors: usize) -> Value {
    let mut commands = vec![atom_at(0.0, 0.0, "C", None)];
    for n in 0..neighbors {
        commands.push(atom_at(10.0 * (n + 1) as f64, 0.0, "C", Some("$0")));
    }
    Value::Array(commands)
}

fn wedge_on(bond: &str) -> Value {
    json!({"op": "set_bond_mark", "bond": bond, "mark": {"kind": "wedge", "narrow": "begin"}})
}

fn center_with(neighbors: &[&str], turn: &str) -> Value {
    json!({"op": "set_atom_configuration", "atom": "a1", "configuration": {
        "kind": "tetrahedral", "neighbors": neighbors, "turn": turn}})
}

fn relative_on(bond: &str, begin: &str, end: &str) -> Value {
    json!({"op": "set_bond_configuration", "bond": bond, "configuration": {
        "kind": "relative", "begin_neighbor": begin, "end_neighbor": end,
        "relation": "same_side"}})
}

#[test]
fn every_reason_a_command_can_produce_is_reported_with_the_failing_command_index() {
    let marker = json!({"op": "rename", "title": "first command is fine"});
    assert_refusal(
        "wedge on a double bond",
        &bond_with("double"),
        &json!([marker, wedge_on("b1")]),
        &at(1, "stereo_mark_bond_order"),
    );
    assert_refusal(
        "wedge on an aromatic bond",
        &bond_with("aromatic"),
        &json!([wedge_on("b1")]),
        &at(0, "stereo_mark_bond_order"),
    );
    assert_refusal(
        "crossed on a single bond",
        &bond_with("single"),
        &json!([{"op": "set_bond_mark", "bond": "b1", "mark": {"kind": "crossed"}}]),
        &at(0, "stereo_mark_bond_order"),
    );
    assert_refusal(
        "update_bond over a mark",
        &bond_with("single"),
        &json!([marker, {"op": "set_bond_mark", "bond": "b1", "mark": {"kind": "wavy"}},
                {"op": "update_bond", "bond": "b1", "order": "double"}]),
        &at(2, "stereo_mark_bond_order"),
    );
    let tetra = center_commands(4);
    assert_refusal(
        "an atom that is not bonded to the center",
        &with(
            center_commands(4).as_array().unwrap().clone(),
            vec![atom_at(90.0, 90.0, "C", None)],
        ),
        &json!([center_with(&["a2", "a3", "a4", "a6"], "clockwise")]),
        &at(0, "stereo_center_neighbors"),
    );
    assert_refusal(
        "an atom that does not exist",
        &tetra,
        &json!([center_with(&["a2", "a3", "a4", "a9"], "clockwise")]),
        &Refused {
            reason: None,
            index: 0,
        },
    );
    assert_refusal(
        "a repeated neighbor",
        &tetra,
        &json!([center_with(&["a2", "a3", "a4", "a4"], "clockwise")]),
        &at(0, "stereo_center_neighbors"),
    );
    assert_refusal(
        "a hydrogen where the center has four bonds",
        &tetra,
        &json!([center_with(
            &["a2", "a3", "a4", "implicit_hydrogen"],
            "clockwise"
        )]),
        &at(0, "stereo_center_neighbors"),
    );
    assert_refusal(
        "two bonds",
        &center_commands(2),
        &json!([center_with(
            &["a2", "a3", "implicit_hydrogen", "lone_pair"],
            "clockwise"
        )]),
        &at(0, "stereo_center_unsupported"),
    );
    assert_refusal(
        "five bonds",
        &center_commands(5),
        &json!([center_with(&["a2", "a3", "a4", "a5"], "clockwise")]),
        &at(0, "stereo_center_unsupported"),
    );
    assert_refusal(
        "a query atom",
        &json!([{"op": "add_atom", "query": {"kind": "any"}, "position": {"x": 0, "y": 0}},
                atom_at(10.0, 0.0, "C", Some("$0")), atom_at(20.0, 0.0, "C", Some("$0")),
                atom_at(30.0, 0.0, "C", Some("$0"))]),
        &json!([center_with(
            &["a2", "a3", "a4", "implicit_hydrogen"],
            "clockwise"
        )]),
        &at(0, "stereo_center_unsupported"),
    );
    assert_refusal(
        "update_atom turning a configured center into a query",
        &with(
            vec![],
            [
                center_commands(3).as_array().unwrap().clone(),
                vec![center_with(
                    &["a2", "a3", "a4", "lone_pair"],
                    "anticlockwise",
                )],
            ]
            .concat(),
        ),
        &json!([marker, {"op": "update_atom", "atom": "a1", "query": {"kind": "any"}}]),
        &at(1, "stereo_center_unsupported"),
    );
    assert_refusal(
        "a configuration on a single bond",
        &bond_with("single"),
        &json!([{"op": "set_bond_configuration", "bond": "b1", "configuration": {"kind": "unknown"}}]),
        &at(0, "stereo_double_bond_required"),
    );
    let alkene = Value::Array(alkene_commands());
    assert_refusal(
        "update_bond away from a configured double bond",
        &with(alkene_commands(), vec![relative_on("b1", "a3", "a4")]),
        &json!([{"op": "update_bond", "bond": "b1", "order": "single"}]),
        &at(0, "stereo_double_bond_required"),
    );
    assert_refusal(
        "the partner as a reference",
        &alkene,
        &json!([relative_on("b1", "a2", "a4")]),
        &at(0, "stereo_reference_invalid"),
    );
    assert_refusal(
        "an atom that is not on the end",
        &alkene,
        &json!([relative_on("b1", "a4", "a3")]),
        &at(0, "stereo_reference_invalid"),
    );
    // `a1=a2=a3`, `a4` on `a1`, `a5` on `a3`: the first double bond is cumulated.
    let allene = json!([
        atom_at(0.0, 0.0, "C", None),
        {"op": "add_atom", "element": "C", "position": {"x": 30, "y": 0},
         "bond_to": {"atom": "$0", "order": "double"}},
        {"op": "add_atom", "element": "C", "position": {"x": 60, "y": 0},
         "bond_to": {"atom": "$1", "order": "double"}},
        atom_at(-10.0, 10.0, "C", Some("$0")),
        atom_at(70.0, 10.0, "C", Some("$2"))
    ]);
    assert_refusal(
        "relative on an allene",
        &allene,
        &json!([relative_on("b1", "a4", "a3")]),
        &at(0, "stereo_geometry_unsupported"),
    );
    assert_refusal(
        "unknown on an allene",
        &allene,
        &json!([{"op": "set_bond_configuration", "bond": "b1", "configuration": {"kind": "unknown"}}]),
        &at(0, "stereo_geometry_unsupported"),
    );
    assert_refusal(
        "update_bond making a neighbor of a configured bond double",
        &with(alkene_commands(), vec![relative_on("b1", "a3", "a4")]),
        &json!([{"op": "update_bond", "bond": "b3", "order": "triple"}]),
        &at(0, "stereo_geometry_unsupported"),
    );
    // References that name nothing keep their existing handling, with no reason.
    assert_refusal(
        "a $N that created no bond",
        &json!([atom_at(0.0, 0.0, "C", None)]),
        &json!([{"op": "set_bond_mark", "bond": "$0", "mark": null}]),
        &Refused {
            reason: None,
            index: 0,
        },
    );
    assert_refusal(
        "a word that is not a neighbor",
        &tetra,
        &json!([center_with(&["a2", "a3", "a4", "hydrogen"], "clockwise")]),
        &Refused {
            reason: None,
            index: 0,
        },
    );
    assert_refusal(
        "an unknown bond",
        &bond_with("single"),
        &json!([wedge_on("b9")]),
        &Refused {
            reason: None,
            index: 0,
        },
    );
}

#[test]
fn the_documented_reasons_are_exactly_the_domain_codes() {
    let contract = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/CONTRACT.md"
    ))
    .unwrap();
    let codes = StereoError::ALL
        .iter()
        .map(|error| error.code())
        .chain(NotDerivable::ALL.iter().map(|reason| reason.code()));
    for code in codes {
        assert!(
            contract.contains(&format!("`{code}`")),
            "CONTRACT.md does not document the reason {code}"
        );
    }
    // The reason that names stored data is not one a command can produce; the reasons a
    // drawing can fail to determine a configuration are the six `stereo_not_derivable_*`.
    assert_eq!(StereoError::ALL.len(), 7);
    assert_eq!(NotDerivable::ALL.len(), 6);
    assert!(
        !contract.contains("stereo_conflict"),
        "the old name of stereo_record_invalid is gone from the documents"
    );
}

// ---------------------------------------------------------------------------------------------
// Notes
// ---------------------------------------------------------------------------------------------

const NEIGHBORS_CHANGED: &str = "its bonded neighbors changed";
const ATOM_DELETED: &str = "the atom was deleted";
const END_CHANGED: &str = "the bonded neighbors of one of its ends changed";
const BOND_DELETED: &str = "the bond was deleted";

/// The loss entry for the record at `path` (`atoms[a1]`, `bonds[b1]`) cleared for `why`.
fn note(path: &str, why: &str) -> Value {
    let subject = path
        .replace("atoms[", "atom ")
        .replace("bonds[", "bond ")
        .replace(']', "");
    json!({
        "feature": "stereo_configuration",
        "source_path": path,
        "disposition": "dropped",
        "reason": format!("The stereo configuration of {subject} was cleared because {why}. Undoing this edit, while it is still in the history, brings it back."),
        "semantic_impact": "stereo"
    })
}

#[test]
fn clearing_stereo_with_a_neighbor_change_is_reported_once_and_undo_restores_it() {
    let (app, _directory) = application();
    let id = created(&app);
    ok(transact(
        &app,
        &id,
        "r1",
        0,
        &with(halo_commands(), wedge_and_center()),
    ));

    // A neighbor is deleted: the center is cleared and says so; the other bonds keep their marks.
    let deleted = ok(transact(
        &app,
        &id,
        "r2",
        1,
        &json!([{"op": "delete_atoms", "atoms": ["a4"]}]),
    ));
    assert_eq!(
        deleted["document"]["stereo"],
        json!({"atoms": [], "bonds": [], "groups": []})
    );
    assert_eq!(notes(&deleted), &vec![note("atoms[a1]", NEIGHBORS_CHANGED)]);
    // The same request again returns the stored reply, notes included.
    let replayed = ok(transact(
        &app,
        &id,
        "r2",
        1,
        &json!([{"op": "delete_atoms", "atoms": ["a4"]}]),
    ));
    assert_eq!(replayed, deleted);

    // Undo restores the record, and nothing is reported for it; `get` repeats no notes.
    let undone = serde_json::to_value(app.undo(&id, Some("u"), 2).unwrap()).unwrap();
    assert_eq!(
        undone["document"]["stereo"]["atoms"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(notes(&undone), &Vec::<Value>::new());
    let redone = serde_json::to_value(app.redo(&id, Some("re"), 3).unwrap()).unwrap();
    assert_eq!(
        notes(&redone),
        &Vec::<Value>::new(),
        "redo repeats no notes"
    );
    let read = serde_json::to_value(app.document(&id).unwrap()).unwrap();
    assert_eq!(notes(&read), &Vec::<Value>::new());
    app.undo(&id, Some("u2"), 4).unwrap();

    // Deleting the center, adding a bond to it, and an explicit clear.
    let center = ok(transact(
        &app,
        &id,
        "r3",
        5,
        &json!([{"op": "delete_atoms", "atoms": ["a1"]}]),
    ));
    assert_eq!(notes(&center), &vec![note("atoms[a1]", ATOM_DELETED)]);
    app.undo(&id, Some("u3"), 6).unwrap();
    let bonded = ok(transact(
        &app,
        &id,
        "r4",
        7,
        &json!([atom_at(0.0, 0.0, "O", Some("a1"))]),
    ));
    assert_eq!(notes(&bonded), &vec![note("atoms[a1]", NEIGHBORS_CHANGED)]);
    app.undo(&id, Some("u4"), 8).unwrap();
    let explicit = ok(transact(
        &app,
        &id,
        "r5",
        9,
        &json!([{"op": "set_atom_configuration", "atom": "a1", "configuration": null}]),
    ));
    assert_eq!(notes(&explicit), &Vec::<Value>::new());
    app.undo(&id, Some("u5"), 10).unwrap();

    // A record cleared and stated again in one transaction is not listed.
    let restated = ok(transact(
        &app,
        &id,
        "r6",
        11,
        &json!([
            {"op": "delete_atoms", "atoms": ["a4"]},
            atom_at(0.0, 0.0, "I", Some("a1")),
            {"op": "set_atom_configuration", "atom": "a1", "configuration": {
                "kind": "tetrahedral", "neighbors": ["a2", "a3", "$1", "implicit_hydrogen"],
                "turn": "anticlockwise"}}
        ]),
    ));
    assert_eq!(notes(&restated), &Vec::<Value>::new());
    assert_eq!(
        restated["document"]["stereo"]["atoms"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn a_cleared_double_bond_names_its_bond_and_a_deleted_bond_says_so() {
    let (app, _directory) = application();
    let id = created(&app);
    ok(transact(
        &app,
        &id,
        "r1",
        0,
        &with(alkene_commands(), vec![relative_on("b1", "a3", "a4")]),
    ));
    let widened = ok(transact(
        &app,
        &id,
        "r2",
        1,
        &json!([atom_at(0.0, 0.0, "C", Some("a1"))]),
    ));
    assert_eq!(notes(&widened), &vec![note("bonds[b1]", END_CHANGED)]);
    app.undo(&id, Some("u"), 2).unwrap();
    let removed = ok(transact(
        &app,
        &id,
        "r3",
        3,
        &json!([{"op": "delete_bonds", "bonds": ["b1"]}]),
    ));
    assert_eq!(notes(&removed), &vec![note("bonds[b1]", BOND_DELETED)]);
}

#[test]
fn a_transaction_that_changed_nothing_has_no_notes_and_moves_no_revision() {
    let (app, _directory) = application();
    let id = created(&app);
    ok(transact(
        &app,
        &id,
        "r1",
        0,
        &with(halo_commands(), wedge_and_center()),
    ));
    let unchanged = ok(transact(
        &app,
        &id,
        "r2",
        1,
        &json!([{"op": "set_bond_mark", "bond": "b2", "mark": {"kind": "wedge", "narrow": "begin"}}]),
    ));
    assert_eq!(unchanged["document"]["revision"], 1);
    assert_eq!(notes(&unchanged), &Vec::<Value>::new());
}

// ---------------------------------------------------------------------------------------------
// The operation contract
// ---------------------------------------------------------------------------------------------

/// The response schema's coupling of outcome and loss list: `ok` never carries a dropped,
/// rejected, or unknown entry, and `partial` always carries one.
fn assert_outcome_matches_losses(reply: &Value) {
    let losses = reply["loss_report"]
        .as_array()
        .expect("loss_report is a list");
    match reply["outcome"].as_str().expect("an outcome") {
        "ok" => {
            for loss in losses {
                assert!(
                    matches!(
                        loss["disposition"].as_str(),
                        Some("retained" | "normalized")
                    ),
                    "an ok reply carries a {} loss: {reply}",
                    loss["disposition"]
                );
            }
        }
        "partial" => assert!(!losses.is_empty(), "a partial reply with no loss: {reply}"),
        _ => {}
    }
}

fn call(app: &App, request: &str, operation: &str, parameters: &Value) -> Value {
    let body = json!({"contract": 1, "request_id": request, "profile": "preserve-semantics",
                      "operation": operation, "parameters": parameters});
    let reply = serde_json::to_value(app.execute(body.to_string().as_bytes())).unwrap();
    assert_outcome_matches_losses(&reply);
    reply
}

#[test]
fn a_transaction_is_partial_exactly_when_it_cleared_stereo() {
    let (app, _directory) = application();
    let id = created(&app);
    let create = call(
        &app,
        "t1",
        "document.transaction",
        &json!({"document_id": id, "expected_revision": 0,
                "commands": with(halo_commands(), wedge_and_center())}),
    );
    assert_eq!(create["outcome"], "ok", "{create}");
    assert_eq!(create["result"]["notes"], json!([]), "{create}");

    let cleared = call(
        &app,
        "t2",
        "document.transaction",
        &json!({"document_id": id, "expected_revision": 1,
                "commands": [{"op": "delete_atoms", "atoms": ["a2"]}]}),
    );
    assert_eq!(cleared["outcome"], "partial", "{cleared}");
    assert_eq!(
        cleared["loss_report"],
        json!([note("atoms[a1]", NEIGHBORS_CHANGED)])
    );
    assert_eq!(cleared["result"]["notes"], cleared["loss_report"]);

    for (request, operation, parameters) in [
        (
            "t3",
            "history.undo",
            json!({"document_id": id, "expected_revision": 2}),
        ),
        (
            "t4",
            "history.redo",
            json!({"document_id": id, "expected_revision": 3}),
        ),
        (
            "t5",
            "document.save",
            json!({"document_id": id, "expected_revision": 4}),
        ),
        ("t6", "document.snapshot", json!({"document_id": id})),
    ] {
        let reply = call(&app, request, operation, &parameters);
        assert_eq!(reply["outcome"], "ok", "{operation}: {reply}");
        assert_eq!(reply["result"]["notes"], json!([]), "{operation}");
    }
    // A replay returns the partial reply again.
    let replay = call(
        &app,
        "t2",
        "document.transaction",
        &json!({"document_id": id, "expected_revision": 1,
                "commands": [{"op": "delete_atoms", "atoms": ["a2"]}]}),
    );
    assert_eq!(replay, cleared);
}

#[test]
fn a_conversion_over_a_marked_ring_bond_is_refused_whole_without_a_command_index() {
    let (app, _directory) = application();
    let id = created(&app);
    let ring: Vec<Value> = {
        let mut commands = vec![atom_at(0.0, 0.0, "C", None)];
        for (n, order) in ["double", "single", "double", "single", "double"]
            .into_iter()
            .enumerate()
        {
            commands.push(json!({"op": "add_atom", "element": "C",
                "position": {"x": 30.0 * (n + 1) as f64, "y": 0},
                "bond_to": {"atom": format!("${n}"), "order": order}}));
        }
        commands.push(json!({"op": "add_bond", "begin": "$5", "end": "$0", "order": "single"}));
        commands
    };
    ok(transact(&app, &id, "r1", 0, &Value::Array(ring)));
    ok(transact(&app, &id, "r2", 1, &json!([wedge_on("b2")])));
    let parameters = json!({"document_id": id, "expected_revision": 2, "to": "aromatic"});
    let refused = call(&app, "c1", "chem.aromaticity_convert", &parameters);
    assert_eq!(refused["outcome"], "error", "{refused}");
    let details = &refused["diagnostics"][0]["details"];
    assert_eq!(details["reason"], "stereo_mark_bond_order", "{refused}");
    assert!(details.get("command_index").is_none(), "{refused}");
    assert_eq!(
        serde_json::to_value(app.document(&id).unwrap()).unwrap()["document"]["revision"],
        2,
        "the document is unchanged"
    );
    ok(transact(
        &app,
        &id,
        "r3",
        2,
        &json!([{"op": "set_bond_mark", "bond": "b2", "mark": null}]),
    ));
    let converted = call(
        &app,
        "c2",
        "chem.aromaticity_convert",
        &json!({"document_id": id, "expected_revision": 3, "to": "aromatic"}),
    );
    assert_eq!(converted["outcome"], "ok", "{converted}");
}

// ---------------------------------------------------------------------------------------------
// Neighboring operations
// ---------------------------------------------------------------------------------------------

#[test]
fn stereo_made_through_the_wire_needs_consent_to_leave_in_smiles_and_is_written_as_marks_in_mol() {
    let (app, _directory) = application();
    let id = created(&app);
    ok(transact(
        &app,
        &id,
        "r1",
        0,
        &with(halo_commands(), wedge_and_center()),
    ));
    let refused = call(
        &app,
        "smiles",
        "document.export",
        &json!({"document_id": id, "format": "smiles", "style": "recorded"}),
    );
    assert_eq!(
        refused["diagnostics"][0]["code"], "loss_consent_required",
        "{refused}"
    );
    assert_eq!(
        refused["diagnostics"][0]["details"]["features"],
        json!(["stereo_marks", "stereo_configuration"])
    );
    let mol = call(
        &app,
        "mol",
        "document.export",
        &json!({"format": "mol", "records": [{"document_id": id}]}),
    );
    // The center's stated configuration is what its wedge shows, so a MOL file carries it and
    // asks for no consent; the mark is the file's bond stereo column.
    assert_eq!(mol["outcome"], "partial", "{mol}");
    assert!(
        mol["result"]["content"]
            .as_str()
            .unwrap()
            .contains("  1  3  1  1  0  0  0"),
        "{mol}"
    );
}

#[test]
fn an_import_leaves_the_stereo_a_document_already_holds() {
    let (app, _directory) = application();
    let id = created(&app);
    ok(transact(
        &app,
        &id,
        "r1",
        0,
        &with(halo_commands(), wedge_and_center()),
    ));
    let before =
        serde_json::to_value(app.document(&id).unwrap()).unwrap()["document"]["stereo"].clone();
    let preview = call(
        &app,
        "preview",
        "document.import",
        &json!({"document_id": id, "format": "smiles", "source": "CCO",
                "expected_revision": 1, "preview": true}),
    );
    let digest = preview["result"]["preview_digest"].clone();
    let imported = call(
        &app,
        "import",
        "document.import",
        &json!({"document_id": id, "format": "smiles", "source": "CCO",
                "expected_revision": 1, "preview_digest": digest,
                "accept_loss": true}),
    );
    assert_ne!(imported["outcome"], "error", "{imported}");
    let after = serde_json::to_value(app.document(&id).unwrap()).unwrap();
    assert_eq!(after["document"]["stereo"], before);
    assert_eq!(after["notes"], json!([]));
    assert_eq!(imported["result"]["notes"], json!([]));
}
