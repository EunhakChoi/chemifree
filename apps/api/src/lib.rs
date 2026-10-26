//! Read-only HTTP entry adapter. Wiring lives here, outside the core crates.

use std::sync::Arc;

use axum::{Json, Router, extract::State, http::StatusCode, routing::get};
use chemifree_application::{API_VERSION, Application, CapabilitiesResponse, HealthResponse};
use chemifree_providers::ScaffoldCapabilities;
use serde::Serialize;

type LocalApplication = Arc<Application<ScaffoldCapabilities>>;

pub fn router() -> Router {
    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/capabilities", get(capabilities))
        .fallback(not_found)
        .method_not_allowed_fallback(method_not_allowed)
        .with_state(Arc::new(Application::new(ScaffoldCapabilities)))
}

async fn health(State(application): State<LocalApplication>) -> Json<HealthResponse> {
    Json(application.health())
}

async fn capabilities(State(application): State<LocalApplication>) -> Json<CapabilitiesResponse> {
    Json(application.capabilities())
}

#[derive(Serialize)]
struct ErrorResponse {
    api_version: &'static str,
    code: &'static str,
    message: &'static str,
}

async fn not_found() -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::NOT_FOUND,
        Json(ErrorResponse {
            api_version: API_VERSION,
            code: "not_found",
            message: "This route is not implemented. See /api/v1/capabilities.",
        }),
    )
}

async fn method_not_allowed() -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::METHOD_NOT_ALLOWED,
        Json(ErrorResponse {
            api_version: API_VERSION,
            code: "method_not_allowed",
            message: "This scaffold exposes read-only GET endpoints.",
        }),
    )
}

#[cfg(test)]
mod tests {
    use axum::{body::Body, body::to_bytes, http::Request};
    use tower::ServiceExt;

    use super::*;

    #[tokio::test]
    async fn routes_serialize_the_shared_application_contract() {
        let response = router()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/capabilities")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 16_384).await.unwrap();
        let actual: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let expected =
            serde_json::to_value(Application::new(ScaffoldCapabilities).capabilities()).unwrap();
        assert_eq!(actual, expected);
    }

    #[tokio::test]
    async fn unknown_routes_and_mutation_methods_are_explicit_errors() {
        for (method, uri, status, code) in [
            ("POST", "/api/v1/capabilities", 405, "method_not_allowed"),
            ("GET", "/api/v1/documents", 404, "not_found"),
            ("GET", "/api/v2/health", 404, "not_found"),
        ] {
            let response = router()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(uri)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status().as_u16(), status);
            let body = to_bytes(response.into_body(), 16_384).await.unwrap();
            let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(body["code"], code);
            assert_eq!(body["api_version"], API_VERSION);
        }
    }
}
