//! The authenticated HTTP boundary shares explicit fragment membership and atomic refusals.

use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use chemifree_api::{ApiState, LocalApplication, SESSION_HEADER, router};
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};
use tower::ServiceExt;

async fn post(api: &Router, path: &str, value: Value) -> (StatusCode, Value) {
    let request = Request::post(path)
        .header("host", "127.0.0.1:5173")
        .header("origin", "http://127.0.0.1:5173")
        .header("sec-fetch-site", "same-origin")
        .header("content-type", "application/json")
        .header(SESSION_HEADER, "fragment-token")
        .body(Body::from(value.to_string()))
        .unwrap();
    let response = api.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1 << 20).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn explicit_branched_unicode_display_replay_and_refusal_keep_the_graph_over_http() {
    let root = tempfile::tempdir().unwrap();
    let app = LocalApplication::new(LocalCapabilities, FileDocumentRepository::new(root.path()));
    let state = ApiState::new(
        app,
        "fragment-token".into(),
        3999,
        &["http://127.0.0.1:5173".into()],
    )
    .unwrap();
    let api = router(Arc::new(state));
    let (status, created) = post(
        &api,
        "/api/v1/documents",
        json!({"request_id":"create-fragment"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = created["document"]["id"].as_str().unwrap();
    let path = format!("/api/v1/documents/{id}/transactions");
    let (_,drawn)=post(&api,&path,json!({"request_id":"draw-fragment","expected_revision":0,"commands":[
        {"op":"add_atom","element":"C","position":{"x":0,"y":0}},
        {"op":"add_atom","element":"N","position":{"x":30,"y":0},"bond_to":{"atom":"$0","order":"single"}},
        {"op":"add_atom","element":"O","position":{"x":45,"y":25},"bond_to":{"atom":"$1","order":"single"}},
        {"op":"add_atom","element":"C","position":{"x":45,"y":-25},"bond_to":{"atom":"$1","order":"single"}}
    ]})).await;
    let parameters = json!({"request_id":"contract-fragment","expected_revision":1,"commands":[{"op":"contract_fragment","instance":"s1","anchor":"a2","members":["a4","a2","a3"],"text":"한 e\u{301} 🧪"}]});
    let (status, contracted) = post(&api, &path, parameters.clone()).await;
    assert_eq!(status, StatusCode::OK, "{contracted}");
    assert_eq!(post(&api, &path, parameters).await.1, contracted);
    assert_eq!(
        contracted["document"]["fragment_displays"],
        json!([{"instance":"s1","anchor":"a2","members":["a2","a3","a4"],"text":"한 e\u{301} 🧪","runs":[{"text":"한 e\u{301} 🧪","script":"baseline"}]}])
    );
    for field in ["atoms", "bonds", "scene", "stereo", "settings"] {
        assert_eq!(contracted["document"][field], drawn["document"][field]);
    }
    let (status,invalid)=post(&api,&path,json!({"request_id":"bad-fragment","expected_revision":2,"commands":[{"op":"rename","title":"rollback"},{"op":"contract_fragment","instance":"s1","anchor":"a3","members":["a3"],"text":"nested"}]})).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(invalid["reason"], "fragment_display_invalid", "{invalid}");
    let (status,expanded)=post(&api,&path,json!({"request_id":"expand-fragment","expected_revision":2,"commands":[{"op":"expand_fragment","instance":"s1","anchor":"a2"}]})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(expanded["document"]["title"], drawn["document"]["title"]);
    assert_eq!(expanded["document"]["fragment_displays"], json!([]));
    assert_eq!(expanded["document"]["atoms"], drawn["document"]["atoms"]);
}
