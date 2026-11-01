use chemifree_application::Application;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};

type App = Application<LocalCapabilities, FileDocumentRepository>;

fn call(app: &App, request_id: &str, operation: &str, parameters: Value) -> Value {
    let request = json!({"contract":1,"request_id":request_id,"profile":"preserve-semantics","operation":operation,"parameters":parameters});
    serde_json::to_value(app.execute(request.to_string().as_bytes())).unwrap()
}

fn draw(app: &App, title: &str, atom_count: usize, edges: &[(usize, usize, &str)]) -> String {
    let id = app.create_document(None, Some(title)).unwrap().document.id;
    let mut commands: Vec<_> = (0..atom_count)
        .map(|i| json!({"op":"add_atom", "element":"C", "position":{"x":i * 20, "y":0}}))
        .collect();
    commands.extend(edges.iter().map(|(a, b, order)| {
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
fn query_returns_distinct_source_target_ids_and_exact_unmodified_snapshots() {
    let directory = tempfile::tempdir().unwrap();
    let app = App::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    );
    let query = draw(&app, "Query", 2, &[(0, 1, "single")]);
    let ring = (0..6)
        .map(|i| (i, (i + 1) % 6, "single"))
        .collect::<Vec<_>>();
    let target = draw(&app, "Target", 6, &ring);
    let before_query = serde_json::to_value(app.document(&query).unwrap()).unwrap();
    let before_target = serde_json::to_value(app.document(&target).unwrap()).unwrap();
    let found = call(
        &app,
        "find",
        "chem.query_match",
        json!({
            "document_id":query,"target_document_id":target,"mode":"recorded",
            "predicates":[{"atom":"a1","ring":true,"min_neighbors":2,"max_neighbors":2,"charge":0}],
            "max_matches":100,"max_states":100_000
        }),
    );
    assert_eq!(found["outcome"], "ok", "{found}");
    assert_eq!(found["model_id"], "chemifree-query-recorded-1");
    assert_eq!(found["result"]["complete"], true);
    assert_eq!(found["result"]["mappings"].as_array().unwrap().len(), 12);
    assert_eq!(
        found["result"]["mappings"][0],
        json!([
            {"query_atom":"a1","target_atom":"a1"},
            {"query_atom":"a2","target_atom":"a2"}
        ])
    );
    assert_eq!(
        found["result"]["query_snapshot"]["document"],
        before_query["document"]
    );
    assert_eq!(
        found["result"]["target_snapshot"]["document"],
        before_target["document"]
    );
    assert_eq!(
        serde_json::to_value(app.document(&query).unwrap()).unwrap(),
        before_query
    );
    assert_eq!(
        serde_json::to_value(app.document(&target).unwrap()).unwrap(),
        before_target
    );
}

#[test]
fn search_limits_report_partial_mappings_without_claiming_completeness() {
    let directory = tempfile::tempdir().unwrap();
    let app = App::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    );
    let query = draw(&app, "Query", 2, &[(0, 1, "single")]);
    let target = draw(
        &app,
        "Target",
        3,
        &[(0, 1, "single"), (1, 2, "single"), (2, 0, "single")],
    );
    for (matches, states, expected_limit, count) in
        [(2, 10_000, "mappings", 2), (10, 1, "states", 0)]
    {
        let found = call(
            &app,
            "bounded",
            "chem.query_match",
            json!({
                "document_id":query,"target_document_id":target,"mode":"recorded",
                "max_matches":matches,"max_states":states
            }),
        );
        assert_eq!(found["outcome"], "partial", "{found}");
        assert_eq!(found["result"]["complete"], false);
        assert_eq!(found["result"]["limit"], expected_limit);
        assert_eq!(found["result"]["mappings"].as_array().unwrap().len(), count);
        assert_eq!(found["loss_report"][0]["feature"], "additional_mappings");
        assert_eq!(found["loss_report"][0]["disposition"], "unknown");
    }
}

#[test]
fn aromatic_target_policy_matches_kekule_benzene_without_rewriting_it() {
    let directory = tempfile::tempdir().unwrap();
    let app = App::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    );
    let query = draw(&app, "Aromatic query", 2, &[(0, 1, "aromatic")]);
    let ring = (0..6)
        .map(|i| (i, (i + 1) % 6, if i % 2 == 0 { "double" } else { "single" }))
        .collect::<Vec<_>>();
    let target = draw(&app, "Kekule", 6, &ring);
    let before = serde_json::to_value(app.document(&target).unwrap()).unwrap();
    let recorded = call(
        &app,
        "recorded",
        "chem.query_match",
        json!({"document_id":query,"target_document_id":target,"mode":"recorded"}),
    );
    assert_eq!(recorded["result"]["mappings"].as_array().unwrap().len(), 0);
    let normalized = call(
        &app,
        "aromatic",
        "chem.query_match",
        json!({"document_id":query,"target_document_id":target,"mode":"aromatic_target"}),
    );
    assert_eq!(normalized["model_id"], "chemifree-query-aromatic-target-1");
    assert_eq!(
        normalized["result"]["mappings"].as_array().unwrap().len(),
        12
    );
    assert_eq!(
        serde_json::to_value(app.document(&target).unwrap()).unwrap(),
        before
    );
}

#[test]
fn unsupported_targets_and_invalid_predicates_have_no_partial_result() {
    let directory = tempfile::tempdir().unwrap();
    let app = App::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    );
    let query = draw(&app, "Query", 1, &[]);
    let target = draw(&app, "Target", 2, &[(0, 1, "unknown")]);
    let failed = call(
        &app,
        "unsupported",
        "chem.query_match",
        json!({"document_id":query,"target_document_id":target,"mode":"recorded"}),
    );
    assert_eq!(failed["outcome"], "unsupported");
    assert!(failed["result"].is_null());
    assert_eq!(failed["diagnostics"][0]["details"]["side"], "target");
    assert_eq!(
        failed["diagnostics"][0]["details"]["snapshot"]["document"]["id"],
        target
    );
    let invalid = call(
        &app,
        "invalid",
        "chem.query_match",
        json!({"document_id":query,"target_document_id":target,"mode":"recorded","predicates":[{"atom":"a99"}]}),
    );
    assert_eq!(invalid["outcome"], "error");
    assert!(invalid["result"].is_null());
    assert_eq!(
        invalid["diagnostics"][0]["details"]["reason"],
        "invalid_predicate"
    );
}

