use chemifree_application::{Application, ListQuery, MAX_OPEN_SESSIONS};
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};

type App = Application<LocalCapabilities, FileDocumentRepository>;

fn call(app: &App, id: &str, operation: &str, parameters: Value) -> Value {
    let request = json!({"contract":1,"request_id":id,"profile":"preserve-semantics","operation":operation,"parameters":parameters});
    serde_json::to_value(app.execute(request.to_string().as_bytes())).unwrap()
}

fn draw(app: &App, title: &str, atoms: &[&str], bonds: &[(usize, usize, &str)]) -> String {
    let id = app.create_document(None, Some(title)).unwrap().document.id;
    let mut commands: Vec<_> = atoms.iter().enumerate().map(|(i, symbol)| {
        json!({"op":"add_atom", "element":symbol, "position":{"x":i * 20, "y":0}})
    }).collect();
    commands.extend(bonds.iter().map(|(a, b, order)| {
        json!({"op":"add_bond", "begin":format!("${a}"), "end":format!("${b}"), "order":order})
    }));
    let result = call(
        app,
        "draw",
        "document.transaction",
        json!({"document_id":id,"expected_revision":0,"commands":commands}),
    );
    assert_eq!(result["outcome"], "ok", "{result}");
    id
}

#[test]
fn two_snapshots_compare_equivalent_encodings_without_editing_either_document() {
    let directory = tempfile::tempdir().unwrap();
    let app = App::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    );
    let aromatic: Vec<_> = (0..6).map(|i| (i, (i + 1) % 6, "aromatic")).collect();
    let kekule: Vec<_> = (0..6)
        .map(|i| (i, (i + 1) % 6, if i % 2 == 0 { "double" } else { "single" }))
        .collect();
    let source = draw(&app, "Aromatic benzene", &["C"; 6], &aromatic);
    let target = draw(&app, "Kekule benzene", &["C"; 6], &kekule);
    let before_source = serde_json::to_value(app.document(&source).unwrap()).unwrap();
    let before_target = serde_json::to_value(app.document(&target).unwrap()).unwrap();

    let compared = call(
        &app,
        "compare",
        "chem.identity",
        json!({"document_id":source,"other_document_id":target,"mode":"chemical"}),
    );
    assert_eq!(compared["outcome"], "ok", "{compared}");
    assert_eq!(compared["model_id"], "chemifree-chemical-graph-1");
    assert_eq!(compared["result"]["comparison"]["equivalent"], true);
    assert_eq!(
        compared["result"]["identity"]["numbering"]
            .as_array()
            .unwrap()
            .len(),
        6
    );
    assert_eq!(
        compared["result"]["identity"]["symmetry"][0]
            .as_array()
            .unwrap()
            .len(),
        6
    );
    assert_eq!(
        compared["result"]["snapshot"]["document"],
        before_source["document"]
    );
    assert_eq!(
        compared["result"]["comparison"]["snapshot"]["document"],
        before_target["document"]
    );
    assert_eq!(
        serde_json::to_value(app.document(&source).unwrap()).unwrap(),
        before_source
    );
    assert_eq!(
        serde_json::to_value(app.document(&target).unwrap()).unwrap(),
        before_target
    );

    let recorded = call(
        &app,
        "recorded",
        "chem.identity",
        json!({"document_id":source,"other_document_id":target,"mode":"recorded"}),
    );
    assert_eq!(recorded["result"]["comparison"]["equivalent"], false);
    let same = call(
        &app,
        "same",
        "chem.identity",
        json!({"document_id":source,"other_document_id":source,"mode":"recorded"}),
    );
    assert_eq!(same["result"]["comparison"]["equivalent"], true);
    assert_eq!(
        same["result"]["snapshot"],
        same["result"]["comparison"]["snapshot"]
    );
}

#[test]
fn a_target_outside_the_model_returns_no_partial_identity_or_document_change() {
    let directory = tempfile::tempdir().unwrap();
    let app = App::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    );
    let source = draw(&app, "Carbon", &["C"], &[]);
    let target = draw(&app, "Iron", &["Fe"], &[]);
    let before_source = serde_json::to_value(app.document(&source).unwrap()).unwrap();
    let before_target = serde_json::to_value(app.document(&target).unwrap()).unwrap();
    let failed = call(
        &app,
        "unsupported",
        "chem.identity",
        json!({"document_id":source,"other_document_id":target,"mode":"chemical"}),
    );
    assert_eq!(failed["outcome"], "unsupported");
    assert!(failed["result"].is_null());
    assert_eq!(failed["diagnostics"][0]["details"]["side"], "target");
    assert_eq!(
        failed["diagnostics"][0]["details"]["reason"],
        "element_or_charge"
    );
    assert_eq!(
        failed["diagnostics"][0]["details"]["snapshot"]["document"],
        before_target["document"]
    );
    assert_eq!(
        failed["diagnostics"][0]["details"]["snapshot"]["session"],
        before_target["session"]
    );
    assert_eq!(
        serde_json::to_value(app.document(&source).unwrap()).unwrap(),
        before_source
    );
    assert_eq!(
        serde_json::to_value(app.document(&target).unwrap()).unwrap(),
        before_target
    );
}

