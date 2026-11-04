//! MOL V3000 over the wire: import and export of the core subset with bond marks, the `ctab`
//! choice, the consent flow shared with SMILES, and the refusals that leave a document alone.
//!
//! Fixtures are hand-authored from the public CTfile description. Expected values are written
//! out here; stored configurations are compared with a document built by explicit commands.

use chemifree_application::Application;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities, V2000Codec};
use serde_json::{Value, json};
use tempfile::TempDir;

type App = Application<LocalCapabilities, FileDocumentRepository>;

/// A center with F up, Cl lower right (wedge, narrow end at the center) and Br lower left.
const WEDGE: &str = "Wedge\n\n\n  0  0  0     0  0            999 V3000\nM  V30 BEGIN CTAB\nM  V30 COUNTS 4 3 0 0 0\nM  V30 BEGIN ATOM\nM  V30 1 C 0.0000 0.0000 0.0000 0\nM  V30 2 F 0.0000 1.5000 0.0000 0\nM  V30 3 Cl 1.3000 -0.7500 0.0000 0\nM  V30 4 Br -1.3000 -0.7500 0.0000 0\nM  V30 END ATOM\nM  V30 BEGIN BOND\nM  V30 1 1 1 2\nM  V30 2 1 1 3 CFG=1\nM  V30 3 1 1 4\nM  V30 END BOND\nM  V30 END CTAB\nM  END\n";

fn setup() -> (App, TempDir, String) {
    let directory = tempfile::tempdir().unwrap();
    let app = Application::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    )
    .with_ctfile_codec(V2000Codec);
    let created = call(&app, "create", "document.create", json!({"title": "T"}));
    let id = created["result"]["document"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    (app, directory, id)
}

fn call(app: &App, request: &str, operation: &str, parameters: Value) -> Value {
    let body = json!({"contract": 1, "request_id": request, "profile": "preserve-semantics",
                      "operation": operation, "parameters": parameters});
    let reply = serde_json::to_value(app.execute(body.to_string().as_bytes())).unwrap();
    let losses = reply["loss_report"].as_array().unwrap();
    match reply["outcome"].as_str().unwrap() {
        "ok" => assert!(
            losses
                .iter()
                .all(|l| matches!(l["disposition"].as_str(), Some("retained" | "normalized"))),
            "an ok reply carries a lossy entry: {reply}"
        ),
        "partial" => assert!(!losses.is_empty(), "a partial reply with no loss: {reply}"),
        _ => {}
    }
    reply
}

fn features(reply: &Value) -> Vec<String> {
    reply["loss_report"]
        .as_array()
        .unwrap()
        .iter()
        .map(|loss| loss["feature"].as_str().unwrap().to_owned())
        .collect()
}

/// Imports `source` into `id` (preview, then commit with the digest and acceptance).
fn import(app: &App, id: &str, revision: u64, source: &str) -> Value {
    let preview = call(
        app,
        "preview",
        "document.import",
        json!({"document_id": id, "expected_revision": revision, "format": "mol",
               "source": source, "record": 1, "preview": true}),
    );
    assert_eq!(preview["outcome"], "partial", "{preview}");
    let digest = preview["result"]["preview_digest"].clone();
    call(
        app,
        &format!("import-{revision}"),
        "document.import",
        json!({"document_id": id, "expected_revision": revision, "format": "mol",
               "source": source, "record": 1, "preview_digest": digest, "accept_loss": true}),
    )
}

fn snapshot(app: &App, id: &str) -> Value {
    call(app, "snap", "document.snapshot", json!({"document_id": id}))["result"]["document"].clone()
}

fn export(app: &App, id: &str, extra: Value) -> Value {
    let mut parameters = json!({"format": "mol", "records": [{"document_id": id}]});
    for (key, value) in extra.as_object().unwrap() {
        parameters[key] = value.clone();
    }
    call(app, "export", "document.export", parameters)
}

