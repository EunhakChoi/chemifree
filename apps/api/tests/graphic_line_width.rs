//! The HTTP transaction adapter exposes nullable graphic width with ordinary replay.

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
                .header(SESSION_HEADER, "width-token")
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
async fn three_shapes_accept_bounded_overrides_replay_and_restore_inheritance_over_http() {
    let root = tempfile::tempdir().unwrap();
    let app = LocalApplication::new(LocalCapabilities, FileDocumentRepository::new(root.path()));
    let api = router(Arc::new(
        ApiState::new(
            app,
            "width-token".into(),
            3999,
            &["http://127.0.0.1:5173".into()],
        )
        .unwrap(),
    ));
    let (_, created) = post(
        &api,
        "/api/v1/documents",
        json!({"request_id":"create-width"}),
    )
    .await;
    let id = created["document"]["id"].as_str().unwrap();
    let path = format!("/api/v1/documents/{id}/transactions");
    let(_,drawn)=post(&api,&path,json!({"request_id":"draw-width","expected_revision":0,"commands":[{"op":"add_graphic","shape":"line","points":[{"x":0,"y":0},{"x":20,"y":10}]},{"op":"add_graphic","shape":"arrow","points":[{"x":0,"y":0},{"x":20,"y":10}]},{"op":"add_graphic","shape":"rectangle","points":[{"x":0,"y":0},{"x":20,"y":10}]}]})).await;
    let update = json!({"request_id":"override-width","expected_revision":1,"commands":[{"op":"set_graphic_line_width","object":"s2","line_width_pt":0.1},{"op":"set_graphic_line_width","object":"s3","line_width_pt":10.0},{"op":"set_graphic_line_width","object":"s4","line_width_pt":1.2}]});
    let (status, edited) = post(&api, &path, update.clone()).await;
    assert_eq!(status, StatusCode::OK, "{edited}");
    assert_eq!(post(&api, &path, update).await.1, edited);
    for (index, width) in [(1, 0.1), (2, 10.0), (3, 1.2)] {
        let object = &edited["document"]["scene"]["objects"][index];
        assert_eq!(object["line_width_pt"], width);
        assert_eq!(
            object["shape"],
            drawn["document"]["scene"]["objects"][index]["shape"]
        );
        assert_eq!(
            object["points"],
            drawn["document"]["scene"]["objects"][index]["points"]
        );
    }
    let(status,error)=post(&api,&path,json!({"request_id":"invalid-width","expected_revision":2,"commands":[{"op":"rename","title":"rollback"},{"op":"set_graphic_line_width","object":"s2","line_width_pt":0.099}]})).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(error["reason"], "line_width_out_of_range");
    let(status,reset)=post(&api,&path,json!({"request_id":"inherit-width","expected_revision":2,"commands":[{"op":"set_graphic_line_width","object":"s2","line_width_pt":null}]})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        reset["document"]["scene"]["objects"][1]["line_width_pt"],
        Value::Null
    );
    assert_eq!(reset["document"]["title"], drawn["document"]["title"]);
}