#[test]
fn query_and_target_recovery_drafts_are_reported_but_not_searched() {
    let directory = tempfile::tempdir().unwrap();
    let (query, target) = {
        let previous = App::new(
            LocalCapabilities,
            FileDocumentRepository::new(directory.path()),
        );
        let query = draw(&previous, "Query with draft", 2, &[(0, 1, "single")]);
        let target = draw(
            &previous,
            "Target with draft",
            3,
            &[(0, 1, "single"), (1, 2, "single"), (2, 0, "single")],
        );
        for id in [&query, &target] {
            previous.save(id, None, 1).unwrap();
            let command = if id == &query {
                json!({"op":"add_atom","element":"O","position":{"x":0,"y":40}})
            } else {
                json!({"op":"add_atom","element":"C","position":{"x":0,"y":40},"bond_to":{"atom":"a1","order":"single"}})
            };
            let edited = call(
                &previous,
                "draft-edit",
                "document.transaction",
                json!({
                    "document_id":id,"expected_revision":1,
                    "commands":[command]
                }),
            );
            assert_eq!(edited["outcome"], "ok");
            previous.autosave(id, None).unwrap();
        }
        (query, target)
    };
    let app = App::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    );
    let result = call(
        &app,
        "query-drafts",
        "chem.query_match",
        json!({"document_id":query,"target_document_id":target,"mode":"recorded",
        "predicates":[{"atom":"a1","ring":true,"min_neighbors":2,"max_neighbors":2}]}),
    );
    assert_eq!(result["outcome"], "ok", "{result}");
    assert_eq!(
        result["result"]["query_snapshot"]["recovery"]["state"],
        "recoverable"
    );
    assert_eq!(
        result["result"]["target_snapshot"]["recovery"]["state"],
        "recoverable"
    );
    assert_eq!(
        result["result"]["query_snapshot"]["document"]["atoms"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        result["result"]["target_snapshot"]["document"]["atoms"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(result["result"]["mappings"].as_array().unwrap().len(), 6);
}

#[test]
fn malformed_and_unknown_predicate_atoms_get_the_same_reply_shape() {
    let directory = tempfile::tempdir().unwrap();
    let app = App::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    );
    let query = draw(&app, "Query", 1, &[]);
    let target = draw(&app, "Target", 1, &[]);
    let reply = |atom: &str| {
        call(
            &app,
            "predicate-shape",
            "chem.query_match",
            json!({"document_id":query,"target_document_id":target,"mode":"recorded","predicates":[{"atom":atom}]}),
        )
    };
    let unknown = reply("a99");
    let malformed = reply("not an id");
    for answer in [&unknown, &malformed] {
        assert_eq!(answer["outcome"], "error", "{answer}");
        assert!(answer["result"].is_null());
        assert_eq!(answer["model_id"], "chemifree-query-recorded-1");
        assert_eq!(answer["diagnostics"][0]["code"], "invalid_query");
        assert_eq!(
            answer["diagnostics"][0]["details"]["reason"],
            "invalid_predicate"
        );
        assert_eq!(answer["diagnostics"][0]["details"]["side"], "query");
        assert_eq!(
            answer["diagnostics"][0]["details"]["snapshot"]["document"]["id"],
            query
        );
    }
}

#[test]
fn a_contradictory_charge_condition_is_an_invalid_predicate_not_an_empty_result() {
    let directory = tempfile::tempdir().unwrap();
    let app = App::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    );
    let query = draw(&app, "Query", 1, &[]);
    let target = draw(&app, "Target", 1, &[]);
    let answer = call(
        &app,
        "contradictory-charge",
        "chem.query_match",
        json!({"document_id":query,"target_document_id":target,"mode":"recorded","predicates":[{"atom":"a1","charge":1}]}),
    );
    assert_eq!(answer["outcome"], "error", "{answer}");
    assert_eq!(
        answer["diagnostics"][0]["details"]["reason"],
        "invalid_predicate"
    );
    assert!(answer["result"].is_null());
}

