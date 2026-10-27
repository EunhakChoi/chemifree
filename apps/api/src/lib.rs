//! Loopback HTTP entry adapter. Wiring and transport guards live here, outside the core crates.
//!
//! Guards, in order:
//! 1. Every request must name an allowed `Host` (DNS-rebinding defense).
//! 2. `GET /api/v1/session` and all writes reject cross-site browser requests by `Origin`
//!    and `Sec-Fetch-Site`. No CORS headers are ever sent.
//! 3. Writes must carry the per-process session token in `x-chemifree-session`, a
//!    `Content-Type: application/json` body, and at most [`MAX_BODY_BYTES`].
//!
//! This keeps other websites from driving the local API. It is not user authentication:
//! any local process that can reach the loopback port can obtain the token.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use axum::extract::rejection::JsonRejection;
use axum::extract::{DefaultBodyLimit, FromRequest, Path, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chemifree_application::{
    API_VERSION, Application, ApplicationError, CapabilitiesResponse, CloseRequest, CloseResponse,
    CopyResponse, CreateDocumentRequest, DocumentListResponse, DocumentResponse,
    DraftDiscardResponse, DraftRequest, ErrorCode, ErrorResponse, HealthResponse,
    RecoverAsCopyRequest, RevisionRequest, SaveAsRequest, TransactionRequest,
};
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde::Serialize;

pub type LocalApplication = Application<LocalCapabilities, FileDocumentRepository>;

pub const SESSION_HEADER: &str = "x-chemifree-session";
pub const MAX_BODY_BYTES: usize = 256 * 1024;
/// Development (Vite) and preview origins allowed when none are configured.
pub const DEFAULT_WEB_ORIGINS: [&str; 4] = [
    "http://127.0.0.1:5173",
    "http://localhost:5173",
    "http://127.0.0.1:4173",
    "http://localhost:4173",
];

pub struct ApiState {
    application: LocalApplication,
    token: String,
    allowed_hosts: HashSet<String>,
    allowed_origins: HashSet<String>,
}

impl ApiState {
    /// `port` is the bound API port; requests addressed to it directly are always allowed.
    /// Each origin must be a loopback `http(s)://host[:port]` without a path.
    pub fn new(
        application: LocalApplication,
        token: String,
        port: u16,
        web_origins: &[String],
    ) -> Result<Self, String> {
        let mut allowed_hosts = HashSet::new();
        let mut allowed_origins = HashSet::new();
        for host in ["127.0.0.1", "localhost"] {
            allowed_hosts.insert(format!("{host}:{port}"));
            allowed_origins.insert(format!("http://{host}:{port}"));
        }
        for origin in web_origins {
            let origin = origin.to_ascii_lowercase();
            let authority = loopback_authority(&origin)
                .ok_or_else(|| format!("{origin:?} is not a loopback http(s) origin"))?;
            allowed_hosts.insert(authority.to_owned());
            allowed_origins.insert(origin);
        }
        Ok(Self {
            application,
            token,
            allowed_hosts,
            allowed_origins,
        })
    }
}

fn loopback_authority(origin: &str) -> Option<&str> {
    let authority = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))?;
    let is_loopback = ["127.0.0.1", "localhost", "[::1]"].iter().any(|host| {
        authority.strip_prefix(host).is_some_and(|rest| {
            rest.is_empty()
                || rest.strip_prefix(':').is_some_and(|port| {
                    port.bytes().all(|b| b.is_ascii_digit()) && port.parse::<u16>().is_ok()
                })
        })
    });
    is_loopback.then_some(authority)
}

type Shared = Arc<ApiState>;

