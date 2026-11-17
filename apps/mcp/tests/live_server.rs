//! The MCP client against a live API server: the reads, the operation calls, the discovery of the
//! server's address, the session refusals and the reply limit.
//!
//! The live server is `chemifree_api::router` on a loopback port over a temporary data directory, built
//! as the API's own tests build it. The refusal cases use a small fake server, because the real server
//! refuses a session only after its token changes, and these tests need that to happen on demand.

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use bytes::Bytes;
use chemifree_api::SESSION_HEADER;
use chemifree_mcp::client::{ApiClient, ClientError, MAX_REPLY_BYTES};
use chemifree_mcp::discovery::{Discovery, DiscoveryError, ServerUrl};
use chemifree_providers::ServerLock;
use common::{Http, TestServer, closed_port, create_document};
use hyper::{Method, StatusCode};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

#[tokio::test]
async fn lists_the_documents_that_the_server_holds() {
    let server = TestServer::start().await;
    let http = server.http();
    let created = create_document(&http, "mcp-list-1").await;
    let client = client_for(&server.url);

    let list = client
        .list_documents()
        .await
        .expect("the running server lists its documents");

    assert_eq!(
        list.count, 1,
        "one document was created, got {}",
        list.count
    );
    assert_eq!(list.reply["documents"][0]["id"], created);
    let (status, direct) = http.request(Method::GET, "/api/v1/documents", None).await;
    assert_eq!(status, StatusCode::OK, "{direct}");
    assert_eq!(
        list.reply, direct,
        "the reply is the server's reply, unchanged"
    );
}

