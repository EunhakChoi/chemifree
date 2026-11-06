//! Native clipboard operation boundaries: exact copy data, safe parsing and atomic fresh-ID paste.

use std::sync::atomic::{AtomicU64, Ordering};

use chemifree_application::Application;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};

type App = Application<LocalCapabilities, FileDocumentRepository>;

fn application() -> (App, tempfile::TempDir) {
    let directory = tempfile::tempdir().unwrap();
    let app = Application::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    );
    (app, directory)
}

fn request(app: &App, request_id: &str, operation: &str, parameters: Value) -> Value {
    let body = json!({"contract":1,"request_id":request_id,"profile":"preserve-semantics",
        "operation":operation,"parameters":parameters});
    serde_json::to_value(app.execute(body.to_string().as_bytes())).unwrap()
}

fn call(app: &App, operation: &str, parameters: Value) -> Value {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    request(
        app,
        &format!("clipboard-{}", NEXT.fetch_add(1, Ordering::Relaxed)),
        operation,
        parameters,
    )
}

fn create(app: &App, title: &str) -> String {
    let result = call(app, "document.create", json!({"title":title}));
    assert_eq!(result["outcome"], "ok", "{result}");
    result["result"]["document"]["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn snapshot(app: &App, id: &str) -> Value {
    let result = call(app, "document.snapshot", json!({"document_id":id}));
    assert_eq!(result["outcome"], "ok", "{result}");
    result["result"].clone()
}

fn edit(app: &App, id: &str, commands: Value) -> Value {
    let revision = snapshot(app, id)["document"]["revision"].clone();
    let result = call(
        app,
        "document.transaction",
        json!({"document_id":id,"expected_revision":revision,"commands":commands}),
    );
    assert_eq!(result["outcome"], "ok", "{result}");
    result["result"].clone()
}

fn source(app: &App) -> String {
    let id = create(app, "Fragment source");
    edit(
        app,
        &id,
        json!([
            {"op":"add_atom","element":"C","position":{"x":0.0,"y":0.0},
             "hydrogens":{"mode":"specified","count":1},"radical":"doublet","attachment_points":[1]},
            {"op":"add_atom","element":"O","position":{"x":10.0,"y":0.0}},
            {"op":"add_atom","element":"N","position":{"x":10.0,"y":10.0}},
            {"op":"add_atom","element":"F","position":{"x":20.0,"y":10.0}},
            {"op":"add_bond","begin":"a1","end":"a2","order":"single"},
            {"op":"add_bond","begin":"a2","end":"a3","order":"double"},
            {"op":"add_bond","begin":"a3","end":"a1","order":"single"},
            {"op":"add_bond","begin":"a3","end":"a4","order":"single"},
            {"op":"set_bond_mark","bond":"b1","mark":{"kind":"wedge","narrow":"begin"}},
            {"op":"update_atom","atom":"a1","isotope":13}
        ]),
    );
    id
}

fn copy(app: &App, id: &str) -> Value {
    let result = call(
        app,
        "document.clipboard_copy",
        json!({"document_id":id,
        "selection":{"atoms":["a3"],"bonds":["b1"]},"instance":"s1"}),
    );
    assert_eq!(result["outcome"], "ok", "{result}");
    result["result"].clone()
}

fn paste_parameters(id: &str, revision: u64, payload: &str) -> Value {
    json!({"document_id":id,"expected_revision":revision,"payload":payload,"offset":{"x":30.0,"y":40.0}})
}

fn assert_refusal(result: &Value, reason: &str) {
    assert_eq!(result["outcome"], "error", "{result}");
    assert_eq!(result["result"], Value::Null, "{result}");
    assert_eq!(
        result["diagnostics"][0]["code"], "invalid_command",
        "{result}"
    );
    assert_eq!(
        result["diagnostics"][0]["details"]["reason"], reason,
        "{result}"
    );
}

fn content(snapshot: &Value) -> Value {
    let mut document = snapshot["document"].clone();
    document.as_object_mut().unwrap().remove("revision");
    document
}

#[test]
fn copy_is_read_only_and_reports_induced_bonds_crossings_and_explicit_secondary_positions() {
    let (app, _directory) = application();
    let id = source(&app);
    edit(
        &app,
        &id,
        json!([{"op":"add_instance","source":"s1","offset":{"x":100.0,"y":20.0}}]),
    );
    let before = snapshot(&app, &id);
    let result = call(
        &app,
        "document.clipboard_copy",
        json!({"document_id":id,
        "selection":{"atoms":["a3","a3"],"bonds":["b1"]},"instance":"s2"}),
    );
    assert_eq!(result["outcome"], "ok", "{result}");
    assert_eq!(result["loss_report"], json!([]));
    assert_eq!(result["result"]["snapshot"], before);
    assert_eq!(result["result"]["instance"], "s2");
    assert_eq!(
        result["result"]["copied"],
        json!({"objects":[],"atoms":["a1","a2","a3"],"bonds":["b1","b2","b3"]})
    );
    assert_eq!(result["result"]["boundary_bonds"], json!(["b4"]));
    let payload: Value =
        serde_json::from_str(result["result"]["payload"].as_str().unwrap()).unwrap();
    assert_eq!(payload["format"], "chemifree.clipboard");
    assert_eq!(payload["version"], 1);
    assert_eq!(payload["settings"], before["document"]["settings"]);
    assert_eq!(
        payload["atoms"][0],
        json!({"id":"a1","kind":{"type":"element","symbol":"C"},
        "charge":0,"isotope":13,"hydrogens":{"mode":"specified","count":1},"radical":"doublet",
        "attachment_points":[1],"position":{"x":100.0,"y":20.0}})
    );
    assert_eq!(payload["atoms"][2]["position"], json!({"x":110.0,"y":30.0}));
    assert_eq!(
        payload["bonds"][0]["mark"],
        json!({"kind":"wedge","narrow":"begin"})
    );
    assert_eq!(
        snapshot(&app, &id),
        before,
        "copy must not change source state or selection"
    );
}

#[test]
fn copy_requires_explicit_instance_and_refuses_scene_or_unknown_targets_without_mutation() {
    let (app, _directory) = application();
    let id = source(&app);
    let before = snapshot(&app, &id);
    let missing = call(
        &app,
        "document.clipboard_copy",
        json!({"document_id":id,"selection":{"atoms":["a1"]}}),
    );
    assert_eq!(missing["outcome"], "error");
    assert_eq!(missing["diagnostics"][0]["code"], "invalid_request");
    for (selection, instance, reason) in [
        (
            json!({"objects":["s1"]}),
            "s1",
            "clipboard_scene_selection_unsupported",
        ),
        (
            json!({"atoms":["a99"]}),
            "s1",
            "clipboard_selection_unknown",
        ),
        (json!({"atoms":["a1"]}), "s99", "clipboard_instance_invalid"),
        (json!({}), "s1", "clipboard_empty_selection"),
    ] {
        let result = call(
            &app,
            "document.clipboard_copy",
            json!({"document_id":id,"selection":selection,"instance":instance}),
        );
        assert_refusal(&result, reason);
        assert_eq!(snapshot(&app, &id), before, "{reason}");
    }
}

#[test]
fn paste_allocates_fresh_ids_preserves_states_and_replays_original_mapping_after_undo_redo() {
    let (app, _directory) = application();
    let source_id = source(&app);
    let source_before = snapshot(&app, &source_id);
    let copied = copy(&app, &source_id);
    let payload = copied["payload"].as_str().unwrap();
    let id = create(&app, "Destination");
    edit(
        &app,
        &id,
        json!([
            {"op":"add_atom","element":"C","position":{"x":-20.0,"y":0.0}},
            {"op":"add_atom","element":"C","position":{"x":-10.0,"y":0.0},"bond_to":{"atom":"a1","order":"single"}}
        ]),
    );
    let before = snapshot(&app, &id);
    let mut parameters = paste_parameters(&id, 1, payload);
    parameters["selection_before"] = json!({"objects":[],"atoms":["a1"],"bonds":[]});
    let pasted = request(
        &app,
        "paste-once",
        "document.clipboard_paste",
        parameters.clone(),
    );
    assert_eq!(pasted["outcome"], "ok", "{pasted}");
    assert_eq!(
        pasted["result"]["mapping"],
        json!({
        "atoms":[{"source":"a1","target":"a3"},{"source":"a2","target":"a4"},{"source":"a3","target":"a5"}],
        "bonds":[{"source":"b1","target":"b2"},{"source":"b2","target":"b3"},{"source":"b3","target":"b4"}]})
    );
    let after = &pasted["result"]["snapshot"];
    assert_eq!(after["document"]["revision"], 2);
    assert_eq!(after["notes"], json!([]));
    assert_eq!(
        after["document"]["selection"],
        json!({"objects":[],"atoms":["a3","a4","a5"],"bonds":["b2","b3","b4"]})
    );
    assert_eq!(
        after["document"]["atoms"][2]["position"],
        json!({"x":30.0,"y":40.0})
    );
    assert_eq!(after["document"]["atoms"][2]["isotope"], 13);
    assert_eq!(
        after["document"]["atoms"][2]["hydrogens"],
        json!({"mode":"specified","count":1})
    );
    assert_eq!(after["document"]["atoms"][2]["radical"], "doublet");
    assert_eq!(
        after["document"]["atoms"][2]["attachment_points"],
        json!([1])
    );
    assert_eq!(
        after["document"]["bonds"][1]["mark"],
        json!({"kind":"wedge","narrow":"begin"})
    );
    assert_eq!(after["document"]["bonds"][2]["order"], "double");
    assert_eq!(
        after["document"]["atoms"][0],
        before["document"]["atoms"][0]
    );
    let undone = call(
        &app,
        "history.undo",
        json!({"document_id":id,"expected_revision":2}),
    );
    assert_eq!(undone["outcome"], "ok", "{undone}");
    let mut expected_before = content(&before);
    expected_before["selection"] = json!({"objects":[],"atoms":["a1"],"bonds":[]});
    assert_eq!(
        content(&undone["result"]),
        expected_before,
        "one undo removes the complete pasted graph"
    );
    let redone = call(
        &app,
        "history.redo",
        json!({"document_id":id,"expected_revision":3}),
    );
    assert_eq!(redone["outcome"], "ok", "{redone}");
    assert_eq!(content(&redone["result"]), content(after));
    edit(&app, &id, json!([{"op":"rename","title":"Later edit"}]));
    let later = snapshot(&app, &id);
    assert_eq!(
        request(
            &app,
            "paste-once",
            "document.clipboard_paste",
            parameters.clone()
        ),
        pasted
    );
    assert_eq!(
        snapshot(&app, &id),
        later,
        "replay must not allocate entities again"
    );
    parameters["offset"]["x"] = json!(31.0);
    let reused = request(&app, "paste-once", "document.clipboard_paste", parameters);
    assert_eq!(reused["outcome"], "error");
    assert_eq!(reused["diagnostics"][0]["code"], "request_id_reused");
    assert_eq!(snapshot(&app, &id), later);
    assert_eq!(snapshot(&app, &source_id), source_before);
    let saved = call(
        &app,
        "document.save",
        json!({"document_id":id,"expected_revision":later["document"]["revision"]}),
    );
    assert_eq!(saved["outcome"], "ok", "{saved}");
    let closed = call(
        &app,
        "document.close",
        json!({"document_id":id,"expected_revision":later["document"]["revision"]}),
    );
    assert_eq!(closed["outcome"], "ok", "{closed}");
    assert_eq!(
        snapshot(&app, &id)["document"],
        later["document"],
        "pasted states, fresh IDs and history selection survive save/reopen"
    );
}

#[test]
fn malformed_native_payloads_are_refused_as_data_without_touching_destination() {
    let (app, _directory) = application();
    let source_id = source(&app);
    let copied = copy(&app, &source_id);
    let payload = copied["payload"].as_str().unwrap();
    let parsed: Value = serde_json::from_str(payload).unwrap();
    let id = create(&app, "Unchanged destination");
    let before = snapshot(&app, &id);
    let mut unknown = parsed.clone();
    unknown["commands"] = json!([{"op":"rename","title":"Injected command"}]);
    let mut duplicate_atom = parsed.clone();
    duplicate_atom["atoms"]
        .as_array_mut()
        .unwrap()
        .push(parsed["atoms"][0].clone());
    let mut duplicate_bond = parsed.clone();
    duplicate_bond["bonds"]
        .as_array_mut()
        .unwrap()
        .push(parsed["bonds"][0].clone());
    let mut dangling = parsed.clone();
    dangling["bonds"][0]["end"] = json!("a999");
    let mut unsupported = parsed.clone();
    unsupported["version"] = json!(2);
    let mut future_extra = unsupported.clone();
    future_extra["future_extension"] = json!({"new_body_shape":true});
    let future_missing = json!({"format":"chemifree.clipboard","version":2});
    let malformed_current = json!({"format":"chemifree.clipboard","version":1});
    let malformed_header = json!({"format":"chemifree.clipboard","version":"1"});
    let mut missing_isotope = parsed.clone();
    missing_isotope["atoms"][0]
        .as_object_mut()
        .unwrap()
        .remove("isotope");
    let mut missing_query = parsed.clone();
    missing_query["bonds"][0]
        .as_object_mut()
        .unwrap()
        .remove("query");
    let mut missing_mark = parsed.clone();
    missing_mark["bonds"][0]
        .as_object_mut()
        .unwrap()
        .remove("mark");
    let duplicate_key = format!("{{\"format\":\"chemifree.clipboard\",{}", &payload[1..]);
    for (text, reason) in [
        (unknown.to_string(), "clipboard_fragment_invalid"),
        (duplicate_key, "clipboard_fragment_invalid"),
        (duplicate_atom.to_string(), "clipboard_duplicate_atom"),
        (duplicate_bond.to_string(), "clipboard_duplicate_bond"),
        (dangling.to_string(), "clipboard_fragment_invalid"),
        (unsupported.to_string(), "clipboard_format_unsupported"),
        (future_extra.to_string(), "clipboard_format_unsupported"),
        (future_missing.to_string(), "clipboard_format_unsupported"),
        (malformed_current.to_string(), "clipboard_fragment_invalid"),
        (malformed_header.to_string(), "clipboard_fragment_invalid"),
        (missing_isotope.to_string(), "clipboard_fragment_invalid"),
        (missing_query.to_string(), "clipboard_fragment_invalid"),
        (missing_mark.to_string(), "clipboard_fragment_invalid"),
    ] {
        let result = call(
            &app,
            "document.clipboard_paste",
            paste_parameters(&id, 0, &text),
        );
        assert_refusal(&result, reason);
        assert_eq!(
            snapshot(&app, &id),
            before,
            "{reason}: malformed native text cannot execute commands or partially paste"
        );
    }
}

#[test]
fn paste_requires_matching_settings_and_exactly_one_destination_placement() {
    let (app, _directory) = application();
    let source_id = source(&app);
    let copied = copy(&app, &source_id);
    let payload = copied["payload"].as_str().unwrap();
    let styled = create(&app, "Different style");
    let style_before = edit(
        &app,
        &styled,
        json!([{"op":"update_settings","line_width_pt":2.0}]),
    );
    let result = call(
        &app,
        "document.clipboard_paste",
        paste_parameters(&styled, 1, payload),
    );
    assert_refusal(&result, "clipboard_style_mismatch");
    assert_eq!(snapshot(&app, &styled), style_before);
    let placed = create(&app, "Two placements");
    let placement_before = edit(
        &app,
        &placed,
        json!([{"op":"add_instance","source":"s1","offset":{"x":10.0,"y":20.0}}]),
    );
    let result = call(
        &app,
        "document.clipboard_paste",
        paste_parameters(&placed, 1, payload),
    );
    assert_refusal(&result, "clipboard_destination_instances");
    assert_eq!(snapshot(&app, &placed), placement_before);
}

#[test]
fn paste_byte_limit_counts_utf8_and_accepts_the_exact_boundary() {
    let (app, _directory) = application();
    let source_id = source(&app);
    let copied = copy(&app, &source_id);
    let payload = copied["payload"].as_str().unwrap();
    let at_limit = format!("{payload}{}", " ".repeat(65536 - payload.len()));
    assert_eq!(at_limit.len(), 65536);
    let id = create(&app, "Byte limit");
    let before = snapshot(&app, &id);
    let excess = format!("{payload}{}é", " ".repeat(65535 - payload.len()));
    assert_eq!(excess.len(), 65537);
    let refused = call(
        &app,
        "document.clipboard_paste",
        paste_parameters(&id, 0, &excess),
    );
    assert_refusal(&refused, "clipboard_byte_limit");
    assert_eq!(snapshot(&app, &id), before);
    let pasted = call(
        &app,
        "document.clipboard_paste",
        paste_parameters(&id, 0, &at_limit),
    );
    assert_eq!(pasted["outcome"], "ok", "{pasted}");
    assert_eq!(
        pasted["result"]["snapshot"]["document"]["atoms"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
}

fn stereo_source(app: &App) -> String {
    let id = create(app, "Stored tetrahedral source");
    edit(
        app,
        &id,
        json!([
            {"op":"add_atom","element":"C","position":{"x":0.0,"y":0.0}},
            {"op":"add_atom","element":"F","position":{"x":0.0,"y":-30.0},"bond_to":{"atom":"a1","order":"single"}},
            {"op":"add_atom","element":"Cl","position":{"x":26.0,"y":15.0},"bond_to":{"atom":"a1","order":"single"}},
            {"op":"add_atom","element":"Br","position":{"x":-26.0,"y":15.0},"bond_to":{"atom":"a1","order":"single"}},
            {"op":"set_bond_mark","bond":"b1","mark":{"kind":"wedge","narrow":"begin"}},
            {"op":"set_atom_configuration","atom":"a1","configuration":{"kind":"tetrahedral","neighbors":["a2","a3","a4","implicit_hydrogen"],"turn":"clockwise"}}
        ]),
    );
    id
}

#[test]
fn copy_refuses_omitted_stereo_neighbors_and_even_complete_enhanced_group_members() {
    let (app, _directory) = application();
    let id = stereo_source(&app);
    let before = snapshot(&app, &id);
    let partial = call(
        &app,
        "document.clipboard_copy",
        json!({"document_id":id,"instance":"s1","selection":{"atoms":["a1","a2","a3"]}}),
    );
    assert_refusal(&partial, "clipboard_stereo_boundary");
    assert_eq!(snapshot(&app, &id), before);
    let grouped = edit(
        &app,
        &id,
        json!([{"op":"create_stereo_group","kind":"absolute","atoms":["a1"],"number":null}]),
    );
    let complete = call(
        &app,
        "document.clipboard_copy",
        json!({"document_id":id,"instance":"s1","selection":{"atoms":["a1","a2","a3","a4"]}}),
    );
    assert_refusal(&complete, "clipboard_enhanced_stereo_unsupported");
    assert_eq!(snapshot(&app, &id), grouped);
}

#[test]
fn paste_preserves_complete_stored_stereo_with_remapped_neighbors_and_wedge() {
    let (app, _directory) = application();
    let source_id = stereo_source(&app);
    let before = snapshot(&app, &source_id);
    let copied = call(
        &app,
        "document.clipboard_copy",
        json!({"document_id":source_id,"instance":"s1","selection":{"atoms":["a1","a2","a3","a4"]}}),
    );
    assert_eq!(copied["outcome"], "ok", "{copied}");
    let id = create(&app, "Stereo destination");
    edit(
        &app,
        &id,
        json!([{"op":"add_atom","element":"O","position":{"x":100.0,"y":100.0}}]),
    );
    let pasted = call(
        &app,
        "document.clipboard_paste",
        paste_parameters(&id, 1, copied["result"]["payload"].as_str().unwrap()),
    );
    assert_eq!(pasted["outcome"], "ok", "{pasted}");
    assert_eq!(
        pasted["result"]["snapshot"]["document"]["stereo"]["atoms"],
        json!([
            {"atom":"a2","configuration":{"kind":"tetrahedral","neighbors":["a3","a4","a5","implicit_hydrogen"],"turn":"clockwise"}}
        ])
    );
    assert_eq!(
        pasted["result"]["snapshot"]["document"]["bonds"][0]["mark"],
        json!({"kind":"wedge","narrow":"begin"})
    );
    assert_eq!(snapshot(&app, &source_id), before);
}

#[test]
fn paste_coordinate_failure_changes_neither_destination_nor_its_existing_selection() {
    let (app, _directory) = application();
    let source_id = source(&app);
    let copied = copy(&app, &source_id);
    let id = create(&app, "Atomic destination");
    let before = snapshot(&app, &id);
    let mut parameters = paste_parameters(&id, 0, copied["payload"].as_str().unwrap());
    parameters["offset"] = json!({"x":100000.0,"y":0.0});
    let result = call(&app, "document.clipboard_paste", parameters);
    assert_refusal(&result, "clipboard_fragment_invalid");
    assert_eq!(snapshot(&app, &id), before);
}

#[test]
fn native_list_queries_require_explicit_negation_and_round_trip_both_values() {
    let (app, _directory) = application();
    let source_id = create(&app, "Query source");
    edit(
        &app,
        &source_id,
        json!([
            {"op":"add_atom","query":{"kind":"list","elements":["N","O"],"negated":false},"position":{"x":0,"y":0}},
            {"op":"add_atom","query":{"kind":"list","elements":["F","Cl"],"negated":true},"position":{"x":10,"y":0}}
        ]),
    );
    let copied = call(
        &app,
        "document.clipboard_copy",
        json!({"document_id":source_id,
        "selection":{"atoms":["a1","a2"]},"instance":"s1"}),
    );
    assert_eq!(copied["outcome"], "ok", "{copied}");
    let text = copied["result"]["payload"].as_str().unwrap();
    let mut payload: Value = serde_json::from_str(text).unwrap();
    assert_eq!(payload["atoms"][0]["kind"]["query"]["negated"], false);
    assert_eq!(payload["atoms"][1]["kind"]["query"]["negated"], true);
    let destination = create(&app, "Query destination");
    let pasted = call(
        &app,
        "document.clipboard_paste",
        paste_parameters(&destination, 0, text),
    );
    assert_eq!(pasted["outcome"], "ok", "{pasted}");
    assert_eq!(
        pasted["result"]["snapshot"]["document"]["atoms"][0]["query"],
        json!({"kind":"list","elements":["N","O"],"negated":false})
    );
    assert_eq!(
        pasted["result"]["snapshot"]["document"]["atoms"][1]["query"],
        json!({"kind":"list","elements":["F","Cl"],"negated":true})
    );
    payload["atoms"][1]["kind"]["query"]
        .as_object_mut()
        .unwrap()
        .remove("negated");
    let before = snapshot(&app, &destination);
    let rejected = call(
        &app,
        "document.clipboard_paste",
        paste_parameters(&destination, 1, &payload.to_string()),
    );
    assert_refusal(&rejected, "clipboard_fragment_invalid");
    assert_eq!(
        snapshot(&app, &destination),
        before,
        "an omitted negation must never default to a different query or partially paste"
    );
}

#[test]
fn native_fragment_and_template_style_comparison_includes_label_weight() {
    let (app, _directory) = application();
    let source_id = source(&app);
    let normal = copy(&app, &source_id);
    let target = create(&app, "Bold destination");
    let before = edit(
        &app,
        &target,
        json!([{"op":"update_settings","label_weight":"bold"}]),
    );
    let refused = call(
        &app,
        "document.clipboard_paste",
        paste_parameters(&target, 1, normal["payload"].as_str().unwrap()),
    );
    assert_refusal(&refused, "clipboard_style_mismatch");
    assert_eq!(snapshot(&app, &target), before);
    edit(
        &app,
        &source_id,
        json!([{"op":"update_settings","label_weight":"bold"}]),
    );
    let bold = copy(&app, &source_id);
    let payload: Value = serde_json::from_str(bold["payload"].as_str().unwrap()).unwrap();
    assert_eq!(payload["settings"]["label_weight"], "bold");
    let pasted = call(
        &app,
        "document.clipboard_paste",
        paste_parameters(&target, 1, bold["payload"].as_str().unwrap()),
    );
    assert_eq!(pasted["outcome"], "ok", "{pasted}");
    assert_eq!(
        snapshot(&app, &target)["document"]["settings"]["label_weight"],
        "bold"
    );
}

#[test]
fn old_normal_templates_paste_as_normal_but_do_not_match_bold_destinations() {
    let (app, _directory) = application();
    let source_id = source(&app);
    let copied = copy(&app, &source_id);
    let mut old: Value = serde_json::from_str(copied["payload"].as_str().unwrap()).unwrap();
    old["settings"]
        .as_object_mut()
        .unwrap()
        .remove("label_weight");
    let normal = create(&app, "Old normal template");
    let pasted = call(
        &app,
        "document.clipboard_paste",
        paste_parameters(&normal, 0, &old.to_string()),
    );
    assert_eq!(pasted["outcome"], "ok", "{pasted}");
    assert_eq!(
        snapshot(&app, &normal)["document"]["settings"]["label_weight"],
        "normal"
    );
    let bold = create(&app, "Old template into bold");
    let before = edit(
        &app,
        &bold,
        json!([{"op":"update_settings","label_weight":"bold"}]),
    );
    let refused = call(
        &app,
        "document.clipboard_paste",
        paste_parameters(&bold, 1, &old.to_string()),
    );
    assert_refusal(&refused, "clipboard_style_mismatch");
    assert_eq!(snapshot(&app, &bold), before);
    for invalid in [Value::Null, json!("heavy"), json!(700)] {
        let mut malformed = old.clone();
        malformed["settings"]["label_weight"] = invalid;
        let before = snapshot(&app, &normal);
        let result = call(
            &app,
            "document.clipboard_paste",
            paste_parameters(&normal, 1, &malformed.to_string()),
        );
        assert_refusal(&result, "clipboard_fragment_invalid");
        assert_eq!(snapshot(&app, &normal), before);
    }
}

#[test]
fn native_fragment_and_template_style_comparison_includes_label_style() {
    let (app, _directory) = application();
    let source_id = source(&app);
    let normal = copy(&app, &source_id);
    let target = create(&app, "Italic destination");
    let before = edit(
        &app,
        &target,
        json!([{"op":"update_settings","label_style":"italic"}]),
    );
    let refused = call(
        &app,
        "document.clipboard_paste",
        paste_parameters(&target, 1, normal["payload"].as_str().unwrap()),
    );
    assert_refusal(&refused, "clipboard_style_mismatch");
    assert_eq!(snapshot(&app, &target), before);
    edit(
        &app,
        &source_id,
        json!([{"op":"update_settings","label_style":"italic"}]),
    );
    let italic = copy(&app, &source_id);
    let payload: Value = serde_json::from_str(italic["payload"].as_str().unwrap()).unwrap();
    assert_eq!(payload["settings"]["label_style"], "italic");
    let pasted = call(
        &app,
        "document.clipboard_paste",
        paste_parameters(&target, 1, italic["payload"].as_str().unwrap()),
    );
    assert_eq!(pasted["outcome"], "ok", "{pasted}");
    assert_eq!(
        snapshot(&app, &target)["document"]["settings"]["label_style"],
        "italic"
    );
}

#[test]
fn old_normal_templates_paste_as_normal_but_do_not_match_italic_destinations() {
    let (app, _directory) = application();
    let source_id = source(&app);
    let copied = copy(&app, &source_id);
    let mut old: Value = serde_json::from_str(copied["payload"].as_str().unwrap()).unwrap();
    old["settings"]
        .as_object_mut()
        .unwrap()
        .remove("label_style");
    let normal = create(&app, "Old normal template");
    let pasted = call(
        &app,
        "document.clipboard_paste",
        paste_parameters(&normal, 0, &old.to_string()),
    );
    assert_eq!(pasted["outcome"], "ok", "{pasted}");
    assert_eq!(
        snapshot(&app, &normal)["document"]["settings"]["label_style"],
        "normal"
    );
    let italic = create(&app, "Old template into italic");
    let before = edit(
        &app,
        &italic,
        json!([{"op":"update_settings","label_style":"italic"}]),
    );
    let refused = call(
        &app,
        "document.clipboard_paste",
        paste_parameters(&italic, 1, &old.to_string()),
    );
    assert_refusal(&refused, "clipboard_style_mismatch");
    assert_eq!(snapshot(&app, &italic), before);
    for invalid in [Value::Null, json!("heavy"), json!(700)] {
        let mut malformed = old.clone();
        malformed["settings"]["label_style"] = invalid;
        let before = snapshot(&app, &normal);
        let result = call(
            &app,
            "document.clipboard_paste",
            paste_parameters(&normal, 1, &malformed.to_string()),
        );
        assert_refusal(&result, "clipboard_fragment_invalid");
        assert_eq!(snapshot(&app, &normal), before);
    }
}

#[test]
fn native_fragment_and_template_style_comparison_includes_label_underline() {
    let (app, _directory) = application();
    let source_id = source(&app);
    let normal = copy(&app, &source_id);
    let target = create(&app, "Underlined destination");
    let before = edit(
        &app,
        &target,
        json!([{"op":"update_settings","label_underline":true}]),
    );
    let refused = call(
        &app,
        "document.clipboard_paste",
        paste_parameters(&target, 1, normal["payload"].as_str().unwrap()),
    );
    assert_refusal(&refused, "clipboard_style_mismatch");
    assert_eq!(snapshot(&app, &target), before);
    edit(
        &app,
        &source_id,
        json!([{"op":"update_settings","label_underline":true}]),
    );
    let italic = copy(&app, &source_id);
    let payload: Value = serde_json::from_str(italic["payload"].as_str().unwrap()).unwrap();
    assert_eq!(payload["settings"]["label_underline"], true);
    let pasted = call(
        &app,
        "document.clipboard_paste",
        paste_parameters(&target, 1, italic["payload"].as_str().unwrap()),
    );
    assert_eq!(pasted["outcome"], "ok", "{pasted}");
    assert_eq!(
        snapshot(&app, &target)["document"]["settings"]["label_underline"],
        true
    );
}

#[test]
fn old_templates_default_false_but_do_not_match_underlined_destinations() {
    let (app, _directory) = application();
    let source_id = source(&app);
    let copied = copy(&app, &source_id);
    let mut old: Value = serde_json::from_str(copied["payload"].as_str().unwrap()).unwrap();
    old["settings"]
        .as_object_mut()
        .unwrap()
        .remove("label_underline");
    let normal = create(&app, "Old plain template");
    let pasted = call(
        &app,
        "document.clipboard_paste",
        paste_parameters(&normal, 0, &old.to_string()),
    );
    assert_eq!(pasted["outcome"], "ok", "{pasted}");
    assert_eq!(
        snapshot(&app, &normal)["document"]["settings"]["label_underline"],
        false
    );
    let italic = create(&app, "Old template into underline");
    let before = edit(
        &app,
        &italic,
        json!([{"op":"update_settings","label_underline":true}]),
    );
    let refused = call(
        &app,
        "document.clipboard_paste",
        paste_parameters(&italic, 1, &old.to_string()),
    );
    assert_refusal(&refused, "clipboard_style_mismatch");
    assert_eq!(snapshot(&app, &italic), before);
    for invalid in [Value::Null, json!("heavy"), json!(700)] {
        let mut malformed = old.clone();
        malformed["settings"]["label_underline"] = invalid;
        let before = snapshot(&app, &normal);
        let result = call(
            &app,
            "document.clipboard_paste",
            paste_parameters(&normal, 1, &malformed.to_string()),
        );
        assert_refusal(&result, "clipboard_fragment_invalid");
        assert_eq!(snapshot(&app, &normal), before);
    }
}

#[test]
fn native_fragment_and_template_style_comparison_includes_label_outline() {
    let (app, _directory) = application();
    let source_id = source(&app);
    let normal = copy(&app, &source_id);
    let target = create(&app, "Underlined destination");
    let before = edit(
        &app,
        &target,
        json!([{"op":"update_settings","label_outline":true}]),
    );
    let refused = call(
        &app,
        "document.clipboard_paste",
        paste_parameters(&target, 1, normal["payload"].as_str().unwrap()),
    );
    assert_refusal(&refused, "clipboard_style_mismatch");
    assert_eq!(snapshot(&app, &target), before);
    edit(
        &app,
        &source_id,
        json!([{"op":"update_settings","label_outline":true}]),
    );
    let italic = copy(&app, &source_id);
    let payload: Value = serde_json::from_str(italic["payload"].as_str().unwrap()).unwrap();
    assert_eq!(payload["settings"]["label_outline"], true);
    let pasted = call(
        &app,
        "document.clipboard_paste",
        paste_parameters(&target, 1, italic["payload"].as_str().unwrap()),
    );
    assert_eq!(pasted["outcome"], "ok", "{pasted}");
    assert_eq!(
        snapshot(&app, &target)["document"]["settings"]["label_outline"],
        true
    );
}

#[test]
fn old_templates_default_false_but_do_not_match_outlined_destinations() {
    let (app, _directory) = application();
    let source_id = source(&app);
    let copied = copy(&app, &source_id);
    let mut old: Value = serde_json::from_str(copied["payload"].as_str().unwrap()).unwrap();
    old["settings"]
        .as_object_mut()
        .unwrap()
        .remove("label_outline");
    let normal = create(&app, "Old plain template");
    let pasted = call(
        &app,
        "document.clipboard_paste",
        paste_parameters(&normal, 0, &old.to_string()),
    );
    assert_eq!(pasted["outcome"], "ok", "{pasted}");
    assert_eq!(
        snapshot(&app, &normal)["document"]["settings"]["label_outline"],
        false
    );
    let italic = create(&app, "Old template into outline");
    let before = edit(
        &app,
        &italic,
        json!([{"op":"update_settings","label_outline":true}]),
    );
    let refused = call(
        &app,
        "document.clipboard_paste",
        paste_parameters(&italic, 1, &old.to_string()),
    );
    assert_refusal(&refused, "clipboard_style_mismatch");
    assert_eq!(snapshot(&app, &italic), before);
    for invalid in [Value::Null, json!("heavy"), json!(700)] {
        let mut malformed = old.clone();
        malformed["settings"]["label_outline"] = invalid;
        let before = snapshot(&app, &normal);
        let result = call(
            &app,
            "document.clipboard_paste",
            paste_parameters(&normal, 1, &malformed.to_string()),
        );
        assert_refusal(&result, "clipboard_fragment_invalid");
        assert_eq!(snapshot(&app, &normal), before);
    }
}
