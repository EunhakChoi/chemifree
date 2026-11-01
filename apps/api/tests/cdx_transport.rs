//! CDX over HTTP: base64 in the operation envelope, bounded by the 256 KiB body limit.

use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use chemifree_api::{ApiState, LocalApplication, MAX_BODY_BYTES, SESSION_HEADER, router};
use chemifree_application::transport_base64;
use chemifree_providers::interchange_codec::LocalInterchangeCodec;
use chemifree_providers::{FileDocumentRepository, FilePreferenceStore, LocalCapabilities};
use serde_json::{Value, json};
use tower::ServiceExt;

/// A CDX file whose only text object holds `length` bytes of unstyled text.
pub fn cdx_with_text(length: usize) -> Vec<u8> {
    let mut bytes = b"VjCD0100".to_vec();
    bytes.extend_from_slice(&[4, 3, 2, 1]);
    bytes.extend_from_slice(&[0; 16]);
    bytes.extend_from_slice(&[0x00, 0x80, 0, 0, 0, 0]);
    bytes.extend_from_slice(&[0x01, 0x80, 1, 0, 0, 0]);
    bytes.extend_from_slice(&[0x06, 0x80, 2, 0, 0, 0]);
    let data_length = (length + 2) as u32;
    bytes.extend_from_slice(&[0x00, 0x07, 0xff, 0xff]);
    bytes.extend_from_slice(&data_length.to_le_bytes());
    bytes.extend_from_slice(&[0, 0]);
    bytes.extend(std::iter::repeat_n(b'a', length));
    bytes.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]);
    bytes
}

async fn operation(source: &str) -> (StatusCode, Value) {
    let data = tempfile::tempdir().unwrap();
    let application =
        LocalApplication::new(LocalCapabilities, FileDocumentRepository::new(data.path()))
            .with_preferences(FilePreferenceStore::new(data.path()))
            .with_interchange_codec(LocalInterchangeCodec);
    let state = ApiState::new(
        application,
        "t".into(),
        3999,
        &["http://127.0.0.1:5173".into()],
    )
    .unwrap();
    let body = json!({"contract":1,"request_id":"r","profile":"preserve-semantics","operation":"document.import","parameters":{"mode":"read","format":"cdx","source":source}});
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
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn a_cdx_that_fits_the_body_limit_is_read() {
    // 190,000 bytes become about 253,400 characters, under the 262,144-byte body limit.
    let (status, reply) = operation(&transport_base64::encode(&cdx_with_text(190_000))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reply["outcome"], "ok", "{reply}");
}

#[tokio::test]
async fn a_cdx_whose_base64_exceeds_the_body_limit_is_refused_before_the_codec() {
    // 200,000 bytes become about 266,700 characters: smaller than the codec's 1 MiB, larger
    // than the HTTP transport's 256 KiB.
    let raw = cdx_with_text(200_000);
    assert!(raw.len() < 1024 * 1024);
    let text = transport_base64::encode(&raw);
    assert!(text.len() > MAX_BODY_BYTES);
    let (status, reply) = operation(&text).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(reply["diagnostics"][0]["code"], "resource_limit", "{reply}");
}
