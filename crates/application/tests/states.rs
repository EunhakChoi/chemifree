//! Hydrogen, radical, query, bond-kind, and attachment-point states through the v1 calls and
//! operation contract 1: recorded, reported, saved, and reopened as given, and refused with
//! the same stable reason in both when they do not hold together.

use chemifree_application::{Application, CommandDto, CommandReason};
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};
use tempfile::TempDir;

type App = Application<LocalCapabilities, FileDocumentRepository>;

fn application() -> (App, TempDir) {
    let data = tempfile::tempdir().unwrap();
    let app = Application::new(LocalCapabilities, FileDocumentRepository::new(data.path()));
    (app, data)
}

fn commands(value: Value) -> Vec<CommandDto> {
    serde_json::from_value(value).unwrap()
}

/// A new document holding C (a1) – N (a2) joined by b1.
fn drawn(app: &App) -> String {
    let id = app
        .create_document(None, Some("States"))
        .unwrap()
        .document
        .id;
    app.apply_transaction(
        &id,
        None,
        0,
        commands(json!([
            {"op": "add_atom", "element": "C", "position": {"x": 0, "y": 0}},
            {"op": "add_atom", "element": "N", "position": {"x": 30, "y": 0},
             "bond_to": {"atom": "$0", "order": "single"}}
        ])),
    )
    .unwrap();
    id
}

fn view(app: &App, id: &str) -> Value {
    serde_json::to_value(app.document(id).unwrap()).unwrap()["document"].clone()
}

#[test]
fn new_atoms_report_inferred_hydrogens_without_a_count() {
    let (app, _data) = application();
    let id = drawn(&app);
    let carbon = &view(&app, &id)["atoms"][0];
    assert_eq!(
        *carbon,
        json!({
            "id": "a1", "element": "C", "charge": 0, "isotope": null,
            "position": {"x": 0.0, "y": 0.0},
            "kind": "element", "hydrogens": {"mode": "inferred"}, "radical": "none",
            "attachment_points": []
        }),
        "no count is reported for inferred hydrogens, and an element atom has no query"
    );
    let bond = &view(&app, &id)["bonds"][0];
    assert_eq!(
        *bond,
        json!({"id": "b1", "begin": "a1", "end": "a2", "order": "single"})
    );
}

