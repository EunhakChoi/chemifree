//! Clipboard v3 is strict and replay-safe through the authenticated HTTP adapter.

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
                .header(SESSION_HEADER, "v3-token")
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
async fn mixed_v3_paste_preserves_width_null_group_and_refs_and_v2_cannot_smuggle_width() {
    let root = tempfile::tempdir().unwrap();
    let app = LocalApplication::new(LocalCapabilities, FileDocumentRepository::new(root.path()));
    let api = router(Arc::new(
        ApiState::new(
            app,
            "v3-token".into(),
            3999,
            &["http://127.0.0.1:5173".into()],
        )
        .unwrap(),
    ));
    let (_, created) = post(&api, "/api/v1/documents", json!({"request_id":"create"})).await;
    let id = created["document"]["id"].as_str().unwrap();
    let payload = json!({"format":"chemifree.clipboard","version":3,"kind":"graphics","settings":created["document"]["settings"],"roots":["s8"],"objects":[{"id":"s8","kind":"group","children":["s4","s9"]},{"id":"s9","kind":"graphic","shape":"arrow","points":[{"x":20,"y":0},{"x":30,"y":5}],"line_width_pt":null},{"id":"s4","kind":"graphic","shape":"line","points":[{"x":0,"y":0},{"x":10,"y":5}],"line_width_pt":0.1}],"references":[{"source":"s4","target":{"kind":"object","id":"s9"}}]});
    let body = request(
        "paste",
        "document.clipboard_scene_paste",
        json!({"document_id":id,"expected_revision":0,"payload":payload.to_string(),"offset":{"x":50,"y":-20}}),
    );
    let (status, pasted) = post(&api, "/api/v1/operations", body.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(pasted["outcome"], "ok", "{pasted}");
    assert_eq!(post(&api, "/api/v1/operations", body).await.1, pasted);
    let document = &pasted["result"]["snapshot"]["document"];
    let line = document["scene"]["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|object| object["id"] == "s2")
        .unwrap();
    let arrow = document["scene"]["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|object| object["id"] == "s3")
        .unwrap();
    assert_eq!(line["line_width_pt"], 0.1);
    assert_eq!(arrow["line_width_pt"], Value::Null);
    assert_eq!(line["parent"], "s4");
    assert_eq!(
        line["points"],
        json!([{"x":50.0,"y":-20.0},{"x":60.0,"y":-15.0}])
    );
    assert_eq!(
        document["scene"]["references"],
        json!([{"source":"s2","target":{"kind":"object","id":"s3"}}])
    );
    let mut old = payload.clone();
    old["version"] = json!(2);
    let(_,refused)=post(&api,"/api/v1/operations",request("bad-v2","document.clipboard_scene_paste",json!({"document_id":id,"expected_revision":1,"payload":old.to_string(),"offset":{"x":0,"y":0}}))).await;
    assert_eq!(
        refused["diagnostics"][0]["details"]["reason"],
        "clipboard_fragment_invalid"
    );
    let (_, snapshot) = post(
        &api,
        "/api/v1/operations",
        request("snapshot", "document.snapshot", json!({"document_id":id})),
    )
    .await;
    assert_eq!(snapshot["result"]["document"], *document);
}
