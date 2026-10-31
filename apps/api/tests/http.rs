use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use chemifree_api::{ApiState, LocalApplication, MAX_BODY_BYTES, SESSION_HEADER, router};
use chemifree_application::{ActiveDataError, DataRegistry};
use chemifree_providers::{FileDocumentRepository, FilePreferenceStore, LocalCapabilities};
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

const PORT: u16 = 3999;
const HOST: &str = "127.0.0.1:3999";
const WEB: &str = "http://127.0.0.1:5173";
const TOKEN: &str = "test-token";

#[tokio::test]
async fn absent_model_data_is_reported_without_blocking_document_creation() {
    let api = api();
    let (status, body) = api.get("/api/v1/data").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["state"], "not_configured");
    assert_eq!(body["engine"], "not_implemented");
    let (status, _) = api
        .post(
            "/api/v1/documents",
            json!({"request_id": "create-1", "title": "Independent drawing"}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
}

#[tokio::test]
async fn refused_model_data_is_reported_without_blocking_document_creation() {
    let api = api_with_registry(DataRegistry::refused(ActiveDataError::RegistryInvalid));
    let (status, body) = api.get("/api/v1/data").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["state"], "refused");
    assert_eq!(body["reason"], "registry_invalid");
    let (status, _) = api
        .post("/api/v1/documents", json!({"request_id": "create-2"}))
        .await;
    assert_eq!(status, StatusCode::CREATED);
}

struct Api {
    router: Router,
    data: TempDir,
}

fn api() -> Api {
    api_with_registry(DataRegistry::not_configured())
}

fn api_with_registry(registry: DataRegistry) -> Api {
    let data = tempfile::tempdir().unwrap();
    let repository = FileDocumentRepository::new(data.path());
    let application = LocalApplication::new(LocalCapabilities, repository)
        .with_preferences(FilePreferenceStore::new(data.path()))
        .with_data_registry(registry);
    let state = ApiState::new(application, TOKEN.into(), PORT, &[WEB.into()]).unwrap();
    Api {
        router: router(Arc::new(state)),
        data,
    }
}

impl Api {
    async fn send(&self, request: Request<Body>) -> (StatusCode, Value) {
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        assert!(
            response
                .headers()
                .get("access-control-allow-origin")
                .is_none(),
            "no CORS grants"
        );
        let body = to_bytes(response.into_body(), 1 << 20).await.unwrap();
        let value = if body.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&body).unwrap()
        };
        (status, value)
    }

    async fn get(&self, uri: &str) -> (StatusCode, Value) {
        self.send(
            Request::get(uri)
                .header("host", HOST)
                .body(Body::empty())
                .unwrap(),
        )
        .await
    }

    /// A same-origin browser write through the development proxy.
    async fn post(&self, uri: &str, body: Value) -> (StatusCode, Value) {
        self.send(write(uri).body(Body::from(body.to_string())).unwrap())
            .await
    }

    async fn put(&self, uri: &str, body: Value) -> (StatusCode, Value) {
        self.send(
            write(uri)
                .method("PUT")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
    }
}

fn write(uri: &str) -> axum::http::request::Builder {
    Request::post(uri)
        .header("host", "127.0.0.1:5173")
        .header("origin", WEB)
        .header("sec-fetch-site", "same-origin")
        .header("content-type", "application/json")
        .header(SESSION_HEADER, TOKEN)
}

fn ethanol() -> Value {
    json!([
        {"op": "add_atom", "element": "C", "position": {"x": 0, "y": 0}},
        {"op": "add_atom", "element": "C", "position": {"x": 26, "y": 15},
         "bond_to": {"atom": "a1", "order": "single"}},
        {"op": "add_atom", "element": "O", "position": {"x": 52, "y": 0},
         "bond_to": {"atom": "a2", "order": "single"}}
    ])
}

