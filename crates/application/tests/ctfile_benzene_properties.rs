use chemifree_application::Application;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities, V2000Codec};
use serde_json::{Value, json};

type App = Application<LocalCapabilities, FileDocumentRepository>;
fn call(app: &App, id: &str, operation: &str, parameters: Value) -> Value {
    let request = json!({"contract":1,"request_id":id,"profile":"preserve-semantics","operation":operation,"parameters":parameters});
    serde_json::to_value(app.execute(request.to_string().as_bytes())).unwrap()
}

#[test]
fn independently_authored_kekule_benzene_mol_import_has_six_hydrogens() {
    // Six carbon vertices with alternating double bonds encode benzene, C6H6.
    let mut source = String::from("Benzene\n\n\n  6  6  0  0  0  0  0  0  0  0999 V2000\n");
    for _ in 0..6 {
        source.push_str("    0.0000    0.0000    0.0000 C   0  0  0  0  0  0  0  0  0  0  0  0\n");
    }
    source.push_str("  1  2  2  0  0  0  0\n  2  3  1  0  0  0  0\n  3  4  2  0  0  0  0\n  4  5  1  0  0  0  0\n  5  6  2  0  0  0  0\n  6  1  1  0  0  0  0\nM  END\n");
    let directory = tempfile::tempdir().unwrap();
    let app = App::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    )
    .with_ctfile_codec(V2000Codec);
    let id = app
        .create_document(None, Some("Benzene"))
        .unwrap()
        .document
        .id;
    let preview = call(
        &app,
        "preview",
        "document.import",
        json!({"document_id":id,"expected_revision":0,"format":"mol","source":source,"record":1,"preview":true}),
    );
    assert_eq!(preview["outcome"], "partial");
    let imported = call(
        &app,
        "import",
        "document.import",
        json!({"document_id":id,"expected_revision":0,"format":"mol","source":source,"record":1,"preview_digest":preview["result"]["preview_digest"],"accept_loss":true}),
    );
    assert_eq!(imported["document_revision"], 1);
    let before = serde_json::to_value(app.document(&id).unwrap()).unwrap();
    let valence = call(&app, "valence", "chem.validate", json!({"document_id":id}));
    let atoms = valence["result"]["atoms"].as_array().unwrap();
    assert_eq!(atoms.len(), 6);
    assert!(atoms.iter().all(|atom| atom["status"] == "valid"
        && atom["bond_order_sum"] == 3
        && atom["hydrogens"] == 1));
    let formula = call(
        &app,
        "formula",
        "chem.properties",
        json!({"document_id":id,"scope":"molecule","property":"formula"}),
    );
    assert_eq!(formula["result"]["components"][0]["formula"], "C6H6");
    assert_eq!(
        serde_json::to_value(app.document(&id).unwrap()).unwrap(),
        before
    );
}
