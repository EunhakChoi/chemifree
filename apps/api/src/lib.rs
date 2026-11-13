//! Loopback HTTP entry adapter. Wiring and transport guards live here, outside the core crates.
//!
//! Guards, in order:
//! 1. Every request must name an allowed `Host` (DNS-rebinding defense).
//! 2. `GET /api/v1/session` and all writes reject cross-site browser requests by `Origin`
//!    and `Sec-Fetch-Site`. No CORS headers are ever sent.
//! 3. Writes must carry the per-process session token in `x-chemifree-session`, a
//!    `Content-Type: application/json` body, and at most [`MAX_BODY_BYTES`].
//!
//! Every answer, API or web file, carries the headers listed in `SECURITY_HEADERS`. With a web
//! directory ([`ApiState::with_web_dir`]), the built web app is served from the same origin: its
//! files pass the same `Host` guard, and no path outside `/api` falls back to `index.html`.
//!
//! This keeps other websites from driving the local API. It is not user authentication:
//! any local process that can reach the loopback port can obtain the token.

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{DefaultBodyLimit, FromRequest, Path, Query, Request, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post, put};
use axum::{Json, Router};
use chemifree_application::{
    API_VERSION, ApplicationError, CancelFlag, CapabilitiesResponse, CloseRequest, CloseResponse,
    CopyResponse, CreateDocumentRequest, DataStatusResponse, DeleteResponse, DocumentListResponse,
    DocumentResponse, DraftDiscardResponse, DraftRequest, ErrorCode, ErrorResponse, HealthResponse,
    ImportPreferencesRequest, ImportSettingsRequest, ListQuery, OperationResponse,
    PreferencesRequest, PreferencesResponse, RecoverAsCopyRequest, RenameRequest,
    ResetSettingsRequest, RestoreRequest, RestoreResponse, RevisionRequest, SaveAsRequest,
    TransactionRequest, TrashListResponse,
};
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use http_body_util::LengthLimitError;
use serde::Serialize;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;

pub use chemifree_host::LocalApplication;

pub const SESSION_HEADER: &str = "x-chemifree-session";
pub const MAX_BODY_BYTES: usize = 256 * 1024;
/// Development (Vite) and preview origins. The API binary allows them when no origin is configured
/// and it serves no web app.
pub const DEFAULT_WEB_ORIGINS: [&str; 4] = [
    "http://127.0.0.1:5173",
    "http://localhost:5173",
    "http://127.0.0.1:4173",
    "http://localhost:4173",
];

/// Headers that every answer carries, API and web files alike.
const SECURITY_HEADERS: [(HeaderName, &str); 4] = [
    (
        header::CONTENT_SECURITY_POLICY,
        "default-src 'self'; img-src 'self' data: blob:; style-src 'self' 'unsafe-inline'; \
         connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'",
    ),
    (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
    (header::REFERRER_POLICY, "no-referrer"),
    (
        HeaderName::from_static("cross-origin-opener-policy"),
        "same-origin",
    ),
];

pub struct ApiState {
    application: Arc<LocalApplication>,
    token: String,
    allowed_hosts: HashSet<String>,
    allowed_origins: HashSet<String>,
    web_dir: Option<WebDir>,
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
            application: Arc::new(application),
            token,
            allowed_hosts,
            allowed_origins,
            web_dir: None,
        })
    }

    /// Also serves the built web app in `web_dir` from this origin, beside the API. Without it,
    /// the API serves no web files.
    #[must_use]
    pub fn with_web_dir(mut self, web_dir: WebDir) -> Self {
        self.web_dir = Some(web_dir);
        self
    }

    /// The web directory served with the API, if any.
    #[must_use]
    pub fn web_dir(&self) -> Option<&WebDir> {
        self.web_dir.as_ref()
    }

    /// Ends every running and waiting computation with `cancelled`, leaving its document
    /// unchanged, and refuses new computations the same way; other requests keep working. Call
    /// it once the process starts to stop, so that graceful shutdown does not wait for them and
    /// their clients still receive `cancelled`.
    pub fn cancel_computations(&self) {
        self.application.cancel_computations();
    }
}

/// A directory of built web assets, checked when it is opened: a directory that holds an
/// `index.html` file. [`ApiState::with_web_dir`] serves it.
#[derive(Clone, Debug)]
pub struct WebDir(PathBuf);