#[tokio::test]
async fn capabilities_report_connected_operations() {
    let api = api();
    let (status, body) = api.get("/api/v1/capabilities").await;
    assert_eq!(status, StatusCode::OK);
    let available: Vec<_> = body["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["status"] == "available")
        .map(|item| item["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        available,
        [
            "runtime_inspection",
            "document_editing",
            "format_conversion",
            "persistence"
        ]
    );
}

#[tokio::test]
async fn mol_import_and_export_use_the_shared_operation_contract_over_http() {
    let api = api();
    let text = include_str!("../../../crates/providers/tests/data/molfile/sodium-acetate.mol");
    let (status, imported) = api
        .post(
            "/api/v1/operations",
            json!({
                "contract": 1, "request_id": "mol-in", "profile": "preserve-semantics",
                "operation": "document.import", "parameters": {"format": "mol", "text": text}
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(imported["outcome"], "ok", "{imported}");
    assert_eq!(imported["result"]["document"]["atoms"][3]["charge"], -1);
    let id = imported["result"]["document"]["id"].as_str().unwrap();
    let (status, exported) = api
        .post(
            "/api/v1/operations",
            json!({
                "contract": 1, "request_id": "mol-out", "profile": "preserve-semantics",
                "operation": "document.export", "parameters": {"format": "mol", "document_id": id}
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(exported["outcome"], "partial");
    assert!(
        exported["result"]["text"]
            .as_str()
            .unwrap()
            .contains("M  CHG  2   4  -1   5   1\n")
    );
}

#[tokio::test]
async fn the_document_flow_works_end_to_end_over_http() {
    let api = api();
    let (status, session) = api.get("/api/v1/session").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(session["header"], SESSION_HEADER);
    assert_eq!(session["token"], TOKEN);

    let (status, created) = api
        .post(
            "/api/v1/documents",
            json!({"request_id": "c1", "title": "Ethanol"}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = created["document"]["id"].as_str().unwrap().to_owned();
    let base = format!("/api/v1/documents/{id}");

    let (status, edited) = api
        .post(
            &format!("{base}/transactions"),
            json!({"request_id": "t1", "expected_revision": 0, "commands": ethanol()}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{edited}");
    assert_eq!(edited["document"]["revision"], 1);
    assert_eq!(edited["session"]["unsaved_changes"], true);

    let (_, changed) = api
        .post(
            &format!("{base}/transactions"),
            json!({"request_id": "t2", "expected_revision": 1, "commands": [
                {"op": "update_bond", "bond": "b2", "order": "double"}
            ]}),
        )
        .await;
    let (_, undone) = api
        .post(
            &format!("{base}/undo"),
            json!({"request_id": "u1", "expected_revision": 2}),
        )
        .await;
    assert_eq!(undone["document"]["bonds"], edited["document"]["bonds"]);
    let (_, redone) = api
        .post(
            &format!("{base}/redo"),
            json!({"request_id": "r1", "expected_revision": 3}),
        )
        .await;
    assert_eq!(redone["document"]["bonds"], changed["document"]["bonds"]);

    let (status, conflict) = api
        .post(
            &format!("{base}/save"),
            json!({"request_id": "s0", "expected_revision": 2}),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(conflict["code"], "revision_conflict");
    assert_eq!(conflict["current_revision"], 4);

    let (status, saved) = api
        .post(
            &format!("{base}/save"),
            json!({"request_id": "s1", "expected_revision": 4}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(saved["session"]["unsaved_changes"], false);
    let (status, closed) = api
        .post(&format!("{base}/close"), json!({"request_id": "x1"}))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(closed["discarded_changes"], false);

    let (status, reopened) = api.get(&base).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reopened, saved);
    let (_, list) = api.get("/api/v1/documents").await;
    assert_eq!(list["documents"][0]["title"], "Ethanol");
    assert_eq!(list["documents"][0]["atom_count"], 3);
}

#[tokio::test]
async fn domain_errors_map_to_v1_envelopes() {
    let api = api();
    let (_, created) = api
        .post("/api/v1/documents", json!({"request_id": "c1"}))
        .await;
    let base = format!(
        "/api/v1/documents/{}",
        created["document"]["id"].as_str().unwrap()
    );

    let (status, body) = api
        .post(
            &format!("{base}/transactions"),
            json!({"request_id": "t1", "expected_revision": 0, "commands": [
                {"op": "add_atom", "element": "C", "position": {"x": 0, "y": 0}},
                {"op": "add_bond", "begin": "a1", "end": "a1", "order": "single"}
            ]}),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["code"], "invalid_command");
    assert_eq!(body["command_index"], 1);
    assert_eq!(body["api_version"], "v1");

    let (status, body) = api
        .post(
            &format!("{base}/undo"),
            json!({"request_id": "u1", "expected_revision": 0}),
        )
        .await;
    assert_eq!(
        (status, body["code"].as_str()),
        (StatusCode::CONFLICT, Some("nothing_to_undo"))
    );

    let (status, body) = api.get("/api/v1/documents/doc-missing").await;
    assert_eq!(
        (status, body["code"].as_str()),
        (StatusCode::NOT_FOUND, Some("document_not_found"))
    );
}

#[tokio::test]
async fn a_failed_save_is_reported_and_the_session_keeps_its_changes() {
    let api = api();
    let (_, created) = api
        .post("/api/v1/documents", json!({"request_id": "c1"}))
        .await;
    let id = created["document"]["id"].as_str().unwrap().to_owned();
    let base = format!("/api/v1/documents/{id}");
    api.post(
        &format!("{base}/transactions"),
        json!({"request_id": "t1", "expected_revision": 0, "commands": ethanol()}),
    )
    .await;

    let file = api.data.path().join("documents").join(format!("{id}.json"));
    let committed = std::fs::read(&file).unwrap();
    std::fs::remove_file(&file).unwrap();
    std::fs::create_dir(&file).unwrap();
    let (status, body) = api
        .post(
            &format!("{base}/save"),
            json!({"request_id": "s1", "expected_revision": 1}),
        )
        .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["code"], "storage_failed");
    let (_, current) = api.get(&base).await;
    assert_eq!(current["session"]["unsaved_changes"], true);
    assert_eq!(current["document"]["atoms"].as_array().unwrap().len(), 3);

    std::fs::remove_dir(&file).unwrap();
    std::fs::write(&file, committed).unwrap();
    let (status, saved) = api
        .post(
            &format!("{base}/save"),
            json!({"request_id": "s1", "expected_revision": 1}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(saved["session"]["saved_revision"], 1);
}

#[tokio::test]
async fn writes_require_an_allowed_host_origin_token_and_json() {
    let api = api();
    let create = || json!({"request_id": "c1"}).to_string();
    let cases = [
        (
            Request::post("/api/v1/documents")
                .header("host", HOST)
                .header("origin", "https://evil.example")
                .header("content-type", "application/json")
                .header(SESSION_HEADER, TOKEN),
            StatusCode::FORBIDDEN,
            "forbidden_origin",
        ),
        (
            Request::post("/api/v1/documents")
                .header("host", HOST)
                .header("sec-fetch-site", "cross-site")
                .header("content-type", "application/json")
                .header(SESSION_HEADER, TOKEN),
            StatusCode::FORBIDDEN,
            "forbidden_origin",
        ),
        (
            Request::post("/api/v1/documents")
                .header("host", HOST)
                .header("content-type", "application/json"),
            StatusCode::UNAUTHORIZED,
            "unauthorized",
        ),
        (
            Request::post("/api/v1/documents")
                .header("host", HOST)
                .header("content-type", "application/json")
                .header(SESSION_HEADER, "test-tokem"),
            StatusCode::UNAUTHORIZED,
            "unauthorized",
        ),
        (
            Request::post("/api/v1/documents")
                .header("host", HOST)
                .header("content-type", "text/plain")
                .header(SESSION_HEADER, TOKEN),
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unsupported_media_type",
        ),
    ];
    for (request, status, code) in cases {
        let (actual, body) = api.send(request.body(Body::from(create())).unwrap()).await;
        assert_eq!((actual, body["code"].as_str()), (status, Some(code)));
    }
    // A rebinding page carries its own name in Host; `insert` replaces the valid one.
    let mut rebound = write("/api/v1/documents")
        .body(Body::from(create()))
        .unwrap();
    rebound
        .headers_mut()
        .insert("host", "localhost.evil.example:5173".parse().unwrap());
    let (status, body) = api.send(rebound).await;
    assert_eq!(
        (status, body["code"].as_str()),
        (StatusCode::FORBIDDEN, Some("forbidden_host"))
    );

    let (status, body) = api
        .send(
            Request::get("/api/v1/session")
                .header("host", HOST)
                .header("origin", "https://evil.example")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(
        (status, body["code"].as_str()),
        (StatusCode::FORBIDDEN, Some("forbidden_origin"))
    );
    let (status, _) = api
        .send(Request::get("/api/v1/health").body(Body::empty()).unwrap())
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "a missing Host is refused");

    let list = api.get("/api/v1/documents").await.1;
    assert_eq!(
        list["documents"],
        json!([]),
        "no rejected write reached storage"
    );
}

#[tokio::test]
async fn malformed_and_oversized_bodies_are_rejected() {
    let api = api();
    for (body, status, code) in [
        ("{".to_owned(), StatusCode::BAD_REQUEST, "invalid_json"),
        (
            json!({"request_id": "c1", "owner": "x"}).to_string(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_request",
        ),
        (
            json!({"title": "missing request id"}).to_string(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_request",
        ),
        (
            json!({"request_id": "c1", "title": "x".repeat(MAX_BODY_BYTES)}).to_string(),
            StatusCode::PAYLOAD_TOO_LARGE,
            "payload_too_large",
        ),
    ] {
        let (actual, response) = api
            .send(write("/api/v1/documents").body(Body::from(body)).unwrap())
            .await;
        assert_eq!(
            (actual, response["code"].as_str()),
            (status, Some(code)),
            "{response}"
        );
    }
}

#[tokio::test]
async fn unknown_routes_and_methods_are_explicit_errors() {
    let api = api();
    for (method, uri, status, code) in [
        ("POST", "/api/v1/capabilities", 405, "method_not_allowed"),
        (
            "DELETE",
            "/api/v1/documents/doc-1",
            405,
            "method_not_allowed",
        ),
        ("GET", "/api/v1/unknown", 404, "not_found"),
        ("GET", "/api/v2/health", 404, "not_found"),
    ] {
        let (actual, body) = api
            .send(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .header("host", HOST)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
        assert_eq!(actual.as_u16(), status);
        assert_eq!(body["code"], code);
        assert_eq!(body["api_version"], "v1");
    }
}

#[test]
fn only_loopback_web_origins_are_accepted() {
    let state = |origin: &str| {
        let data = tempfile::tempdir().unwrap();
        let repository = FileDocumentRepository::new(data.path());
        let application = LocalApplication::new(LocalCapabilities, repository);
        ApiState::new(application, TOKEN.into(), PORT, &[origin.into()]).map(|_| ())
    };
    for origin in [
        "http://localhost:5173",
        "https://127.0.0.1",
        "http://[::1]:8080",
    ] {
        assert!(state(origin).is_ok(), "{origin}");
    }
    for origin in [
        "http://example.com",
        "http://localhost.evil.example",
        "http://127.0.0.1:5173/path",
        "http://127.0.0.1:+80",
        "file://localhost",
    ] {
        assert!(state(origin).is_err(), "{origin}");
    }
}

#[tokio::test]
async fn autosave_save_as_and_draft_routes_map_to_v1_statuses() {
    let api = api();
    let (_, created) = api
        .post(
            "/api/v1/documents",
            json!({"request_id": "c1", "title": "Source"}),
        )
        .await;
    let id = created["document"]["id"].as_str().unwrap().to_owned();
    let path = |action: &str| format!("/api/v1/documents/{id}/{action}");
    api.post(
        &path("transactions"),
        json!({"request_id": "t1", "expected_revision": 0, "commands": ethanol()}),
    )
    .await;

    let (status, autosaved) = api
        .post(&path("autosave"), json!({"request_id": "a1"}))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(autosaved["session"]["draft_revision"], 1);
    assert_eq!(autosaved["recovery"], Value::Null);
    let draft = api.data.path().join("drafts").join(format!("{id}.json"));
    assert!(draft.exists());

    for (action, code, status) in [
        ("recover", "unsaved_changes", StatusCode::CONFLICT),
        ("discard-draft", "draft_in_use", StatusCode::CONFLICT),
    ] {
        let (actual, body) = api.post(&path(action), json!({"request_id": action})).await;
        assert_eq!(
            (actual, body["code"].as_str()),
            (status, Some(code)),
            "{action}"
        );
    }
    let (status, body) = api
        .post(&path("recover-as-copy"), json!({"request_id": "rc"}))
        .await;
    assert_eq!(
        (status, body["code"].as_str()),
        (StatusCode::CONFLICT, Some("draft_in_use"))
    );

    let (status, copied) = api
        .post(
            &path("save-as"),
            json!({"request_id": "s1", "expected_revision": 1, "title": "Copy"}),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(copied["document"]["title"], "Copy");
    assert_eq!(copied["document"]["atoms"].as_array().unwrap().len(), 3);
    assert_eq!(
        copied["source"],
        json!({"id": id, "closed": true, "discarded_changes": true})
    );
    assert!(!draft.exists(), "the source's draft went with its session");

    let (status, body) = api
        .post(&path("discard-draft"), json!({"request_id": "d1"}))
        .await;
    assert_eq!(
        (status, body["code"].as_str()),
        (StatusCode::NOT_FOUND, Some("draft_not_found"))
    );
    let (status, body) = api
        .post(
            &path("save-as"),
            json!({"request_id": "s2", "expected_revision": 1, "x": 1}),
        )
        .await;
    assert_eq!(
        (status, body["code"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("invalid_request"))
    );
}

#[tokio::test]
async fn preferences_are_read_and_written_with_a_revision_and_guarded_like_writes() {
    let api = api();
    let (status, body) = api.get("/api/v1/preferences").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["revision"], 0);
    assert_eq!(body["ui"], json!({"theme": "system"}));
    assert_eq!(body["new_documents"], json!({}));
    assert_eq!(body["new_document_settings"]["bond_length_pt"], 30.0);
    assert_eq!(body["computation"]["nmr"]["status"], "not_implemented");
    assert_eq!(
        body["computation"]["numeric_precision"]["owner"],
        "preferences"
    );

    let change = json!({"request_id": "p1", "expected_revision": 0,
        "ui": {"theme": "dark"}, "new_documents": {"bond_length_pt": 40}});
    let unguarded = api
        .send(
            Request::put("/api/v1/preferences")
                .header("host", HOST)
                .header("content-type", "application/json")
                .body(Body::from(change.to_string()))
                .unwrap(),
        )
        .await;
    assert_eq!(unguarded.0, StatusCode::UNAUTHORIZED);
    assert!(
        !api.data.path().join("preferences.json").exists(),
        "a refused write stores nothing"
    );

    let (status, body) = api.put("/api/v1/preferences", change.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        (body["revision"].clone(), body["ui"]["theme"].clone()),
        (json!(1), json!("dark"))
    );
    assert!(api.data.path().join("preferences.json").exists());
    let (status, replayed) = api.put("/api/v1/preferences", change).await;
    assert_eq!((status, replayed), (StatusCode::OK, body));

    let (status, body) = api
        .put(
            "/api/v1/preferences",
            json!({"request_id": "p2", "expected_revision": 0,
                "ui": {"theme": "light"}, "new_documents": {}}),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        (body["code"].as_str(), body["current_revision"].as_u64()),
        (Some("revision_conflict"), Some(1))
    );

    let (status, body) = api
        .put(
            "/api/v1/preferences",
            json!({"request_id": "p3", "expected_revision": 1,
                "ui": {"theme": "light"}, "new_documents": {},
                "computation": {"nmr": {"solvent": "CDCl3"}}}),
        )
        .await;
    assert_eq!(
        (status, body["code"].as_str()),
        (StatusCode::NOT_IMPLEMENTED, Some("setting_not_implemented"))
    );

    let (status, body) = api
        .put(
            "/api/v1/preferences",
            json!({"request_id": "p4", "expected_revision": 1,
                "ui": {"theme": "light"}, "new_documents": {"label_size_pt": 100}}),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        (body["code"].as_str(), body["reason"].as_str()),
        (Some("invalid_request"), Some("label_size_out_of_range"))
    );

    let (status, body) = api
        .post(
            "/api/v1/preferences/reset",
            json!({"request_id": "p5", "expected_revision": 1}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        (body["revision"].clone(), body["new_documents"].clone()),
        (json!(2), json!({}))
    );
    let (status, _) = api.post("/api/v1/preferences", json!({})).await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn document_settings_are_edited_reset_and_imported_over_http() {
    let api = api();
    let (_, preferences) = api
        .put(
            "/api/v1/preferences",
            json!({"request_id": "p1", "expected_revision": 0,
                "ui": {"theme": "system"}, "new_documents": {"label_font": "serif"}}),
        )
        .await;
    assert_eq!(preferences["revision"], 1);
    let (_, created) = api
        .post("/api/v1/documents", json!({"request_id": "c1"}))
        .await;
    let id = created["document"]["id"].as_str().unwrap().to_owned();
    assert_eq!(created["document"]["settings"]["label_font"], "serif");
    assert_eq!(created["document"]["settings"]["bond_length_pt"], 30.0);

    let (status, body) = api
        .post(
            &format!("/api/v1/documents/{id}/transactions"),
            json!({"request_id": "t1", "expected_revision": 0, "commands": [
                {"op": "update_settings", "line_width_pt": 0}
            ]}),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        (
            body["code"].as_str(),
            body["command_index"].as_u64(),
            body["reason"].as_str()
        ),
        (
            Some("invalid_command"),
            Some(0),
            Some("line_width_out_of_range")
        )
    );
    let (status, body) = api
        .post(
            &format!("/api/v1/documents/{id}/transactions"),
            json!({"request_id": "t2", "expected_revision": 0, "commands": [
                {"op": "update_settings", "bond_length_pt": 45, "display_unit": "mm"}
            ]}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["document"]["settings"]["bond_length_pt"], 45.0);
    assert_eq!(body["document"]["settings"]["display_unit"], "mm");

    let (status, body) = api
        .post(
            &format!("/api/v1/documents/{id}/settings/reset"),
            json!({"request_id": "r1", "expected_revision": 1, "to": "defaults"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["document"]["settings"],
        json!({"bond_length_pt": 30.0, "line_width_pt": 1.2, "label_font": "sans-serif",
            "label_size_pt": 12.0, "carbon_labels": "default", "display_unit": "pt"})
    );
    assert_eq!(body["session"]["undo_depth"], 2);

    let (_, other) = api
        .post("/api/v1/documents", json!({"request_id": "c2"}))
        .await;
    let other = other["document"]["id"].as_str().unwrap().to_owned();
    let (status, body) = api
        .post(
            &format!("/api/v1/documents/{other}/settings/import"),
            json!({"request_id": "i1", "expected_revision": 0, "source": id}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["document"]["settings"]["label_font"], "sans-serif");
    let (status, body) = api
        .post(
            &format!("/api/v1/documents/{other}/settings/import"),
            json!({"request_id": "i2", "expected_revision": 1, "source": "doc-missing"}),
        )
        .await;
    assert_eq!(
        (status, body["code"].as_str()),
        (StatusCode::NOT_FOUND, Some("document_not_found"))
    );
    let (status, body) = api
        .post(
            &format!("/api/v1/documents/{other}/settings/reset"),
            json!({"request_id": "r2", "expected_revision": 1, "to": "preferences"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["document"]["settings"]["label_font"], "serif");

    let (status, body) = api
        .post(
            "/api/v1/preferences/import",
            json!({"request_id": "p2", "expected_revision": 1, "document": id}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["revision"], 2);
    assert_eq!(body["new_documents"]["label_font"], "sans-serif");
    assert_eq!(body["new_documents"]["display_unit"], "pt");
}

#[tokio::test]
async fn settings_replacements_record_the_command_time_selection_and_replay_it() {
    let api = api();
    let (_, created) = api
        .post("/api/v1/documents", json!({"request_id": "create"}))
        .await;
    let id = created["document"]["id"].as_str().unwrap().to_owned();
    let base = format!("/api/v1/documents/{id}");
    let a1 = json!({"objects": [], "atoms": ["a1"], "bonds": []});
    let a2 = json!({"objects": [], "atoms": ["a2"], "bonds": []});
    let (status, _) = api
        .post(
            &format!("{base}/transactions"),
            json!({
                "request_id": "edit", "expected_revision": 0,
                "commands": [
                    {"op": "add_atom", "element": "C", "position": {"x": 0, "y": 0}},
                    {"op": "add_atom", "element": "O", "position": {"x": 30, "y": 0}},
                    {"op": "update_settings", "line_width_pt": 2}
                ],
                "selection_after": a1
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let reset = json!({"request_id": "reset", "expected_revision": 1, "to": "defaults",
        "selection_before": a2, "selection_after": a2});
    let (status, reset_reply) = api
        .post(&format!("{base}/settings/reset"), reset.clone())
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reset_reply["document"]["selection"], a2);
    let (status, replay) = api
        .post(&format!("{base}/settings/reset"), reset.clone())
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replay, reset_reply);
    let mut different = reset;
    different["selection_before"] = a1.clone();
    let (status, _) = api.post(&format!("{base}/settings/reset"), different).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, undo) = api
        .post(
            &format!("{base}/undo"),
            json!({"request_id":"undo-reset", "expected_revision": 2}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(undo["document"]["selection"], a2);
    let (_, source) = api
        .post("/api/v1/documents", json!({"request_id":"source"}))
        .await;
    let source = source["document"]["id"].as_str().unwrap();
    let (status, imported) = api
        .post(
            &format!("{base}/settings/import"),
            json!({
                "request_id": "import", "expected_revision": 3, "source": source,
                "selection_before": a1, "selection_after": a1
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(imported["document"]["selection"], a1);
    let (status, undo) = api
        .post(
            &format!("{base}/undo"),
            json!({"request_id":"undo-import", "expected_revision": 4}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(undo["document"]["selection"], a1);
}
