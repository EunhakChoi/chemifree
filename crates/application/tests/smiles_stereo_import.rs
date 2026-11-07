//! SMILES stereo through `document.import`: stored configurations with no marks, CXSMILES groups
//! as stereo groups, and the all-or-nothing rule when the text is refused.

use chemifree_application::Application;
use chemifree_providers::interchange_codec::LocalInterchangeCodec;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities, OpenSmilesCodec, V2000Codec};
use serde_json::{Value, json};
use tempfile::TempDir;

type App = Application<LocalCapabilities, FileDocumentRepository>;

fn setup() -> (App, TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    let app = Application::new(LocalCapabilities, FileDocumentRepository::new(dir.path()))
        .with_ctfile_codec(V2000Codec)
        .with_interchange_codec(LocalInterchangeCodec)
        .with_smiles_codec(OpenSmilesCodec);
    let created = call(&app, "create", "document.create", json!({}));
    let id = created["result"]["document"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    (app, dir, id)
}

fn call(app: &App, id: &str, operation: &str, parameters: Value) -> Value {
    let request = json!({"contract":1,"request_id":id,"profile":"preserve-semantics","operation":operation,"parameters":parameters});
    serde_json::to_value(app.execute(request.to_string().as_bytes())).unwrap()
}

fn import(app: &App, id: &str, revision: u64, source: &str) -> Value {
    let params = |extra: Value| {
        let mut base = json!({"document_id":id,"expected_revision":revision,"format":"smiles","source":source});
        base.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        base
    };
    let preview = call(
        app,
        &format!("p{revision}"),
        "document.import",
        params(json!({"preview": true})),
    );
    let Some(digest) = preview["result"]["preview_digest"].as_str() else {
        return preview;
    };
    call(
        app,
        &format!("c{revision}"),
        "document.import",
        params(json!({"preview_digest": digest, "accept_loss": true})),
    )
}

fn document(app: &App, id: &str) -> Value {
    call(app, "s", "document.snapshot", json!({"document_id":id}))["result"]["document"].clone()
}

#[test]
fn a_tagged_center_is_stored_as_a_configuration_with_no_mark() {
    let (app, _dir, id) = setup();
    let done = import(&app, &id, 0, "F[C@H](Cl)[C@H](Br)I");
    assert_eq!(done["document_revision"], 1, "{done}");
    let doc = document(&app, &id);
    let stereo = &doc["stereo"];
    // F0 C1 Cl2 C3 Br4 I5 are a1..a6; each center reads (preceding, H, ...), ACW, which the
    // store keeps with the hydrogen last (two steps, even).
    assert_eq!(
        stereo["atoms"],
        json!([
            {"atom":"a2","configuration":{"kind":"tetrahedral","neighbors":["a1","a3","a4","implicit_hydrogen"],"turn":"anticlockwise"}},
            {"atom":"a4","configuration":{"kind":"tetrahedral","neighbors":["a2","a5","a6","implicit_hydrogen"],"turn":"anticlockwise"}}
        ]),
        "{stereo}"
    );
    assert!(
        doc["bonds"]
            .as_array()
            .unwrap()
            .iter()
            .all(|bond| bond["mark"].is_null() || bond.get("mark").is_none()),
        "no bond carries a mark: {doc}"
    );
}

#[test]
fn a_cx_block_becomes_stereo_groups_and_a_loss_free_import() {
    let (app, _dir, id) = setup();
    let done = import(&app, &id, 0, "F[C@H](Cl)[C@H](Br)I |o1:1,3|");
    assert_eq!(done["document_revision"], 1, "{done}");
    let groups = document(&app, &id)["stereo"]["groups"].clone();
    let groups = groups.as_array().unwrap();
    assert_eq!(groups.len(), 1, "{groups:?}");
    assert_eq!(groups[0]["kind"], "or");
    assert_eq!(groups[0]["number"], 1);
    assert_eq!(groups[0]["members"], json!(["a2", "a4"]));
    let features: Vec<&str> = done["loss_report"]
        .as_array()
        .unwrap()
        .iter()
        .map(|loss| loss["feature"].as_str().unwrap())
        .collect();
    assert!(!features.contains(&"ignored_text"), "{features:?}");
}

#[test]
fn a_second_import_with_the_same_group_number_is_renumbered_and_reported() {
    let (app, _dir, id) = setup();
    import(&app, &id, 0, "F[C@H](Cl)Br |o1:1|");
    let done = import(&app, &id, 1, "I[C@H](Cl)Br |o1:1|");
    assert_eq!(done["document_revision"], 2, "{done}");
    let groups = document(&app, &id)["stereo"]["groups"].clone();
    let numbers: Vec<i64> = groups
        .as_array()
        .unwrap()
        .iter()
        .map(|group| group["number"].as_i64().unwrap())
        .collect();
    assert_eq!(numbers, [1, 2]);
    assert!(
        done["loss_report"]
            .as_array()
            .unwrap()
            .iter()
            .any(|loss| loss["feature"] == "stereo_group_number"),
        "{done}"
    );
}

#[test]
fn a_refused_text_leaves_the_document_unchanged() {
    let (app, _dir, id) = setup();
    import(&app, &id, 0, "CCO");
    let before = document(&app, &id);
    for (source, family) in [
        ("F[C@H](Cl)Br |a:0|", "invalid_chemistry"),
        ("F[C@H](Cl)Br |r|", "unsupported_feature"),
        ("F[C@H](Cl)Br |a:1", "malformed_input"),
        ("F[C@TH1H](Cl)Br", "unsupported_feature"),
        ("[S@](=O)(C)CC", "unsupported_feature"),
        ("F/P=C/F", "unsupported_feature"),
        ("C[C@H](F)", "unsupported_feature"),
    ] {
        let reply = import(&app, &id, 1, source);
        assert_eq!(reply["outcome"], "error", "{source}: {reply}");
        assert_eq!(
            reply["diagnostics"][0]["details"]["family"], family,
            "{source}: {reply}"
        );
        assert_eq!(document(&app, &id), before, "{source}");
    }
}

#[test]
fn a_replayed_import_reports_the_group_losses_of_the_original() {
    let (app, _dir, id) = setup();
    let source = "F[C@H](Cl)[C@H](Br)I |o1:1,a:3|";
    import(&app, &id, 0, source);
    // The document now has or 1 and an absolute group: the same text collides with both.
    let base = json!({"document_id":id,"expected_revision":1,"format":"smiles","source":source});
    let mut preview = base.clone();
    preview["preview"] = json!(true);
    let preview = call(&app, "again-p", "document.import", preview);
    let digest = preview["result"]["preview_digest"].clone();
    let mut commit = base;
    commit["preview_digest"] = digest;
    commit["accept_loss"] = json!(true);
    let first = call(&app, "again", "document.import", commit.clone());
    assert_eq!(first["document_revision"], 2, "{first}");
    let features = |reply: &Value| -> Vec<String> {
        reply["loss_report"]
            .as_array()
            .unwrap()
            .iter()
            .map(|loss| loss["feature"].as_str().unwrap().to_owned())
            .collect()
    };
    let original = features(&first);
    assert!(
        original.contains(&"stereo_group_number".to_owned())
            && original.contains(&"stereo_group_merged".to_owned()),
        "{original:?}"
    );
    // Same request id and parameters: the committed reply is replayed, not applied again.
    let replay = call(&app, "again", "document.import", commit);
    assert_eq!(replay["document_revision"], 2, "{replay}");
    assert_eq!(replay["loss_report"], first["loss_report"], "{replay}");
}

#[test]
fn a_text_of_exactly_the_command_bound_commits_with_its_groups_and_one_more_is_refused() {
    // 70 units of `C[C@H](F)` and `C[C@H](F)Cl` are 214 atoms, 213 bonds and 71 configurations,
    // 498 commands; each group is one more. Two groups make 500, the bound of one transaction.
    let text = |groups: &str| format!("{}C[C@H](F)Cl |{groups}|", "C[C@H](F)".repeat(70));
    let (app, _dir, id) = setup();
    let done = import(&app, &id, 0, &text("o1:1,o2:4"));
    assert_eq!(done["document_revision"], 1, "{done}");
    assert_eq!(
        document(&app, &id)["stereo"]["groups"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );
    // A third group is the 501st command: refused by the reader, nothing imported.
    let (app, _dir, id) = setup();
    let refused = import(&app, &id, 0, &text("o1:1,o2:4,a:7"));
    assert_eq!(refused["outcome"], "error", "{refused}");
    assert_eq!(refused["diagnostics"][0]["code"], "command_limit");
    assert_eq!(document(&app, &id)["revision"], 0);
}

#[test]
fn an_import_next_to_an_existing_absolute_group_still_fits_the_bound_it_was_counted_against() {
    // Joining the document's absolute group is one command, as creating one is.
    let (app, _dir, id) = setup();
    import(&app, &id, 0, "F[C@H](Cl)Br |a:1|");
    let text = format!("{}C[C@H](F)Cl |a:1,o1:4|", "C[C@H](F)".repeat(70));
    let done = import(&app, &id, 1, &text);
    assert_eq!(done["document_revision"], 2, "{done}");
}

#[test]
fn recorded_export_preserves_imported_centers_and_groups_without_stereo_consent() {
    let (app, _dir, id) = setup();
    let source = "F[C@H](Cl)[C@@H](Br)I |o1:1,3|";
    let imported = import(&app, &id, 0, source);
    assert_eq!(imported["document_revision"], 1, "{imported}");
    let before = document(&app, &id);
    let written = call(
        &app,
        "recorded",
        "document.export",
        json!({"document_id": id, "format": "smiles", "style": "recorded"}),
    );
    assert_eq!(written["outcome"], "partial", "{written}");
    assert_eq!(written["result"]["snapshot"]["document"], before);
    assert_eq!(written["result"]["content"], source);
    assert!(
        written["loss_report"]
            .as_array()
            .unwrap()
            .iter()
            .all(|loss| {
                !["stereo_configuration", "stereo_marks", "stereo_group"]
                    .contains(&loss["feature"].as_str().unwrap())
            }),
        "preserved stereo must not be reported as lost: {written}"
    );
    assert_eq!(document(&app, &id), before);
}

#[test]
fn accepting_attachment_loss_preserves_the_same_snapshots_stereo() {
    let (app, _dir, id) = setup();
    let source = "F[C@H](Cl)Br |a:1|";
    let imported = import(&app, &id, 0, source);
    assert_eq!(imported["document_revision"], 1, "{imported}");
    let edited = call(
        &app,
        "attachment",
        "document.transaction",
        json!({"document_id": id, "expected_revision": 1, "commands": [
            {"op": "update_atom", "atom": "a2", "attachment_points": [1]}
        ]}),
    );
    assert_eq!(edited["outcome"], "ok", "{edited}");
    let before = document(&app, &id);
    let parameters = json!({"document_id": id, "format": "smiles", "style": "recorded"});
    let refused = call(&app, "refused", "document.export", parameters.clone());
    let details = &refused["diagnostics"][0]["details"];
    assert_eq!(
        refused["diagnostics"][0]["code"], "loss_consent_required",
        "{refused}"
    );
    assert_eq!(details["features"], json!(["attachment_points"]));
    let mut accepted = parameters;
    accepted["accepted_losses"] = details["loss_digest"].clone();
    let written = call(&app, "accepted", "document.export", accepted);
    assert_eq!(written["outcome"], "partial", "{written}");
    assert_eq!(written["result"]["snapshot"]["document"], before);
    assert_eq!(written["result"]["content"], source);
    assert_eq!(document(&app, &id), before);
}

// ---------------------------------------------------------------------------------------------
// Directional bonds `/` and `\` (P29c2a): stored double-bond configurations, no marks
// ---------------------------------------------------------------------------------------------

fn cip(app: &App, id: &str) -> Value {
    call(app, "cip", "chem.cip", json!({"document_id": id}))
}

fn stored_bonds(app: &App, id: &str) -> Value {
    document(app, id)["stereo"]["bonds"].clone()
}

/// The CIP descriptor of the one double bond `chem.cip` lists for the document.
fn descriptor(app: &App, id: &str) -> Value {
    let reply = cip(app, id);
    assert_eq!(reply["outcome"], "ok", "{reply}");
    let bonds = reply["result"]["bonds"].as_array().unwrap();
    assert_eq!(bonds.len(), 1, "{reply}");
    bonds[0]["descriptor"].clone()
}

#[test]
fn a_directional_import_stores_the_double_bond_configuration_with_no_mark() {
    for (source, relation, descriptor_expected) in [
        ("Cl/C=C\\F", "same_side", "Z"),
        ("Cl/C=C/F", "opposite_sides", "E"),
        ("Cl\\C=C/F", "same_side", "Z"),
        // The chlorine is above the first carbon and the fluorine written first is above too.
        ("C(/Cl)=C\\F", "opposite_sides", "E"),
    ] {
        let (app, _dir, id) = setup();
        let done = import(&app, &id, 0, source);
        assert_eq!(done["document_revision"], 1, "{source}: {done}");
        // Cl, C, C, F are a1..a4 (the branch form is C, Cl, C, F) and b2 is the double bond: the
        // configuration names the marked neighbour of each end.
        let begin = if source.starts_with("C(") { "a2" } else { "a1" };
        assert_eq!(
            stored_bonds(&app, &id),
            json!([{"bond": "b2",
                    "configuration": {"kind":"relative","begin_neighbor":begin,
                                      "end_neighbor":"a4","relation":relation}}]),
            "{source}"
        );
        assert_eq!(descriptor(&app, &id), descriptor_expected, "{source}");
        assert!(
            document(&app, &id)["bonds"]
                .as_array()
                .unwrap()
                .iter()
                .all(|bond| bond["mark"].is_null()),
            "no bond carries a mark: {source}"
        );
    }
}

#[test]
fn an_unmarked_lower_neighbour_is_the_stored_reference_with_the_relation_flipped() {
    // C0 C1 F2 C3 F4: the fluorines are both above, so they are on the same side; the store
    // names the lowest neighbour of the first carbon, C1, which is on the other side.
    let (app, _dir, id) = setup();
    import(&app, &id, 0, "C(C)(/F)=C/F");
    assert_eq!(
        stored_bonds(&app, &id),
        json!([{"bond": "b3", "configuration": {"kind":"relative","begin_neighbor":"a2",
                "end_neighbor":"a5","relation":"opposite_sides"}}])
    );
    assert_eq!(descriptor(&app, &id), "Z");
}

#[test]
fn every_double_bond_of_a_conjugated_chain_is_stored() {
    let (app, _dir, id) = setup();
    import(&app, &id, 0, "C/C=C/C=C\\C");
    assert_eq!(
        stored_bonds(&app, &id),
        json!([
            {"bond": "b2", "configuration": {"kind":"relative","begin_neighbor":"a1","end_neighbor":"a4","relation":"opposite_sides"}},
            {"bond": "b4", "configuration": {"kind":"relative","begin_neighbor":"a3","end_neighbor":"a6","relation":"same_side"}}
        ])
    );
}

#[test]
fn a_double_bond_served_by_its_neighbour_adds_no_record_and_no_loss() {
    let (app, _dir, id) = setup();
    let done = import(&app, &id, 0, "C/C=C/C=CC");
    assert_eq!(done["document_revision"], 1, "{done}");
    assert_eq!(stored_bonds(&app, &id).as_array().map(Vec::len), Some(1));
    let features: Vec<&str> = done["loss_report"]
        .as_array()
        .unwrap()
        .iter()
        .map(|loss| loss["feature"].as_str().unwrap())
        .collect();
    assert_eq!(features, ["generated_layout"], "{done}");
}

#[test]
fn a_ring_double_bond_of_eight_atoms_is_stored_and_labelled_by_cip_because_it_is_stored() {
    let (app, _dir, id) = setup();
    import(&app, &id, 0, "C1CC/C=C\\CCC1");
    assert_eq!(stored_bonds(&app, &id).as_array().map(Vec::len), Some(1));
    // `chem.cip` lists every stored configuration, in a ring or not, in both scopes.
    assert_eq!(descriptor(&app, &id), "Z");
    let scanned = call(
        &app,
        "scan",
        "chem.cip",
        json!({"document_id": id, "scope": "stereogenic"}),
    );
    assert_eq!(scanned["result"]["bonds"][0]["bond"], "b4", "{scanned}");
    assert_eq!(
        scanned["result"]["bonds"][0]["descriptor"], "Z",
        "{scanned}"
    );
    // The same ring with no marks stores nothing, and the scan leaves ring double bonds out.
    let (app, _dir, id) = setup();
    import(&app, &id, 0, "C1CCC=CCCC1");
    let unmarked = call(
        &app,
        "scan",
        "chem.cip",
        json!({"document_id": id, "scope": "stereogenic"}),
    );
    assert_eq!(unmarked["result"]["bonds"], json!([]), "{unmarked}");
}

#[test]
fn directional_refusals_name_their_cause_and_leave_the_document_unchanged() {
    let (app, _dir, id) = setup();
    import(&app, &id, 0, "CCO");
    let before = document(&app, &id);
    for (source, code, family, offset) in [
        ("C/C=C", "directional_bond_incomplete", "malformed_input", 1),
        (
            "F/CC",
            "directional_bond_without_double_bond",
            "malformed_input",
            1,
        ),
        (
            "C/C(\\F)=C/C",
            "directional_bond_conflict",
            "invalid_chemistry",
            4,
        ),
        (
            "C1C/C=C\\CCC1",
            "directional_bond_small_ring",
            "unsupported_feature",
            3,
        ),
        (
            "F/C=C=C/F",
            "directional_bond_cumulated",
            "unsupported_feature",
            1,
        ),
        (
            "F/P=C/F",
            "directional_bond_end_unsupported",
            "unsupported_feature",
            1,
        ),
        (
            "C/1=C/CCCCCCC1",
            "directional_bond_unsupported",
            "unsupported_feature",
            1,
        ),
        (
            "c1ccc/cc1",
            "directional_bond_unsupported",
            "unsupported_feature",
            5,
        ),
    ] {
        let reply = import(&app, &id, 1, source);
        assert_eq!(reply["outcome"], "error", "{source}: {reply}");
        let found = &reply["diagnostics"][0];
        assert_eq!(found["code"], code, "{source}: {reply}");
        assert_eq!(found["details"]["family"], family, "{source}: {reply}");
        assert_eq!(found["details"]["offset"], offset, "{source}: {reply}");
        assert_eq!(found["details"]["length"], 1, "{source}: {reply}");
        assert!(found["details"]["reason"].is_string(), "{source}: {reply}");
        assert_eq!(document(&app, &id), before, "{source}");
    }
}

#[test]
fn a_replayed_directional_import_returns_the_original_reply() {
    let (app, _dir, id) = setup();
    let source = "C/C=C/C=C\\C";
    let base = json!({"document_id":id,"expected_revision":0,"format":"smiles","source":source});
    let mut preview = base.clone();
    preview["preview"] = json!(true);
    let preview = call(&app, "dir-p", "document.import", preview);
    let mut commit = base;
    commit["preview_digest"] = preview["result"]["preview_digest"].clone();
    commit["accept_loss"] = json!(true);
    let first = call(&app, "dir", "document.import", commit.clone());
    assert_eq!(first["document_revision"], 1, "{first}");
    let replay = call(&app, "dir", "document.import", commit);
    assert_eq!(replay["document_revision"], 1, "{replay}");
    assert_eq!(replay["loss_report"], first["loss_report"], "{replay}");
    assert_eq!(replay["result"], first["result"], "{replay}");
    assert_eq!(stored_bonds(&app, &id).as_array().map(Vec::len), Some(2));
}

#[test]
fn a_text_of_exactly_the_command_bound_commits_with_its_configurations() {
    // 62 units of `C/C=C/C` are 248 atoms, 186 bonds and 62 configurations, 496 commands;
    // `CC.C` adds 3 atoms and a bond: 500, the bound of one transaction.
    let units = vec!["C/C=C/C"; 62].join(".");
    let (app, _dir, id) = setup();
    let done = import(&app, &id, 0, &format!("{units}.CC.C"));
    assert_eq!(done["document_revision"], 1, "{done}");
    assert_eq!(stored_bonds(&app, &id).as_array().map(Vec::len), Some(62));
    let (app, _dir, id) = setup();
    let refused = import(&app, &id, 0, &format!("{units}.CC.C.C"));
    assert_eq!(refused["outcome"], "error", "{refused}");
    assert_eq!(refused["diagnostics"][0]["code"], "command_limit");
    assert_eq!(document(&app, &id)["revision"], 0);
}

#[test]
fn exporting_an_imported_double_bond_configuration_needs_consent_and_writes_no_marks() {
    // The writers do not write `/` and `\` yet: recorded output reports the configuration as a
    // consent-required loss, canonical output as the loss of all SMILES stereo.
    for (source, recorded) in [("F/C=C/F", "FC=CF"), ("F/C=C\\F", "FC=CF")] {
        let (app, _dir, id) = setup();
        import(&app, &id, 0, source);
        let before = document(&app, &id);
        for (style, expected) in [("recorded", recorded), ("canonical", "")] {
            let parameters = json!({"document_id": id, "format": "smiles", "style": style});
            let refused = call(&app, "refused", "document.export", parameters.clone());
            let found = &refused["diagnostics"][0];
            assert_eq!(found["code"], "loss_consent_required", "{style}: {refused}");
            assert_eq!(
                found["details"]["features"],
                json!(["stereo_configuration"])
            );
            assert_eq!(found["details"]["losses"][0]["source_path"], "bonds[b2]");
            let mut accepted = parameters;
            accepted["accepted_losses"] = found["details"]["loss_digest"].clone();
            let written = call(&app, "accepted", "document.export", accepted);
            let content = written["result"]["content"].as_str().unwrap();
            assert!(!content.contains(['/', '\\']), "{style}: {content}");
            if !expected.is_empty() {
                assert_eq!(content, expected, "{source} {style}");
            }
            assert_eq!(document(&app, &id), before);
        }
    }
}
