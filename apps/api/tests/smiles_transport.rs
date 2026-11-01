//! SMILES over HTTP: the same operations as every other entry point, through the 256 KiB body
//! limit and the session header.

use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use chemifree_api::{ApiState, LocalApplication, SESSION_HEADER, router};
use chemifree_providers::{FileDocumentRepository, FilePreferenceStore, LocalCapabilities};
use serde_json::{Value, json};
use tower::ServiceExt;

async fn post(
    state: &Arc<ApiState>,
    id: &str,
    operation: &str,
    parameters: Value,
) -> (StatusCode, Value) {
    let body = json!({"contract":1,"request_id":id,"profile":"preserve-semantics","operation":operation,"parameters":parameters});
    let request = Request::post("/api/v1/operations")
        .header("host", "127.0.0.1:5173")
        .header("origin", "http://127.0.0.1:5173")
        .header("sec-fetch-site", "same-origin")
        .header("content-type", "application/json")
        .header(SESSION_HEADER, "t")
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = router(Arc::clone(state)).oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1 << 20).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn smiles_is_imported_and_written_over_http() {
    let data = tempfile::tempdir().unwrap();
    let application =
        LocalApplication::new(LocalCapabilities, FileDocumentRepository::new(data.path()))
            .with_preferences(FilePreferenceStore::new(data.path()))
            .with_smiles();
    let state = Arc::new(
        ApiState::new(
            application,
            "t".into(),
            3999,
            &["http://127.0.0.1:5173".into()],
        )
        .unwrap(),
    );
    let (_, created) = post(&state, "create", "document.create", json!({"title":"T"})).await;
    let id = created["result"]["document"]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let (status, shown) = post(
        &state,
        "preview",
        "document.import",
        json!({"document_id":id,"expected_revision":0,"format":"smiles","source":"OCC","preview":true}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(shown["outcome"], "partial", "{shown}");
    let digest = shown["result"]["preview_digest"].as_str().unwrap();
    let (_, made) = post(
        &state,
        "create-import",
        "document.import",
        json!({"document_id":id,"expected_revision":0,"format":"smiles","source":"OCC","preview_digest":digest,"accept_loss":true}),
    )
    .await;
    assert_eq!(made["document_revision"], 1, "{made}");

    let (_, written) = post(
        &state,
        "write",
        "document.export",
        json!({"format":"smiles","document_id":id,"form":"canonical"}),
    )
    .await;
    assert_eq!(written["result"]["content"], "CCO");
    assert_eq!(written["model_id"], "chemifree-smiles-canonical-1");

    let (status, bad) = post(
        &state,
        "bad",
        "document.import",
        json!({"document_id":id,"expected_revision":1,"format":"smiles","source":"C=(O)","preview":true}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bad["diagnostics"][0]["code"], "dangling_bond");
    assert_eq!(bad["diagnostics"][0]["details"]["offset"], 1);

    let caps = {
        let request = Request::get("/api/v1/capabilities")
            .header("host", "127.0.0.1:5173")
            .header("origin", "http://127.0.0.1:5173")
            .header("sec-fetch-site", "same-origin")
            .body(Body::empty())
            .unwrap();
        let response = router(Arc::clone(&state)).oneshot(request).await.unwrap();
        let bytes = to_bytes(response.into_body(), 1 << 20).await.unwrap();
        serde_json::from_slice::<Value>(&bytes).unwrap()
    };
    let conversion = caps["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "format_conversion")
        .unwrap();
    assert_eq!(conversion["label"], "SMILES subset");
}
