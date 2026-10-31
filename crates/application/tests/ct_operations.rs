//! V2000 operations use the same application, storage and transaction boundaries as other edits.
use chemifree_application::Application;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities, V2000Codec};
use serde_json::{Value, json};
use tempfile::TempDir;

type App = Application<LocalCapabilities, FileDocumentRepository>;

const MOL: &str = "Ammonium\n\n\n  2  1  0  0  0  0  0  0  0  0999 V2000\n    0.0000    1.0000    0.0000 N   0  0  0  0  0  0  0  0  0  0  0  0\n    1.0000    0.0000    0.0000 H   0  0  0  0  0  0  0  0  0  0  0  0\n  1  2  1  0  0  0  0\nM  CHG  1   1   1\nM  ISO  1   2   2\nM  END\n";

fn app(path: &std::path::Path) -> App {
    Application::new(LocalCapabilities, FileDocumentRepository::new(path))
        .with_ctfile_codec(V2000Codec)
}
fn setup() -> (App, TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    let application = app(dir.path());
    let result = call(
        &application,
        "create",
        "document.create",
        json!({"title":"Target"}),
    );
    let id = result["result"]["document"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    (application, dir, id)
}
fn call(app: &App, id: &str, operation: &str, parameters: Value) -> Value {
    let request = json!({"contract":1,"request_id":id,"profile":"preserve-semantics","operation":operation,"parameters":parameters});
    serde_json::to_value(app.execute(request.to_string().as_bytes())).unwrap()
}
fn snapshot(app: &App, id: &str) -> Value {
    call(
        app,
        "snapshot",
        "document.snapshot",
        json!({"document_id":id}),
    )["result"]["document"]
        .clone()
}
fn preview(
    app: &App,
    document: &str,
    source: &str,
    format: &str,
    record: usize,
    revision: u64,
) -> Value {
    call(
        app,
        "preview",
        "document.import",
        json!({"document_id":document,"expected_revision":revision,"format":format,"source":source,"record":record,"preview":true}),
    )
}

#[test]
fn preview_consent_commit_replay_undo_redo_and_persistence_share_one_graph() {
    let (application, directory, id) = setup();
    let before = snapshot(&application, &id);
    let report = preview(&application, &id, MOL, "mol", 1, 0);
    assert_eq!(report["outcome"], "partial");
    assert_eq!(report["document_revision"], 0);
    assert_eq!(
        snapshot(&application, &id),
        before,
        "preview must not mutate the document"
    );
    assert!(
        report["loss_report"]
            .as_array()
            .unwrap()
            .iter()
            .any(|loss| loss["feature"] == "isotope_validation")
    );
    let digest = report["result"]["preview_digest"].as_str().unwrap();
    let parameters = json!({"document_id":id,"expected_revision":0,"format":"mol","source":MOL,"record":1,"preview_digest":digest,"accept_loss":true});
    let result = call(
        &application,
        "import-1",
        "document.import",
        parameters.clone(),
    );
    assert_eq!(result["outcome"], "partial");
    assert_eq!(result["document_revision"], 1);
    let doc = &result["result"]["document"];
    assert_eq!(
        (
            doc["atoms"][0]["element"].as_str(),
            doc["atoms"][0]["charge"].as_i64()
        ),
        (Some("N"), Some(1))
    );
    assert_eq!(
        (
            doc["atoms"][1]["element"].as_str(),
            doc["atoms"][1]["isotope"].as_u64()
        ),
        (Some("H"), Some(2))
    );
    assert_eq!(
        (
            doc["atoms"][0]["position"]["x"].as_f64(),
            doc["atoms"][0]["position"]["y"].as_f64()
        ),
        (Some(0.), Some(-20.))
    );
    assert_eq!(
        result["result"]["import_mapping"],
        json!({"atoms":["a1","a2"],"bonds":["b1"]})
    );
    assert_eq!(
        call(
            &application,
            "import-1",
            "document.import",
            parameters.clone()
        ),
        result
    );
    let mut reused = parameters;
    reused["source"] = json!(MOL.replace("Ammonium", "Another"));
    assert_eq!(
        call(&application, "import-1", "document.import", reused)["diagnostics"][0]["code"],
        "request_id_reused"
    );
    assert_eq!(
        call(
            &application,
            "undo",
            "history.undo",
            json!({"document_id":id,"expected_revision":1})
        )["result"]["document"]["atoms"],
        json!([])
    );
    assert_eq!(
        call(
            &application,
            "redo",
            "history.redo",
            json!({"document_id":id,"expected_revision":2})
        )["result"]["document"]["atoms"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        call(
            &application,
            "save",
            "document.save",
            json!({"document_id":id,"expected_revision":3})
        )["outcome"],
        "ok"
    );
    drop(application);
    let reopened = app(directory.path());
    let stored = snapshot(&reopened, &id);
    assert_eq!(stored["revision"], 3);
    assert_eq!(stored["atoms"][1]["isotope"], 2);
}

#[test]
fn later_bad_record_and_changed_preview_cannot_modify_the_document() {
    let (application, _directory, id) = setup();
    let before = snapshot(&application, &id);
    let malformed = format!(
        "{MOL}$$$$\n{}$$$$\n",
        MOL.replace("M  END", "m  unsupported  1\nM  END")
    );
    let error = preview(&application, &id, &malformed, "sdf", 1, 0);
    assert_eq!(
        (
            error["diagnostics"][0]["code"].as_str(),
            error["diagnostics"][0]["details"]["record"].as_u64()
        ),
        (Some("unsupported_directive"), Some(2))
    );
    assert_eq!(snapshot(&application, &id), before);
    let result = preview(&application, &id, MOL, "mol", 1, 0);
    let digest = result["result"]["preview_digest"].as_str().unwrap();
    let mut commit = json!({"document_id":id,"expected_revision":0,"format":"mol","source":MOL,"record":1,"preview_digest":digest,"accept_loss":false});
    assert_eq!(
        call(
            &application,
            "without-consent",
            "document.import",
            commit.clone()
        )["diagnostics"][0]["code"],
        "loss_consent_required"
    );
    assert_eq!(snapshot(&application, &id), before);
    commit["accept_loss"] = json!(true);
    commit["source"] = json!(MOL.replace("Ammonium", "Different"));
    assert_eq!(
        call(&application, "changed", "document.import", commit)["diagnostics"][0]["code"],
        "preview_changed"
    );
    assert_eq!(snapshot(&application, &id), before);
}

#[test]
fn selected_and_unselected_record_losses_and_indexes_are_visible() {
    let (application, _directory, id) = setup();
    let sdf = format!(
        "{MOL}> <Label>\nFirst\n\n$$$$\n{}> <Label>\nSecond\n\n$$$$\n",
        MOL.replace("Ammonium", "Second")
    );
    let report = preview(&application, &id, &sdf, "sdf", 2, 0);
    assert_eq!(report["result"]["records"].as_array().unwrap().len(), 2);
    let losses = report["loss_report"].as_array().unwrap();
    assert!(
        losses
            .iter()
            .any(|loss| loss["feature"] == "unselected_record"
                && loss["source_path"] == "records[1]")
    );
    assert!(
        losses
            .iter()
            .any(|loss| loss["feature"] == "sd_field"
                && loss["source_path"] == "records[2].fields[1]")
    );
    assert_eq!(
        report["result"]["records"][1]["fields"][0]["value"],
        "Second"
    );
}

#[test]
fn later_export_failures_report_the_record_and_return_no_content() {
    let (application, _directory, id) = setup();
    let absent = call(
        &application,
        "missing",
        "document.export",
        json!({"format":"sdf","records":[{"document_id":id},{"document_id":"absent"}]}),
    );
    assert_eq!(
        (
            absent["diagnostics"][0]["code"].as_str(),
            absent["diagnostics"][0]["details"]["record"].as_u64()
        ),
        (Some("document_not_found"), Some(2))
    );
    assert!(absent["result"].is_null());
    let invalid = call(
        &application,
        "field",
        "document.export",
        json!({"format":"sdf","records":[{"document_id":id},{"document_id":id,"fields":[{"name":"Long","value":"가".repeat(201)}]}]}),
    );
    assert_eq!(
        (
            invalid["diagnostics"][0]["code"].as_str(),
            invalid["diagnostics"][0]["details"]["record"].as_u64()
        ),
        (Some("data_line_too_long"), Some(2))
    );
    assert!(invalid["result"].is_null());
    let created = call(
        &application,
        "create-query",
        "document.create",
        json!({"title":"Query"}),
    );
    let query_id = created["result"]["document"]["id"].as_str().unwrap();
    let result = call(
        &application,
        "query",
        "document.transaction",
        json!({"document_id":query_id,"expected_revision":0,"commands":[{"op":"add_atom","query":{"kind":"any"},"position":{"x":0,"y":0}}]}),
    );
    assert_eq!(result["outcome"], "ok");
    let query = call(
        &application,
        "unsupported",
        "document.export",
        json!({"format":"sdf","records":[{"document_id":id},{"document_id":query_id}]}),
    );
    assert_eq!(
        (
            query["diagnostics"][0]["code"].as_str(),
            query["diagnostics"][0]["details"]["record"].as_u64()
        ),
        (Some("unsupported_atom"), Some(2))
    );
    assert!(query["result"].is_null());
}

#[test]
fn cumulative_export_limit_points_to_the_record_that_crosses_it() {
    let (application, _directory, id) = setup();
    let value = vec!["x".repeat(200); 21_000].join("\n");
    let result = call(
        &application,
        "limit",
        "document.export",
        json!({
            "format":"sdf", "records":[{"document_id":id}, {"document_id":id,"fields":[{"name":"Big","value":value}]}]
        }),
    );
    assert_eq!(
        (
            result["diagnostics"][0]["code"].as_str(),
            result["diagnostics"][0]["details"]["record"].as_u64()
        ),
        (Some("resource_limit"), Some(2))
    );
    assert!(result["result"].is_null());
    assert_eq!(snapshot(&application, &id)["revision"], 0);
}

#[test]
fn preview_and_export_are_bound_to_graph_when_restart_reuses_revision() {
    let (application, directory, id) = setup();
    let edit = |element: &str| json!({"document_id":id,"expected_revision":0,"commands":[{"op":"add_atom","element":element,"position":{"x":0,"y":0}}]});
    assert_eq!(
        call(
            &application,
            "edit-carbon",
            "document.transaction",
            edit("C")
        )["document_revision"],
        1
    );
    let previewed = preview(&application, &id, MOL, "mol", 1, 1);
    let digest = previewed["result"]["preview_digest"]
        .as_str()
        .unwrap()
        .to_owned();
    let old = snapshot(&application, &id);
    drop(application);

    let restarted = app(directory.path());
    assert_eq!(
        call(&restarted, "edit-oxygen", "document.transaction", edit("O"))["document_revision"],
        1
    );
    let current = snapshot(&restarted, &id);
    assert_eq!(old["revision"], current["revision"]);
    assert_eq!(
        (
            old["atoms"][0]["element"].as_str(),
            current["atoms"][0]["element"].as_str()
        ),
        (Some("C"), Some("O"))
    );
    let commit = call(
        &restarted,
        "stale-import",
        "document.import",
        json!({"document_id":id,"expected_revision":1,"format":"mol","source":MOL,"record":1,"preview_digest":digest,"accept_loss":true}),
    );
    assert_eq!(commit["diagnostics"][0]["code"], "preview_changed");
    assert_eq!(snapshot(&restarted, &id), current);
    let export = call(
        &restarted,
        "export-current",
        "document.export",
        json!({"format":"mol","records":[{"document_id":id}]}),
    );
    assert_eq!(
        export["result"]["records"][0]["snapshot"]["document"],
        current
    );
}

#[test]
fn an_empty_mol_is_a_replayable_no_op_without_revision_or_history_change() {
    let (application, _directory, id) = setup();
    let empty = "Empty\n\n\n  0  0  0  0  0  0  0  0  0  0999 V2000\nM  END\n";
    let before = snapshot(&application, &id);
    let inspected = preview(&application, &id, empty, "mol", 1, 0);
    assert_eq!(inspected["outcome"], "partial");
    assert_eq!(inspected["result"]["snapshot"]["document"], before);
    let digest = inspected["result"]["preview_digest"].as_str().unwrap();
    let request = json!({"document_id":id,"expected_revision":0,"format":"mol","source":empty,"record":1,"preview_digest":digest,"accept_loss":true});
    let first = call(&application, "empty", "document.import", request.clone());
    assert_eq!(first["outcome"], "partial");
    assert_eq!(first["document_revision"], 0);
    assert_eq!(first["result"]["session"]["undo_depth"], 0);
    assert_eq!(
        first["result"]["import_mapping"],
        json!({"atoms":[],"bonds":[]})
    );
    assert_eq!(snapshot(&application, &id), before);
    assert_eq!(
        call(&application, "empty", "document.import", request),
        first
    );
}

#[test]
fn a_new_service_rejects_even_a_matching_graph_preview_from_the_previous_service() {
    let (application, directory, id) = setup();
    let result = preview(&application, &id, MOL, "mol", 1, 0);
    let digest = result["result"]["preview_digest"]
        .as_str()
        .unwrap()
        .to_owned();
    let original = snapshot(&application, &id);
    drop(application);
    let restarted = app(directory.path());
    assert_eq!(snapshot(&restarted, &id), original);
    let commit = call(
        &restarted,
        "old-preview",
        "document.import",
        json!({"document_id":id,"expected_revision":0,"format":"mol","source":MOL,"record":1,"preview_digest":digest,"accept_loss":true}),
    );
    assert_eq!(commit["diagnostics"][0]["code"], "preview_changed");
    assert_eq!(snapshot(&restarted, &id), original);
}

#[test]
fn selected_record_that_exceeds_editor_transaction_limit_does_not_change_target() {
    let (application, _directory, id) = setup();
    let mut large = String::from("Many\n\n\n501  0  0  0  0  0  0  0  0  0999 V2000\n");
    for _ in 0..501 {
        large.push_str("    0.0000    0.0000    0.0000 C   0  0  0  0  0  0  0  0  0  0  0  0\n");
    }
    large.push_str("M  END\n$$$$\n");
    let source = format!("{MOL}$$$$\n{large}");
    let before = snapshot(&application, &id);
    let result = preview(&application, &id, &source, "sdf", 2, 0);
    assert_eq!(result["outcome"], "error");
    assert_eq!(result["diagnostics"][0]["details"]["record"], 2);
    assert!(result["result"].is_null());
    assert_eq!(snapshot(&application, &id), before);
}
