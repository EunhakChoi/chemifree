//! Enhanced-stereo groups over the wire: the eight commands (shape, references, every refusal
//! with its command index), the `stereo.groups` snapshot list, the notes and the reply outcome
//! a change of meaning leaves, undo and redo, the consent a SMILES export needs, and the
//! operations that ignore groups. Every document is made by transactions sent to the
//! Application, as HTTP and the CLI send them.

use chemifree_application::{Application, CommandDto};
use chemifree_domain::stereo_group::StereoGroupError;
use chemifree_providers::interchange_codec::LocalInterchangeCodec;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities, OpenSmilesCodec, V2000Codec};
use serde_json::{Value, json};
use tempfile::TempDir;

type App = Application<LocalCapabilities, FileDocumentRepository>;

fn application() -> (App, TempDir) {
    let directory = tempfile::tempdir().unwrap();
    let app = Application::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    )
    .with_ctfile_codec(V2000Codec)
    .with_interchange_codec(LocalInterchangeCodec)
    .with_smiles_codec(OpenSmilesCodec);
    (app, directory)
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

/// `count` blocks of a carbon with F, Cl and Br (atoms `a1`-`a4`, `a5`-`a8`, ...), each carbon
/// a stated tetrahedral center: the centers are `a1`, `a5`, `a9` and `a13`.
fn centers(count: usize) -> Value {
    let mut commands = Vec::new();
    for block in 0..count {
        let (x, y) = (100.0 * block as f64, 100.0);
        commands.push(json!({"op": "add_atom", "element": "C", "position": {"x": x, "y": y}}));
        for (dx, dy, element) in [(0.0, -30.0, "F"), (26.0, 15.0, "Cl"), (-26.0, 15.0, "Br")] {
            commands.push(json!({"op": "add_atom", "element": element,
                "position": {"x": x + dx, "y": y + dy},
                "bond_to": {"atom": format!("${}", 4 * block), "order": "single"}}));
        }
    }
    for block in 0..count {
        let first = 4 * block;
        commands.push(
            json!({"op": "set_atom_configuration", "atom": format!("${first}"),
            "configuration": {"kind": "tetrahedral",
                "neighbors": [format!("${}", first + 1), format!("${}", first + 2),
                              format!("${}", first + 3), "implicit_hydrogen"],
                "turn": "clockwise"}}),
        );
    }
    Value::Array(commands)
}

/// A document with four centers, at revision 1.
fn four_centers() -> (App, TempDir, String) {
    let (app, directory) = application();
    let id = app
        .create_document(Some("create"), Some("Groups"))
        .unwrap()
        .document
        .id;
    ok(transact(&app, &id, "centers", 0, &centers(4)));
    (app, directory, id)
}

fn create(kind: &str, atoms: &[&str], number: Value) -> Value {
    json!({"op": "create_stereo_group", "kind": kind, "atoms": atoms, "number": number})
}

fn groups(reply: &Value) -> &Value {
    &reply["document"]["stereo"]["groups"]
}

fn group_json(
    id: &str,
    kind: &str,
    stored: Value,
    number: Value,
    label: &str,
    members: &[&str],
) -> Value {
    json!({"id": id, "kind": kind, "stored_number": stored, "number": number,
           "label": label, "members": members})
}

fn revision(app: &App, id: &str) -> u64 {
    serde_json::to_value(app.document(id).unwrap()).unwrap()["document"]["revision"]
        .as_u64()
        .unwrap()
}

// ---------------------------------------------------------------------------------------------
// The snapshot and the numbers
// ---------------------------------------------------------------------------------------------

#[test]
fn groups_are_listed_in_id_order_with_the_stored_and_the_interpreted_number_and_a_label() {
    let (app, _directory, id) = four_centers();
    let before = serde_json::to_value(app.document(&id).unwrap()).unwrap();
    assert_eq!(
        groups(&before),
        &json!([]),
        "always present, empty when none"
    );

    let reply = ok(transact(
        &app,
        &id,
        "g",
        1,
        &json!([
            create("or", &["a5", "a1"], json!(2)),
            create("and", &["a9"], Value::Null),
            create("absolute", &["a13"], Value::Null),
        ]),
    ));
    let expected = json!([
        group_json("e1", "or", json!(2), json!(2), "or2", &["a1", "a5"]),
        group_json("e2", "and", Value::Null, json!(0), "and0", &["a9"]),
        group_json("e3", "absolute", Value::Null, Value::Null, "abs", &["a13"]),
    ]);
    assert_eq!(groups(&reply), &expected);
    assert_eq!(reply["notes"], json!([]), "making groups reports nothing");
    let read = serde_json::to_value(app.document(&id).unwrap()).unwrap();
    assert_eq!(groups(&read), &expected, "a later read shows the same list");
}

