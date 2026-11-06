//! Inspect/plan are read-only; apply preserves native content and ordinary destination stereo notes.
use chemifree_application::Application;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};
type App = Application<LocalCapabilities, FileDocumentRepository>;

#[test]
fn terminal_replacement_retains_boundary_identity_references_and_neighbor_stereo() {
    let directory = tempfile::tempdir().unwrap();
    let app = Application::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    );
    let (source, dest, payload) = seed(&app);
    let changed = request(
        &app,
        "replace-dest-state",
        "document.transaction",
        json!({"document_id":dest,"expected_revision":1,"commands":[
            {"op":"update_atom","atom":"a3","charge":1,"isotope":19,"hydrogens":{"mode":"specified","count":0},"radical":"triplet","attachment_points":[3]},
            {"op":"set_bond_mark","bond":"b2","mark":{"kind":"hash","narrow":"end"}},
            {"op":"add_reference","source":"s2","target":{"kind":"atom","id":"a3"}},
            {"op":"add_reference","source":"s2","target":{"kind":"bond","id":"b2"}}
        ]}),
    );
    assert_eq!(changed["outcome"], "ok", "{changed}");
    let before = snapshot(&app, &dest);
    let source_before = snapshot(&app, &source);
    let mut native: Value = serde_json::from_str(&payload).unwrap();
    native["atoms"].as_array_mut().unwrap().rotate_left(1);
    let params = json!({"document_id":dest,"expected_revision":2,"payload":native.to_string(),"instance":"s1","source_anchor":"a1","atom":"a3",
        "selection_before":{"objects":["s2"],"atoms":["a3"],"bonds":["b2"]}});
    let preview = request(
        &app,
        "replace-preview",
        "document.template_replace_plan",
        params.clone(),
    );
    assert_eq!(preview["outcome"], "ok", "{preview}");
    assert_eq!(snapshot(&app, &dest), before);
    let applied = request(
        &app,
        "replace-apply",
        "document.template_replace",
        params.clone(),
    );
    assert_eq!(applied["outcome"], "ok", "{applied}");
    assert_eq!(applied["loss_report"], json!([]));
    assert_eq!(applied["result"]["snapshot"], preview["result"]["snapshot"]);
    assert_eq!(applied["result"]["boundary"], "b2");
    assert_eq!(
        applied["result"]["mapping"],
        json!({"atoms":[{"source":"a2","target":"a5"},{"source":"a3","target":"a6"},{"source":"a1","target":"a3"}],
        "bonds":[{"source":"b1","target":"b4"},{"source":"b2","target":"b5"}]})
    );
    let after = &applied["result"]["snapshot"];
    assert_eq!(after["document"]["revision"], 3);
    assert_eq!(after["document"]["atoms"].as_array().unwrap().len(), 6);
    assert_eq!(after["document"]["bonds"].as_array().unwrap().len(), 5);
    assert_eq!(
        after["document"]["atoms"][2],
        json!({"id":"a3","element":"C","kind":"element","query":null,"charge":0,"isotope":null,
        "hydrogens":{"mode":"inferred"},"radical":"none","attachment_points":[],"position":{"x":0.0,"y":30.0}})
    );
    assert_eq!(
        after["document"]["atoms"][4]["position"],
        json!({"x":30.0,"y":30.0})
    );
    assert_eq!(after["document"]["atoms"][4]["charge"], -1);
    assert_eq!(after["document"]["atoms"][4]["isotope"], 18);
    assert_eq!(
        after["document"]["bonds"][1],
        before["document"]["bonds"][1]
    );
    assert_eq!(
        after["document"]["scene"]["references"],
        before["document"]["scene"]["references"]
    );
    assert_eq!(
        after["document"]["stereo"]["atoms"],
        before["document"]["stereo"]["atoms"]
    );
    assert_eq!(
        after["document"]["stereo"]["groups"],
        before["document"]["stereo"]["groups"]
    );
    assert_eq!(
        after["document"]["stereo"]["bonds"],
        json!([
            {"bond":"b1","configuration":{"kind":"unknown"}}, {"bond":"b5","configuration":{"kind":"unknown"}}
        ])
    );
    assert_eq!(
        after["document"]["selection"],
        json!({"objects":[],"atoms":["a3","a5","a6"],"bonds":["b4","b5"]})
    );
    assert_eq!(
        request(
            &app,
            "replace-apply",
            "document.template_replace",
            params.clone()
        ),
        applied
    );
    let mut reused = params.clone();
    reused["atom"] = json!("a4");
    assert_eq!(
        request(&app, "replace-apply", "document.template_replace", reused)["diagnostics"][0]["code"],
        "request_id_reused"
    );
    let undone = request(
        &app,
        "replace-undo",
        "history.undo",
        json!({"document_id":dest,"expected_revision":3}),
    );
    assert_eq!(
        undone["result"]["document"]["atoms"],
        before["document"]["atoms"]
    );
    assert_eq!(
        undone["result"]["document"]["selection"],
        params["selection_before"]
    );
    assert_eq!(
        request(&app, "replace-apply", "document.template_replace", params),
        applied
    );
    let redone = request(
        &app,
        "replace-redo",
        "history.redo",
        json!({"document_id":dest,"expected_revision":4}),
    );
    assert_eq!(
        redone["result"]["document"]["atoms"],
        after["document"]["atoms"]
    );
    assert_eq!(
        redone["result"]["document"]["stereo"],
        after["document"]["stereo"]
    );
    assert_eq!(snapshot(&app, &source), source_before);
    assert_eq!(
        request(
            &app,
            "replace-save",
            "document.save",
            json!({"document_id":dest,"expected_revision":5})
        )["outcome"],
        "ok"
    );
    drop(app);
    let app = Application::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    );
    let reopened = snapshot(&app, &dest);
    assert_eq!(reopened["document"]["atoms"], after["document"]["atoms"]);
    assert_eq!(reopened["document"]["scene"], after["document"]["scene"]);
}

