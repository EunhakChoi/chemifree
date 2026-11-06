//! Attachment shares ordinary add-bond stereo clearing, notes, groups and persistence.

use chemifree_application::Application;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};

type App = Application<LocalCapabilities, FileDocumentRepository>;
fn request(app: &App, key: &str, operation: &str, parameters: Value) -> Value {
    serde_json::to_value(app.execute(json!({"contract":1,"request_id":key,"profile":"preserve-semantics","operation":operation,"parameters":parameters}).to_string().as_bytes())).unwrap()
}
fn snapshot(app: &App, id: &str) -> Value {
    request(
        app,
        "snapshot",
        "document.snapshot",
        json!({"document_id":id}),
    )["result"]
        .clone()
}
fn seed(app: &App, id: &str, singleton: bool) -> Value {
    let members = if singleton {
        json!(["a1"])
    } else {
        json!(["a1", "a5"])
    };
    let reply = request(
        app,
        "seed",
        "document.transaction",
        json!({"document_id":id,"expected_revision":0,"commands":[
        {"op":"add_atom","element":"C","position":{"x":0,"y":0}},
        {"op":"add_atom","element":"C","position":{"x":30,"y":0},"bond_to":{"atom":"a1","order":"double"}},
        {"op":"add_atom","element":"F","position":{"x":0,"y":30},"bond_to":{"atom":"a1","order":"single"}},
        {"op":"add_atom","element":"Cl","position":{"x":-30,"y":0},"bond_to":{"atom":"a1","order":"single"}},
        {"op":"set_atom_configuration","atom":"a1","configuration":{"kind":"tetrahedral","neighbors":["a2","a3","a4","implicit_hydrogen"],"turn":"clockwise"}},
        {"op":"set_bond_configuration","bond":"b1","configuration":{"kind":"unknown"}},
        {"op":"set_bond_mark","bond":"b1","mark":{"kind":"crossed"}},
        {"op":"set_bond_mark","bond":"b2","mark":{"kind":"wedge","narrow":"begin"}},
        {"op":"add_atom","element":"C","position":{"x":-100,"y":0}},
        {"op":"add_atom","element":"F","position":{"x":-70,"y":0},"bond_to":{"atom":"a5","order":"single"}},
        {"op":"add_atom","element":"Cl","position":{"x":-100,"y":30},"bond_to":{"atom":"a5","order":"single"}},
        {"op":"add_atom","element":"Br","position":{"x":-130,"y":0},"bond_to":{"atom":"a5","order":"single"}},
        {"op":"set_atom_configuration","atom":"a5","configuration":{"kind":"tetrahedral","neighbors":["a6","a7","a8","implicit_hydrogen"],"turn":"clockwise"}},
        {"op":"create_stereo_group","kind":"and","number":1,"atoms":members},
        {"op":"add_graphic","shape":"line","points":[{"x":0,"y":60},{"x":20,"y":60}]},
        {"op":"add_reference","source":"s2","target":{"kind":"atom","id":"a1"}}
    ],"selection_after":{"objects":["s2"],"atoms":["a1"],"bonds":["b1"]}}),
    );
    assert_eq!(reply["outcome"], "ok", "{reply}");
    reply
}