pub fn router(state: Shared) -> Router {
    let writes = Router::new()
        .route("/api/v1/documents", post(create_document))
        .route(
            "/api/v1/documents/{id}/transactions",
            post(apply_transaction),
        )
        .route("/api/v1/documents/{id}/undo", post(undo))
        .route("/api/v1/documents/{id}/redo", post(redo))
        .route("/api/v1/documents/{id}/save", post(save))
        .route("/api/v1/documents/{id}/close", post(close))
        .route("/api/v1/documents/{id}/autosave", post(autosave))
        .route("/api/v1/documents/{id}/save-as", post(save_as))
        .route("/api/v1/documents/{id}/recover", post(recover))
        .route(
            "/api/v1/documents/{id}/recover-as-copy",
            post(recover_as_copy),
        )
        .route("/api/v1/documents/{id}/discard-draft", post(discard_draft))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_token))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            same_site_only,
        ));
    let session = Router::new()
        .route("/api/v1/session", get(session))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            same_site_only,
        ));
    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/capabilities", get(capabilities))
        .route("/api/v1/documents", get(list_documents))
        .route("/api/v1/documents/{id}", get(document))
        .merge(writes)
        .merge(session)
        .fallback(not_found)
        .method_not_allowed_fallback(method_not_allowed)
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(middleware::from_fn_with_state(state.clone(), allowed_host))
        .with_state(state)
}

/// Adds one line per request on standard error: `request <unix ms> <method> <path> <status>
/// <duration>ms`, written when the response is ready. For diagnosing local runs; bodies, query
/// strings, and headers (including the write token) are never printed.
pub fn log_requests(router: Router) -> Router {
    router.layer(middleware::from_fn(log_request))
}

async fn log_request(request: Request, next: Next) -> Response {
    let started = Instant::now();
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let response = next.run(request).await;
    let at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis());
    eprintln!(
        "request {at} {method} {path} {} {}ms",
        response.status().as_u16(),
        started.elapsed().as_millis()
    );
    response
}

/// An HTTP error with the v1 envelope.
pub struct ApiError(StatusCode, ErrorResponse);

impl ApiError {
    fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self(
            status,
            ErrorResponse {
                api_version: API_VERSION,
                code,
                message: message.into(),
                current_revision: None,
                command_index: None,
            },
        )
    }
}

impl From<ApplicationError> for ApiError {
    fn from(error: ApplicationError) -> Self {
        let status = match error.code {
            ErrorCode::InvalidRequest | ErrorCode::InvalidCommand => {
                StatusCode::UNPROCESSABLE_ENTITY
            }
            ErrorCode::DocumentNotFound | ErrorCode::DraftNotFound => StatusCode::NOT_FOUND,
            ErrorCode::RevisionConflict
            | ErrorCode::RequestIdReused
            | ErrorCode::NothingToUndo
            | ErrorCode::NothingToRedo
            | ErrorCode::UnsavedChanges
            | ErrorCode::SaveConflict
            | ErrorCode::RecoveryPending
            | ErrorCode::DraftInUse
            | ErrorCode::DraftOutdated => StatusCode::CONFLICT,
            ErrorCode::StorageFailed | ErrorCode::ResourceLimit => StatusCode::SERVICE_UNAVAILABLE,
            ErrorCode::DocumentUnreadable => StatusCode::INTERNAL_SERVER_ERROR,
        };
        Self(status, error.to_response())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(self.1)).into_response()
    }
}

/// JSON body extractor whose rejections use the v1 error envelope.
pub struct JsonBody<T>(T);

impl<T, S> FromRequest<S> for JsonBody<T>
where
    Json<T>: FromRequest<S, Rejection = JsonRejection>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        match Json::<T>::from_request(request, state).await {
            Ok(Json(value)) => Ok(Self(value)),
            Err(rejection) => {
                let status = rejection.status();
                let code = match status {
                    StatusCode::BAD_REQUEST => "invalid_json",
                    StatusCode::UNSUPPORTED_MEDIA_TYPE => "unsupported_media_type",
                    StatusCode::PAYLOAD_TOO_LARGE => "payload_too_large",
                    _ => "invalid_request",
                };
                Err(ApiError::new(status, code, rejection.body_text()))
            }
        }
    }
}