#[test]
fn replacement_repeated_invalid_boundaries_are_atomic_and_single_atom_noop_keeps_selection() {
    let directory = tempfile::tempdir().unwrap();
    let app = Application::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    );
    let (_, dest, payload) = seed(&app);
    let before = snapshot(&app, &dest);
    let params = json!({"document_id":dest,"expected_revision":1,"payload":payload,"instance":"s1","source_anchor":"a1","atom":"a1"});
    for (key, operation) in [
        ("replacement-plan-refused", "document.template_replace_plan"),
        ("replacement-apply-refused", "document.template_replace"),
    ] {
        let first = request(&app, key, operation, params.clone());
        assert_eq!(first["envelope"], "operation_result", "{first}");
        assert_eq!(first["outcome"], "error", "{first}");
        assert_eq!(
            first["diagnostics"][0]["code"], "invalid_command",
            "{first}"
        );
        assert_eq!(
            first["diagnostics"][0]["details"]["reason"], "template_target_not_terminal_single",
            "{first}"
        );
        assert_eq!(request(&app, key, operation, params.clone()), first);
        assert_eq!(snapshot(&app, &dest), before);
    }
    let copied = request(
        &app,
        "copy-leaf",
        "document.clipboard_copy",
        json!({"document_id":dest,"instance":"s1","selection":{"atoms":["a3"],"bonds":[],"objects":[]}}),
    );
    let same = json!({"document_id":dest,"expected_revision":1,"payload":copied["result"]["payload"],"instance":"s1","source_anchor":"a3","atom":"a3",
        "selection_before":{"objects":[],"atoms":["a3"],"bonds":[]}});
    let preview = request(
        &app,
        "noop-preview",
        "document.template_replace_plan",
        same.clone(),
    );
    let applied = request(&app, "noop-apply", "document.template_replace", same);
    assert_eq!(applied["outcome"], "ok", "{applied}");
    assert_eq!(preview["result"]["snapshot"], applied["result"]["snapshot"]);
    assert_eq!(
        applied["result"]["snapshot"]["document"],
        before["document"]
    );
    assert_eq!(snapshot(&app, &dest), before);
}
fn request(app: &App, key: &str, operation: &str, parameters: Value) -> Value {
    assert!(
        (1..=64).contains(&key.len())
            && key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'),
        "Invalid operation fixture request_id: {key:?}"
    );
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
fn create(app: &App, key: &str) -> String {
    request(app, key, "document.create", json!({}))["result"]["document"]["id"]
        .as_str()
        .unwrap()
        .into()
}
fn seed(app: &App) -> (String, String, String) {
    let source = create(app, "source");
    let dest = create(app, "destination");
    let seeded = request(
        app,
        "source-seed",
        "document.transaction",
        json!({"document_id":source,"expected_revision":0,"commands":[
         {"op":"add_atom","element":"C","position":{"x":100,"y":200}},
         {"op":"add_atom","element":"O","hydrogens":{"mode":"specified","count":0},"attachment_points":[1],"position":{"x":130,"y":200},"bond_to":{"atom":"a1","order":"single"}},
         {"op":"add_atom","element":"N","position":{"x":160,"y":200},"bond_to":{"atom":"a2","order":"double"}},
         {"op":"update_atom","atom":"a2","charge":-1,"isotope":18},
         {"op":"set_bond_mark","bond":"b1","mark":{"kind":"wedge","narrow":"begin"}},
         {"op":"set_bond_configuration","bond":"b2","configuration":{"kind":"unknown"}}
        ]}),
    );
    assert_eq!(seeded["outcome"], "ok", "{seeded}");
    assert_eq!(seeded["result"]["document"]["atoms"][1]["charge"], -1);
    assert_eq!(seeded["result"]["document"]["atoms"][1]["isotope"], 18);

    let seeded = request(
        app,
        "dest-seed",
        "document.transaction",
        json!({"document_id":dest,"expected_revision":0,"commands":[
         {"op":"add_atom","element":"C","position":{"x":0,"y":0}},
         {"op":"add_atom","element":"C","position":{"x":30,"y":0},"bond_to":{"atom":"a1","order":"double"}},
         {"op":"add_atom","element":"F","position":{"x":0,"y":30},"bond_to":{"atom":"a1","order":"single"}},
         {"op":"add_atom","element":"Cl","position":{"x":-30,"y":0},"bond_to":{"atom":"a1","order":"single"}},
         {"op":"set_atom_configuration","atom":"a1","configuration":{"kind":"tetrahedral","neighbors":["a2","a3","a4","implicit_hydrogen"],"turn":"clockwise"}},
         {"op":"set_bond_configuration","bond":"b1","configuration":{"kind":"unknown"}},
         {"op":"set_bond_mark","bond":"b1","mark":{"kind":"crossed"}},
         {"op":"create_stereo_group","kind":"absolute","number":null,"atoms":["a1"]},
         {"op":"add_graphic","shape":"line","points":[{"x":0,"y":70},{"x":30,"y":70}]},
         {"op":"add_reference","source":"s2","target":{"kind":"atom","id":"a1"}}
        ]}),
    );
    assert_eq!(seeded["outcome"], "ok", "{seeded}");
    let copied = request(
        app,
        "copy",
        "document.clipboard_copy",
        json!({"document_id":source,"instance":"s1","selection":{"atoms":["a1","a2","a3"],"bonds":[],"objects":[]}}),
    );
    assert_eq!(copied["outcome"], "ok", "{copied}");
    (
        source,
        dest,
        copied["result"]["payload"].as_str().unwrap().into(),
    )
}
fn parameters(dest: &str, payload: &str) -> Value {
    json!({"document_id":dest,"expected_revision":1,"payload":payload,"instance":"s1","source_anchor":"a1","atom":"a1","direction":{"dx":2,"dy":0},"selection_before":{"objects":["s2"],"atoms":["a1"],"bonds":["b1"]}})
}
#[test]
fn inspection_plan_atomic_apply_notes_replay_and_persistence_preserve_both_sources() {
    let directory = tempfile::tempdir().unwrap();
    let app = Application::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    );
    let (source, dest, payload) = seed(&app);
    let source_before = snapshot(&app, &source);
    let before = snapshot(&app, &dest);
    let inspect = request(
        &app,
        "inspect",
        "document.template_inspect",
        json!({"payload":payload}),
    );
    assert_eq!(inspect["outcome"], "ok", "{inspect}");
    assert_eq!(
        inspect["document_revision"],
        Value::Null,
        "source-only inspection has no document revision"
    );
    assert_eq!(
        inspect["result"]["atoms"]
            .as_array()
            .unwrap()
            .iter()
            .map(|atom| (
                atom["id"].as_str().unwrap(),
                atom["supported"].as_bool().unwrap()
            ))
            .collect::<Vec<_>>(),
        vec![("a1", true), ("a2", false), ("a3", false)]
    );
    let input = parameters(&dest, &payload);
    let plan = request(&app, "plan", "document.template_plan", input.clone());
    assert_eq!(plan["outcome"], "partial", "{plan}");
    assert_eq!(
        plan["document_revision"], 1,
        "read-only envelope names the current source, not the candidate"
    );
    assert_eq!(
        snapshot(&app, &dest),
        before,
        "preview reserves no IDs/history/selection"
    );
    assert_eq!(plan["result"]["source"]["document"], before["document"]);
    let applied = request(&app, "attach", "document.template_attach", input.clone());
    assert_eq!(applied["outcome"], "partial", "{applied}");
    for field in ["document", "notes"] {
        assert_eq!(
            applied["result"]["snapshot"][field],
            plan["result"]["snapshot"][field]
        );
    }
    assert_eq!(applied["result"]["mapping"], plan["result"]["mapping"]);
    assert_eq!(applied["result"]["connection"], "b6");
    let after = &applied["result"]["snapshot"]["document"];
    assert_eq!(after["atoms"].as_array().unwrap().len(), 7);
    assert_eq!(after["bonds"].as_array().unwrap().len(), 6);
    assert_eq!(
        after["selection"],
        json!({"objects":[],"atoms":["a5","a6","a7"],"bonds":["b4","b5"]})
    );
    assert_eq!(
        &after["atoms"].as_array().unwrap()[..4],
        before["document"]["atoms"].as_array().unwrap()
    );
    assert_eq!(
        &after["bonds"].as_array().unwrap()[..3],
        before["document"]["bonds"].as_array().unwrap()
    );
    for field in ["objects", "roots", "references"] {
        assert_eq!(after["scene"][field], before["document"]["scene"][field]);
    }
    for (index, x) in [(4, 30), (5, 60), (6, 90)] {
        assert_eq!(
            after["atoms"][index]["position"],
            json!({"x":x as f64,"y":0.0})
        );
    }
    for field in [
        "element",
        "kind",
        "charge",
        "isotope",
        "hydrogens",
        "radical",
        "attachment_points",
    ] {
        assert_eq!(
            after["atoms"][5][field], source_before["document"]["atoms"][1][field],
            "{field}"
        );
    }
    assert_eq!(after["stereo"]["atoms"], json!([]));
    assert_eq!(after["stereo"]["groups"], json!([]));
    assert_eq!(
        after["stereo"]["bonds"],
        json!([{"bond":"b5","configuration":{"kind":"unknown"}}])
    );
    assert_eq!(applied["loss_report"].as_array().unwrap().len(), 4);
    // Compare destination notes against ordinary native paste followed by AddBond. Their two
    // history steps are deliberately not used as the atomic attachment's history oracle.
    let oracle_directory = tempfile::tempdir().unwrap();
    let oracle = Application::new(
        LocalCapabilities,
        FileDocumentRepository::new(oracle_directory.path()),
    );
    let (_, oracle_dest, _) = seed(&oracle);
    let pasted = request(
        &oracle,
        "oracle-paste",
        "document.clipboard_paste",
        json!({"document_id":oracle_dest,
        "expected_revision":1,"payload":payload,"offset":{"x":-70,"y":-200}}),
    );
    assert_eq!(pasted["outcome"], "ok", "{pasted}");
    let linked = request(
        &oracle,
        "oracle-link",
        "document.transaction",
        json!({"document_id":oracle_dest,
        "expected_revision":2,"commands":[{"op":"add_bond","begin":"a1","end":"a5","order":"single"}]}),
    );
    assert_eq!(linked["outcome"], "partial", "{linked}");
    assert_eq!(
        linked["result"]["notes"],
        applied["result"]["snapshot"]["notes"]
    );
    for field in ["atoms", "bonds", "stereo", "scene", "settings", "selection"] {
        assert_eq!(
            linked["result"]["document"][field], after[field],
            "ordinary AddBond oracle {field}"
        );
    }

    assert_eq!(
        request(&app, "attach", "document.template_attach", input.clone()),
        applied
    );
    let mut changed = input.clone();
    changed["direction"] = json!({"dx":0,"dy":1});
    assert_eq!(
        request(&app, "attach", "document.template_attach", changed)["diagnostics"][0]["code"],
        "request_id_reused"
    );
    let undone = request(
        &app,
        "undo",
        "history.undo",
        json!({"document_id":dest,"expected_revision":2}),
    );
    for field in ["atoms", "bonds", "stereo", "scene"] {
        assert_eq!(
            undone["result"]["document"][field],
            before["document"][field]
        );
    }
    assert_eq!(
        undone["result"]["document"]["selection"],
        input["selection_before"]
    );
    assert_eq!(
        request(&app, "attach", "document.template_attach", input),
        applied
    );
    let redo = request(
        &app,
        "redo",
        "history.redo",
        json!({"document_id":dest,"expected_revision":3}),
    );
    for field in ["atoms", "bonds", "stereo", "scene", "selection"] {
        assert_eq!(redo["result"]["document"][field], after[field]);
    }
    assert_eq!(snapshot(&app, &source), source_before);
    assert_eq!(
        request(
            &app,
            "save",
            "document.save",
            json!({"document_id":dest,"expected_revision":4})
        )["outcome"],
        "ok"
    );
    let stored = snapshot(&app, &dest)["document"].clone();
    drop(app);
    let reopened = Application::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    );
    assert_eq!(snapshot(&reopened, &dest)["document"], stored);
}
#[test]
fn anchor_stereo_disconnected_style_target_direction_and_stale_requests_do_not_edit() {
    let directory = tempfile::tempdir().unwrap();
    let app = Application::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    );
    let (_, dest, payload) = seed(&app);
    let before = snapshot(&app, &dest);
    let mut cases = Vec::new();
    for anchor in ["a2", "a3", "a99"] {
        let mut input = parameters(&dest, &payload);
        input["source_anchor"] = json!(anchor);
        cases.push(input);
    }
    let mut input = parameters(&dest, &payload);
    input["direction"] = json!({"dx":0,"dy":0});
    cases.push(input);
    let mut input = parameters(&dest, &payload);
    input["atom"] = json!("a99");
    cases.push(input);
    let mut input = parameters(&dest, &payload);
    input["expected_revision"] = json!(0);
    cases.push(input);
    let mut value: Value = serde_json::from_str(&payload).unwrap();
    value["bonds"].as_array_mut().unwrap().remove(0);
    let mut input = parameters(&dest, &value.to_string());
    cases.push(input.clone());
    value = serde_json::from_str(&payload).unwrap();
    value["settings"]["bond_length_pt"] = json!(40);
    input["payload"] = json!(value.to_string());
    cases.push(input);
    for (index, input) in cases.iter().enumerate() {
        for (suffix, operation) in [
            ("plan", "document.template_plan"),
            ("apply", "document.template_attach"),
        ] {
            let reply = request(
                &app,
                &format!("reject-{index}-{suffix}"),
                operation,
                input.clone(),
            );
            assert_eq!(reply["outcome"], "error", "{reply}");
            assert_eq!(snapshot(&app, &dest), before);
        }
    }
}