#[test]
fn attachment_clears_only_touched_stereo_and_prunes_groups_with_undo_replay_and_reopen() {
    for (size, singleton) in [(5, false), (6, false), (5, true)] {
        let directory = tempfile::tempdir().unwrap();
        let app = Application::new(
            LocalCapabilities,
            FileDocumentRepository::new(directory.path()),
        );
        let created = request(&app, "create", "document.create", json!({}));
        let id = created["result"]["document"]["id"].as_str().unwrap();
        let before = seed(&app, id, singleton)["result"]["document"].clone();
        let before_state = snapshot(&app, id);
        let late = request(
            &app,
            "late-rollback",
            "document.transaction",
            json!({"document_id":id,"expected_revision":1,"commands":[
                {"op":"attach_ring","instance":"s1","atom":"a1","direction":{"dx":1,"dy":0},"size":size},
                {"op":"move_atoms","atoms":["a2"],"instance":"s1","delta":{"x":100000,"y":0}}
            ]}),
        );
        assert_eq!(late["diagnostics"][0]["code"], "invalid_command", "{late}");
        assert_eq!(late["diagnostics"][0]["details"]["command_index"], 1);
        assert_eq!(
            snapshot(&app, id),
            before_state,
            "stereo/group clearing must roll back with a later command"
        );
        let parameters = json!({"document_id":id,"expected_revision":1,"commands":[{"op":"attach_ring","instance":"s1","atom":"a1","direction":{"dx":1,"dy":0},"size":size}]});
        let attached = request(&app, "attach", "document.transaction", parameters.clone());
        assert_eq!(attached["outcome"], "partial", "{attached}");
        let after = &attached["result"]["document"];
        assert_eq!(after["atoms"].as_array().unwrap().len(), 8 + size);
        assert_eq!(after["bonds"].as_array().unwrap().len(), 7 + size);
        assert_eq!(
            &after["atoms"].as_array().unwrap()[..8],
            before["atoms"].as_array().unwrap()
        );
        assert_eq!(
            &after["bonds"].as_array().unwrap()[..6],
            before["bonds"].as_array().unwrap()
        );
        for field in ["selection", "settings", "title"] {
            assert_eq!(after[field], before[field]);
        }
        for field in ["objects", "roots", "references"] {
            assert_eq!(after["scene"][field], before["scene"][field]);
        }
        assert_eq!(
            after["stereo"]["atoms"],
            json!([before["stereo"]["atoms"][1].clone()])
        );
        assert_eq!(after["stereo"]["bonds"], json!([]));
        let notes = attached["result"]["notes"].as_array().unwrap();
        let summaries: Vec<_> = notes
            .iter()
            .map(|note| {
                (
                    note["feature"].as_str().unwrap(),
                    note["source_path"].as_str().unwrap(),
                )
            })
            .collect();
        let mut expected = vec![
            ("stereo_configuration", "atoms[a1]"),
            ("stereo_configuration", "bonds[b1]"),
            ("stereo_group_member", "atoms[a1]"),
        ];
        if singleton {
            expected.push(("stereo_group", "groups[e1]"));
            assert_eq!(after["stereo"]["groups"], json!([]));
        } else {
            assert_eq!(after["stereo"]["groups"][0]["members"], json!(["a5"]));
            assert!(
                notes[2]["reason"]
                    .as_str()
                    .unwrap()
                    .contains("no longer stated")
            );
        }
        assert_eq!(summaries, expected);
        // Ordinary public add_atom/add_bond commands are the independent topology/notes oracle.
        // Geometry is checked independently at the Domain boundary; reuse these coordinates only
        // so this comparison isolates stereo clearing and group meaning changes.
        let oracle_directory = tempfile::tempdir().unwrap();
        let oracle = Application::new(
            LocalCapabilities,
            FileDocumentRepository::new(oracle_directory.path()),
        );
        let oracle_created = request(&oracle, "create", "document.create", json!({}));
        let oracle_id = oracle_created["result"]["document"]["id"].as_str().unwrap();
        seed(&oracle, oracle_id, singleton);
        let fresh = &after["atoms"].as_array().unwrap()[8..];
        let mut commands: Vec<Value> = fresh
            .iter()
            .map(|atom| json!({"op":"add_atom","element":"C","position":atom["position"]}))
            .collect();
        for index in 0..size {
            commands.push(json!({"op":"add_bond","begin":fresh[index]["id"],"end":fresh[(index+1)%size]["id"],"order":"single"}));
        }
        commands.push(json!({"op":"add_bond","begin":"a1","end":fresh[0]["id"],"order":"single"}));
        let reference = request(
            &oracle,
            "reference",
            "document.transaction",
            json!({"document_id":oracle_id,"expected_revision":1,"commands":commands}),
        );
        assert_eq!(reference["outcome"], "partial", "{reference}");
        assert_eq!(reference["result"]["notes"], attached["result"]["notes"]);
        assert_eq!(reference["result"]["document"]["stereo"], after["stereo"]);

        assert_eq!(
            request(&app, "attach", "document.transaction", parameters.clone()),
            attached
        );
        let undone = request(
            &app,
            "undo",
            "history.undo",
            json!({"document_id":id,"expected_revision":2}),
        );
        for field in ["atoms", "bonds", "stereo", "scene", "selection"] {
            assert_eq!(undone["result"]["document"][field], before[field]);
        }
        assert_eq!(
            request(&app, "attach", "document.transaction", parameters),
            attached
        );
        let redone = request(
            &app,
            "redo",
            "history.redo",
            json!({"document_id":id,"expected_revision":3}),
        );
        for field in ["atoms", "bonds", "stereo", "scene", "selection"] {
            assert_eq!(redone["result"]["document"][field], after[field]);
        }
        assert_eq!(
            request(
                &app,
                "save",
                "document.save",
                json!({"document_id":id,"expected_revision":4})
            )["outcome"],
            "ok"
        );
        let stored = snapshot(&app, id)["document"].clone();
        drop(app);
        let reopened = Application::new(
            LocalCapabilities,
            FileDocumentRepository::new(directory.path()),
        );
        assert_eq!(snapshot(&reopened, id)["document"], stored);
    }
}

#[test]
fn malformed_target_size_and_direction_requests_preserve_the_complete_snapshot() {
    let directory = tempfile::tempdir().unwrap();
    let app = Application::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    );
    let created = request(&app, "create", "document.create", json!({}));
    let id = created["result"]["document"]["id"].as_str().unwrap();
    seed(&app, id, false);
    let before = snapshot(&app, id);
    let base = json!({"op":"attach_ring","instance":"s1","atom":"a1","direction":{"dx":1,"dy":0}});
    let mut invalid: Vec<(Value, &str)> = Vec::new();
    for size in [json!(null), json!(4), json!(7), json!(5.5), json!("5")] {
        let mut command = base.clone();
        command["size"] = size;
        invalid.push((command, "invalid_request"));
    }
    for direction in [json!(null), json!({"dx":1}), json!({"dx":1,"dy":0,"z":1})] {
        let mut command = base.clone();
        command["direction"] = direction;
        invalid.push((command, "invalid_request"));
    }
    let mut zero = base.clone();
    zero["direction"] = json!({"dx":0,"dy":0});
    invalid.push((zero, "invalid_command"));
    let mut missing = base.clone();
    missing["atom"] = json!("a99");
    invalid.push((missing, "invalid_command"));
    let mut local = base.clone();
    local["atom"] = json!("$0");
    invalid.push((local, "invalid_command"));
    for (index, (command, code)) in invalid.into_iter().enumerate() {
        let reply = request(
            &app,
            &format!("bad-{index}"),
            "document.transaction",
            json!({"document_id":id,"expected_revision":1,"commands":[{"op":"rename","title":"Rollback"},command]}),
        );
        assert_eq!(reply["diagnostics"][0]["code"], code, "{reply}");
        assert_eq!(snapshot(&app, id), before);
    }
}
