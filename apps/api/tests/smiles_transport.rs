//! SMILES over HTTP: the entry adapter connects the codec and reports its errors with the
//! same envelope as every operation.

use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use chemifree_api::{ApiState, LocalApplication, SESSION_HEADER, router};
use chemifree_providers::smiles::LocalSmilesCodec;
use chemifree_providers::{FileDocumentRepository, FilePreferenceStore, LocalCapabilities};
use serde_json::{Value, json};
use tower::ServiceExt;

async fn operation(parameters: Value, operation: &str) -> (StatusCode, Value) {
    let data = tempfile::tempdir().unwrap();
    let application =
        LocalApplication::new(LocalCapabilities, FileDocumentRepository::new(data.path()))
            .with_preferences(FilePreferenceStore::new(data.path()))
            .with_smiles_codec(LocalSmilesCodec);
    let state = ApiState::new(
        application,
        "t".into(),
        3999,
        &["http://127.0.0.1:5173".into()],
    )
    .unwrap();
    let body = json!({"contract":1,"request_id":"r","profile":"preserve-semantics","operation":operation,"parameters":parameters});
    let request = Request::post("/api/v1/operations")
        .header("host", "127.0.0.1:5173")
        .header("origin", "http://127.0.0.1:5173")
        .header("sec-fetch-site", "same-origin")
        .header("content-type", "application/json")
        .header(SESSION_HEADER, "t")
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = router(Arc::new(state)).oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1 << 20).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

#[tokio::test]
async fn smiles_syntax_errors_carry_their_byte_range_over_http() {
    let (status, reply) = operation(
        json!({"format":"smiles","document_id":"doc","expected_revision":0,"source":"C=(O)","preview":true}),
        "document.import",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reply["diagnostics"][0]["code"], "dangling_bond", "{reply}");
    assert_eq!(reply["diagnostics"][0]["details"]["byte_start"], 1);
    assert_eq!(reply["diagnostics"][0]["details"]["byte_end_exclusive"], 2);
}

#[tokio::test]
async fn a_missing_document_is_reported_by_the_smiles_route_not_as_an_unsupported_format() {
    let (status, reply) = operation(
        json!({"format":"smiles","document_id":"missing","style":"recorded"}),
        "document.export",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reply["diagnostics"][0]["code"], "document_not_found", "{reply}");
}