impl WebDir {
    /// Opens `dir` as a web directory.
    ///
    /// # Errors
    ///
    /// Returns a message naming `dir` when it cannot be read, is not a directory, or holds no
    /// `index.html` file.
    pub fn open(dir: &std::path::Path) -> Result<Self, String> {
        let shown = dir.display();
        let metadata = fs::metadata(dir)
            .map_err(|error| format!("web directory {shown} cannot be read: {error}"))?;
        if !metadata.is_dir() {
            return Err(format!("web directory {shown} is not a directory"));
        }
        if !dir.join("index.html").is_file() {
            return Err(format!(
                "web directory {shown} has no index.html; pass the output directory of a web build"
            ));
        }
        let root = dir
            .canonicalize()
            .map_err(|error| format!("web directory {shown} cannot be resolved: {error}"))?;
        Ok(Self(root))
    }

    /// The canonical path of the directory.
    #[must_use]
    pub fn path(&self) -> &std::path::Path {
        &self.0
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

/// Adds a diagnostic log to `router`: one line per request on standard error, written when
/// the response is ready, as `+<ms since the log started> <method> <path> <status> <ms>ms`.
/// Times come from the monotonic clock, so wall-clock adjustments do not skew them. Headers,
/// query strings, and bodies are never logged.
pub fn log_requests(router: Router) -> Router {
    let started = Instant::now();
    router.layer(middleware::from_fn(
        move |request: Request, next: Next| async move {
            let method = request.method().clone();
            let path = request.uri().path().to_owned();
            let received = Instant::now();
            let response = next.run(request).await;
            eprintln!(
                "+{} {method} {path} {} {}ms",
                received.duration_since(started).as_millis(),
                response.status().as_u16(),
                received.elapsed().as_millis()
            );
            response
        },
    ))
}

/// How often the background writer looks for drafts that are due.
const DRAFT_TICK: Duration = Duration::from_millis(250);

/// Writes recovery drafts in the background, so unsaved work is drafted whether or not a client
/// autosaves. [`DraftWriter::finish`] stops it, waits for drafts still being written, and drafts
/// whatever is still undrafted, as the process does before it exits on a shutdown signal.
pub struct DraftWriter(
    chemifree_application::DraftWriter<LocalCapabilities, FileDocumentRepository>,
);

impl DraftWriter {
    pub fn start(state: Shared) -> Self {
        Self(chemifree_application::DraftWriter::start(
            Arc::clone(&state.application),
            DRAFT_TICK,
        ))
    }

    /// Returns the documents that could not be drafted, with why.
    pub fn finish(self) -> Vec<(String, ApplicationError)> {
        self.0.finish()
    }
}

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
        .route("/api/v1/documents/{id}/rename", post(rename))
        .route(
            "/api/v1/documents/{id}/settings/reset",
            post(reset_settings),
        )
        .route(
            "/api/v1/documents/{id}/settings/import",
            post(import_settings),
        )
        .route("/api/v1/preferences", put(set_preferences))
        .route("/api/v1/preferences/reset", post(reset_preferences))
        .route("/api/v1/preferences/import", post(import_preferences))
        .route("/api/v1/documents/{id}/delete", post(delete))
        .route("/api/v1/trash/{id}/restore", post(restore))
        .route("/api/v1/operations", post(operations))
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
    let api = Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/capabilities", get(capabilities))
        .route("/api/v1/data", get(data_status))
        .route("/api/v1/documents", get(list_documents))
        .route("/api/v1/trash", get(list_trash))
        .route("/api/v1/preferences", get(preferences))
        .route("/api/v1/documents/{id}", get(document))
        .merge(writes)
        .merge(session)
        .method_not_allowed_fallback(method_not_allowed);
    let routes = match state.web_dir() {
        // Without a web app, every path that matches no route is an unknown API route.
        None => api.fallback(not_found),
        // With one, unknown API paths keep their JSON answer, and every other path is a web file.
        Some(web_dir) => api
            .route("/api", any(not_found))
            .route("/api/", any(not_found))
            .route("/api/{*rest}", any(not_found))
            .fallback_service(web_files(web_dir)),
    };
    with_security_headers(
        routes
            .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
            .layer(middleware::from_fn_with_state(state.clone(), allowed_host)),
    )
    .with_state(state)
}

/// The web app's files under `web_dir`, with the cache policy of [`web_cache_control`]. A path
/// without a file answers 404, because the app has no client-side routes to fall back to.
fn web_files(web_dir: &WebDir) -> Router {
    Router::new()
        // `ServeDir` answers a directory request, `/` included, with that directory's `index.html`.
        .fallback_service(ServeDir::new(web_dir.path()))
        .layer(middleware::from_fn(web_cache_control))
}

/// Sets how a web file may be cached. Vite names the files under `assets/` by content hash, so
/// their names never change and they may be cached for a year; every other file, the index
/// included, is revalidated on use. Only an answer that carries the file gets the long lifetime,
/// so a missing asset is never cached as one.
async fn web_cache_control(request: Request, next: Next) -> Response {
    let hashed = request.uri().path().starts_with("/assets/");
    let mut response = next.run(request).await;
    let status = response.status();
    let carries_file = status.is_success() || status == StatusCode::NOT_MODIFIED;
    let policy = if hashed && carries_file {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static(policy));
    response
}

/// Adds [`SECURITY_HEADERS`] to every answer of `router`, including the refusals made by the
/// layers beneath it.
fn with_security_headers(mut router: Router<Shared>) -> Router<Shared> {
    for (name, value) in SECURITY_HEADERS {
        router = router.layer(SetResponseHeaderLayer::overriding(
            name,
            HeaderValue::from_static(value),
        ));
    }
    router
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
                reason: None,
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
            | ErrorCode::DraftOutdated
            | ErrorCode::RequestInProgress => StatusCode::CONFLICT,
            // Mapping only: no route reaches it. A computation stopped by its time limit or by
            // the caller is reported in an operation reply's diagnostics (`POST
            // /api/v1/operations` answers 200), never as an `ApiError`. `docs/CONTRACT.md`
            // (Computation time limit and cancel) says the same.
            ErrorCode::StorageFailed
            | ErrorCode::ResourceLimit
            | ErrorCode::TimeLimit
            | ErrorCode::Cancelled => StatusCode::SERVICE_UNAVAILABLE,
            ErrorCode::SettingNotImplemented => StatusCode::NOT_IMPLEMENTED,
            ErrorCode::DocumentUnreadable | ErrorCode::PreferencesUnreadable => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
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

async fn data_status(State(state): State<Shared>) -> Json<DataStatusResponse> {
    Json(state.application.data_status())
}

/// Runs one request of the operation contract. Every reply is the contract's envelope: 200 with
/// an operation result whatever its outcome, 400 with a protocol error, 413 for a body over
/// [`MAX_BODY_BYTES`], and 415 without a JSON content type.
async fn operations(State(state): State<Shared>, request: Request) -> Response {
    let json = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(is_json_media_type);
    if !json {
        let reply = OperationResponse::unread(
            "unsupported_media_type",
            "Operation requests need Content-Type: application/json.",
        );
        return (StatusCode::UNSUPPORTED_MEDIA_TYPE, Json(reply)).into_response();
    }
    let body = match axum::body::to_bytes(request.into_body(), MAX_BODY_BYTES).await {
        Ok(body) => body,
        Err(error) => return unread_body(error),
    };
    // A client that goes away drops this future, and with it the guard, which stops the
    // computation at its next checkpoint instead of letting it run on unheard.
    let cancel = Arc::new(CancelFlag::new());
    let _cancel_on_drop = CancelOnDrop(Arc::clone(&cancel));
    match run(state, move |app| Ok(app.execute_with(&body, &*cancel))).await {
        Ok(reply) if reply.is_protocol_error() => {
            (StatusCode::BAD_REQUEST, Json(reply)).into_response()
        }
        Ok(reply) => Json(reply).into_response(),
        Err(error) => error.into_response(),
    }
}

/// Cancels the computations that check `flag` when dropped.
struct CancelOnDrop(Arc<CancelFlag>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

/// Whether a `Content-Type` value names JSON: `application/json` or an `application/*+json`
/// type, parameters aside, as the v1 routes accept.
fn is_json_media_type(value: &str) -> bool {
    let media = value.split(';').next().unwrap_or_default().trim();
    let Some((kind, subtype)) = media.split_once('/') else {
        return false;
    };
    let subtype = subtype.to_ascii_lowercase();
    kind.eq_ignore_ascii_case("application") && (subtype == "json" || subtype.ends_with("+json"))
}

/// The reply to an operation request body that could not be read whole: 413 when it is over
/// [`MAX_BODY_BYTES`], otherwise 400 (a broken transfer, for one).
fn unread_body(error: axum::Error) -> Response {
    if error.into_inner().is::<LengthLimitError>() {
        let reply = OperationResponse::unread(
            ErrorCode::ResourceLimit.as_str(),
            format!("The request is larger than {MAX_BODY_BYTES} bytes."),
        );
        return (StatusCode::PAYLOAD_TOO_LARGE, Json(reply)).into_response();
    }
    let reply =
        OperationResponse::unread("malformed_request", "The request body could not be read.");
    (StatusCode::BAD_REQUEST, Json(reply)).into_response()
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
    query: Result<Query<ListQuery>, QueryRejection>,
) -> Result<Json<DocumentListResponse>, ApiError> {
    let Query(query) = query.map_err(|rejection| {
        ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_request",
            rejection.body_text(),
        )
    })?;
    run(state, move |app| app.list_documents(&query))
        .await
        .map(Json)
}

async fn rename(
    State(state): State<Shared>,
    Path(id): Path<String>,
    JsonBody(body): JsonBody<RenameRequest>,
) -> Result<Json<DocumentResponse>, ApiError> {
    run(state, move |app| {
        app.rename(
            &id,
            Some(&body.request_id),
            body.expected_revision,
            &body.title,
        )
    })
    .await
    .map(Json)
}

async fn reset_settings(
    State(state): State<Shared>,
    Path(id): Path<String>,
    JsonBody(body): JsonBody<ResetSettingsRequest>,
) -> Result<Json<DocumentResponse>, ApiError> {
    run(state, move |app| {
        app.reset_settings_selecting(
            &id,
            Some(&body.request_id),
            body.expected_revision,
            body.to,
            chemifree_application::TransactionSelections {
                before: body.selection_before,
                after: body.selection_after,
            },
        )
    })
    .await
    .map(Json)
}

async fn import_settings(
    State(state): State<Shared>,
    Path(id): Path<String>,
    JsonBody(body): JsonBody<ImportSettingsRequest>,
) -> Result<Json<DocumentResponse>, ApiError> {
    run(state, move |app| {
        app.import_settings_selecting(
            &id,
            Some(&body.request_id),
            body.expected_revision,
            &body.source,
            chemifree_application::TransactionSelections {
                before: body.selection_before,
                after: body.selection_after,
            },
        )
    })
    .await
    .map(Json)
}

async fn preferences(State(state): State<Shared>) -> Result<Json<PreferencesResponse>, ApiError> {
    run(state, |app| app.preferences()).await.map(Json)
}

async fn set_preferences(
    State(state): State<Shared>,
    JsonBody(body): JsonBody<PreferencesRequest>,
) -> Result<Json<PreferencesResponse>, ApiError> {
    run(state, move |app| {
        app.set_preferences(
            Some(&body.request_id),
            body.expected_revision,
            body.change(),
        )
    })
    .await
    .map(Json)
}

async fn reset_preferences(
    State(state): State<Shared>,
    JsonBody(body): JsonBody<RevisionRequest>,
) -> Result<Json<PreferencesResponse>, ApiError> {
    run(state, move |app| {
        app.reset_preferences(Some(&body.request_id), body.expected_revision)
    })
    .await
    .map(Json)
}

async fn import_preferences(
    State(state): State<Shared>,
    JsonBody(body): JsonBody<ImportPreferencesRequest>,
) -> Result<Json<PreferencesResponse>, ApiError> {
    run(state, move |app| {
        app.preferences_from_document(
            Some(&body.request_id),
            body.expected_revision,
            &body.document,
        )
    })
    .await
    .map(Json)
}

async fn delete(
    State(state): State<Shared>,
    Path(id): Path<String>,
    JsonBody(body): JsonBody<RevisionRequest>,
) -> Result<Json<DeleteResponse>, ApiError> {
    run(state, move |app| {
        app.delete(&id, Some(&body.request_id), body.expected_revision)
    })
    .await
    .map(Json)
}

async fn list_trash(State(state): State<Shared>) -> Result<Json<TrashListResponse>, ApiError> {
    run(state, |app| app.list_trash()).await.map(Json)
}

async fn restore(
    State(state): State<Shared>,
    Path(id): Path<String>,
    JsonBody(body): JsonBody<RestoreRequest>,
) -> Result<Json<RestoreResponse>, ApiError> {
    run(state, move |app| app.restore(&id, Some(&body.request_id)))
        .await
        .map(Json)
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
        app.apply_transaction_selecting(
            &id,
            Some(&body.request_id),
            body.expected_revision,
            body.commands,
            chemifree_application::TransactionSelections {
                before: body.selection_before,
                after: body.selection_after,
            },
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

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use serde_json::Value;

    use super::*;

    #[test]
    fn json_media_types_match_the_v1_routes() {
        for accepted in [
            "application/json",
            "Application/JSON; charset=utf-8",
            "application/merge-patch+json",
        ] {
            assert!(is_json_media_type(accepted), "{accepted} is JSON");
        }
        for refused in [
            "text/plain",
            "text/json",
            "application/jsonx",
            "application/json-seq",
            "json",
            "",
        ] {
            assert!(!is_json_media_type(refused), "{refused:?} is not JSON");
        }
    }

    #[tokio::test]
    async fn only_an_oversized_body_is_answered_as_too_large() {
        let oversized =
            axum::body::to_bytes(Body::from(vec![b' '; MAX_BODY_BYTES + 1]), MAX_BODY_BYTES)
                .await
                .expect_err("the body is over the limit");
        assert_eq!(
            unread_body(oversized).status(),
            StatusCode::PAYLOAD_TOO_LARGE
        );

        let broken = axum::Error::new(std::io::Error::other("connection reset"));
        assert_eq!(unread_body(broken).status(), StatusCode::BAD_REQUEST);
    }

    const TEST_PORT: u16 = 3999;
    const TEST_HOST: &str = "127.0.0.1:3999";
    const TEST_TOKEN: &str = "web-test-token";

    /// A router over a fresh data directory, serving `web` when given. The data directory is
    /// returned as well, so that it lives as long as the test needs it.
    fn router_for_test(web: Option<WebDir>, origins: &[String]) -> (Router, tempfile::TempDir) {
        let data = tempfile::tempdir().unwrap();
        let application =
            chemifree_host::local_application(chemifree_host::LocalStores::new(data.path()));
        let state = ApiState::new(application, TEST_TOKEN.into(), TEST_PORT, origins).unwrap();
        let state = match web {
            Some(web) => state.with_web_dir(web),
            None => state,
        };
        (router(Arc::new(state)), data)
    }

    /// Writes what a Vite build leaves in a web directory: an index, a hashed asset and a favicon.
    /// A secret file sits beside the web directory, where no request may reach it.
    fn web_fixture(root: &std::path::Path) -> std::path::PathBuf {
        let web = root.join("web");
        fs::create_dir_all(web.join("assets")).unwrap();
        fs::write(
            web.join("index.html"),
            "<!doctype html><title>Chemifree</title>",
        )
        .unwrap();
        fs::write(web.join("assets/app-3f9c.js"), "console.log('app');").unwrap();
        fs::write(
            web.join("favicon.svg"),
            "<svg xmlns=\"http://www.w3.org/2000/svg\"/>",
        )
        .unwrap();
        fs::write(root.join("secret.txt"), "not for the web").unwrap();
        web
    }

    fn get(uri: &str) -> Request<Body> {
        Request::get(uri)
            .header("host", TEST_HOST)
            .body(Body::empty())
            .unwrap()
    }

    async fn ask(router: &Router, request: Request<Body>) -> Response {
        use tower::ServiceExt;

        router.clone().oneshot(request).await.unwrap()
    }

    async fn text_of(response: Response) -> String {
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        String::from_utf8_lossy(&bytes).into_owned()
    }

    async fn json_of(response: Response) -> Value {
        serde_json::from_str(&text_of(response).await).unwrap()
    }

    fn cache_control(response: &Response) -> &str {
        response
            .headers()
            .get(header::CACHE_CONTROL)
            .expect("the answer names its cache policy")
            .to_str()
            .unwrap()
    }

    fn assert_security_headers(response: &Response, answer: &str) {
        for (name, value) in SECURITY_HEADERS {
            assert_eq!(
                response.headers().get(&name).map(|v| v.to_str().unwrap()),
                Some(value),
                "{answer} carries {}",
                name.as_str()
            );
        }
    }

    #[test]
    fn the_security_policy_is_the_documented_one() {
        let actual: Vec<(String, &str)> = SECURITY_HEADERS
            .into_iter()
            .map(|(name, value)| (name.as_str().to_owned(), value))
            .collect();
        let expected: Vec<(String, &str)> = [
            (
                "content-security-policy",
                "default-src 'self'; img-src 'self' data: blob:; style-src 'self' 'unsafe-inline'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'",
            ),
            ("x-content-type-options", "nosniff"),
            ("referrer-policy", "no-referrer"),
            ("cross-origin-opener-policy", "same-origin"),
        ]
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect();
        assert_eq!(actual, expected);
    }

    #[tokio::test]
    async fn the_web_app_is_served_with_its_cache_policy() {
        let root = tempfile::tempdir().unwrap();
        let web = WebDir::open(&web_fixture(root.path())).unwrap();
        let (api, _data) = router_for_test(Some(web), &[]);

        for uri in ["/", "/index.html"] {
            let response = ask(&api, get(uri)).await;
            assert_eq!(response.status(), StatusCode::OK, "{uri} is the index");
            assert!(
                response
                    .headers()
                    .get(header::CONTENT_TYPE)
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .starts_with("text/html"),
                "{uri} is served as HTML"
            );
            assert_eq!(cache_control(&response), "no-cache", "{uri} is revalidated");
            let body = text_of(response).await;
            assert!(body.contains("<title>Chemifree</title>"), "{uri}: {body:?}");
        }

        let response = ask(&api, get("/assets/app-3f9c.js")).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            cache_control(&response),
            "public, max-age=31536000, immutable",
            "a hashed asset is cached for a year"
        );

        let response = ask(&api, get("/favicon.svg")).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            cache_control(&response),
            "no-cache",
            "a file at the root is revalidated"
        );
    }

    #[tokio::test]
    async fn paths_without_a_file_are_not_found_and_never_fall_back_to_the_index() {
        let root = tempfile::tempdir().unwrap();
        let web = WebDir::open(&web_fixture(root.path())).unwrap();
        let (api, _data) = router_for_test(Some(web), &[]);

        for uri in ["/documents/42", "/assets/missing-0000.js"] {
            let response = ask(&api, get(uri)).await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{uri}");
            assert_eq!(
                cache_control(&response),
                "no-cache",
                "{uri}: a miss is never cached as a file"
            );
            let body = text_of(response).await;
            assert!(!body.contains("<title>"), "{uri} answered with the index");
        }
    }

    #[tokio::test]
    async fn a_not_modified_hashed_asset_keeps_its_long_lifetime() {
        let root = tempfile::tempdir().unwrap();
        let web = WebDir::open(&web_fixture(root.path())).unwrap();
        let (api, _data) = router_for_test(Some(web), &[]);

        let first = ask(&api, get("/assets/app-3f9c.js")).await;
        let modified = first.headers()[header::LAST_MODIFIED].clone();
        let conditional = Request::get("/assets/app-3f9c.js")
            .header("host", TEST_HOST)
            .header(header::IF_MODIFIED_SINCE, modified)
            .body(Body::empty())
            .unwrap();
        let response = ask(&api, conditional).await;
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
        assert_eq!(
            cache_control(&response),
            "public, max-age=31536000, immutable",
            "the revalidated answer keeps the asset's lifetime"
        );
    }

    #[tokio::test]
    async fn files_outside_the_web_directory_are_not_reachable() {
        let root = tempfile::tempdir().unwrap();
        let web = WebDir::open(&web_fixture(root.path())).unwrap();
        let (api, _data) = router_for_test(Some(web), &[]);

        for uri in [
            "/secret.txt",
            "/../secret.txt",
            "/%2e%2e/secret.txt",
            "/%2E%2E%2fsecret.txt",
            "/assets/..%2f..%2fsecret.txt",
            "/assets/%2e%2e/%2e%2e/secret.txt",
            "/%00",
        ] {
            let response = ask(&api, get(uri)).await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{uri}");
            let body = text_of(response).await;
            assert!(
                !body.contains("not for the web"),
                "{uri} reached the secret"
            );
        }
    }

    #[tokio::test]
    async fn api_paths_without_a_route_keep_their_json_answer() {
        let root = tempfile::tempdir().unwrap();
        let web = WebDir::open(&web_fixture(root.path())).unwrap();
        let (api, _data) = router_for_test(Some(web), &[]);

        for uri in ["/api", "/api/", "/api/nope", "/api/v1/nope"] {
            let response = ask(&api, get(uri)).await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{uri}");
            assert_eq!(json_of(response).await["code"], "not_found", "{uri}");
        }

        let response = ask(&api, get("/api/v1/health")).await;
        assert_eq!(response.status(), StatusCode::OK);
        let wrong_method = Request::post("/api/v1/health")
            .header("host", TEST_HOST)
            .body(Body::empty())
            .unwrap();
        let response = ask(&api, wrong_method).await;
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(json_of(response).await["code"], "method_not_allowed");
    }

    #[tokio::test]
    async fn every_answer_carries_the_security_headers() {
        let root = tempfile::tempdir().unwrap();
        let web = WebDir::open(&web_fixture(root.path())).unwrap();
        let (api, _data) = router_for_test(Some(web), &[]);
        let (plain, _plain_data) = router_for_test(None, &[]);
        let foreign_host = Request::get("/")
            .header("host", "evil.example")
            .body(Body::empty())
            .unwrap();

        let answers = [
            ("GET / from the web app", ask(&api, get("/")).await),
            (
                "GET /documents/42 from the web app",
                ask(&api, get("/documents/42")).await,
            ),
            ("GET /api/v1/health", ask(&api, get("/api/v1/health")).await),
            ("GET /api/nope", ask(&api, get("/api/nope")).await),
            ("GET with a foreign Host", ask(&api, foreign_host).await),
            ("GET / without a web app", ask(&plain, get("/")).await),
        ];
        for (answer, response) in &answers {
            assert_security_headers(response, answer);
        }
    }

    #[tokio::test]
    async fn a_foreign_host_is_refused_for_web_files_as_for_the_api() {
        let root = tempfile::tempdir().unwrap();
        let web = WebDir::open(&web_fixture(root.path())).unwrap();
        let (api, _data) = router_for_test(Some(web), &[]);

        for uri in ["/", "/assets/app-3f9c.js"] {
            let request = Request::get(uri)
                .header("host", "evil.example")
                .body(Body::empty())
                .unwrap();
            let response = ask(&api, request).await;
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{uri}");
            assert_eq!(json_of(response).await["code"], "forbidden_host", "{uri}");
        }
    }

    #[tokio::test]
    async fn without_a_web_directory_the_root_is_an_unknown_api_path() {
        let (plain, _data) = router_for_test(None, &[]);
        let response = ask(&plain, get("/")).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(json_of(response).await["code"], "not_found");
    }

    #[tokio::test]
    async fn with_a_web_directory_only_the_own_origin_may_write() {
        let root = tempfile::tempdir().unwrap();
        let web = WebDir::open(&web_fixture(root.path())).unwrap();
        let (api, _data) = router_for_test(Some(web), &[]);
        let create = |origin: &str| {
            Request::post("/api/v1/documents")
                .header("host", TEST_HOST)
                .header("origin", origin)
                .header("sec-fetch-site", "same-origin")
                .header("content-type", "application/json")
                .header(SESSION_HEADER, TEST_TOKEN)
                .body(Body::from(r#"{"request_id":"create-1"}"#))
                .unwrap()
        };

        let response = ask(&api, create("http://127.0.0.1:5173")).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(json_of(response).await["code"], "forbidden_origin");

        let response = ask(&api, create("http://127.0.0.1:3999")).await;
        assert_eq!(response.status(), StatusCode::CREATED);
    }

    #[test]
    fn a_web_directory_must_be_a_directory_with_an_index() {
        let root = tempfile::tempdir().unwrap();

        let error = WebDir::open(&root.path().join("missing")).unwrap_err();
        assert!(error.contains("cannot be read"), "{error}");

        let file = root.path().join("index.html");
        fs::write(&file, "<!doctype html>").unwrap();
        let error = WebDir::open(&file).unwrap_err();
        assert!(error.contains("is not a directory"), "{error}");

        let bare = root.path().join("bare");
        fs::create_dir(&bare).unwrap();
        let error = WebDir::open(&bare).unwrap_err();
        assert!(error.contains("has no index.html"), "{error}");

        let dir = web_fixture(root.path());
        let web = WebDir::open(&dir).unwrap();
        assert_eq!(web.path(), dir.canonicalize().unwrap());
    }
}
