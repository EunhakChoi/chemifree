//! Recorded atom and bond states through the shared application: the v1 commands and view,
//! the `reason` code of a refused combination in the error envelope and the operation
//! diagnostic, and malformed values refused as malformed bodies.

use chemifree_application::{Application, ApplicationError, CommandDto, ErrorCode};
use chemifree_domain::states::StateError;
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

/// A document holding C1–C2, bond b1 from a1 to a2, at revision 1.
fn pair(app: &App) -> String {
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
            {"op": "add_atom", "element": "C", "position": {"x": 30, "y": 0}, "bond_to": {"atom": "$0", "order": "single"}}
        ])),
    )
    .unwrap();
    id
}

fn view(app: &App, id: &str) -> Value {
    serde_json::to_value(app.document(id).unwrap()).unwrap()["document"].clone()
}

#[test]
fn new_atoms_report_default_states_and_inferred_hydrogens_have_no_count() {
    let (app, _data) = application();
    let id = pair(&app);
    let document = view(&app, &id);
    assert_eq!(
        document["atoms"][0],
        json!({
            "id": "a1", "element": "C", "charge": 0, "isotope": null,
            "position": {"x": 0.0, "y": 0.0},
            "kind": "element", "query": null, "hydrogens": {"mode": "inferred"},
            "radical": "none", "attachment_points": []
        })
    );
    assert_eq!(document["bonds"][0]["query"], Value::Null);
}

#[test]
fn every_state_is_set_by_the_v1_commands_and_shown_in_the_view() {
    let (app, _data) = application();
    let id = pair(&app);
    let edited = app
        .apply_transaction(
            &id,
            None,
            1,
            commands(json!([
                {"op": "update_atom", "atom": "a1", "hydrogens": {"mode": "specified", "count": 3},
                 "radical": "doublet", "attachment_points": [1, 3]},
                {"op": "update_atom", "atom": "a2", "query": {"kind": "list", "elements": ["N", "O", "S", "P"], "negated": true},
                 "hydrogens": {"mode": "unknown"}},
                {"op": "update_bond", "bond": "b1", "order": "query", "query": {"kind": "list", "orders": ["aromatic", "single"]}},
                {"op": "add_atom", "query": {"kind": "halogen"}, "position": {"x": 60, "y": 0},
                 "radical": "triplet", "bond_to": {"atom": "a1", "order": "dative"}},
                {"op": "add_bond", "begin": "a2", "end": "$3", "order": "query", "query": {"kind": "any"}}
            ])),
        )
        .unwrap();
    let document = serde_json::to_value(edited).unwrap()["document"].clone();
    let atom = |index: usize, field: &str| document["atoms"][index][field].clone();
    assert_eq!(
        atom(0, "hydrogens"),
        json!({"mode": "specified", "count": 3})
    );
    assert_eq!(atom(0, "radical"), "doublet");
    assert_eq!(atom(0, "attachment_points"), json!([1, 3]));
    assert_eq!(atom(1, "kind"), "query");
    assert_eq!(
        atom(1, "query"),
        json!({"kind": "list", "elements": ["N", "O", "S", "P"], "negated": true})
    );
    assert_eq!(
        atom(1, "element"),
        "![N,O,S,P]",
        "a query atom's element is its label, not a symbol"
    );
    assert_eq!(atom(1, "hydrogens"), json!({"mode": "unknown"}));
    assert_eq!(
        (atom(2, "kind"), atom(2, "element"), atom(2, "radical")),
        (json!("query"), json!("X"), json!("triplet"))
    );
    let bonds = &document["bonds"];
    assert_eq!(
        (bonds[0]["order"].clone(), bonds[0]["query"].clone()),
        (
            json!("query"),
            json!({"kind": "list", "orders": ["aromatic", "single"]})
        )
    );
    assert_eq!(
        (
            bonds[1]["begin"].clone(),
            bonds[1]["order"].clone(),
            bonds[1]["query"].clone()
        ),
        (json!("a1"), json!("dative"), Value::Null),
        "the bond starts at its donor"
    );
    assert_eq!(bonds[2]["query"], json!({"kind": "any"}));

    // Saved, closed, and reopened from storage, every state is the same.
    app.save(&id, None, 2).unwrap();
    app.close(&id, None, false, None).unwrap();
    let reopened = view(&app, &id);
    assert_eq!(reopened["atoms"], document["atoms"]);
    assert_eq!(reopened["bonds"], document["bonds"]);

    // Undo goes back to the default states, redo returns them.
    let undone = serde_json::to_value(app.undo(&id, None, 2).unwrap()).unwrap();
    assert_eq!(
        undone["document"]["atoms"][1]["hydrogens"],
        json!({"mode": "inferred"})
    );
    assert_eq!(undone["document"]["atoms"][1]["kind"], "element");
    let redone = serde_json::to_value(app.redo(&id, None, 3).unwrap()).unwrap();
    assert_eq!(redone["document"]["atoms"], document["atoms"]);
}

