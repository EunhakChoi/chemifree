//! The HTTP transaction adapter validates two explicit graphic points without changing shape.

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
                .header(SESSION_HEADER, "points-token")
                .body(Body::from(value.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1 << 20).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn point_update_replays_over_http_and_wrong_targets_or_coordinates_leave_no_edits() {
    let root = tempfile::tempdir().unwrap();
    let app = LocalApplication::new(LocalCapabilities, FileDocumentRepository::new(root.path()));
    let api = router(Arc::new(
        ApiState::new(
            app,
            "points-token".into(),
            3999,
            &["http://127.0.0.1:5173".into()],
        )
        .unwrap(),
    ));
    let (_, created) = post(
        &api,
        "/api/v1/documents",
        json!({"request_id":"create-points"}),
    )
    .await;
    let id = created["document"]["id"].as_str().unwrap();
    let path = format!("/api/v1/documents/{id}/transactions");
    let (_,drawn)=post(&api,&path,json!({"request_id":"draw-points","expected_revision":0,"commands":[{"op":"add_graphic","shape":"rectangle","points":[{"x":0,"y":0},{"x":20,"y":10}]}]})).await;
    let new_points = json!([{"x":20.0,"y":10.0},{"x":-100000.0,"y":100000.0}]);
    let update = json!({"request_id":"update-points","expected_revision":1,"commands":[{"op":"update_graphic_points","object":"s2","points":new_points}]});
    let (status, edited) = post(&api, &path, update.clone()).await;
    assert_eq!(status, StatusCode::OK, "{edited}");
    assert_eq!(post(&api, &path, update).await.1, edited);
    let graphic = &edited["document"]["scene"]["objects"][1];
    assert_eq!(graphic["shape"], "rectangle");
    assert_eq!(graphic["points"], new_points);
    assert_eq!(edited["document"]["atoms"], drawn["document"]["atoms"]);
    for (index, (object, points, reason)) in [
        (
            "s1",
            json!([{"x":1,"y":1},{"x":2,"y":2}]),
            Some("object_kind_mismatch"),
        ),
        (
            "s99",
            json!([{"x":1,"y":1},{"x":2,"y":2}]),
            Some("object_unknown"),
        ),
        ("s2", json!([{"x":1,"y":1},{"x":100001,"y":2}]), None),
    ]
    .into_iter()
    .enumerate()
    {
        let (status,error)=post(&api,&path,json!({"request_id":format!("invalid-{index}"),"expected_revision":2,"commands":[{"op":"rename","title":"rollback"},{"op":"update_graphic_points","object":object,"points":points}]})).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(error["code"], "invalid_command");
        if let Some(reason) = reason {
            assert_eq!(error["reason"], reason);
        }
    }
    let (status,unchanged)=post(&api,&path,json!({"request_id":"noop-points","expected_revision":2,"commands":[{"op":"update_graphic_points","object":"s2","points":new_points}]})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(unchanged["document"], edited["document"]);
}