async fn allowed_host(State(state): State<Shared>, request: Request, next: Next) -> Response {
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .map(str::to_ascii_lowercase);
    match host {
        Some(host) if state.allowed_hosts.contains(&host) => next.run(request).await,
        _ => ApiError::new(
            StatusCode::FORBIDDEN,
            "forbidden_host",
            "The Host header does not name this local API.",
        )
        .into_response(),
    }
}

async fn same_site_only(State(state): State<Shared>, request: Request, next: Next) -> Response {
    match check_same_site(&state, request.headers()) {
        Ok(()) => next.run(request).await,
        Err(error) => error.into_response(),
    }
}

fn check_same_site(state: &ApiState, headers: &HeaderMap) -> Result<(), ApiError> {
    let forbidden = |message| ApiError::new(StatusCode::FORBIDDEN, "forbidden_origin", message);
    if let Some(origin) = headers.get(header::ORIGIN) {
        let origin = origin.to_str().unwrap_or_default().to_ascii_lowercase();
        if !state.allowed_origins.contains(&origin) {
            return Err(forbidden("This origin may not use the local API."));
        }
    }
    if let Some(site) = headers.get("sec-fetch-site")
        && !matches!(site.to_str(), Ok("same-origin" | "none"))
    {
        return Err(forbidden("Cross-site requests may not use the local API."));
    }
    Ok(())
}

async fn require_token(State(state): State<Shared>, request: Request, next: Next) -> Response {
    let presented = request
        .headers()
        .get(SESSION_HEADER)
        .map(|value| value.as_bytes())
        .unwrap_or_default();
    if constant_time_eq(presented, state.token.as_bytes()) {
        next.run(request).await
    } else {
        ApiError::new(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            format!("Writes need the {SESSION_HEADER} header from GET /api/v1/session."),
        )
        .into_response()
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Runs a synchronous application call (which may touch the file system) off the reactor.
async fn run<T: Send + 'static>(
    state: Shared,
    call: impl FnOnce(&LocalApplication) -> Result<T, ApplicationError> + Send + 'static,
) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(move || call(&state.application))
        .await
        .map_err(|_| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "The request failed unexpectedly.",
            )
        })?
        .map_err(ApiError::from)
}

async fn health(State(state): State<Shared>) -> Json<HealthResponse> {
    Json(state.application.health())
}

async fn capabilities(State(state): State<Shared>) -> Json<CapabilitiesResponse> {
    Json(state.application.capabilities())
}

#[derive(Serialize)]
struct SessionResponse {
    api_version: &'static str,
    header: &'static str,
    token: String,
}

async fn session(State(state): State<Shared>) -> Json<SessionResponse> {
    Json(SessionResponse {
        api_version: API_VERSION,
        header: SESSION_HEADER,
        token: state.token.clone(),
    })
}

async fn list_documents(
    State(state): State<Shared>,
) -> Result<Json<DocumentListResponse>, ApiError> {
    run(state, |app| app.list_documents()).await.map(Json)
}

async fn create_document(
    State(state): State<Shared>,
    JsonBody(body): JsonBody<CreateDocumentRequest>,
) -> Result<(StatusCode, Json<DocumentResponse>), ApiError> {
    let response = run(state, move |app| {
        app.create_document(Some(&body.request_id), body.title.as_deref())
    })
    .await?;
    Ok((StatusCode::CREATED, Json(response)))
}

async fn document(
    State(state): State<Shared>,
    Path(id): Path<String>,
) -> Result<Json<DocumentResponse>, ApiError> {
    run(state, move |app| app.document(&id)).await.map(Json)
}