#[test]
fn plan_and_apply_report_missing_targets_without_masking_invalid_fragment_references() {
    let directory = tempfile::tempdir().unwrap();
    let app = Application::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    );
    let (_, dest, payload) = seed(&app);
    let before = snapshot(&app, &dest);
    let mut malformed: Value = serde_json::from_str(&payload).unwrap();
    malformed["bonds"][0]["end"] = json!("a999");
    for (index, (target, invalid_source, reason)) in [
        ("a99", false, "template_target_invalid"),
        ("$0", false, "template_target_invalid"),
        ("a1", true, "clipboard_fragment_invalid"),
        ("a99", true, "clipboard_fragment_invalid"),
    ]
    .into_iter()
    .enumerate()
    {
        let mut input = parameters(&dest, &payload);
        input["atom"] = json!(target);
        if invalid_source {
            input["payload"] = json!(malformed.to_string());
        }
        for (suffix, operation) in [
            ("plan", "document.template_plan"),
            ("apply", "document.template_attach"),
        ] {
            let rejected = request(
                &app,
                &format!("target-{index}-{suffix}"),
                operation,
                input.clone(),
            );
            assert_eq!(rejected["outcome"], "error", "{rejected}");
            assert_eq!(
                rejected["diagnostics"][0]["code"], "invalid_command",
                "{rejected}"
            );
            assert_eq!(
                rejected["diagnostics"][0]["details"]["reason"], reason,
                "{rejected}"
            );
            assert_eq!(snapshot(&app, &dest), before);
        }
    }
}