#[test]
fn numbers_are_stored_as_stated_and_an_omitted_number_reads_as_zero() {
    let (app, _directory, id) = four_centers();
    let reply = ok(transact(
        &app,
        &id,
        "n",
        1,
        &json!([
            create("or", &["a1"], json!(0)),
            create("or", &["a5"], json!(-3)),
            create("and", &["a9"], json!(i32::MAX)),
            create("and", &["a13"], json!(i32::MIN)),
        ]),
    ));
    let shown: Vec<(Value, Value, String)> = groups(&reply)
        .as_array()
        .unwrap()
        .iter()
        .map(|g| {
            (
                g["stored_number"].clone(),
                g["number"].clone(),
                g["label"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert_eq!(
        shown,
        [
            (json!(0), json!(0), "or0".to_owned()),
            (json!(-3), json!(-3), "or-3".to_owned()),
            (json!(i32::MAX), json!(i32::MAX), "and2147483647".to_owned()),
            (
                json!(i32::MIN),
                json!(i32::MIN),
                "and-2147483648".to_owned()
            ),
        ]
    );
    // Clearing a number stores none, which reads as 0.
    let cleared = ok(transact(
        &app,
        &id,
        "clear",
        2,
        &json!([{"op": "set_stereo_group_number", "group": "e3", "number": null}]),
    ));
    assert_eq!(
        groups(&cleared)[2],
        group_json("e3", "and", Value::Null, json!(0), "and0", &["a9"])
    );
}

#[test]
fn a_number_outside_the_32_bit_range_is_refused_with_its_command_index() {
    let (app, _directory, id) = four_centers();
    for (name, number) in [("above", 2_147_483_648_i64), ("below", -2_147_483_649_i64)] {
        for (position, command) in [
            ("create", create("or", &["a1"], json!(number))),
            (
                "set",
                json!({"op": "set_stereo_group_number", "group": "$0", "number": number}),
            ),
            (
                "split",
                json!({"op": "split_stereo_group", "group": "$0", "atoms": ["a5"], "number": number}),
            ),
        ] {
            let commands = if position == "create" {
                json!([command])
            } else {
                json!([create("or", &["a1", "a5"], json!(1)), command])
            };
            let reply = transact(&app, &id, &format!("{name}-{position}"), 1, &commands);
            assert_eq!(
                reply["code"], "invalid_command",
                "{name} {position}: {reply}"
            );
            assert_eq!(
                reply["reason"], "stereo_group_number_range",
                "{name} {position}"
            );
            assert_eq!(
                reply["command_index"],
                if position == "create" { 0 } else { 1 },
                "{name} {position}"
            );
        }
    }
    assert_eq!(revision(&app, &id), 1, "nothing was changed");
    // Beyond 64 bits the request does not read at all.
    let huge = call(
        &app,
        "huge",
        "document.transaction",
        &json!({"document_id": id, "expected_revision": 1,
                "commands": [{"op": "create_stereo_group", "kind": "or", "atoms": ["a1"],
                              "number": 9_223_372_036_854_775_808_u64}]}),
    );
    assert_eq!(huge["diagnostics"][0]["code"], "invalid_request", "{huge}");
}

#[test]
fn the_same_kind_and_number_is_one_group_and_other_pairs_are_distinct() {
    let (app, _directory, id) = four_centers();
    // Or 1 and And 1 are two groups; And and Or without a number are two groups.
    let reply = ok(transact(
        &app,
        &id,
        "distinct",
        1,
        &json!([
            create("or", &["a1"], json!(1)),
            create("and", &["a5"], json!(1)),
            create("or", &["a9"], Value::Null),
            create("and", &["a13"], Value::Null),
        ]),
    ));
    assert_eq!(groups(&reply).as_array().unwrap().len(), 4);
    let taken = |request: &str, revision: u64, commands: Value, index: u64| {
        let reply = transact(&app, &id, request, revision, &commands);
        assert_eq!(reply["code"], "invalid_command", "{request}: {reply}");
        assert_eq!(
            reply["reason"], "stereo_group_number_taken",
            "{request}: {reply}"
        );
        assert_eq!(reply["command_index"], index, "{request}");
    };
    // e1 is Or 1 and e3 is Or (no number). A new Or needs a number neither has.
    taken(
        "dup-number",
        2,
        json!([{"op": "set_stereo_group_number", "group": "e3", "number": 1}]),
        0,
    );
    taken(
        "dup-kind",
        2,
        json!([{"op": "set_stereo_group_kind", "group": "e2", "kind": "or"}]),
        0,
    );
    // An explicit 0 collides with the group whose omitted number reads as 0, and the reverse.
    taken(
        "zero-vs-omitted",
        2,
        json!([{"op": "set_stereo_group_number", "group": "e1", "number": 0}]),
        0,
    );
    taken(
        "omitted-vs-zero",
        2,
        json!([
            {"op": "set_stereo_group_number", "group": "e3", "number": 7},
            {"op": "set_stereo_group_number", "group": "e1", "number": null},
            {"op": "set_stereo_group_number", "group": "e3", "number": 0}
        ]),
        2,
    );
}

// ---------------------------------------------------------------------------------------------
// Refusals, shapes and references
// ---------------------------------------------------------------------------------------------

/// A document with groups `e1` (or 1: a1, a5), `e2` (or 2: a13), the free center `a9` and the
/// center-less `a2`, at revision 2, then `commands` sent: the reply must be the `invalid_command` refusal for
/// `reason` at `index`, change nothing and repeat on a second request.
fn refused(name: &str, commands: &Value, index: u64, reason: &str) {
    let (app, _directory, id) = four_centers();
    ok(transact(
        &app,
        &id,
        "setup",
        1,
        &json!([
            create("or", &["a1", "a5"], json!(1)),
            create("or", &["a13"], json!(2))
        ]),
    ));
    let before = serde_json::to_value(app.document(&id).unwrap()).unwrap();
    let reply = transact(&app, &id, "refused", 2, commands);
    assert_eq!(reply["code"], "invalid_command", "{name}: {reply}");
    assert_eq!(reply["reason"], reason, "{name}: {reply}");
    assert_eq!(reply["command_index"], index, "{name}: {reply}");
    assert_eq!(
        serde_json::to_value(app.document(&id).unwrap()).unwrap(),
        before,
        "{name}: a refused transaction changes nothing, earlier commands included"
    );
    assert_eq!(
        transact(&app, &id, "refused-2", 2, commands),
        reply,
        "{name}"
    );
    // The operation form carries the same reason and index in its diagnostic.
    let operation = call(
        &app,
        "op",
        "document.transaction",
        &json!({"document_id": id, "expected_revision": 2, "commands": commands}),
    );
    assert_eq!(operation["outcome"], "error", "{name}: {operation}");
    let details = &operation["diagnostics"][0]["details"];
    assert_eq!(
        operation["diagnostics"][0]["code"], "invalid_command",
        "{name}"
    );
    assert_eq!(details["reason"], reason, "{name}: {operation}");
    assert_eq!(details["command_index"], index, "{name}: {operation}");
}

#[test]
fn every_group_refusal_names_its_reason_and_the_failing_command() {
    let rename = json!({"op": "rename", "title": "First"});
    let cases: Vec<(&str, Value, u64, StereoGroupError)> = vec![
        (
            "empty",
            json!([rename, create("or", &[], json!(5))]),
            1,
            StereoGroupError::Empty,
        ),
        (
            "add nothing",
            json!([{"op": "add_stereo_group_members", "group": "e1", "atoms": []}]),
            0,
            StereoGroupError::Empty,
        ),
        (
            "remove everything",
            json!([{"op": "remove_stereo_group_members", "group": "e2", "atoms": ["a13"]}]),
            0,
            StereoGroupError::Empty,
        ),
        (
            "split everything",
            json!([{"op": "split_stereo_group", "group": "e1", "atoms": ["a1", "a5"], "number": 9}]),
            0,
            StereoGroupError::Empty,
        ),
        (
            "an atom with no configuration",
            json!([rename, create("and", &["a2"], json!(1))]),
            1,
            StereoGroupError::MemberNotCenter,
        ),
        (
            "an atom that does not exist",
            json!([create("and", &["a99"], json!(1))]),
            0,
            StereoGroupError::MemberNotCenter,
        ),
        (
            "taken",
            json!([create("and", &["a1"], json!(1))]),
            0,
            StereoGroupError::MemberTaken,
        ),
        (
            "add a taken atom",
            json!([{"op": "add_stereo_group_members", "group": "e2", "atoms": ["a1"]}]),
            0,
            StereoGroupError::MemberTaken,
        ),
        (
            "remove a stranger",
            json!([{"op": "remove_stereo_group_members", "group": "e1", "atoms": ["a13"]}]),
            0,
            StereoGroupError::MemberAbsent,
        ),
        (
            "split a stranger",
            json!([{"op": "split_stereo_group", "group": "e1", "atoms": ["a13"], "number": 4}]),
            0,
            StereoGroupError::MemberAbsent,
        ),
        (
            "absolute with a number",
            json!([create("absolute", &["a9"], json!(1))]),
            0,
            StereoGroupError::NumberNotAllowed,
        ),
        (
            "to absolute with a number",
            json!([{"op": "set_stereo_group_kind", "group": "e2", "kind": "absolute"}]),
            0,
            StereoGroupError::NumberNotAllowed,
        ),
        (
            "a number taken",
            json!([{"op": "set_stereo_group_number", "group": "e2", "number": 1}]),
            0,
            StereoGroupError::NumberTaken,
        ),
        (
            "a split number taken",
            json!([{"op": "split_stereo_group", "group": "e1", "atoms": ["a1"], "number": 2}]),
            0,
            StereoGroupError::NumberTaken,
        ),
        (
            "a second absolute group",
            json!([
                {"op": "set_stereo_group_number", "group": "e2", "number": null},
                {"op": "set_stereo_group_kind", "group": "e2", "kind": "absolute"},
                {"op": "set_stereo_group_number", "group": "e1", "number": null},
                {"op": "set_stereo_group_kind", "group": "e1", "kind": "absolute"}
            ]),
            3,
            StereoGroupError::AbsoluteExists,
        ),
        (
            "an unknown group",
            json!([{"op": "delete_stereo_group", "group": "e9"}]),
            0,
            StereoGroupError::Unknown,
        ),
        (
            "merge into itself",
            json!([{"op": "merge_stereo_groups", "into": "e1", "from": "e1"}]),
            0,
            StereoGroupError::Same,
        ),
        (
            "merge across kinds",
            json!([
                {"op": "set_stereo_group_kind", "group": "e2", "kind": "and"},
                {"op": "merge_stereo_groups", "into": "e1", "from": "e2"}
            ]),
            1,
            StereoGroupError::KindMismatch,
        ),
    ];
    // The split of the absolute group needs a document where it can be reached.
    let mut reached = Vec::new();
    for (name, commands, index, error) in &cases {
        refused(name, commands, *index, error.code());
        reached.push(*error);
    }
    let (app, _directory, id) = four_centers();
    ok(transact(
        &app,
        &id,
        "abs",
        1,
        &json!([create("absolute", &["a1", "a5"], Value::Null)]),
    ));
    let split = transact(
        &app,
        &id,
        "split",
        2,
        &json!([{"op": "split_stereo_group", "group": "e1", "atoms": ["a1"], "number": null}]),
    );
    assert_eq!(
        split["reason"],
        StereoGroupError::SplitAbsolute.code(),
        "{split}"
    );
    reached.push(StereoGroupError::SplitAbsolute);
    for error in StereoGroupError::ALL {
        assert!(reached.contains(&error), "{error:?} has no wire case");
    }
}

#[test]
fn commands_with_a_missing_or_unknown_key_or_a_wrong_value_do_not_read() {
    let bad = [
        // `number` is required, with `null` for none.
        json!({"op": "create_stereo_group", "kind": "or", "atoms": ["a1"]}),
        json!({"op": "set_stereo_group_number", "group": "e1"}),
        json!({"op": "split_stereo_group", "group": "e1", "atoms": ["a1"]}),
        // The other keys are required too.
        json!({"op": "create_stereo_group", "atoms": ["a1"], "number": null}),
        json!({"op": "create_stereo_group", "kind": "or", "number": null}),
        json!({"op": "delete_stereo_group"}),
        json!({"op": "set_stereo_group_kind", "group": "e1"}),
        json!({"op": "add_stereo_group_members", "group": "e1"}),
        json!({"op": "remove_stereo_group_members", "atoms": ["a1"]}),
        json!({"op": "merge_stereo_groups", "into": "e1"}),
        // Unknown keys.
        json!({"op": "create_stereo_group", "kind": "or", "atoms": ["a1"], "number": null, "ratio": 1}),
        json!({"op": "delete_stereo_group", "group": "e1", "force": true}),
        // Wrong values: the short label is not a kind; a number is an integer.
        json!({"op": "create_stereo_group", "kind": "abs", "atoms": ["a1"], "number": null}),
        json!({"op": "create_stereo_group", "kind": "or", "atoms": ["a1"], "number": 1.5}),
        json!({"op": "create_stereo_group", "kind": "or", "atoms": ["a1"], "number": "1"}),
        json!({"op": "create_stereo_group", "kind": "or", "atoms": "a1", "number": null}),
    ];
    let (app, _directory, id) = four_centers();
    for command in &bad {
        assert!(
            serde_json::from_value::<CommandDto>(command.clone()).is_err(),
            "read: {command}"
        );
        let reply = call(
            &app,
            "bad",
            "document.transaction",
            &json!({"document_id": id, "expected_revision": 1, "commands": [command]}),
        );
        assert_eq!(reply["outcome"], "error", "{command}: {reply}");
        assert_eq!(
            reply["diagnostics"][0]["code"], "invalid_request",
            "{command}: {reply}"
        );
    }
    assert_eq!(revision(&app, &id), 1);
}

#[test]
fn group_references_name_a_group_made_earlier_in_the_transaction() {
    let (app, _directory, id) = four_centers();
    let reply = ok(transact(
        &app,
        &id,
        "refs",
        1,
        &json!([
            create("or", &["a1", "a5", "a9"], json!(1)),
            {"op": "split_stereo_group", "group": "$0", "atoms": ["a5"], "number": 2},
            {"op": "set_stereo_group_number", "group": "$1", "number": 3},
            {"op": "merge_stereo_groups", "into": "$0", "from": "$1"},
        ]),
    ));
    assert_eq!(
        groups(&reply),
        &json!([group_json(
            "e1",
            "or",
            json!(1),
            json!(1),
            "or1",
            &["a1", "a5", "a9"]
        )])
    );
    for (name, commands, index) in [
        (
            "not-yet-made",
            json!([{"op": "delete_stereo_group", "group": "$0"}]),
            0,
        ),
        (
            "later",
            json!([{"op": "delete_stereo_group", "group": "$1"}, create("or", &["a1"], json!(1))]),
            0,
        ),
        (
            "no-group-made",
            json!([{"op": "rename", "title": "x"}, {"op": "delete_stereo_group", "group": "$0"}]),
            1,
        ),
        (
            "malformed",
            json!([{"op": "delete_stereo_group", "group": "$x"}]),
            0,
        ),
        (
            "leading-zero",
            json!([{"op": "delete_stereo_group", "group": "e01"}]),
            0,
        ),
        (
            "atom-as-group",
            json!([{"op": "delete_stereo_group", "group": "a1"}]),
            0,
        ),
        ("group-as-atom", json!([create("or", &["e1"], json!(1))]), 0),
        ("bond-as-atom", json!([create("or", &["b1"], json!(1))]), 0),
    ] {
        let reply = transact(&app, &id, name, 2, &commands);
        assert_eq!(reply["code"], "invalid_command", "{name}: {reply}");
        assert_eq!(reply["command_index"], index, "{name}: {reply}");
        assert!(reply.get("reason").is_none(), "{name}: no reason: {reply}");
    }
}

// ---------------------------------------------------------------------------------------------
// Notes, outcome, undo and redo
// ---------------------------------------------------------------------------------------------

/// `document.transaction` as an operation, so the outcome and the loss report are seen.
fn operate(app: &App, id: &str, request: &str, revision: u64, commands: &Value) -> Value {
    let reply = call(
        app,
        request,
        "document.transaction",
        &json!({"document_id": id, "expected_revision": revision, "commands": commands}),
    );
    assert_eq!(reply["result"]["notes"], reply["loss_report"], "{reply}");
    reply
}

fn features(reply: &Value) -> Vec<(String, String, String)> {
    reply["result"]["notes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|note| {
            assert_eq!(note["semantic_impact"], "stereo", "{note}");
            (
                note["feature"].as_str().unwrap().to_owned(),
                note["source_path"].as_str().unwrap().to_owned(),
                note["disposition"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

fn note(feature: &str, path: &str, disposition: &str) -> (String, String, String) {
    (feature.to_owned(), path.to_owned(), disposition.to_owned())
}

/// Four centers with `e1` = or 1 (a1, a5, a9) and `e2` = or 2 (a13), at revision 2.
fn grouped() -> (App, TempDir, String) {
    let (app, directory, id) = four_centers();
    ok(transact(
        &app,
        &id,
        "groups",
        1,
        &json!([
            create("or", &["a1", "a5", "a9"], json!(1)),
            create("or", &["a13"], json!(2))
        ]),
    ));
    (app, directory, id)
}

#[test]
fn a_change_of_meaning_is_a_note_and_makes_the_reply_partial_and_a_renumbering_does_not() {
    let quiet = |commands: Value| {
        let (app, _directory, id) = grouped();
        let reply = operate(&app, &id, "quiet", 2, &commands);
        assert_eq!(reply["outcome"], "ok", "{reply}");
        assert_eq!(features(&reply), []);
    };
    quiet(json!([{"op": "set_stereo_group_number", "group": "e1", "number": 7}]));
    quiet(json!([{"op": "set_stereo_group_number", "group": "e1", "number": null}]));
    quiet(json!([{"op": "delete_stereo_group", "group": "e2"}]));
    quiet(json!([{"op": "move_atoms", "atoms": ["a1"], "delta": {"x": 1, "y": 1}}]));

    let noted = |commands: Value, expected: Vec<(String, String, String)>| {
        let (app, _directory, id) = grouped();
        let reply = operate(&app, &id, "noted", 2, &commands);
        assert_eq!(reply["outcome"], "partial", "{reply}");
        assert_eq!(features(&reply), expected, "{reply}");
        assert_eq!(reply["result"]["document"]["revision"], 3);
        reply
    };
    // Split, a removal that leaves two together, and a merge, each of an `or` group.
    let split = noted(
        json!([{"op": "split_stereo_group", "group": "e1", "atoms": ["a9"], "number": 5}]),
        vec![note("stereo_group_relation", "groups[e1]", "dropped")],
    );
    assert!(
        split["result"]["notes"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("a1, a5, a9"),
        "{split}"
    );
    noted(
        json!([{"op": "remove_stereo_group_members", "group": "e1", "atoms": ["a9"]}]),
        vec![note("stereo_group_relation", "groups[e1]", "dropped")],
    );
    let merged = noted(
        json!([{"op": "merge_stereo_groups", "into": "e1", "from": "e2"}]),
        vec![note("stereo_group_relation", "groups[e1]", "normalized")],
    );
    assert!(
        merged["result"]["notes"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("The number 2 of group e2 is not kept"),
        "{merged}"
    );
    // A kind change and the deletion of a group that stated a relation also say so; the
    // deletion of a one-member group says nothing.
    noted(
        json!([{"op": "set_stereo_group_kind", "group": "e1", "kind": "and"}]),
        vec![note("stereo_group_relation", "groups[e1]", "normalized")],
    );
    noted(
        json!([{"op": "delete_stereo_group", "group": "e1"}]),
        vec![note("stereo_group_relation", "groups[e1]", "dropped")],
    );
}

#[test]
fn a_member_that_loses_its_configuration_leaves_its_group_with_a_note_and_undo_brings_it_back() {
    let (app, _directory, id) = grouped();
    let before = groups(&serde_json::to_value(app.document(&id).unwrap()).unwrap()).clone();
    // `null` names the configuration, not the membership; a13 is alone in its group.
    let reply = operate(
        &app,
        &id,
        "clear",
        2,
        &json!([
            {"op": "set_atom_configuration", "atom": "a5", "configuration": null},
            {"op": "set_atom_configuration", "atom": "a13", "configuration": null}
        ]),
    );
    assert_eq!(reply["outcome"], "partial", "{reply}");
    assert_eq!(
        features(&reply),
        [
            note("stereo_group_member", "atoms[a5]", "dropped"),
            note("stereo_group_member", "atoms[a13]", "dropped"),
            note("stereo_group", "groups[e2]", "dropped"),
        ]
    );
    let after = &reply["result"]["document"]["stereo"]["groups"];
    assert_eq!(
        after,
        &json!([group_json(
            "e1",
            "or",
            json!(1),
            json!(1),
            "or1",
            &["a1", "a9"]
        )])
    );

    // Undo brings back the members, with the same group ids, and reports nothing; redo
    // removes them again, also silently, and the ids are not reused for a new group.
    let undone = serde_json::to_value(app.undo(&id, Some("u"), 3).unwrap()).unwrap();
    assert_eq!(groups(&undone), &before);
    assert_eq!(undone["notes"], json!([]));
    let redone = serde_json::to_value(app.redo(&id, Some("r"), 4).unwrap()).unwrap();
    assert_eq!(groups(&redone), after);
    assert_eq!(redone["notes"], json!([]));
    let later = ok(transact(
        &app,
        &id,
        "later",
        5,
        &json!([
            {"op": "set_atom_configuration", "atom": "a5", "configuration": {
                "kind": "tetrahedral",
                "neighbors": ["a6", "a7", "a8", "implicit_hydrogen"], "turn": "clockwise"}},
            create("and", &["a5"], json!(1))
        ]),
    ));
    assert_eq!(groups(&later)[1]["id"], "e3", "e2 is not issued again");
}

#[test]
fn unrelated_edits_keep_the_groups_and_say_nothing() {
    let (app, _directory, id) = grouped();
    let before = groups(&serde_json::to_value(app.document(&id).unwrap()).unwrap()).clone();
    let reply = operate(
        &app,
        &id,
        "edit",
        2,
        &json!([
            {"op": "rename", "title": "Renamed"},
            {"op": "move_atoms", "atoms": ["a1", "a5"], "delta": {"x": 4, "y": 4}},
            {"op": "add_atom", "element": "O", "position": {"x": 400, "y": 400}},
            {"op": "update_atom", "atom": "a2", "isotope": 19}
        ]),
    );
    assert_eq!(reply["result"]["notes"], json!([]), "{reply}");
    assert_eq!(reply["outcome"], "ok");
    assert_eq!(&reply["result"]["document"]["stereo"]["groups"], &before);
}

// ---------------------------------------------------------------------------------------------
// Neighboring operations
// ---------------------------------------------------------------------------------------------

#[test]
fn a_canonical_smiles_export_needs_consent_to_drop_the_groups_and_names_them_all() {
    let (app, _directory, id) = grouped();
    let export = |request: &str, extra: Value| {
        let mut parameters = json!({"document_id": id, "format": "smiles", "style": "canonical"});
        for (key, value) in extra.as_object().unwrap() {
            parameters[key] = value.clone();
        }
        call(&app, request, "document.export", &parameters)
    };
    let refused = export("first", json!({}));
    let diagnostic = &refused["diagnostics"][0];
    assert_eq!(diagnostic["code"], "loss_consent_required", "{refused}");
    assert_eq!(
        diagnostic["details"]["features"],
        json!(["stereo_configuration", "stereo_group"])
    );
    let digest = diagnostic["details"]["loss_digest"]
        .as_str()
        .unwrap()
        .to_owned();
    let accepted = export("second", json!({"accepted_losses": digest}));
    assert_eq!(accepted["outcome"], "partial", "{accepted}");
    let group_loss = accepted["loss_report"]
        .as_array()
        .unwrap()
        .iter()
        .find(|loss| loss["feature"] == "stereo_group")
        .unwrap_or_else(|| panic!("{accepted}"));
    assert_eq!(group_loss["source_path"], "groups[e1, e2]");
    assert_eq!(group_loss["disposition"], "dropped");
    assert_eq!(group_loss["semantic_impact"], "stereo");
    let text = group_loss["reason"].as_str().unwrap();
    assert!(!text.contains("1:1") && !text.contains("ratio"), "{text}");

    // A singleton counts: canonical SMILES also omits its group.
    let (single, _directory, one) = four_centers();
    ok(transact(
        &single,
        &one,
        "g",
        1,
        &json!([create("absolute", &["a1"], Value::Null)]),
    ));
    let refused = call(
        &single,
        "single",
        "document.export",
        &json!({"document_id": one, "format": "smiles", "style": "canonical"}),
    );
    assert_eq!(
        refused["diagnostics"][0]["details"]["features"],
        json!(["stereo_configuration", "stereo_group"]),
        "{refused}"
    );
    // V2000 has no groups: it needs consent. V3000 would write them as collections, but these
    // members carry no wedge or hash, so it refuses (no consent covers that).
    let v2000 = call(
        &app,
        "mol2",
        "document.export",
        &json!({"format": "mol", "ctab": "v2000", "records": [{"document_id": id}]}),
    );
    assert_eq!(
        v2000["diagnostics"][0]["code"], "loss_consent_required",
        "{v2000}"
    );
    assert!(
        v2000["diagnostics"][0]["details"]["features"]
            .as_array()
            .unwrap()
            .contains(&json!("stereo_group")),
        "{v2000}"
    );
    let v3000 = call(
        &app,
        "mol3",
        "document.export",
        &json!({"format": "mol", "ctab": "v3000", "records": [{"document_id": id}]}),
    );
    assert_eq!(
        v3000["diagnostics"][0]["code"], "v3000_group_member_not_drawn",
        "{v3000}"
    );
}

/// A reply without the snapshot it carries, which names the groups.
fn without_snapshots(mut value: Value) -> Value {
    fn strip(value: &mut Value) {
        match value {
            Value::Object(map) => {
                for key in [
                    "snapshot",
                    "query",
                    "target",
                    "request_id",
                    "document_revision",
                ] {
                    map.remove(key);
                }
                map.values_mut().for_each(strip);
            }
            Value::Array(items) => items.iter_mut().for_each(strip),
            _ => {}
        }
    }
    strip(&mut value);
    value
}

#[test]
fn cip_identity_substructure_and_validate_read_the_same_with_and_without_groups() {
    let (app, _directory, id) = four_centers();
    let read = |round: &str| {
        let parameters = [
            ("chem.cip", json!({"document_id": id})),
            (
                "chem.identity",
                json!({"document_id": id, "other_document_id": id, "mode": "chemical"}),
            ),
            (
                "chem.substructure",
                json!({"query_document_id": id, "target_document_id": id}),
            ),
            ("chem.validate", json!({"document_id": id})),
        ];
        parameters
            .iter()
            .map(|(operation, parameters)| {
                let reply = call(
                    &app,
                    &format!("{round}-{}", operation.replace('.', "-")),
                    operation,
                    parameters,
                );
                assert_ne!(reply["outcome"], "error", "{operation}: {reply}");
                without_snapshots(reply)
            })
            .collect::<Vec<_>>()
    };
    let plain = read("plain");
    ok(transact(
        &app,
        &id,
        "groups",
        1,
        &json!([
            create("and", &["a1", "a5"], json!(1)),
            create("or", &["a9"], Value::Null),
            create("absolute", &["a13"], Value::Null)
        ]),
    ));
    assert_eq!(read("grouped"), plain);
}

#[test]
fn documents_that_differ_only_in_groups_have_the_same_identity_code() {
    let (app, _directory, plain) = four_centers();
    let grouped_id = app
        .create_document(Some("c2"), Some("Grouped"))
        .unwrap()
        .document
        .id;
    ok(transact(&app, &grouped_id, "centers", 0, &centers(4)));
    ok(transact(
        &app,
        &grouped_id,
        "groups",
        1,
        &json!([
            create("or", &["a1", "a9"], json!(3)),
            create("absolute", &["a13"], Value::Null)
        ]),
    ));
    let compared = call(
        &app,
        "compare",
        "chem.identity",
        &json!({"document_id": plain, "other_document_id": grouped_id, "mode": "chemical"}),
    );
    assert_eq!(
        compared["outcome"], "partial",
        "stereo is not read, as before: {compared}"
    );
    assert_eq!(
        compared["result"]["comparison"]["equivalent"], true,
        "{compared}"
    );
    // The canonical code and numbering of each, read alone, are equal too.
    let identity = |document: &str, request: &str| {
        let reply = call(
            &app,
            request,
            "chem.identity",
            &json!({"document_id": document, "mode": "chemical"}),
        );
        without_snapshots(reply["result"]["identity"].clone())
    };
    let plain_identity = identity(&plain, "one");
    assert!(plain_identity.is_object(), "{plain_identity}");
    assert_eq!(identity(&grouped_id, "two"), plain_identity);
}

#[test]
fn chem_stereo_says_groups_are_not_examined_only_when_there_are_groups() {
    let (app, _directory, id) = four_centers();
    let codes = |round: &str| -> Vec<String> {
        let reply = call(&app, round, "chem.stereo", &json!({"document_id": id}));
        assert_eq!(reply["outcome"], "ok", "{reply}");
        reply["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d["code"].as_str().unwrap().to_owned())
            .collect()
    };
    let without = codes("plain");
    assert!(
        !without.contains(&"stereo_groups_not_examined".to_owned()),
        "{without:?}"
    );
    ok(transact(
        &app,
        &id,
        "groups",
        1,
        &json!([create("or", &["a1"], json!(1))]),
    ));
    let with = codes("grouped");
    let mut expected = without.clone();
    expected.insert(expected.len() - 1, "stereo_groups_not_examined".to_owned());
    assert_eq!(
        with, expected,
        "one more info, before the closing stereo_not_checked"
    );
}

#[test]
fn the_catalog_lists_the_group_commands_and_the_capability_that_they_carry_out() {
    let (app, _directory) = application();
    let reply = call(&app, "caps", "capabilities.inspect", &json!({}));
    let catalog = &reply["result"]["operations"];
    assert_eq!(
        catalog.as_array().unwrap().len(),
        28,
        "the catalog includes the optional InChI operation"
    );
    let transaction = catalog
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["id"] == "document.transaction")
        .unwrap();
    let rows = transaction["sub_operations"].as_array().unwrap();
    let names = [
        "create_stereo_group",
        "delete_stereo_group",
        "set_stereo_group_kind",
        "set_stereo_group_number",
        "add_stereo_group_members",
        "remove_stereo_group_members",
        "merge_stereo_groups",
        "split_stereo_group",
    ];
    for name in names {
        let row = rows
            .iter()
            .find(|row| row["id"] == name)
            .unwrap_or_else(|| panic!("{name}"));
        assert_eq!(row["execution"], "executable", "{name}");
    }
    let capability = rows.iter().find(|row| row["id"] == "stereo_group").unwrap();
    assert_eq!(capability["commands"], json!(names));
    let note = capability["note"].as_str().unwrap();
    assert!(note.contains("2,000") && note.contains("32-bit"), "{note}");
    for row in rows {
        let text = row["note"].as_str().unwrap();
        assert!(!text.contains("No groups"), "{text}");
    }
}

#[test]
fn setting_what_is_set_and_adding_a_member_the_group_has_change_nothing() {
    let (app, _directory, id) = grouped();
    let before = serde_json::to_value(app.document(&id).unwrap()).unwrap();
    let reply = ok(transact(
        &app,
        &id,
        "same",
        2,
        &json!([
            {"op": "set_stereo_group_number", "group": "e1", "number": 1},
            {"op": "set_stereo_group_kind", "group": "e1", "kind": "or"},
            {"op": "add_stereo_group_members", "group": "e1", "atoms": ["a1", "a5"]}
        ]),
    ));
    assert_eq!(reply["document"]["revision"], 2, "{reply}");
    assert_eq!(reply["notes"], json!([]));
    assert_eq!(
        serde_json::to_value(app.document(&id).unwrap()).unwrap()["session"],
        before["session"],
        "no history entry"
    );
}

#[test]
fn the_example_in_the_contract_is_what_the_service_sends() {
    let contract = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/CONTRACT.md"
    ))
    .unwrap();
    let start = contract
        .find("### Stereo groups")
        .expect("the section exists");
    let section = &contract[start..];
    let section = &section[..section[1..]
        .find("\n#")
        .map_or(section.len(), |end| end + 1)];
    let blocks: Vec<Value> = section
        .split("```json\n")
        .skip(1)
        .map(|block| {
            let body = block.split("```").next().unwrap();
            serde_json::from_str(body).unwrap_or_else(|error| panic!("{error}: {body}"))
        })
        .collect();
    assert_eq!(blocks.len(), 2, "one request and its reply");

    let (app, _directory, id) = four_centers();
    let mut request = blocks[0].clone();
    request["parameters"]["document_id"] = json!(id);
    let reply = serde_json::to_value(app.execute(request.to_string().as_bytes())).unwrap();
    assert_eq!(reply["outcome"], "ok", "{reply}");
    assert_eq!(
        reply["result"]["document"]["stereo"]["groups"],
        blocks[1]["groups"]
    );
    assert_eq!(reply["result"]["notes"], blocks[1]["notes"]);
}

/// Notes are one per change of meaning, not one per feature and path: a client must not use
/// the pair as a key (the web page keys its list by position as well).
#[test]
fn one_transaction_can_report_the_same_feature_and_path_twice() {
    let (app, _directory, id) = grouped();
    let reply = operate(
        &app,
        &id,
        "twice",
        2,
        &json!([{"op": "merge_stereo_groups", "into": "e1", "from": "e2"},
                {"op": "split_stereo_group", "group": "e1", "atoms": ["a1"], "number": 9}]),
    );
    let relation = note("stereo_group_relation", "groups[e1]", "dropped");
    assert_eq!(features(&reply), [relation.clone(), relation]);
}
