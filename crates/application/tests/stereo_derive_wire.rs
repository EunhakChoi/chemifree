//! The command `set_configuration_from_drawing` over the wire: its shape, the six
//! `stereo_not_derivable_*` reasons and the existing structural ones with the failing command's
//! index, replay and reuse of a request id, and the model identifier its operation replies
//! carry. Every document is made by transactions sent to the Application, as HTTP and the CLI
//! send them.

use chemifree_application::{Application, CommandDto, TransactionRequest};
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};
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
    app.create_document(Some("create"), Some("Derive"))
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

fn call(app: &App, request: &str, operation: &str, parameters: &Value) -> Value {
    let body = json!({"contract": 1, "request_id": request, "profile": "preserve-semantics",
                      "operation": operation, "parameters": parameters});
    serde_json::to_value(app.execute(body.to_string().as_bytes())).unwrap()
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
fn halo() -> Vec<Value> {
    vec![
        atom_at(100.0, 100.0, "C", None),
        atom_at(100.0, 70.0, "F", Some("$0")),
        atom_at(126.0, 115.0, "Cl", Some("$0")),
        atom_at(74.0, 115.0, "Br", Some("$0")),
    ]
}

fn mark(bond: &str, mark: &Value) -> Value {
    json!({"op": "set_bond_mark", "bond": bond, "mark": mark})
}

fn wedge() -> Value {
    json!({"kind": "wedge", "narrow": "begin"})
}

fn with(mut commands: Vec<Value>, more: Vec<Value>) -> Value {
    commands.extend(more);
    Value::Array(commands)
}

fn e1_clockwise() -> Value {
    json!({"atom": "a1", "configuration": {
        "kind": "tetrahedral",
        "neighbors": ["a2", "a3", "a4", "implicit_hydrogen"], "turn": "clockwise"}})
}

// ---------------------------------------------------------------------------------------------
// The command
// ---------------------------------------------------------------------------------------------

#[test]
fn a_mark_and_the_derivation_in_one_request_store_the_e1_configuration_and_a_repeat_changes_nothing()
 {
    let (app, _directory) = application();
    let id = created(&app);
    // `$2` is the bond made with the third atom (`b2`); `$0` is the carbon.
    let reply = ok(transact(
        &app,
        &id,
        "r1",
        0,
        &with(
            halo(),
            vec![
                mark("$2", &wedge()),
                json!({"op": "set_configuration_from_drawing", "atoms": ["$0"]}),
            ],
        ),
    ));
    assert_eq!(reply["document"]["revision"], 1);
    assert_eq!(
        reply["document"]["stereo"],
        json!({"atoms": [e1_clockwise()], "bonds": []})
    );
    assert_eq!(reply["notes"], json!([]));

    // The repeat, with no list and with the list, changes nothing.
    for (request, command) in [
        (
            "r2",
            json!({"op": "set_configuration_from_drawing", "atoms": ["a1"]}),
        ),
        ("r3", json!({"op": "set_configuration_from_drawing"})),
    ] {
        let again = ok(transact(&app, &id, request, 1, &json!([command])));
        assert_eq!(again["document"]["revision"], 1, "{request}");
        assert_eq!(again["document"], reply["document"], "{request}");
    }
    let session = serde_json::to_value(app.document(&id).unwrap().session).unwrap();
    assert_eq!(session["undo_depth"], 1, "one step for the whole request");

    // Undo removes the mark and the record together; redo brings both back.
    let undone = serde_json::to_value(app.undo(&id, Some("u"), 1).unwrap()).unwrap();
    assert_eq!(
        undone["document"]["stereo"],
        json!({"atoms": [], "bonds": []})
    );
    assert_eq!(undone["document"]["bonds"][1]["mark"], Value::Null);
    let redone = serde_json::to_value(app.redo(&id, Some("re"), 2).unwrap()).unwrap();
    assert_eq!(redone["document"]["stereo"], reply["document"]["stereo"]);
}

#[test]
fn a_wavy_bond_gives_unknown_and_a_crossed_double_bond_gives_unknown_by_name_or_by_scope() {
    let (app, _directory) = application();
    let id = created(&app);
    let alkene = vec![
        atom_at(300.0, 100.0, "C", None),
        json!({"op": "add_atom", "element": "C", "position": {"x": 330, "y": 100},
               "bond_to": {"atom": "$4", "order": "double"}}),
        atom_at(285.0, 80.0, "C", Some("$4")),
        atom_at(345.0, 120.0, "C", Some("$5")),
    ];
    // The halo is a1-a4 (b1-b3), the alkene a5=a6 (b4) with a7 and a8.
    let reply = ok(transact(
        &app,
        &id,
        "r1",
        0,
        &with(
            [halo(), alkene].concat(),
            vec![
                mark("$2", &json!({"kind": "wavy"})),
                mark("b4", &json!({"kind": "crossed"})),
                json!({"op": "set_configuration_from_drawing"}),
            ],
        ),
    ));
    assert_eq!(
        reply["document"]["stereo"],
        json!({
            "atoms": [{"atom": "a1", "configuration": {
                "kind": "unknown", "neighbors": ["a2", "a3", "a4", "implicit_hydrogen"]}}],
            "bonds": [{"bond": "b4", "configuration": {"kind": "unknown"}}]
        })
    );
}

// ---------------------------------------------------------------------------------------------
// Refusals
// ---------------------------------------------------------------------------------------------

/// What a refusal looks like on the wire.
struct Refused {
    reason: Option<&'static str>,
    index: usize,
}

fn at(index: usize, reason: &'static str) -> Refused {
    Refused {
        reason: Some(reason),
        index,
    }
}

/// A document with `setup` applied, then `commands` sent: the reply must be an
/// `invalid_command` refusal that names `expected`, changes nothing, and is the same when the
/// request is sent again. Returns the refusal.
fn assert_refusal(name: &str, setup: &Value, commands: &Value, expected: &Refused) -> Value {
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
    reply
}

fn derive(atoms: &[&str], bonds: &[&str]) -> Value {
    let mut command = json!({"op": "set_configuration_from_drawing"});
    if !atoms.is_empty() {
        command["atoms"] = json!(atoms);
    }
    if !bonds.is_empty() {
        command["bonds"] = json!(bonds);
    }
    command
}

/// `a1=a2` (`b1`) with `a3` on `a1`, plus `a2` bonded to `a4`; `ends` of them are drawn.
fn alkene(a4: bool) -> Value {
    let mut commands = vec![
        atom_at(0.0, 0.0, "C", None),
        json!({"op": "add_atom", "element": "C", "position": {"x": 30, "y": 0},
               "bond_to": {"atom": "$0", "order": "double"}}),
        atom_at(-20.0, 20.0, "C", Some("$0")),
    ];
    if a4 {
        commands.push(atom_at(50.0, 20.0, "C", Some("$1")));
    }
    Value::Array(commands)
}

#[test]
fn every_reason_a_drawing_can_fail_to_determine_is_reported_with_the_failing_command_index() {
    let marker = json!({"op": "rename", "title": "first command is fine"});
    let plain = Value::Array(halo());
    assert_refusal(
        "no mark bears on the center",
        &plain,
        &json!([marker, derive(&["a1"], &[])]),
        &at(1, "stereo_not_derivable_no_marks"),
    );
    // Cl is wedged and Br is wavy, and Br is a terminal atom (not another center).
    assert_refusal(
        "a wavy bond beside a wedge",
        &with(
            halo(),
            vec![mark("$2", &wedge()), mark("$3", &json!({"kind": "wavy"}))],
        ),
        &json!([derive(&["a1"], &[])]),
        &at(0, "stereo_not_derivable_conflicting_marks"),
    );
    // The same marks without a list: the whole command is refused, with the same reason.
    assert_refusal(
        "the same without a list",
        &with(
            halo(),
            vec![mark("$2", &wedge()), mark("$3", &json!({"kind": "wavy"}))],
        ),
        &json!([derive(&[], &[])]),
        &at(0, "stereo_not_derivable_conflicting_marks"),
    );
    // North wedge, south hash, east and west plain: flat.
    let cross = json!([
        atom_at(100.0, 100.0, "C", None),
        atom_at(100.0, 70.0, "C", Some("$0")),
        atom_at(130.0, 100.0, "C", Some("$0")),
        atom_at(100.0, 130.0, "C", Some("$0")),
        atom_at(70.0, 100.0, "C", Some("$0")),
        mark("$1", &wedge()),
        mark("$3", &json!({"kind": "hash", "narrow": "begin"})),
    ]);
    assert_refusal(
        "wedge and hash on opposite bonds",
        &cross,
        &json!([derive(&["a1"], &[])]),
        &at(0, "stereo_not_derivable_degenerate"),
    );
    // Cl drawn on top of the carbon.
    assert_refusal(
        "a neighbor on top of the center",
        &with(
            vec![
                atom_at(100.0, 100.0, "C", None),
                atom_at(100.0, 70.0, "F", Some("$0")),
                atom_at(100.0, 100.0, "Cl", Some("$0")),
                atom_at(74.0, 115.0, "Br", Some("$0")),
            ],
            vec![mark("$2", &wedge())],
        ),
        &json!([derive(&["a1"], &[])]),
        &at(0, "stereo_not_derivable_coincident_atoms"),
    );
    // C(=O)(F)Cl with a wedge: the carbonyl carbon has no nameable fourth position.
    assert_refusal(
        "a carbonyl carbon",
        &json!([
            atom_at(100.0, 100.0, "C", None),
            {"op": "add_atom", "element": "O", "position": {"x": 100, "y": 70},
             "bond_to": {"atom": "$0", "order": "double"}},
            atom_at(126.0, 115.0, "F", Some("$0")),
            atom_at(74.0, 115.0, "Cl", Some("$0")),
            mark("$2", &wedge()),
        ]),
        &json!([derive(&["a1"], &[])]),
        &at(0, "stereo_not_derivable_fourth_position"),
    );
    // `a2` has no neighbor besides `a1`.
    assert_refusal(
        "a double bond end with no other neighbor",
        &alkene(false),
        &json!([derive(&[], &["b1"])]),
        &at(0, "stereo_not_derivable_no_reference"),
    );
    let refusal = assert_refusal(
        "the message names the item",
        &plain,
        &json!([derive(&["a1"], &[])]),
        &at(0, "stereo_not_derivable_no_marks"),
    );
    assert_eq!(
        refusal["message"],
        "command 0: atom a1: no wedge, hash, or wavy mark bears on this center"
    );
}

#[test]
fn a_named_item_the_model_cannot_hold_keeps_its_structural_reason_and_the_rest_keep_none() {
    let marked = with(halo(), vec![mark("$2", &wedge())]);
    assert_refusal(
        "a terminal atom is not a center",
        &marked,
        &json!([derive(&["a2"], &[])]),
        &at(0, "stereo_center_unsupported"),
    );
    assert_refusal(
        "a single bond is not a double bond",
        &marked,
        &json!([derive(&[], &["b1"])]),
        &at(0, "stereo_double_bond_required"),
    );
    // An empty list, a name that is not an identifier, an unknown atom, a `$N` that names
    // nothing: refused without a reason, as for the other commands.
    let none = |index| Refused {
        reason: None,
        index,
    };
    assert_refusal(
        "an empty list",
        &marked,
        &json!([{"op": "set_configuration_from_drawing", "atoms": []}]),
        &none(0),
    );
    assert_refusal(
        "not an identifier",
        &marked,
        &json!([derive(&["atom1"], &[])]),
        &none(0),
    );
    assert_refusal(
        "an unknown atom",
        &marked,
        &json!([derive(&["a9"], &[])]),
        &none(0),
    );
    assert_refusal(
        "a $N that names nothing",
        &marked,
        &json!([derive(&["$7"], &[])]),
        &none(0),
    );
    // Without an instance, with two: refused with no reason; naming a missing one is
    // `object_unknown`.
    let two = with(
        halo(),
        vec![
            mark("$2", &wedge()),
            json!({"op": "add_instance", "offset": {"x": 200, "y": 0}}),
        ],
    );
    let required = assert_refusal(
        "two instances",
        &two,
        &json!([derive(&["a1"], &[])]),
        &none(0),
    );
    assert_eq!(
        required["message"],
        "command 0: the molecule has 2 scene instances; name the `instance` to use"
    );
    assert_refusal(
        "an instance that does not exist",
        &two,
        &json!([{"op": "set_configuration_from_drawing", "atoms": ["a1"], "instance": "s9"}]),
        &at(0, "object_unknown"),
    );
}

#[test]
fn unknown_fields_and_wrong_types_are_malformed_commands() {
    for command in [
        json!({"op": "set_configuration_from_drawing", "atom": "a1"}),
        json!({"op": "set_configuration_from_drawing", "atoms": "a1"}),
        json!({"op": "set_configuration_from_drawing", "bonds": [1]}),
        json!({"op": "set_configuration_from_drawing", "instance": 1}),
        json!({"op": "set_configuration_from_drawing", "turn": "clockwise"}),
    ] {
        let request = json!({"request_id": "x", "expected_revision": 0, "commands": [command]});
        assert!(
            serde_json::from_value::<TransactionRequest>(request).is_err(),
            "{command} is read as a command"
        );
    }
    for command in [
        json!({"op": "set_configuration_from_drawing"}),
        json!({"op": "set_configuration_from_drawing", "atoms": ["a1", "$2"], "bonds": ["b1"],
               "instance": "s1"}),
    ] {
        let request = json!({"request_id": "x", "expected_revision": 0, "commands": [command]});
        assert!(
            serde_json::from_value::<TransactionRequest>(request).is_ok(),
            "{command} is not read"
        );
    }
}

// ---------------------------------------------------------------------------------------------
// The operation reply
// ---------------------------------------------------------------------------------------------

#[test]
fn a_transaction_with_the_command_carries_the_stereo_model_and_replays_and_reuses_like_any_other() {
    let (app, _directory) = application();
    let id = created(&app);
    let commands = with(halo(), vec![mark("$2", &wedge()), derive(&["$0"], &[])]);
    let parameters = json!({"document_id": id, "expected_revision": 0, "commands": commands});
    let first = call(&app, "t1", "document.transaction", &parameters);
    assert_eq!(first["outcome"], "ok", "{first}");
    assert_eq!(first["model_id"], "chemifree-stereo-1");
    assert_eq!(
        first["result"]["document"]["stereo"]["atoms"],
        json!([e1_clockwise()])
    );

    // The same request is answered from the record; another one under the same id is refused.
    assert_eq!(
        call(&app, "t1", "document.transaction", &parameters),
        first,
        "a replay"
    );
    let changed = json!({"document_id": id, "expected_revision": 0,
        "commands": with(halo(), vec![mark("$2", &wedge()),
            json!({"op": "set_configuration_from_drawing", "atoms": ["$0"], "instance": "s1"})])});
    let reused = call(&app, "t1", "document.transaction", &changed);
    assert_eq!(reused["diagnostics"][0]["code"], "request_id_reused");

    // A no-op and a refusal carry the model too, since the command was part of the request;
    // a request without the command does not.
    let unchanged = call(
        &app,
        "t2",
        "document.transaction",
        &json!({"document_id": id, "expected_revision": 1,
                "commands": [derive(&["a1"], &[])]}),
    );
    assert_eq!(unchanged["outcome"], "ok", "{unchanged}");
    assert_eq!(unchanged["model_id"], "chemifree-stereo-1");
    assert_eq!(unchanged["document_revision"], 1);
    let refused = call(
        &app,
        "t3",
        "document.transaction",
        &json!({"document_id": id, "expected_revision": 1,
                "commands": [derive(&["a2"], &[])]}),
    );
    assert_eq!(refused["outcome"], "error", "{refused}");
    assert_eq!(refused["model_id"], "chemifree-stereo-1");
    assert_eq!(refused["diagnostics"][0]["code"], "invalid_command");
    assert_eq!(
        refused["diagnostics"][0]["details"]["reason"],
        "stereo_center_unsupported"
    );
    assert_eq!(refused["diagnostics"][0]["details"]["command_index"], 0);
    let rename = call(
        &app,
        "t4",
        "document.transaction",
        &json!({"document_id": id, "expected_revision": 1,
                "commands": [{"op": "rename", "title": "Other"}]}),
    );
    assert_eq!(rename["outcome"], "ok", "{rename}");
    assert!(
        rename.get("model_id").is_none_or(Value::is_null),
        "a transaction without the command names no stereo model: {rename}"
    );
}

// ---------------------------------------------------------------------------------------------
// The documentation
// ---------------------------------------------------------------------------------------------

/// The `json` code blocks of the CONTRACT section on deriving a configuration, in order.
fn contract_examples() -> Vec<Value> {
    let contract = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/CONTRACT.md"
    ))
    .unwrap();
    let start = contract
        .find("### Deriving a configuration from a drawing")
        .expect("the section exists");
    let section = &contract[start..];
    let end = section.find("\n## ").unwrap_or(section.len());
    section[..end]
        .split("```json\n")
        .skip(1)
        .map(|block| {
            let body = block.split("```").next().unwrap();
            serde_json::from_str(body).unwrap_or_else(|error| panic!("{error}: {body}"))
        })
        .collect()
}

