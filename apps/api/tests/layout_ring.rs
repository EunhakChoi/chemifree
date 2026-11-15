//! A hexagon over HTTP: its preview names the ring under the layout model, and applying that preview
//! changes nothing, so the revision, the undo history and every position stay as they were.

use std::collections::BTreeMap;
use std::error::Error;
use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use chemifree_api::{ApiState, LocalApplication, SESSION_HEADER, router};
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};
use tower::ServiceExt;

/// The model that every `layout.structure` reply names.
const MODEL_ID: &str = "chemifree-chain-layout-2";

/// Atom IDs mapped to their `[x, y]` in pt.
type Positions = BTreeMap<String, [f64; 2]>;

async fn post(
    api: &Router,
    path: &str,
    value: Value,
) -> Result<(StatusCode, Value), Box<dyn Error>> {
    let request = Request::post(path)
        .header("host", "127.0.0.1:5173")
        .header("origin", "http://127.0.0.1:5173")
        .header("sec-fetch-site", "same-origin")
        .header("content-type", "application/json")
        .header(SESSION_HEADER, "layout-token")
        .body(Body::from(value.to_string()))?;
    let response = api.clone().oneshot(request).await?;
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1 << 20).await?;
    Ok((status, serde_json::from_slice(&bytes)?))
}

fn request(id: &str, operation: &str, parameters: Value) -> Value {
    json!({"contract":1,"request_id":id,"profile":"preserve-semantics","operation":operation,"parameters":parameters})
}

/// Posts one operation request to `/api/v1/operations` and returns its reply.
async fn operate(
    api: &Router,
    id: &str,
    operation: &str,
    parameters: Value,
) -> Result<Value, Box<dyn Error>> {
    let (_, reply) = post(
        api,
        "/api/v1/operations",
        request(id, operation, parameters),
    )
    .await?;
    Ok(reply)
}

/// The atom positions of a document object, by atom ID.
fn positions(document: &Value) -> Result<Positions, Box<dyn Error>> {
    let atoms = document["atoms"]
        .as_array()
        .ok_or("the document has no atom array")?;
    let mut positions = Positions::new();
    for atom in atoms {
        let id = atom["id"].as_str().ok_or("an atom has no id")?;
        let x = atom["position"]["x"].as_f64().ok_or("an atom has no x")?;
        let y = atom["position"]["y"].as_f64().ok_or("an atom has no y")?;
        positions.insert(id.to_owned(), [x, y]);
    }
    Ok(positions)
}

/// The diagnostic codes of a reply, in the order reported.
fn codes(reply: &Value) -> Result<Vec<&str>, Box<dyn Error>> {
    let diagnostics = reply["diagnostics"]
        .as_array()
        .ok_or("the reply has no diagnostic array")?;
    let mut codes: Vec<&str> = Vec::new();
    for diagnostic in diagnostics {
        codes.push(
            diagnostic["code"]
                .as_str()
                .ok_or("a diagnostic has no code")?,
        );
    }
    Ok(codes)
}

#[tokio::test]
async fn http_hexagon_preview_names_its_ring_and_apply_changes_nothing()
-> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let app = LocalApplication::new(LocalCapabilities, FileDocumentRepository::new(root.path()));
    let origins = ["http://127.0.0.1:5173".to_owned()];
    let state = ApiState::new(app, "layout-token".into(), 3999, &origins)?;
    let api = router(Arc::new(state));
    let (_, created) = post(&api, "/api/v1/documents", json!({"request_id": "create"})).await?;
    let id = created["document"]["id"]
        .as_str()
        .ok_or("the created document has no id")?;
    let path = format!("/api/v1/documents/{id}/transactions");
    let hexagon = json!({"request_id": "hexagon", "expected_revision": 0, "commands": [
        {"op": "insert_ring", "instance": "s1", "center": {"x": 0, "y": 0}}
    ]});
    let (status, inserted) = post(&api, &path, hexagon).await?;
    assert_eq!(status, StatusCode::OK, "{inserted}");
    let before = positions(&inserted["document"])?;
    assert_eq!(before.len(), 6, "the hexagon has six atoms: {inserted}");

    let preview = operate(
        &api,
        "preview",
        "layout.structure",
        json!({"document_id": id, "mode": "preview", "selection": {"atoms": ["a1"]}}),
    )
    .await?;
    assert_eq!(preview["outcome"], "ok", "{preview}");
    assert_eq!(preview["model_id"], MODEL_ID, "{preview}");
    assert_eq!(preview["document_revision"], 1, "{preview}");
    let components = &preview["result"]["components"];
    assert_eq!(components.as_array().map(Vec::len), Some(1), "{preview}");
    assert_eq!(
        components[0]["rings"],
        json!([{"atoms": ["a1", "a2", "a3", "a4", "a5", "a6"], "branch_from": null}]),
        "{preview}"
    );
    assert_eq!(components[0]["chains"], json!([]), "{preview}");

    let apply = json!({"document_id": id, "mode": "apply", "expected_revision": 1, "selection": {"atoms": ["a1"]}});
    let applied = operate(&api, "apply", "layout.structure", apply).await?;
    assert_eq!(applied["outcome"], "ok", "{applied}");
    assert_eq!(applied["document_revision"], 1, "{applied}");
    assert_eq!(
        codes(&applied)?,
        ["layout_unchanged", "layout_not_implemented"],
        "{applied}"
    );
    assert_eq!(applied["result"]["session"]["undo_depth"], 1, "{applied}");

    let snapshot = operate(
        &api,
        "read",
        "document.snapshot",
        json!({"document_id": id}),
    )
    .await?;
    assert_eq!(
        positions(&snapshot["result"]["document"])?,
        before,
        "{snapshot}"
    );
    Ok(())
}
