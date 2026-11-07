//! HTTP exposes ellipse creation, atomic axis-aligned transform refusal and clipboard v4.

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
                .header(SESSION_HEADER, "ellipse-token")
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
async fn ellipse_http_replay_quarter_turn_and_v4_copy_preserve_graph_and_reject_arbitrary_rotation()
{
    let root = tempfile::tempdir().unwrap();
    let app = LocalApplication::new(LocalCapabilities, FileDocumentRepository::new(root.path()));
    let api = router(Arc::new(
        ApiState::new(
            app,
            "ellipse-token".into(),
            3999,
            &["http://127.0.0.1:5173".into()],
        )
        .unwrap(),
    ));
    let (_, created) = post(&api, "/api/v1/documents", json!({"request_id":"create"})).await;
    let id = created["document"]["id"].as_str().unwrap();
    let path = format!("/api/v1/documents/{id}/transactions");
    let draw = json!({"request_id":"draw","expected_revision":0,"commands":[{"op":"add_atom","element":"C","position":{"x":2,"y":3}},{"op":"add_graphic","shape":"ellipse","points":[{"x":-4,"y":-2},{"x":6,"y":8}]},{"op":"set_graphic_line_width","object":"s2","line_width_pt":2.75}]});
    let (status, drawn) = post(&api, &path, draw.clone()).await;
    assert_eq!(status, StatusCode::OK, "{drawn}");
    assert_eq!(post(&api, &path, draw).await.1, drawn);
    assert_eq!(drawn["document"]["scene"]["objects"][1]["shape"], "ellipse");
    let(status,failed)=post(&api,&path,json!({"request_id":"bad-angle","expected_revision":1,"commands":[{"op":"rename","title":"rollback"},{"op":"transform_selection","selection":{"objects":["s2"],"atoms":["a1"]},"pivot":{"x":0,"y":0},"transform":{"type":"rotate","degrees":37}}]})).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(failed["reason"], "transform_ellipse_unsupported");
    let(status,rotated)=post(&api,&path,json!({"request_id":"quarter","expected_revision":1,"commands":[{"op":"transform_selection","selection":{"objects":["s2"]},"pivot":{"x":0,"y":0},"transform":{"type":"rotate","degrees":90}}]})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(rotated["document"]["atoms"], drawn["document"]["atoms"]);
    assert_eq!(rotated["document"]["title"], drawn["document"]["title"]);
    assert_eq!(
        rotated["document"]["scene"]["objects"][1]["points"],
        json!([{"x":2.0,"y":-4.0},{"x":-8.0,"y":6.0}])
    );
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
    assert_eq!(copy["outcome"], "ok", "{copy}");
    let mut payload: Value =
        serde_json::from_str(copy["result"]["payload"].as_str().unwrap()).unwrap();
    assert_eq!(payload["version"], 4);
    assert_eq!(payload["objects"][0]["shape"], "ellipse");
    assert_eq!(payload["objects"][0]["line_width_pt"], 2.75);
    payload["version"] = json!(3);
    let(_,refused)=post(&api,"/api/v1/operations",request("old-profile","document.clipboard_scene_paste",json!({"document_id":id,"expected_revision":2,"payload":payload.to_string(),"offset":{"x":0,"y":0}}))).await;
    assert_eq!(
        refused["diagnostics"][0]["details"]["reason"],
        "clipboard_fragment_invalid"
    );
}
