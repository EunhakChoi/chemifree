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
        ("F/C=C/F", "unsupported_feature"),
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
