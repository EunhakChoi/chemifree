//! HTTP uses real PNG validation and common immutable image geometry.

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
                .header(SESSION_HEADER, "image-token")
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

const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DQAAAEgQGALFXOsAAAAABJRU5ErkJggg==";
#[tokio::test]
async fn png_http_replay_geometry_and_explicit_transform_and_clipboard_refusals() {
    let root = tempfile::tempdir().unwrap();
    let app = LocalApplication::new(LocalCapabilities, FileDocumentRepository::new(root.path()))
        .with_image_decoder(chemifree_providers::png_image::EmbeddedPngDecoder);
    let api = router(Arc::new(
        ApiState::new(
            app,
            "image-token".into(),
            3999,
            &["http://127.0.0.1:5173".into()],
        )
        .unwrap(),
    ));
    let (_, created) = post(&api, "/api/v1/documents", json!({"request_id":"create"})).await;
    let id = created["document"]["id"].as_str().unwrap();
    let path = format!("/api/v1/documents/{id}/transactions");
    let add = json!({"request_id":"add","expected_revision":0,"commands":[{"op":"add_image","mime_type":"image/png","data_base64":PNG,"origin":{"x":1,"y":2},"size_pt":{"x":30,"y":40}}]});
    let (status, drawn) = post(&api, &path, add.clone()).await;
    assert_eq!(status, StatusCode::OK, "{drawn}");
    assert_eq!(post(&api, &path, add).await.1, drawn);
    assert_eq!(drawn["document"]["scene"]["objects"][1]["data_base64"], PNG);
    assert_eq!(drawn["document"]["scene"]["objects"][1]["pixel_height"], 1);
    let(status,edited)=post(&api,&path,json!({"request_id":"geometry","expected_revision":1,"commands":[{"op":"update_image_geometry","object":"s2","origin":{"x":5,"y":6},"size_pt":{"x":7,"y":8}}]})).await;
    assert_eq!(status, StatusCode::OK, "{edited}");
    assert_eq!(
        edited["document"]["scene"]["objects"][1]["origin"],
        json!({"x":5.,"y":6.})
    );
    assert_eq!(
        edited["document"]["scene"]["objects"][1]["data_base64"],
        PNG
    );
    let(status,failed)=post(&api,&path,json!({"request_id":"rotate","expected_revision":2,"commands":[{"op":"transform_selection","selection":{"objects":["s2"]},"pivot":{"x":0,"y":0},"transform":{"type":"rotate","degrees":0}}]})).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(failed["reason"], "image_unsupported");
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
    assert_eq!(
        copy["diagnostics"][0]["details"]["reason"],
        "image_clipboard_unsupported"
    );
}