#[test]
fn bounded_identity_reports_indeterminate_instead_of_a_partial_canonical_code() {
    let directory = tempfile::tempdir().unwrap();
    let app = App::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    );
    let atoms = vec!["C"; 129];
    let source = draw(&app, "Above identity bound", &atoms, &[]);
    let before = serde_json::to_value(app.document(&source).unwrap()).unwrap();
    let result = call(
        &app,
        "limit",
        "chem.identity",
        json!({"document_id":source,"mode":"recorded"}),
    );
    assert_eq!(result["outcome"], "error");
    assert!(result["result"].is_null());
    assert_eq!(result["diagnostics"][0]["details"]["reason"], "atom_limit");
    assert_eq!(
        serde_json::to_value(app.document(&source).unwrap()).unwrap(),
        before
    );
}

#[test]
fn comparing_a_closed_document_does_not_evict_the_only_clean_source_session() {
    let directory = tempfile::tempdir().unwrap();
    let app = App::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    );
    let target = draw(&app, "Saved target", &["C"], &[]);
    app.save(&target, None, 1).unwrap();
    app.close(&target, None, false, None).unwrap();
    let source = app
        .create_document(None, Some("Clean source"))
        .unwrap()
        .document
        .id;
    for index in 1..MAX_OPEN_SESSIONS {
        draw(&app, &format!("Dirty {index}"), &["O"], &[]);
    }
    let status = |id: &str| {
        app.list_documents(&ListQuery::default())
            .unwrap()
            .documents
            .into_iter()
            .find(|entry| entry.id == id)
            .unwrap()
            .open
    };
    assert!(status(&source));
    assert!(!status(&target));
    let result = call(
        &app,
        "at-capacity",
        "chem.identity",
        json!({"document_id":source,"other_document_id":target,"mode":"recorded"}),
    );
    assert_eq!(result["outcome"], "ok", "{result}");
    assert_eq!(result["result"]["comparison"]["equivalent"], false);
    assert!(
        status(&source),
        "read-only comparison must not retire the source session"
    );
    assert!(
        !status(&target),
        "reading a stored target need not open a session"
    );
    let missing = call(
        &app,
        "missing-at-capacity",
        "chem.identity",
        json!({"document_id":target,"other_document_id":"doc-missing","mode":"recorded"}),
    );
    assert_eq!(missing["outcome"], "error");
    assert!(
        status(&source),
        "a refused comparison must not retire another session"
    );
    assert!(
        !status(&target),
        "a refused comparison must not open the source"
    );
}

#[test]
fn a_closed_target_with_an_abandoned_draft_compares_its_saved_graph_and_reports_recovery() {
    let directory = tempfile::tempdir().unwrap();
    let target = {
        let previous = App::new(
            LocalCapabilities,
            FileDocumentRepository::new(directory.path()),
        );
        let id = draw(&previous, "Draft target", &["C"], &[]);
        previous.autosave(&id, None).unwrap();
        id
    };
    let app = App::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    );
    let source = app
        .create_document(None, Some("Empty source"))
        .unwrap()
        .document
        .id;
    let result = call(
        &app,
        "draft-target",
        "chem.identity",
        json!({"document_id":source,"other_document_id":target,"mode":"recorded"}),
    );
    assert_eq!(result["outcome"], "ok", "{result}");
    assert_eq!(
        result["result"]["comparison"]["equivalent"], true,
        "the saved empty graph is compared, not the draft containing carbon"
    );
    assert_eq!(
        result["result"]["comparison"]["snapshot"]["recovery"]["state"],
        "recoverable"
    );
    assert_eq!(
        result["result"]["comparison"]["snapshot"]["document"]["atoms"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
}

#[test]
fn a_closed_snapshot_reserves_its_revision_before_a_later_external_commit() {
    let directory = tempfile::tempdir().unwrap();
    let app = App::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    );
    let target = draw(&app, "Discarded target edit", &["C"], &[]);
    app.close(&target, None, true, Some(1)).unwrap();
    let source = app
        .create_document(None, Some("Source"))
        .unwrap()
        .document
        .id;
    let compared = call(
        &app,
        "closed-snapshot",
        "chem.identity",
        json!({"document_id":source,"other_document_id":target,"mode":"recorded"}),
    );
    assert_eq!(compared["outcome"], "ok", "{compared}");
    let seen = compared["result"]["comparison"]["snapshot"]["document"]["revision"]
        .as_u64()
        .unwrap();
    assert!(
        compared["result"]["comparison"]["snapshot"]["document"]["atoms"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    let other = App::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    );
    let opened = other.document(&target).unwrap();
    let changed = call(
        &other,
        "external-edit",
        "document.transaction",
        json!({"document_id":target,"expected_revision":opened.document.revision,"commands":[{"op":"add_atom","element":"N","position":{"x":0,"y":0}}]}),
    );
    assert_eq!(changed["outcome"], "ok", "{changed}");
    other
        .save(
            &target,
            None,
            changed["document_revision"].as_u64().unwrap(),
        )
        .unwrap();
    let reopened = app.document(&target).unwrap();
    assert!(
        reopened.document.revision > seen,
        "a different graph must receive a later revision than the closed snapshot"
    );
    assert_eq!(reopened.document.atoms[0].element, "N");
}