#[tokio::test]
async fn a_snapshot_reports_the_revision_of_unsaved_edits() {
    let server = TestServer::start().await;
    let http = server.http();
    let created = create_document(&http, "mcp-snapshot-1").await;
    let edited = add_carbon(&http, &created, "mcp-snapshot-edit-1", 0).await;
    assert_eq!(
        edited["document"]["revision"], 1,
        "the edit moved the document to revision 1"
    );
    assert_eq!(
        edited["session"]["unsaved_changes"], true,
        "the edit is not saved"
    );
    let client = client_for(&server.url);

    let reply = client
        .run_operation("document.snapshot", json!({"document_id": created}), None)
        .await
        .expect("the snapshot operation answers");

    let (status, direct) = http
        .request(Method::GET, &format!("/api/v1/documents/{created}"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{direct}");
    assert_eq!(reply["envelope"], "operation_result", "{reply}");
    assert_eq!(reply["outcome"], "ok", "{reply}");
    assert_eq!(
        reply["document_revision"], direct["document"]["revision"],
        "the snapshot names the revision that HTTP reports for the same document"
    );
    assert_eq!(reply["document_revision"], 1, "{reply}");
}

#[tokio::test]
async fn a_bad_request_id_is_a_protocol_error_reply_not_a_failed_call() {
    let server = TestServer::start().await;
    let client = client_for(&server.url);

    let reply = client
        .run_operation(
            "document.snapshot",
            json!({"document_id": "missing"}),
            Some("bad id".to_owned()),
        )
        .await
        .expect("the server answers with a protocol error, which is a reply");

    assert_eq!(reply["envelope"], "protocol_error", "{reply}");
    assert_eq!(reply["outcome"], "error", "{reply}");
}

#[tokio::test]
async fn the_running_server_is_found_through_its_recorded_address() {
    let server = TestServer::start().await;
    let lock = ServerLock::acquire(server.data.path())
        .expect("the test data directory has no other server");
    lock.record_url(&server.url)
        .expect("the address is recorded");
    let client = ApiClient::new(Discovery::new(None, Some(server.data.path().to_path_buf())));

    let list = client
        .list_documents()
        .await
        .expect("the recorded server answers");

    assert_eq!(list.count, 0, "a new data directory holds no documents");
}

#[tokio::test]
async fn a_data_directory_without_a_record_names_the_directory_in_the_error() {
    let data = tempfile::tempdir().expect("a temporary data directory can be created");
    let client = ApiClient::new(Discovery::new(None, Some(data.path().to_path_buf())));

    let error = client
        .list_documents()
        .await
        .expect_err("no server is recorded in the directory");

    assert_eq!(
        error.to_string(),
        format!(
            "No running Chemifree API server was found for {}. Start chemifree-api for that data directory.",
            data.path().display()
        )
    );
}

#[tokio::test]
async fn a_server_recorded_after_a_failed_call_is_found_by_the_next_call() {
    let server = TestServer::start().await;
    let client = ApiClient::new(Discovery::new(None, Some(server.data.path().to_path_buf())));

    let before = client
        .list_documents()
        .await
        .expect_err("nothing is recorded yet");
    assert!(
        matches!(
            before,
            ClientError::Discovery(DiscoveryError::NoServerRecorded { .. })
        ),
        "got {before}"
    );

    let lock = ServerLock::acquire(server.data.path())
        .expect("the test data directory has no other server");
    lock.record_url(&server.url)
        .expect("the address is recorded");
    let list = client
        .list_documents()
        .await
        .expect("the next call finds the server that is now recorded");
    assert_eq!(list.count, 0, "a new data directory holds no documents");
}

#[tokio::test]
async fn a_recorded_address_that_refuses_is_read_again_on_the_next_call() {
    let server = TestServer::start().await;
    let lock = ServerLock::acquire(server.data.path())
        .expect("the test data directory has no other server");
    let dead = format!("http://127.0.0.1:{}", closed_port());
    lock.record_url(&dead).expect("the address is recorded");
    let client = ApiClient::new(Discovery::new(None, Some(server.data.path().to_path_buf())));

    let refused = client
        .list_documents()
        .await
        .expect_err("the recorded address has no server");
    assert!(
        matches!(refused, ClientError::Unreachable { .. }),
        "got {refused}"
    );
    assert!(
        refused.to_string().contains(&dead),
        "the error names the refused address, got {refused}"
    );

    lock.record_url(&server.url)
        .expect("the new address is recorded");
    let list = client
        .list_documents()
        .await
        .expect("the next call reads the record again and reaches the server");
    assert_eq!(list.count, 0, "a new data directory holds no documents");
}

#[tokio::test]
async fn an_address_with_no_server_is_named_in_the_error() {
    let url = format!("http://127.0.0.1:{}", closed_port());
    let client = client_for(&url);

    let error = client
        .list_documents()
        .await
        .expect_err("nothing listens at the address");

    assert!(
        matches!(error, ClientError::Unreachable { .. }),
        "got {error}"
    );
    assert_eq!(
        error.to_string(),
        format!(
            "No running Chemifree API server answered at {url}. Start chemifree-api for that data directory, or check --server-url."
        )
    );
}

#[tokio::test]
async fn a_refused_session_is_replaced_and_the_same_operation_is_sent_again() {
    let fake = FakeServer::start(StatusCode::UNAUTHORIZED, "token-1", "token-1").await;
    let client = client_for(fake.url());
    client
        .list_documents()
        .await
        .expect("the first session is accepted");
    fake.rotate("token-2");

    let reply = client
        .run_operation(
            "document.snapshot",
            json!({"document_id": "d1"}),
            Some("retry-1".to_owned()),
        )
        .await
        .expect("the refreshed session is accepted");

    assert_eq!(reply["outcome"], "ok", "{reply}");
    assert_eq!(
        fake.session_fetches(),
        2,
        "the refused session is fetched again, once"
    );
    assert_eq!(
        fake.operation_attempts(),
        vec![
            Attempt {
                token: Some("token-1".to_owned()),
                request_id: Some("retry-1".to_owned()),
            },
            Attempt {
                token: Some("token-2".to_owned()),
                request_id: Some("retry-1".to_owned()),
            },
        ],
        "the retry carries the same request id with the new session"
    );
}

#[tokio::test]
async fn a_refused_document_list_gets_one_fresh_session() {
    let fake = FakeServer::start(StatusCode::UNAUTHORIZED, "token-1", "token-1").await;
    let client = client_for(fake.url());
    client
        .list_documents()
        .await
        .expect("the first session is accepted");
    fake.rotate("token-2");

    let list = client
        .list_documents()
        .await
        .expect("the refreshed session is accepted");

    assert_eq!(list.count, 0, "a new server holds no documents");
    assert_eq!(
        fake.session_fetches(),
        2,
        "the refused session is fetched again, once"
    );
}

#[tokio::test]
async fn a_refusal_that_persists_is_retried_only_once() {
    let fake = FakeServer::start(StatusCode::UNAUTHORIZED, "token-a", "token-b").await;
    let client = client_for(fake.url());

    let error = client
        .run_operation(
            "document.snapshot",
            json!({"document_id": "d1"}),
            Some("persist-1".to_owned()),
        )
        .await
        .expect_err("the server refuses every session");

    assert!(
        matches!(error, ClientError::Status { status, .. } if status == StatusCode::UNAUTHORIZED),
        "got {error}"
    );
    assert_eq!(
        fake.session_fetches(),
        2,
        "one fetch for the first session and one refresh, and no more"
    );
    let attempt = Attempt {
        token: Some("token-b".to_owned()),
        request_id: Some("persist-1".to_owned()),
    };
    assert_eq!(
        fake.operation_attempts(),
        vec![attempt.clone(), attempt],
        "two sends of one request, and no third"
    );
}

#[tokio::test]
async fn a_reply_over_the_limit_is_an_error_not_a_truncated_list() {
    let router = Router::new()
        .route("/api/v1/session", get(fixed_session))
        .route("/api/v1/documents", get(oversized_documents));
    let served = Served::start(router).await;
    let client = client_for(&served.url);

    let error = client
        .list_documents()
        .await
        .expect_err("the reply is larger than the limit");

    assert!(matches!(error, ClientError::ReplyTooLarge), "got {error}");
}

#[tokio::test]
async fn a_session_that_names_another_header_is_refused() {
    let router = Router::new().route("/api/v1/session", get(foreign_header_session));
    let served = Served::start(router).await;
    let client = client_for(&served.url);

    let error = client
        .list_documents()
        .await
        .expect_err("the session names a header that is not the API's");

    assert!(
        matches!(error, ClientError::MalformedReply { .. }),
        "got {error}"
    );
}

async fn foreign_header_session() -> Json<Value> {
    Json(json!({"api_version": "v1", "header": "host", "token": "t"}))
}

#[tokio::test]
async fn an_explicit_address_takes_precedence_over_the_recorded_one() {
    let server = TestServer::start().await;
    let lock = ServerLock::acquire(server.data.path())
        .expect("the test data directory has no other server");
    lock.record_url(&format!("http://127.0.0.1:{}", closed_port()))
        .expect("the address is recorded");
    let explicit: ServerUrl = server
        .url
        .parse()
        .expect("the test address is a loopback address");
    let client = ApiClient::new(Discovery::new(
        Some(explicit),
        Some(server.data.path().to_path_buf()),
    ));

    let list = client
        .list_documents()
        .await
        .expect("the explicit address is used, not the recorded one");

    assert_eq!(list.count, 0, "a new data directory holds no documents");
}

#[tokio::test]
async fn a_server_that_stopped_is_replaced_by_the_server_recorded_next() {
    let data = tempfile::tempdir().expect("a temporary data directory can be created");
    let lock =
        ServerLock::acquire(data.path()).expect("the test data directory has no other server");
    let mut first = Served::start(
        Router::new()
            .route("/api/v1/session", get(closing_session))
            .route("/api/v1/documents", get(closing_documents)),
    )
    .await;
    lock.record_url(&first.url)
        .expect("the address is recorded");
    let client = ApiClient::new(Discovery::new(None, Some(data.path().to_path_buf())));
    client
        .list_documents()
        .await
        .expect("the first server answers");

    // The second server starts while the first still holds its address, so the two addresses differ and
    // the stopped address cannot be answered by the second server.
    let second = TestServer::start().await;
    create_document(&second.http(), "mcp-replaced-1").await;
    first.stop().await;

    let refused = client
        .list_documents()
        .await
        .expect_err("the cached session points at the stopped server");
    assert!(
        matches!(refused, ClientError::Unreachable { .. }),
        "got {refused}"
    );

    lock.record_url(&second.url)
        .expect("the new address is recorded");
    let list = client
        .list_documents()
        .await
        .expect("the call after the refusal finds the server recorded now");
    assert_eq!(
        list.count, 1,
        "the list comes from the second server, which holds one document"
    );
}

#[tokio::test]
async fn a_forbidden_answer_is_reported_without_a_fresh_session() {
    // 403 is the API's Host or Origin guard; a fresh token cannot change it.
    let fake = FakeServer::start(StatusCode::FORBIDDEN, "token-a", "token-b").await;
    let client = client_for(fake.url());

    let error = client
        .run_operation(
            "document.snapshot",
            json!({"document_id": "d1"}),
            Some("forbidden-1".to_owned()),
        )
        .await
        .expect_err("the server forbids every request");

    assert!(
        matches!(error, ClientError::Status { status, .. } if status == StatusCode::FORBIDDEN),
        "got {error}"
    );
    assert_eq!(fake.session_fetches(), 1, "no fresh session after a 403");
    assert_eq!(
        fake.operation_attempts().len(),
        1,
        "the request is sent once"
    );
}

#[tokio::test]
async fn a_refusal_other_than_401_is_reported_without_a_second_send() {
    let fake = FakeServer::start(StatusCode::INTERNAL_SERVER_ERROR, "token-a", "token-b").await;
    let client = client_for(fake.url());

    let error = client
        .run_operation(
            "document.snapshot",
            json!({"document_id": "d1"}),
            Some("server-error-1".to_owned()),
        )
        .await
        .expect_err("the server answers every request with a server error");

    assert!(
        matches!(error, ClientError::Status { status, .. } if status == StatusCode::INTERNAL_SERVER_ERROR),
        "got {error}"
    );
    assert_eq!(
        fake.session_fetches(),
        1,
        "a server error does not refuse the session, so no session is fetched again"
    );
    assert_eq!(
        fake.operation_attempts().len(),
        1,
        "the request is sent once"
    );
}

fn client_for(url: &str) -> ApiClient {
    let url: ServerUrl = url.parse().expect("the test address is a loopback address");
    ApiClient::new(Discovery::new(Some(url), None))
}

/// Applies one `add_atom` transaction through the API. The edit is not saved.
async fn add_carbon(
    http: &Http,
    document_id: &str,
    request_id: &str,
    expected_revision: u64,
) -> Value {
    let body = json!({
        "request_id": request_id,
        "expected_revision": expected_revision,
        "commands": [{"op": "add_atom", "element": "C", "position": {"x": 0, "y": 0}}]
    });
    let (status, reply) = http
        .request(
            Method::POST,
            &format!("/api/v1/documents/{document_id}/transactions"),
            Some(&body),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the transaction was refused: {reply}"
    );
    reply
}

/// One operation request that reached the fake server.
#[derive(Clone, Debug, PartialEq)]
struct Attempt {
    token: Option<String>,
    request_id: Option<String>,
}

/// The fake server's state. It accepts requests that carry `accepted`, and its session endpoint issues
/// `issued`.
struct FakeState {
    accepted: Mutex<String>,
    issued: Mutex<String>,
    refusal: StatusCode,
    session_fetches: AtomicUsize,
    attempts: Mutex<Vec<Attempt>>,
}

impl FakeState {
    fn accepts(&self, headers: &HeaderMap) -> bool {
        let accepted = self
            .accepted
            .lock()
            .expect("the fake state is not poisoned");
        presented_token(headers).as_deref() == Some(accepted.as_str())
    }

    fn refuse(&self) -> Response {
        (self.refusal, Json(json!({"error": "refused"}))).into_response()
    }
}

/// A server that refuses a session with `refusal`, and that answers the two calls which the refresh
/// tests make.
struct FakeServer {
    served: Served,
    state: Arc<FakeState>,
}

impl FakeServer {
    /// A fake whose session endpoint issues `issued`, and which accepts only `accepted`.
    async fn start(refusal: StatusCode, accepted: &str, issued: &str) -> Self {
        let state = Arc::new(FakeState {
            accepted: Mutex::new(accepted.to_owned()),
            issued: Mutex::new(issued.to_owned()),
            refusal,
            session_fetches: AtomicUsize::new(0),
            attempts: Mutex::new(Vec::new()),
        });
        let router = Router::new()
            .route("/api/v1/session", get(fake_session))
            .route("/api/v1/documents", get(fake_documents))
            .route("/api/v1/operations", post(fake_operations))
            .with_state(Arc::clone(&state));
        Self {
            served: Served::start(router).await,
            state,
        }
    }

    fn url(&self) -> &str {
        &self.served.url
    }

    /// The server changes its token. From now on it accepts and issues `token` only, so every session
    /// that a client holds is refused.
    fn rotate(&self, token: &str) {
        *self
            .state
            .accepted
            .lock()
            .expect("the fake state is not poisoned") = token.to_owned();
        *self
            .state
            .issued
            .lock()
            .expect("the fake state is not poisoned") = token.to_owned();
    }

    fn session_fetches(&self) -> usize {
        self.state.session_fetches.load(Ordering::SeqCst)
    }

    fn operation_attempts(&self) -> Vec<Attempt> {
        self.state
            .attempts
            .lock()
            .expect("the fake state is not poisoned")
            .clone()
    }
}

/// A router served on a loopback port. The server stops when the value is dropped.
struct Served {
    url: String,
    task: JoinHandle<()>,
}

impl Served {
    /// Stops the server and waits until its task has ended, so that its listener is closed and
    /// connections to its address are refused.
    async fn stop(&mut self) {
        self.task.abort();
        let _ = (&mut self.task).await;
    }

    async fn start(router: Router) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a loopback port is free");
        let port = listener
            .local_addr()
            .expect("the listener has an address")
            .port();
        let task = tokio::spawn(async move {
            axum::serve(listener, router)
                .await
                .expect("the fake server runs until it is dropped");
        });
        Self {
            url: format!("http://127.0.0.1:{port}"),
            task,
        }
    }
}

impl Drop for Served {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn presented_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get(SESSION_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

async fn fake_session(State(state): State<Arc<FakeState>>) -> Json<Value> {
    state.session_fetches.fetch_add(1, Ordering::SeqCst);
    let token = state
        .issued
        .lock()
        .expect("the fake state is not poisoned")
        .clone();
    Json(json!({"api_version": "v1", "header": SESSION_HEADER, "token": token}))
}

async fn fake_documents(State(state): State<Arc<FakeState>>, headers: HeaderMap) -> Response {
    if state.accepts(&headers) {
        Json(json!({
            "api_version": "v1",
            "documents": [],
            "unreadable": [],
            "truncated": false
        }))
        .into_response()
    } else {
        state.refuse()
    }
}

async fn fake_operations(
    State(state): State<Arc<FakeState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    // A body that is not JSON records no request id, which fails the test's comparison.
    let request: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    state
        .attempts
        .lock()
        .expect("the fake state is not poisoned")
        .push(Attempt {
            token: presented_token(&headers),
            request_id: request["request_id"].as_str().map(str::to_owned),
        });
    if state.accepts(&headers) {
        Json(json!({
            "envelope": "operation_result",
            "outcome": "ok",
            "document_revision": 1,
            "result": {}
        }))
        .into_response()
    } else {
        state.refuse()
    }
}

async fn fixed_session() -> Json<Value> {
    Json(json!({"api_version": "v1", "header": SESSION_HEADER, "token": "fixed-token"}))
}

/// Answers like the session endpoint, and closes the connection after the reply, so that the client keeps
/// no connection to this server in its pool.
async fn closing_session() -> impl IntoResponse {
    (
        [(header::CONNECTION, "close")],
        Json(json!({"api_version": "v1", "header": SESSION_HEADER, "token": "closing-token"})),
    )
}

async fn closing_documents() -> impl IntoResponse {
    (
        [(header::CONNECTION, "close")],
        Json(json!({"api_version": "v1", "documents": [], "unreadable": [], "truncated": false})),
    )
}

async fn oversized_documents() -> Vec<u8> {
    vec![b' '; MAX_REPLY_BYTES + 1]
}