/// A refused combination is `invalid_command` with the failing command's index and its code,
/// and changes nothing.
#[test]
fn refused_combinations_carry_their_reason_and_change_nothing() {
    let cases = [
        (
            json!({"op": "update_atom", "atom": "a1", "hydrogens": {"mode": "specified", "count": 17}}),
            StateError::HydrogenCountOutOfRange,
        ),
        (
            json!({"op": "update_atom", "atom": "a1", "hydrogens": {"mode": "specified", "count": 300}}),
            StateError::HydrogenCountOutOfRange,
        ),
        (
            json!({"op": "update_atom", "atom": "a1", "query": {"kind": "any"}, "charge": 1}),
            StateError::QueryAtomCharge,
        ),
        (
            json!({"op": "update_atom", "atom": "a1", "query": {"kind": "any"}, "isotope": 13}),
            StateError::QueryAtomIsotope,
        ),
        (
            json!({"op": "update_atom", "atom": "a1", "query": {"kind": "list", "elements": []}}),
            StateError::QueryListInvalid,
        ),
        (
            json!({"op": "update_bond", "bond": "b1", "order": "query", "query": {"kind": "list", "orders": ["single", "single"]}}),
            StateError::BondQueryInvalid,
        ),
        (
            json!({"op": "update_bond", "bond": "b1", "order": "query"}),
            StateError::BondQueryMissing,
        ),
        (
            json!({"op": "add_bond", "begin": "a1", "end": "$0", "order": "aromatic", "query": {"kind": "any"}}),
            StateError::BondQueryUnexpected,
        ),
        (
            json!({"op": "update_atom", "atom": "a1", "attachment_points": [2, 1]}),
            StateError::AttachmentPointsInvalid,
        ),
        (
            json!({"op": "update_atom", "atom": "a1", "attachment_points": [5]}),
            StateError::AttachmentPointsInvalid,
        ),
        (
            json!({"op": "add_atom", "element": "C", "query": {"kind": "any"}, "position": {"x": 0, "y": 0}}),
            StateError::AtomKindConflict,
        ),
        (
            json!({"op": "add_atom", "position": {"x": 0, "y": 0}}),
            StateError::AtomKindConflict,
        ),
    ];
    let (app, _data) = application();
    let id = pair(&app);
    let before = view(&app, &id);
    for (command, code) in cases {
        let error: ApplicationError = app
            .apply_transaction(
                &id,
                None,
                1,
                commands(json!([
                    {"op": "add_atom", "element": "O", "position": {"x": 90, "y": 0}},
                    command
                ])),
            )
            .unwrap_err();
        assert_eq!(
            (error.code, error.command_index, error.invalid_state),
            (ErrorCode::InvalidCommand, Some(1), Some(code)),
            "{command}: {error}"
        );
        let envelope = serde_json::to_value(error.to_response()).unwrap();
        assert_eq!(envelope["reason"], code.code(), "{command}");
        assert_eq!(view(&app, &id), before, "{command} changed nothing");
    }
}

#[test]
fn other_invalid_commands_carry_no_reason() {
    let (app, _data) = application();
    let id = pair(&app);
    let error = app
        .apply_transaction(
            &id,
            None,
            1,
            commands(json!([{"op": "update_atom", "atom": "a9", "radical": "doublet"}])),
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidCommand);
    assert_eq!(error.invalid_state, None);
    assert!(
        serde_json::to_value(error.to_response())
            .unwrap()
            .get("reason")
            .is_none()
    );
}

/// Wrong types and unknown values are malformed bodies, refused before any command runs.
#[test]
fn malformed_state_values_are_malformed_commands() {
    for command in [
        json!({"op": "update_atom", "atom": "a1", "radical": "quartet"}),
        json!({"op": "update_atom", "atom": "a1", "hydrogens": {"mode": "implicit"}}),
        json!({"op": "update_atom", "atom": "a1", "hydrogens": {"mode": "inferred", "count": 2}}),
        json!({"op": "update_atom", "atom": "a1", "hydrogens": {"mode": "specified", "count": -1}}),
        json!({"op": "update_atom", "atom": "a1", "query": {"kind": "any", "negated": true}}),
        json!({"op": "update_atom", "atom": "a1", "query": {"kind": "smarts"}}),
        json!({"op": "update_atom", "atom": "a1", "attachment_points": [-1]}),
        json!({"op": "update_bond", "bond": "b1", "order": "quadruple"}),
        json!({"op": "update_bond", "bond": "b1", "order": "query", "query": {"kind": "list", "orders": ["dative"]}}),
    ] {
        let parsed: Result<Vec<CommandDto>, _> = serde_json::from_value(json!([command]));
        assert!(parsed.is_err(), "{command} is refused as a body");
    }
}

#[test]
fn the_operation_diagnostic_carries_the_reason() {
    let (app, _data) = application();
    let id = pair(&app);
    let request = json!({
        "contract": 1, "request_id": "states-1", "profile": "preserve-semantics",
        "operation": "document.transaction",
        "parameters": {"document_id": id, "expected_revision": 1, "commands": [
            {"op": "update_atom", "atom": "a2", "radical": "singlet"},
            {"op": "update_atom", "atom": "a1", "query": {"kind": "heteroatom"}, "charge": -2}
        ]}
    });
    let reply = serde_json::to_value(app.execute(request.to_string().as_bytes())).unwrap();
    assert_eq!(reply["outcome"], "error");
    assert_eq!(reply["diagnostics"][0]["code"], "invalid_command");
    assert_eq!(reply["diagnostics"][0]["path"], "parameters.commands[1]");
    assert_eq!(
        reply["diagnostics"][0]["details"],
        json!({"command_index": 1, "reason": "query_atom_charge"})
    );
    assert_eq!(view(&app, &id)["revision"], 1, "nothing changed");
}