#[test]
fn the_examples_in_the_contract_are_what_the_service_sends() {
    let examples = contract_examples();
    assert_eq!(examples.len(), 2, "one request and one refusal");

    let (app, _directory) = application();
    let id = created(&app);
    ok(transact(&app, &id, "setup", 0, &Value::Array(halo())));
    let request = &examples[0];
    let reply = ok(transact(
        &app,
        &id,
        request["request_id"].as_str().unwrap(),
        request["expected_revision"].as_u64().unwrap(),
        &request["commands"],
    ));
    assert_eq!(reply["document"]["revision"], 2);
    assert_eq!(
        reply["document"]["stereo"]["atoms"],
        json!([e1_clockwise()])
    );
    // Sending only the second command again returns revision 2 unchanged.
    let again = ok(transact(
        &app,
        &id,
        "again",
        2,
        &json!([request["commands"][1]]),
    ));
    assert_eq!(again["document"]["revision"], 2);

    let (app, _directory) = application();
    let id = created(&app);
    ok(transact(&app, &id, "setup", 0, &Value::Array(halo())));
    let refusal = transact(
        &app,
        &id,
        "refused",
        1,
        &json!([{"op": "set_configuration_from_drawing", "atoms": ["a1"]}]),
    );
    assert_eq!(refusal, examples[1]);
}