#[test]
fn undetermined_target_hydrogens_are_unsupported_for_a_stated_hydrogen_query() {
    let directory = tempfile::tempdir().unwrap();
    let app = App::new(
        LocalCapabilities,
        FileDocumentRepository::new(directory.path()),
    );
    let query = app
        .create_document(None, Some("Query"))
        .unwrap()
        .document
        .id;
    let drawn = call(
        &app,
        "hydrogen-query",
        "document.transaction",
        json!({"document_id":query,"expected_revision":0,"commands":[
            {"op":"add_atom","element":"C","position":{"x":0,"y":0},"hydrogens":{"mode":"specified","count":1}}]}),
    );
    assert_eq!(drawn["outcome"], "ok", "{drawn}");
    let target = draw(
        &app,
        "Target",
        3,
        &[(0, 1, "aromatic"), (1, 2, "aromatic"), (2, 0, "aromatic")],
    );
    let answer = call(
        &app,
        "hydrogen-target",
        "chem.query_match",
        json!({"document_id":query,"target_document_id":target,"mode":"recorded"}),
    );
    assert_eq!(answer["outcome"], "unsupported", "{answer}");
    assert_eq!(
        answer["diagnostics"][0]["details"]["reason"],
        "target_hydrogens_undetermined"
    );
    assert_eq!(answer["diagnostics"][0]["details"]["side"], "target");
    assert!(answer["result"].is_null());
}