#[test]
fn every_state_is_reported_saved_and_reopened_as_given() {
    let (app, data) = application();
    let id = drawn(&app);
    let response = app
        .apply_transaction(
            &id,
            None,
            1,
            commands(json!([
                {"op": "update_atom", "atom": "a1", "hydrogens": {"mode": "specified", "count": 3},
                 "radical": "doublet", "attachment_points": [2, 1]},
                {"op": "update_atom", "atom": "a2",
                 "query": {"kind": "list", "elements": ["O", "N"], "negated": true},
                 "hydrogens": {"mode": "unknown"}},
                {"op": "update_bond", "bond": "b1", "order": "query",
                 "query": {"kind": "list", "orders": ["double", "aromatic"]}},
                {"op": "add_atom", "query": {"kind": "halogen"}, "position": {"x": 60, "y": 0},
                 "bond_to": {"atom": "a1", "order": "dative"}, "radical": "unknown"},
                {"op": "add_bond", "begin": "a2", "end": "$3", "order": "aromatic"}
            ])),
        )
        .unwrap();
    let expected_atoms = json!([
        {"id": "a1", "element": "C", "kind": "element",
         "hydrogens": {"mode": "specified", "count": 3}, "radical": "doublet",
         "attachment_points": [1, 2]},
        {"id": "a2", "element": "![O,N]", "kind": "query",
         "query": {"kind": "list", "elements": ["O", "N"], "negated": true},
         "hydrogens": {"mode": "unknown"}, "radical": "none", "attachment_points": []},
        {"id": "a3", "element": "X", "kind": "query", "query": {"kind": "halogen"},
         "hydrogens": {"mode": "inferred"}, "radical": "unknown", "attachment_points": []}
    ]);
    let expected_bonds = json!([
        {"id": "b1", "begin": "a1", "end": "a2", "order": "query",
         "query": {"kind": "list", "orders": ["double", "aromatic"]}},
        {"id": "b2", "begin": "a1", "end": "a3", "order": "dative"},
        {"id": "b3", "begin": "a2", "end": "a3", "order": "aromatic"}
    ]);
    let check = |document: &Value, when: &str| {
        let atoms: Vec<Value> = document["atoms"]
            .as_array()
            .unwrap()
            .iter()
            .map(|atom| {
                let mut atom = atom.clone();
                let fields = atom.as_object_mut().unwrap();
                for left_out in ["charge", "isotope", "position"] {
                    fields.remove(left_out);
                }
                atom
            })
            .collect();
        assert_eq!(Value::from(atoms), expected_atoms, "{when}");
        assert_eq!(document["bonds"], expected_bonds, "{when}");
    };
    check(
        &serde_json::to_value(&response).unwrap()["document"],
        "edited",
    );
    app.save(&id, None, 2).unwrap();
    let reopened = Application::new(LocalCapabilities, FileDocumentRepository::new(data.path()));
    check(&view(&reopened, &id), "reopened");

    // Omitted fields stay as they are; undo restores every one.
    reopened
        .apply_transaction(
            &id,
            None,
            2,
            commands(json!([{"op": "update_atom", "atom": "a1", "charge": 1}])),
        )
        .unwrap();
    assert_eq!(
        view(&reopened, &id)["atoms"][0]["hydrogens"],
        json!({"mode": "specified", "count": 3})
    );
    reopened.undo(&id, None, 3).unwrap();
    reopened.undo(&id, None, 4).unwrap();
    let undone = view(&reopened, &id);
    assert_eq!(
        undone["atoms"][1]["kind"], "element",
        "undo after reopening restores the element"
    );
    assert_eq!(undone["bonds"][0]["order"], "single");
    assert!(undone["bonds"][0].get("query").is_none());
}

/// Each refused combination: its commands, the failing index, and the reason code.
fn refusals() -> Vec<(Value, usize, &'static str)> {
    let add = json!({"op": "add_atom", "element": "S", "position": {"x": 90, "y": 0}});
    let with = |command: Value| json!([add.clone(), command]);
    vec![
        (
            with(
                json!({"op": "update_atom", "atom": "a1", "hydrogens": {"mode": "specified", "count": 17}}),
            ),
            1,
            "hydrogen_count_out_of_range",
        ),
        (
            with(
                json!({"op": "update_atom", "atom": "a1", "hydrogens": {"mode": "specified", "count": 4_294_967_296_u64}}),
            ),
            1,
            "hydrogen_count_out_of_range",
        ),
        (
            with(json!({"op": "update_atom", "atom": "a1", "hydrogens": {"mode": "specified"}})),
            1,
            "hydrogen_count_missing",
        ),
        (
            with(
                json!({"op": "update_atom", "atom": "a1", "hydrogens": {"mode": "inferred", "count": 0}}),
            ),
            1,
            "hydrogen_count_unexpected",
        ),
        (
            with(json!({"op": "update_atom", "atom": "a1", "query": {"kind": "any"}, "charge": 1})),
            1,
            "query_atom_charge",
        ),
        (
            with(
                json!({"op": "update_atom", "atom": "a1", "query": {"kind": "halogen"}, "isotope": 37}),
            ),
            1,
            "query_atom_isotope",
        ),
        (
            with(
                json!({"op": "update_atom", "atom": "a1", "query": {"kind": "list", "elements": []}}),
            ),
            1,
            "query_list_empty",
        ),
        (
            with(
                json!({"op": "update_atom", "atom": "a1", "query": {"kind": "list", "elements": ["N", "N"]}}),
            ),
            1,
            "query_list_repeated",
        ),
        (
            with(
                json!({"op": "update_atom", "atom": "a1", "query": {"kind": "list",
                "elements": ["H", "B", "C", "N", "O", "F", "P", "S", "Cl", "Br", "I"]}}),
            ),
            1,
            "query_list_too_long",
        ),
        (
            with(
                json!({"op": "update_atom", "atom": "a1", "query": {"kind": "any", "negated": true}}),
            ),
            1,
            "query_list_unexpected",
        ),
        (
            with(
                json!({"op": "update_atom", "atom": "a1", "element": "O", "query": {"kind": "any"}}),
            ),
            1,
            "element_and_query",
        ),
        (
            json!([{"op": "add_atom", "position": {"x": 0, "y": 0}}]),
            0,
            "atom_kind_missing",
        ),
        (
            with(json!({"op": "update_bond", "bond": "b1", "order": "query"})),
            1,
            "bond_query_missing",
        ),
        (
            with(
                json!({"op": "add_bond", "begin": "a2", "end": "$0", "order": "single", "query": {"kind": "any"}}),
            ),
            1,
            "bond_query_unexpected",
        ),
        (
            with(
                json!({"op": "update_bond", "bond": "b1", "order": "query", "query": {"kind": "list", "orders": ["single", "single"]}}),
            ),
            1,
            "query_list_repeated",
        ),
        (
            with(json!({"op": "update_atom", "atom": "a1", "attachment_points": [5]})),
            1,
            "attachment_point_out_of_range",
        ),
        (
            with(json!({"op": "update_atom", "atom": "a1", "attachment_points": [2, 2]})),
            1,
            "attachment_point_repeated",
        ),
    ]
}