/// The reference drawing of `WEDGE`, built by explicit commands: the same graph and marks with
/// a configuration stated by hand.
fn reference(app: &App) -> String {
    let created = call(
        app,
        "ref-create",
        "document.create",
        json!({"title": "Ref"}),
    );
    let id = created["result"]["document"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let at = |x: f64, y: f64, element: &str, to: Option<&str>| {
        let mut c = json!({"op": "add_atom", "element": element, "position": {"x": x, "y": y}});
        if let Some(to) = to {
            c["bond_to"] = json!({"atom": to, "order": "single"});
        }
        c
    };
    let reply = call(
        app,
        "ref-edit",
        "document.transaction",
        json!({"document_id": id, "expected_revision": 0, "commands": [
            at(100.0, 100.0, "C", None),
            at(100.0, 70.0, "F", Some("$0")),
            at(126.0, 115.0, "Cl", Some("$0")),
            at(74.0, 115.0, "Br", Some("$0")),
            {"op": "set_bond_mark", "bond": "$2", "mark": {"kind": "wedge", "narrow": "begin"}},
            {"op": "set_atom_configuration", "atom": "$0", "configuration": {
                "kind": "tetrahedral",
                "neighbors": ["$3", "$1", "$2", "implicit_hydrogen"], "turn": "clockwise"}}]}),
    );
    assert_eq!(reply["outcome"], "ok", "{reply}");
    id
}

#[test]
fn a_v3000_wedge_imports_as_a_mark_with_the_configuration_it_shows() {
    let (app, _directory, id) = setup();
    let reference_id = reference(&app);
    let reference_stereo = snapshot(&app, &reference_id)["stereo"].clone();
    let imported = import(&app, &id, 0, WEDGE);
    assert_eq!(imported["outcome"], "partial", "{imported}");
    let document = snapshot(&app, &id);
    assert_eq!(
        document["bonds"][1]["mark"],
        json!({"kind": "wedge", "narrow": "begin"}),
        "{document}"
    );
    assert_eq!(document["bonds"][0]["mark"], Value::Null);
    assert_eq!(
        document["stereo"]["atoms"], reference_stereo["atoms"],
        "the derived configuration is the hand-stated one"
    );
    assert_eq!(document["stereo"]["groups"], json!([]));
}

#[test]
fn a_center_the_marks_do_not_determine_keeps_its_mark_and_costs_a_loss_not_the_import() {
    // The wedge sits at a center whose three neighbours lie on one line.
    let degenerate = WEDGE
        .replace("0.0000 1.5000 0.0000", "1.0000 0.0000 0.0000")
        .replace("1.3000 -0.7500", "2.0000 0.0000")
        .replace("-1.3000 -0.7500", "-1.0000 0.0000");
    let (app, _directory, id) = setup();
    let imported = import(&app, &id, 0, &degenerate);
    assert_eq!(imported["outcome"], "partial", "{imported}");
    let unreadable: Vec<&Value> = imported["loss_report"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|loss| loss["feature"] == "stereo_unreadable")
        .collect();
    assert_eq!(unreadable.len(), 1, "{imported}");
    assert_eq!(unreadable[0]["source_path"], "records[1].atoms[1]");
    assert_eq!(unreadable[0]["semantic_impact"], "stereo");
    assert_eq!(unreadable[0]["disposition"], "dropped");
    let document = snapshot(&app, &id);
    assert_eq!(document["bonds"][1]["mark"]["kind"], "wedge");
    assert_eq!(document["stereo"]["atoms"], json!([]));
}

#[test]
fn a_hash_either_and_crossed_mark_import_and_export_with_the_narrow_end_first() {
    let source = "Marks\n\n\n  0  0  0     0  0            999 V3000\nM  V30 BEGIN CTAB\nM  V30 COUNTS 5 4 0 0 0\nM  V30 BEGIN ATOM\nM  V30 1 C 0.0000 0.0000 0.0000 0\nM  V30 2 C 1.5000 0.0000 0.0000 0\nM  V30 3 C 3.0000 0.0000 0.0000 0\nM  V30 4 C 3.0000 1.5000 0.0000 0\nM  V30 5 C 4.5000 0.0000 0.0000 0\nM  V30 END ATOM\nM  V30 BEGIN BOND\nM  V30 1 1 1 2 CFG=3\nM  V30 2 1 3 2 CFG=2\nM  V30 3 2 3 4 CFG=2\nM  V30 4 1 3 5\nM  V30 END BOND\nM  V30 END CTAB\nM  END\n";
    let (app, _directory, id) = setup();
    let imported = import(&app, &id, 0, source);
    assert_eq!(imported["outcome"], "partial", "{imported}");
    let document = snapshot(&app, &id);
    assert_eq!(
        document["bonds"][0]["mark"],
        json!({"kind": "hash", "narrow": "begin"})
    );
    assert_eq!(document["bonds"][1]["mark"], json!({"kind": "wavy"}));
    assert_eq!(document["bonds"][2]["mark"], json!({"kind": "crossed"}));
    // A crossed mark states the arrangement, so no coordinate_stereo loss is left for it.
    assert!(
        !features(&imported).contains(&"coordinate_stereo".to_owned()),
        "{imported}"
    );
    let written = export(&app, &id, json!({"ctab": "v3000"}));
    let text = written["result"]["content"].as_str().unwrap();
    assert!(text.contains("M  V30 1 1 1 2 CFG=3\n"), "{text}");
    assert!(text.contains("M  V30 2 1 3 2 CFG=2\n"), "{text}");
    assert!(text.contains("M  V30 3 2 3 4 CFG=2\n"), "{text}");
    assert!(text.contains("M  V30 4 1 3 5\n"), "{text}");
}

#[test]
fn a_narrow_end_at_the_bonds_end_is_written_with_the_endpoints_swapped() {
    let (app, _directory, id) = setup();
    let edit = call(
        &app,
        "edit",
        "document.transaction",
        json!({"document_id": id, "expected_revision": 0, "commands": [
            {"op": "add_atom", "element": "C", "position": {"x": 0, "y": 0}},
            {"op": "add_atom", "element": "O", "position": {"x": 30, "y": 0},
             "bond_to": {"atom": "$0", "order": "single"}},
            {"op": "set_bond_mark", "bond": "$1", "mark": {"kind": "wedge", "narrow": "end"}}]}),
    );
    assert_eq!(edit["outcome"], "ok", "{edit}");
    let v3000 = export(&app, &id, json!({"ctab": "v3000"}));
    let text = v3000["result"]["content"].as_str().unwrap();
    assert!(text.contains("M  V30 1 1 2 1 CFG=1\n"), "{text}");
    let v2000 = export(&app, &id, json!({}));
    let text = v2000["result"]["content"].as_str().unwrap();
    assert!(text.contains("\n  2  1  1  1  0  0  0\n"), "{text}");
    // Read back, the bond has the narrow end first, which is the begin atom of the new bond.
    let (second, _other, target) = setup();
    let content = v3000["result"]["content"].as_str().unwrap();
    import(&second, &target, 0, content);
    let document = snapshot(&second, &target);
    assert_eq!(document["bonds"][0]["begin"], "a2");
    assert_eq!(
        document["bonds"][0]["mark"],
        json!({"kind": "wedge", "narrow": "begin"})
    );
}

#[test]
fn stereo_collections_and_the_chiral_flag_are_not_dropped_silently() {
    let (app, _directory, id) = setup();
    let collection = WEDGE.replace(
        "M  V30 END CTAB",
        "M  V30 BEGIN COLLECTION\nM  V30 MDLV30/STEABS ATOMS=(1 1)\nM  V30 END COLLECTION\nM  V30 END CTAB",
    );
    let refused = call(
        &app,
        "preview",
        "document.import",
        json!({"document_id": id, "expected_revision": 0, "format": "mol",
               "source": collection, "record": 1, "preview": true}),
    );
    assert_eq!(refused["outcome"], "error", "{refused}");
    assert_eq!(
        refused["diagnostics"][0]["code"],
        "v3000_collection_not_supported_yet"
    );
    assert_eq!(snapshot(&app, &id)["revision"], 0);

    let chiral = WEDGE.replace("COUNTS 4 3 0 0 0", "COUNTS 4 3 0 0 1");
    let imported = import(&app, &id, 0, &chiral);
    let flag: Vec<&Value> = imported["loss_report"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|loss| loss["feature"] == "chiral_flag")
        .collect();
    assert_eq!(flag.len(), 1, "{imported}");
    assert_eq!(flag[0]["disposition"], "dropped");
    assert_eq!(flag[0]["semantic_impact"], "stereo");
}

#[test]
fn a_refused_v3000_file_leaves_the_document_unchanged() {
    let (app, _directory, id) = setup();
    for (name, source) in [
        ("dative bond type", WEDGE.replace("1 1 1 2\n", "1 9 1 2\n")),
        (
            "wedge on an aromatic bond",
            WEDGE.replace("2 1 1 3 CFG=1", "2 4 1 3 CFG=1"),
        ),
        ("unknown atom", WEDGE.replace("3 1 1 4", "3 1 1 9")),
    ] {
        let refused = call(
            &app,
            name,
            "document.import",
            json!({"document_id": id, "expected_revision": 0, "format": "mol",
                   "source": source, "record": 1, "preview": true}),
        );
        assert_eq!(refused["outcome"], "error", "{name}: {refused}");
    }
    let document = snapshot(&app, &id);
    assert_eq!(document["revision"], 0);
    assert_eq!(document["atoms"], json!([]));
}

#[test]
fn ctab_omitted_is_v2000_and_auto_chooses_v3000_only_when_it_is_needed() {
    let (app, _directory, id) = setup();
    import(&app, &id, 0, WEDGE);
    let omitted = export(&app, &id, json!({}));
    let v2000 = export(&app, &id, json!({"ctab": "v2000"}));
    assert_eq!(omitted["result"]["content"], v2000["result"]["content"]);
    assert_eq!(omitted["result"]["ctab"], "v2000");
    let auto = export(&app, &id, json!({"ctab": "auto"}));
    assert_eq!(auto["result"]["ctab"], "v2000", "{auto}");
    assert_eq!(auto["result"]["content"], omitted["result"]["content"]);
    assert!(!features(&auto).is_empty(), "auto always lists its losses");

    let (benzene, _other, ring) = setup();
    let edit = call(
        &benzene,
        "ring",
        "document.transaction",
        json!({"document_id": ring, "expected_revision": 0, "commands": [
            {"op": "add_atom", "element": "C", "position": {"x": 0, "y": 0}},
            {"op": "add_atom", "element": "C", "position": {"x": 30, "y": 0},
             "bond_to": {"atom": "$0", "order": "aromatic"}}]}),
    );
    assert_eq!(edit["outcome"], "ok", "{edit}");
    let refused = export(&benzene, &ring, json!({}));
    assert_eq!(refused["diagnostics"][0]["code"], "unsupported_bond");
    assert!(
        refused["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("V3000"),
        "{refused}"
    );
    let auto = export(&benzene, &ring, json!({"ctab": "auto"}));
    assert_eq!(auto["result"]["ctab"], "v3000", "{auto}");
    assert!(
        auto["result"]["content"]
            .as_str()
            .unwrap()
            .contains("M  V30 1 4 1 2\n")
    );
    let bad = export(&benzene, &ring, json!({"ctab": "v4000"}));
    assert_eq!(bad["outcome"], "error", "{bad}");
}

#[test]
fn more_than_999_atoms_need_v3000_and_a_v2000_request_says_so() {
    let (app, _directory, id) = setup();
    for (revision, start) in [(0, 0), (1, 400), (2, 800)] {
        let commands: Vec<Value> = (start..start + 400_i32)
            .map(|n| json!({"op": "add_atom", "element": "C", "position": {"x": n, "y": 0}}))
            .collect();
        let edit = call(
            &app,
            &format!("big-{revision}"),
            "document.transaction",
            json!({"document_id": id, "expected_revision": revision, "commands": commands}),
        );
        assert_eq!(edit["outcome"], "ok", "{edit}");
    }
    let refused = export(&app, &id, json!({"ctab": "v2000"}));
    assert_eq!(
        refused["diagnostics"][0]["code"], "resource_limit",
        "{refused}"
    );
    assert!(
        refused["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("V3000")
    );
    let auto = export(&app, &id, json!({"ctab": "auto"}));
    assert_eq!(auto["result"]["ctab"], "v3000", "{auto}");
}

/// A document whose center states a configuration its marks do not show.
fn mismatched(app: &App) -> String {
    let id = reference(app);
    let reply = call(
        app,
        "flip",
        "document.transaction",
        json!({"document_id": id, "expected_revision": 1, "commands": [
            {"op": "set_atom_configuration", "atom": "a1", "configuration": {
                "kind": "tetrahedral",
                "neighbors": ["a4", "a2", "a3", "implicit_hydrogen"], "turn": "anticlockwise"}}]}),
    );
    assert_eq!(reply["outcome"], "ok", "{reply}");
    id
}

#[test]
fn a_configuration_the_marks_do_not_show_needs_consent_in_either_dialect() {
    let (app, _directory, _unused) = setup();
    let id = mismatched(&app);
    let revision = snapshot(&app, &id)["revision"].clone();
    for ctab in ["v2000", "v3000"] {
        let refused = export(&app, &id, json!({"ctab": ctab}));
        assert_eq!(refused["outcome"], "error", "{refused}");
        let diagnostic = &refused["diagnostics"][0];
        assert_eq!(diagnostic["code"], "loss_consent_required");
        assert_eq!(
            diagnostic["details"]["features"],
            json!(["stereo_configuration_not_drawn"])
        );
        let loss = &diagnostic["details"]["losses"][0];
        assert_eq!(loss["source_path"], "records[1].atoms[a1]");
        assert_eq!(loss["disposition"], "dropped");
        assert_eq!(loss["semantic_impact"], "stereo");
        // A digest for other text is not accepted.
        let wrong = export(
            &app,
            &id,
            json!({"ctab": ctab, "accepted_losses": "0".repeat(64)}),
        );
        assert_eq!(wrong["diagnostics"][0]["code"], "loss_consent_changed");
        // The right digest writes the file, partial, with the marks.
        let digest = diagnostic["details"]["loss_digest"].clone();
        let accepted = export(&app, &id, json!({"ctab": ctab, "accepted_losses": digest}));
        assert_eq!(accepted["outcome"], "partial", "{accepted}");
        assert!(features(&accepted).contains(&"chiral_flag".to_owned()));
        // The digest of one dialect does not accept the other.
        let other = if ctab == "v2000" { "v3000" } else { "v2000" };
        let crossed = export(&app, &id, json!({"ctab": other, "accepted_losses": digest}));
        assert_eq!(crossed["diagnostics"][0]["code"], "loss_consent_changed");
    }
    assert_eq!(
        snapshot(&app, &id)["revision"],
        revision,
        "export is a read"
    );
}

#[test]
fn a_v3000_export_imports_back_to_the_same_marks_and_configuration() {
    let (app, _directory, id) = setup();
    import(&app, &id, 0, WEDGE);
    let written = export(&app, &id, json!({"ctab": "v3000"}));
    let text = written["result"]["content"].as_str().unwrap().to_owned();
    let (second, _other, target) = setup();
    import(&second, &target, 0, &text);
    let (first, again) = (snapshot(&app, &id), snapshot(&second, &target));
    assert_eq!(first["bonds"], again["bonds"]);
    assert_eq!(first["stereo"], again["stereo"]);
    // The writer is deterministic.
    let twice = export(&app, &id, json!({"ctab": "v3000"}));
    assert_eq!(twice["result"]["content"], written["result"]["content"]);
}