async fn apply_transaction(
    State(state): State<Shared>,
    Path(id): Path<String>,
    JsonBody(body): JsonBody<TransactionRequest>,
) -> Result<Json<DocumentResponse>, ApiError> {
    run(state, move |app| {
        app.apply_transaction(
            &id,
            Some(&body.request_id),
            body.expected_revision,
            body.commands,
        )
    })
    .await
    .map(Json)
}

async fn undo(
    State(state): State<Shared>,
    Path(id): Path<String>,
    JsonBody(body): JsonBody<RevisionRequest>,
) -> Result<Json<DocumentResponse>, ApiError> {
    run(state, move |app| {
        app.undo(&id, Some(&body.request_id), body.expected_revision)
    })
    .await
    .map(Json)
}

async fn redo(
    State(state): State<Shared>,
    Path(id): Path<String>,
    JsonBody(body): JsonBody<RevisionRequest>,
) -> Result<Json<DocumentResponse>, ApiError> {
    run(state, move |app| {
        app.redo(&id, Some(&body.request_id), body.expected_revision)
    })
    .await
    .map(Json)
}

async fn save(
    State(state): State<Shared>,
    Path(id): Path<String>,
    JsonBody(body): JsonBody<RevisionRequest>,
) -> Result<Json<DocumentResponse>, ApiError> {
    run(state, move |app| {
        app.save(&id, Some(&body.request_id), body.expected_revision)
    })
    .await
    .map(Json)
}

async fn close(
    State(state): State<Shared>,
    Path(id): Path<String>,
    JsonBody(body): JsonBody<CloseRequest>,
) -> Result<Json<CloseResponse>, ApiError> {
    run(state, move |app| {
        app.close(
            &id,
            Some(&body.request_id),
            body.discard_unsaved,
            body.expected_revision,
        )
    })
    .await
    .map(Json)
}

async fn autosave(
    State(state): State<Shared>,
    Path(id): Path<String>,
    JsonBody(body): JsonBody<DraftRequest>,
) -> Result<Json<DocumentResponse>, ApiError> {
    run(state, move |app| app.autosave(&id, Some(&body.request_id)))
        .await
        .map(Json)
}

async fn save_as(
    State(state): State<Shared>,
    Path(id): Path<String>,
    JsonBody(body): JsonBody<SaveAsRequest>,
) -> Result<(StatusCode, Json<CopyResponse>), ApiError> {
    let response = run(state, move |app| {
        app.save_as(
            &id,
            Some(&body.request_id),
            body.expected_revision,
            body.title.as_deref(),
        )
    })
    .await?;
    Ok((StatusCode::CREATED, Json(response)))
}

async fn recover(
    State(state): State<Shared>,
    Path(id): Path<String>,
    JsonBody(body): JsonBody<DraftRequest>,
) -> Result<Json<DocumentResponse>, ApiError> {
    run(state, move |app| app.recover(&id, Some(&body.request_id)))
        .await
        .map(Json)
}

async fn recover_as_copy(
    State(state): State<Shared>,
    Path(id): Path<String>,
    JsonBody(body): JsonBody<RecoverAsCopyRequest>,
) -> Result<(StatusCode, Json<CopyResponse>), ApiError> {
    let response = run(state, move |app| {
        app.recover_as_copy(&id, Some(&body.request_id), body.title.as_deref())
    })
    .await?;
    Ok((StatusCode::CREATED, Json(response)))
}

async fn discard_draft(
    State(state): State<Shared>,
    Path(id): Path<String>,
    JsonBody(body): JsonBody<DraftRequest>,
) -> Result<Json<DraftDiscardResponse>, ApiError> {
    run(state, move |app| {
        app.discard_draft(&id, Some(&body.request_id))
    })
    .await
    .map(Json)
}

async fn not_found() -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        "not_found",
        "This route is not implemented. See /api/v1/capabilities.",
    )
}

async fn method_not_allowed() -> ApiError {
    ApiError::new(
        StatusCode::METHOD_NOT_ALLOWED,
        "method_not_allowed",
        "This route does not support the request method.",
    )
}
