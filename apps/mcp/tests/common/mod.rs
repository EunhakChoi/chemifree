//! Shared helpers for the integration tests: an in-process Chemifree API server on a loopback port, the
//! HTTP calls that the web editor makes to it, and loopback ports with nothing listening on them.

#![allow(
    dead_code,
    reason = "each integration test uses part of these shared helpers"
)]

use std::net::TcpListener as StdTcpListener;
use std::sync::Arc;

use bytes::Bytes;
use chemifree_api::{ApiState, LocalApplication, SESSION_HEADER, router};
use chemifree_providers::{FileDocumentRepository, FilePreferenceStore, LocalCapabilities};
use http_body_util::{BodyExt, Full};
use hyper::{Method, Request, StatusCode, header};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

/// The session token of every test server.
pub const TOKEN: &str = "mcp-test-token";

/// A `chemifree-api` router on a loopback port, over a temporary data directory. The server stops when
/// the value is dropped.
pub struct TestServer {
    pub url: String,
    /// The instance this server gives in its session reply; record it with
    /// `ServerLock::record_server`.
    pub instance: String,
    pub data: TempDir,
    task: JoinHandle<()>,
}

impl TestServer {
    /// Starts a server over an empty data directory. The port is bound first, because the API accepts
    /// only the `Host` header of the port that its state was built for.
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a loopback port is free");
        let port = listener
            .local_addr()
            .expect("the listener has an address")
            .port();
        let data = tempfile::tempdir().expect("a temporary data directory can be created");
        let application =
            LocalApplication::new(LocalCapabilities, FileDocumentRepository::new(data.path()))
                .with_preferences(FilePreferenceStore::new(data.path()));
        // Unique per server in a test run: ports differ while both are bound.
        let instance = format!("{port:04x}");
        let state = ApiState::new(application, TOKEN.to_owned(), port, &[])
            .expect("the test server state is valid")
            .with_instance(instance.clone());
        let app = router(Arc::new(state));
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("the test server runs until it is dropped");
        });
        Self {
            url: format!("http://127.0.0.1:{port}"),
            instance,
            data,
            task,
        }
    }

    /// HTTP calls to this server, sent the way the web editor sends them.
    pub fn http(&self) -> Http {
        Http::new(&self.url, TOKEN)
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// HTTP calls to one server, with the `Host` header and the session header that the web editor sends.
pub struct Http {
    client: Client<HttpConnector, Full<Bytes>>,
    url: String,
    authority: String,
    token: String,
}

impl Http {
    /// Calls to the server at `url`, which has the form `http://127.0.0.1:<port>`.
    pub fn new(url: &str, token: &str) -> Self {
        Self {
            client: Client::builder(TokioExecutor::new()).build_http(),
            url: url.to_owned(),
            authority: url.trim_start_matches("http://").to_owned(),
            token: token.to_owned(),
        }
    }

    /// Sends one request and returns its status and its JSON body, which is `null` when the body is
    /// empty. A body that is not JSON fails the test.
    pub async fn request(
        &self,
        method: Method,
        path: &str,
        body: Option<&Value>,
    ) -> (StatusCode, Value) {
        let mut builder = Request::builder()
            .method(method)
            .uri(format!("{}{path}", self.url))
            .header(header::HOST, &self.authority)
            .header(SESSION_HEADER, &self.token);
        let payload = match body {
            Some(value) => {
                builder = builder.header(header::CONTENT_TYPE, "application/json");
                Bytes::from(value.to_string())
            }
            None => Bytes::new(),
        };
        let request = builder
            .body(Full::new(payload))
            .expect("the test request is valid");
        let response = self
            .client
            .request(request)
            .await
            .expect("the test server answers");
        let status = response.status();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("the reply body is read")
            .to_bytes();
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or_else(|error| {
                panic!(
                    "the reply is not JSON ({error}): {}",
                    String::from_utf8_lossy(&bytes)
                )
            })
        };
        (status, value)
    }
}

/// Creates a document through the API and returns its id.
pub async fn create_document(http: &Http, request_id: &str) -> String {
    let body = json!({"request_id": request_id});
    let (status, reply) = http
        .request(Method::POST, "/api/v1/documents", Some(&body))
        .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "creating a document failed: {reply}"
    );
    reply["document"]["id"]
        .as_str()
        .expect("the created document has an id")
        .to_owned()
}

/// A loopback port with nothing listening on it: it was bound and released just before this returns.
pub fn closed_port() -> u16 {
    let listener = StdTcpListener::bind("127.0.0.1:0").expect("a loopback port is free");
    listener
        .local_addr()
        .expect("the listener has an address")
        .port()
}