#[test]
fn plan_and_apply_classify_each_identifier_and_require_all_identifier_fields() {
    let directory = tempfile::tempdir().unwrap();
    let app = Application::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    );
    let (source, dest, payload) = seed(&app);
    let source_before = snapshot(&app, &source);
    let before = snapshot(&app, &dest);
    for (field, reason, absent, wrong_kind) in [
        ("instance", "template_instance_invalid", "s99", "a1"),
        ("source_anchor", "template_anchor_unknown", "a99", "s2"),
        ("atom", "template_target_invalid", "a99", "s2"),
    ] {
        // Malformed strings and well-formed but absent IDs belong to the same field-specific
        // semantic category. Missing/non-string fields must still fail the closed DTO parser.
        let cases = [
            (Some(json!("")), "invalid_command", Some(reason)),
            (Some(json!("$0")), "invalid_command", Some(reason)),
            (Some(json!(wrong_kind)), "invalid_command", Some(reason)),
            (Some(json!(absent)), "invalid_command", Some(reason)),
            (None, "invalid_request", None),
            (Some(Value::Null), "invalid_request", None),
            (Some(json!(1)), "invalid_request", None),
        ];
        for (index, (value, code, reason)) in cases.into_iter().enumerate() {
            let mut input = parameters(&dest, &payload);
            match value {
                Some(value) => input[field] = value,
                None => {
                    input.as_object_mut().unwrap().remove(field);
                }
            }
            for (suffix, operation) in [
                ("plan", "document.template_plan"),
                ("apply", "document.template_attach"),
            ] {
                let rejected = request(
                    &app,
                    &format!("identifier-{field}-{index}-{suffix}"),
                    operation,
                    input.clone(),
                );
                assert_eq!(rejected["outcome"], "error", "{rejected}");
                assert_eq!(rejected["result"], Value::Null, "{rejected}");
                assert_eq!(rejected["diagnostics"][0]["code"], code, "{rejected}");
                assert_eq!(
                    rejected["diagnostics"][0]["details"]["reason"],
                    reason.map_or(Value::Null, |reason| json!(reason)),
                    "{rejected}"
                );
                assert_eq!(snapshot(&app, &dest), before);
                assert_eq!(snapshot(&app, &source), source_before);
            }
        }
    }
}
