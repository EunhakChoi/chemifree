//! HTTP reaches the same `layout.structure` apply: the preview's positions are committed as one
//! undoable edit, undo restores the positions exactly, and a stale revision is refused unchanged.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use chemifree_api::{ApiState, LocalApplication, SESSION_HEADER, router};
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};
use tower::ServiceExt;

/// Atom IDs mapped to their `[x, y]` in pt.
type Positions = BTreeMap<String, [f64; 2]>;

/// The worked example of the applied butane, rounded to 2 decimals, so checked to 0.01 pt.
const WORKED: [(&str, [f64; 2]); 4] = [
    ("a1", [6.03, 7.5]),
    ("a2", [32.01, -7.5]),
    ("a3", [57.99, 7.5]),
    ("a4", [83.97, -7.5]),
];

async fn post(api: &Router, path: &str, value: Value) -> (StatusCode, Value) {
    let request = Request::post(path)
        .header("host", "127.0.0.1:5173")
        .header("origin", "http://127.0.0.1:5173")
        .header("sec-fetch-site", "same-origin")
        .header("content-type", "application/json")
        .header(SESSION_HEADER, "layout-token")
        .body(Body::from(value.to_string()))
        .unwrap();
    let response = api.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1 << 20).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

fn request(id: &str, operation: &str, parameters: Value) -> Value {
    json!({"contract":1,"request_id":id,"profile":"preserve-semantics","operation":operation,"parameters":parameters})
}

/// Posts one operation request to `/api/v1/operations` and returns its reply.
async fn operate(api: &Router, id: &str, operation: &str, parameters: Value) -> Value {
    let path = "/api/v1/operations";
    let (_, reply) = post(api, path, request(id, operation, parameters)).await;
    reply
}

/// Carbons a1 to a4 in a row, 30 pt apart, each bonded to the one before it.
fn butane() -> Value {
    json!([
        {"op":"add_atom","element":"C","position":{"x":0,"y":0}},
        {"op":"add_atom","element":"C","position":{"x":30,"y":0},"bond_to":{"atom":"$0","order":"single"}},
        {"op":"add_atom","element":"C","position":{"x":60,"y":0},"bond_to":{"atom":"$1","order":"single"}},
        {"op":"add_atom","element":"C","position":{"x":90,"y":0},"bond_to":{"atom":"$2","order":"single"}}
    ])
}

/// The `[x, y]` of a coordinate object.
fn point(value: &Value) -> [f64; 2] {
    [value["x"].as_f64().unwrap(), value["y"].as_f64().unwrap()]
}

/// The atom positions of a document object, by atom ID.
fn positions(document: &Value) -> Positions {
    let mut positions = Positions::new();
    for atom in document["atoms"].as_array().unwrap() {
        let id = atom["id"].as_str().unwrap();
        positions.insert(id.to_owned(), point(&atom["position"]));
    }
    positions
}

/// Asserts that each atom of `expected` lies within `tolerance` pt of its position in `actual`.
fn assert_near(actual: &Positions, expected: &[(&str, [f64; 2])], tolerance: f64, label: &str) {
    for &(atom, [x, y]) in expected {
        let [actual_x, actual_y] = actual
            .get(atom)
            .unwrap_or_else(|| panic!("{label}: {atom} has no position"));
        assert!(
            (actual_x - x).abs() <= tolerance && (actual_y - y).abs() <= tolerance,
            "{label}: {atom} is at ({actual_x}, {actual_y}), expected ({x}, {y})"
        );
    }
}

#[tokio::test]
async fn http_layout_apply_commits_undoes_exactly_and_refuses_a_stale_revision() {
    let root = tempfile::tempdir().unwrap();
    let app = LocalApplication::new(LocalCapabilities, FileDocumentRepository::new(root.path()));
    let origins = ["http://127.0.0.1:5173".to_owned()];
    let state = ApiState::new(app, "layout-token".into(), 3999, &origins).unwrap();
    let api = router(Arc::new(state));
    let (_, created) = post(&api, "/api/v1/documents", json!({"request_id": "create"})).await;
    let id = created["document"]["id"].as_str().unwrap();
    let path = format!("/api/v1/documents/{id}/transactions");
    let build = json!({"request_id": "butane", "expected_revision": 0, "commands": butane()});
    let (status, built) = post(&api, &path, build).await;
    assert_eq!(status, StatusCode::OK, "{built}");
    let before = positions(&built["document"]);

    let preview = operate(
        &api,
        "preview",
        "layout.structure",
        json!({"document_id": id, "mode": "preview", "selection": {"atoms": ["a2"]}}),
    )
    .await;
    assert_eq!(preview["outcome"], "ok", "{preview}");
    let proposed: Vec<(&str, [f64; 2])> = preview["result"]["positions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| (entry["atom"].as_str().unwrap(), point(&entry["after"])))
        .collect();
    assert_eq!(proposed.len(), 4, "{preview}");

    let apply = json!({"document_id": id, "mode": "apply", "expected_revision": 1, "selection": {"atoms": ["a2"]}});
    let applied = operate(&api, "apply", "layout.structure", apply).await;
    assert_eq!(applied["outcome"], "ok", "{applied}");
    assert_eq!(applied["document_revision"], 2, "{applied}");
    let after = positions(&applied["result"]["document"]);
    assert_near(&after, &proposed, 1e-9, "the preview");
    assert_near(&after, &WORKED, 0.01, "the worked example");

    let undo = json!({"document_id": id, "expected_revision": 2});
    let undone = operate(&api, "undo", "history.undo", undo).await;
    assert_eq!(undone["outcome"], "ok", "{undone}");
    assert_eq!(undone["document_revision"], 3, "{undone}");
    let restored = positions(&undone["result"]["document"]);
    assert_eq!(restored, before, "{undone}");

    // A client that still holds revision 2 is refused, and the positions stay as undo left them.
    let stale = json!({"document_id": id, "mode": "apply", "expected_revision": 2, "selection": {"atoms": ["a2"]}});
    let refused = operate(&api, "stale", "layout.structure", stale).await;
    assert_eq!(refused["outcome"], "error", "{refused}");
    assert_eq!(
        refused["diagnostics"][0]["code"], "revision_conflict",
        "{refused}"
    );
    assert_eq!(refused["document_revision"], 3, "{refused}");
    let snapshot = operate(
        &api,
        "read",
        "document.snapshot",
        json!({"document_id": id}),
    )
    .await;
    let now = positions(&snapshot["result"]["document"]);
    assert_eq!(now, before, "{snapshot}");
}
