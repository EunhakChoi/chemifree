//! HTTP uses the common polygon edit, replay and strict clipboard profile.

use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use chemifree_api::{ApiState, LocalApplication, SESSION_HEADER, router};
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};
use tower::ServiceExt;

async fn post(api: &Router, path: &str, value: Value) -> (StatusCode, Value) {
    let response = api
        .clone()
        .oneshot(
            Request::post(path)
                .header("host", "127.0.0.1:5173")
                .header("origin", "http://127.0.0.1:5173")
                .header("sec-fetch-site", "same-origin")
                .header("content-type", "application/json")
                .header(SESSION_HEADER, "polygon-token")
                .body(Body::from(value.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1 << 20).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}
fn request(id: &str, operation: &str, parameters: Value) -> Value {
    json!({"contract":1,"request_id":id,"profile":"preserve-semantics","operation":operation,"parameters":parameters})
}

#[tokio::test]
async fn polygon_http_edits_exact_vertices_replays_and_refuses_cardinality_atomically() {
    let root = tempfile::tempdir().unwrap();
    let app = LocalApplication::new(LocalCapabilities, FileDocumentRepository::new(root.path()));
    let api = router(Arc::new(
        ApiState::new(
            app,
            "polygon-token".into(),
            3999,
            &["http://127.0.0.1:5173".into()],
        )
        .unwrap(),
    ));
    let (_, created) = post(&api, "/api/v1/documents", json!({"request_id":"create"})).await;
    let id = created["document"]["id"].as_str().unwrap();
    let path = format!("/api/v1/documents/{id}/transactions");
    let vertices = json!([{"x":0.,"y":0.},{"x":1.,"y":2.},{"x":-5.,"y":9.}]);
    let draw = json!({"request_id":"draw","expected_revision":0,"commands":[{"op":"add_polygon","vertices":vertices}]});
    let (status, drawn) = post(&api, &path, draw.clone()).await;
    assert_eq!(status, StatusCode::OK, "{drawn}");
    assert_eq!(post(&api, &path, draw).await.1, drawn);
    assert_eq!(drawn["document"]["scene"]["objects"][1]["points"], vertices);
    let(status,refused)=post(&api,&path,json!({"request_id":"bad","expected_revision":1,"commands":[{"op":"rename","title":"rollback"},{"op":"update_polygon_vertices","object":"s2","vertices":[]}]})).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(refused["reason"], "graphic_geometry_invalid");
    let reversed = json!([{"x":-5.,"y":9.},{"x":1.,"y":2.},{"x":0.,"y":0.}]);
    let(status,edited)=post(&api,&path,json!({"request_id":"update","expected_revision":1,"commands":[{"op":"update_polygon_vertices","object":"s2","vertices":reversed}]})).await;
    assert_eq!(status, StatusCode::OK, "{edited}");
    assert_eq!(
        edited["document"]["scene"]["objects"][1]["points"],
        reversed
    );
    assert_eq!(edited["document"]["title"], drawn["document"]["title"]);
    let (_, copy) = post(
        &api,
        "/api/v1/operations",
        request(
            "copy",
            "document.clipboard_scene_copy",
            json!({"document_id":id,"selection":{"objects":["s2"]}}),
        ),
    )
    .await;
    assert_eq!(copy["outcome"], "ok");
    let payload: Value = serde_json::from_str(copy["result"]["payload"].as_str().unwrap()).unwrap();
    assert_eq!(payload["version"], 5);
    assert_eq!(payload["objects"][0]["points"], reversed);
    assert_eq!(payload["objects"][0]["line_width_pt"], Value::Null);
}