#[test]
fn refused_combinations_carry_their_reason_and_change_nothing() {
    let (app, _data) = application();
    let id = drawn(&app);
    let before = view(&app, &id);
    let mut seen = Vec::new();
    for (body, index, reason) in refusals() {
        let error = app
            .apply_transaction(&id, None, 1, commands(body.clone()))
            .unwrap_err();
        let envelope = serde_json::to_value(error.to_response()).unwrap();
        assert_eq!(
            (
                &envelope["code"],
                &envelope["command_index"],
                &envelope["reason"]
            ),
            (&json!("invalid_command"), &json!(index), &json!(reason)),
            "{body}: {envelope}"
        );
        assert_eq!(view(&app, &id), before, "{body} changed nothing");
        seen.push(reason);
    }
    for reason in CommandReason::ALL {
        assert!(
            seen.contains(&reason.as_str()),
            "{} has a case",
            reason.as_str()
        );
    }
}

#[test]
fn the_operation_contract_reports_the_same_reason_in_its_diagnostic() {
    let (app, _data) = application();
    let id = drawn(&app);
    for (number, (body, index, reason)) in refusals().into_iter().enumerate() {
        let request = json!({
            "contract": 1, "request_id": format!("refused-{number}"),
            "profile": "preserve-semantics", "operation": "document.transaction",
            "parameters": {"document_id": id, "expected_revision": 1, "commands": body}
        });
        let reply = serde_json::to_value(app.execute(request.to_string().as_bytes())).unwrap();
        assert_eq!(reply["outcome"], "error", "{reply}");
        let diagnostic = &reply["diagnostics"][0];
        assert_eq!(diagnostic["code"], "invalid_command");
        assert_eq!(diagnostic["path"], format!("parameters.commands[{index}]"));
        assert_eq!(
            diagnostic["details"],
            json!({"command_index": index, "reason": reason}),
            "{reply}"
        );
    }
    assert_eq!(view(&app, &id)["revision"], 1, "nothing changed");
}

#[test]
fn other_invalid_commands_carry_no_reason() {
    let (app, _data) = application();
    let id = drawn(&app);
    let error = app
        .apply_transaction(
            &id,
            None,
            1,
            commands(json!([{"op": "update_atom", "atom": "a9", "charge": 1}])),
        )
        .unwrap_err();
    let envelope = serde_json::to_value(error.to_response()).unwrap();
    assert_eq!(envelope["code"], "invalid_command");
    assert!(envelope.get("reason").is_none(), "{envelope}");
}
