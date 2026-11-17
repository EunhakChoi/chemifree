//! The HTTP client of the running Chemifree API server.
//!
//! The client makes the calls that the web editor makes: `GET /api/v1/session` for the session header
//! name and token, `GET /api/v1/documents`, and `POST /api/v1/operations` for one request of the
//! operation contract. It keeps one session, the one from the server it last reached, and sends it
//! with every call. A server that refuses the session (401, the answer to a missing or stale token) gets
//! one fresh session and one more send of the same request, and no more. A 403 is the server's Host or
//! Origin guard, which a fresh token cannot change, so it is reported as it is. A transport failure drops the session, so the next call looks
//! the server up again. There is no client-side timeout: an operation may run for as long as its own
//! time limit allows, and the server answers when it is done.

use std::fmt;
use std::sync::Mutex;

use bytes::Bytes;
use http_body_util::{BodyExt, Full, LengthLimitError, Limited};
use hyper::header::{HeaderName, HeaderValue};
use hyper::{Method, Request, StatusCode, Uri, header};
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::{Client, Error as LegacyError};
use hyper_util::rt::TokioExecutor;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::discovery::{Discovery, DiscoveryError, ServerUrl};

/// The largest reply body that is read. A larger reply is an error, never a truncated reply.
pub const MAX_REPLY_BYTES: usize = 8 * 1024 * 1024;

/// The operation contract version that every request names.
const CONTRACT: u32 = 1;
/// The operation profile that every request asks for.
/// The session header of the Chemifree API (`chemifree_api::SESSION_HEADER`).
const SESSION_HEADER: &str = "x-chemifree-session";
const PROFILE: &str = "preserve-semantics";
const SESSION_PATH: &str = "/api/v1/session";
const DOCUMENTS_PATH: &str = "/api/v1/documents";
const OPERATIONS_PATH: &str = "/api/v1/operations";

/// The client of one running server, found through [`Discovery`].
#[derive(Debug)]
pub struct ApiClient {
    http: Client<HttpConnector, Full<Bytes>>,
    discovery: Discovery,
    /// The session of the server last reached. The lock is never held across an await.
    session: Mutex<Option<Session>>,
}

/// A session of one server: the header that carries the token, and the token. The token is marked
/// sensitive, so the `Debug` output of the header value does not print it.
#[derive(Clone, Debug)]
struct Session {
    url: ServerUrl,
    /// The instance the server must give, when it was found through a record.
    instance: Option<String>,
    header: HeaderName,
    token: HeaderValue,
}

/// The fields of `GET /api/v1/session` that the client uses. The other fields are ignored.
#[derive(Deserialize)]
struct SessionReply {
    header: String,
    token: String,
    #[serde(default)]
    instance: Option<String>,
}

/// A reply read in full: its status and its body.
struct Reply {
    status: StatusCode,
    body: Bytes,
}

/// The reply to `GET /api/v1/documents`: the server's JSON, and the number of documents it lists.
#[derive(Clone, Debug, PartialEq)]
pub struct DocumentList {
    /// The number of entries in the reply's `documents` array.
    pub count: usize,
    /// The reply, unchanged.
    pub reply: Value,
}

impl ApiClient {
    /// A client that finds its server through `discovery`. Nothing is contacted until a call is made.
    #[must_use]
    pub fn new(discovery: Discovery) -> Self {
        Self {
            http: Client::builder(TokioExecutor::new()).build_http(),
            discovery,
            session: Mutex::new(None),
        }
    }

    /// Lists the documents of the running server.
    ///
    /// # Errors
    ///
    /// Returns an error when the server cannot be found or does not answer, when it answers with a
    /// status other than 200 even after a session refresh, or when the reply is too large or is not a
    /// document list.
    pub async fn list_documents(&self) -> Result<DocumentList, ClientError> {
        let reply = self.authorized(Method::GET, DOCUMENTS_PATH, None).await?;
        if reply.status != StatusCode::OK {
            return Err(reply.into_status_error());
        }
        let status = reply.status;
        let value = reply.into_object()?;
        let count = value
            .get("documents")
            .and_then(Value::as_array)
            .map(Vec::len)
            .ok_or_else(|| ClientError::MalformedReply {
                status,
                message: "the document list has no documents array".to_owned(),
            })?;
        Ok(DocumentList {
            count,
            reply: value,
        })
    }

    /// Runs one operation of the contract and returns the server's reply, as JSON.
    ///
    /// The reply is an operation result (HTTP 200, whatever its outcome) or a protocol error (HTTP
    /// 400). The request id is `request_id` when one is given, and otherwise a new random one.
    ///
    /// # Errors
    ///
    /// Returns an error when the server cannot be found or does not answer, when it answers with any
    /// other status even after a session refresh, when the reply is too large, or when the reply is not
    /// a JSON object. An operation that failed inside the contract is not an error here: its reply says
    /// so.
    pub async fn run_operation(
        &self,
        operation: &str,
        parameters: Value,
        request_id: Option<String>,
    ) -> Result<Value, ClientError> {
        let request_id = match request_id {
            Some(id) => id,
            None => new_request_id()?,
        };
        let envelope = operation_envelope(&request_id, operation, parameters);
        let body = Bytes::from(envelope.to_string());
        let reply = self
            .authorized(Method::POST, OPERATIONS_PATH, Some(body))
            .await?;
        match reply.status {
            StatusCode::OK | StatusCode::BAD_REQUEST => reply.into_object(),
            _ => Err(reply.into_status_error()),
        }
    }

    /// Sends one call with the session, and when the server refuses the session, sends the same request
    /// once more with a fresh session from the same server.
    async fn authorized(
        &self,
        method: Method,
        path: &str,
        body: Option<Bytes>,
    ) -> Result<Reply, ClientError> {
        let session = self.current_session().await?;
        let reply = self
            .exchange(
                &session.url,
                method.clone(),
                path,
                Some(&session),
                body.clone(),
            )
            .await?;
        if !is_refusal(reply.status) {
            return Ok(reply);
        }
        // The guards in front of the handlers refuse a request before any handler runs, so the refused
        // request changed nothing. Sending it again with the same request id cannot apply it twice.
        let fresh = self
            .fetch_session(&session.url, session.instance.as_deref())
            .await?;
        self.exchange(&fresh.url, method, path, Some(&fresh), body)
            .await
    }

    /// The cached session, or, when there is none, a new one from the located server.
    async fn current_session(&self) -> Result<Session, ClientError> {
        if let Some(session) = self.cached_session()? {
            return Ok(session);
        }
        let located = self.discovery.locate().await?;
        self.fetch_session(&located.url, located.instance.as_deref())
            .await
    }

    /// Fetches the session of `url` with `GET /api/v1/session`, and caches it. When `instance` is
    /// given, the server must give the same instance, or it is not the server that was recorded.
    async fn fetch_session(
        &self,
        url: &ServerUrl,
        instance: Option<&str>,
    ) -> Result<Session, ClientError> {
        let reply = self
            .exchange(url, Method::GET, SESSION_PATH, None, None)
            .await?;
        if reply.status != StatusCode::OK {
            return Err(reply.into_status_error());
        }
        let status = reply.status;
        let parsed: SessionReply =
            serde_json::from_slice(&reply.body).map_err(|error| ClientError::MalformedReply {
                status,
                message: format!("the session reply is not a session object ({error})"),
            })?;
        // The header is the API's, never one the reply chooses: a process at the recorded address
        // that is not a Chemifree server is refused here rather than given control of a header.
        if parsed.header != SESSION_HEADER {
            return Err(ClientError::MalformedReply {
                status,
                message: format!(
                    "the session names the header {:?}, not {SESSION_HEADER}",
                    parsed.header
                ),
            });
        }
        let name = HeaderName::from_static(SESSION_HEADER);
        if let Some(expected) = instance
            && parsed.instance.as_deref() != Some(expected)
        {
            self.forget_session();
            return Err(ClientError::NotTheRecordedServer { url: url.clone() });
        }
        let mut token =
            HeaderValue::from_str(&parsed.token).map_err(|_| ClientError::MalformedReply {
                status,
                message: "the session token is not a valid header value".to_owned(),
            })?;
        token.set_sensitive(true);
        let session = Session {
            url: url.clone(),
            instance: instance.map(str::to_owned),
            header: name,
            token,
        };
        self.store_session(session.clone())?;
        Ok(session)
    }

    /// Sends one request to `url` and reads the whole reply. A transport failure drops the session.
    async fn exchange(
        &self,
        url: &ServerUrl,
        method: Method,
        path: &str,
        session: Option<&Session>,
        body: Option<Bytes>,
    ) -> Result<Reply, ClientError> {
        let request = build_request(url, method, path, session, body)?;
        let response = match self.http.request(request).await {
            Ok(response) => response,
            Err(error) => {
                self.forget_session();
                return Err(if error.is_connect() {
                    ClientError::Unreachable {
                        url: url.clone(),
                        source: error,
                    }
                } else {
                    ClientError::Transport {
                        url: url.clone(),
                        message: error.to_string(),
                    }
                });
            }
        };
        let status = response.status();
        match Limited::new(response.into_body(), MAX_REPLY_BYTES)
            .collect()
            .await
        {
            Ok(collected) => Ok(Reply {
                status,
                body: collected.to_bytes(),
            }),
            Err(error) if error.is::<LengthLimitError>() => Err(ClientError::ReplyTooLarge),
            Err(error) => {
                self.forget_session();
                Err(ClientError::Transport {
                    url: url.clone(),
                    message: error.to_string(),
                })
            }
        }
    }

    fn cached_session(&self) -> Result<Option<Session>, ClientError> {
        let cached = self
            .session
            .lock()
            .map_err(|_| ClientError::StatePoisoned)?;
        Ok(cached.clone())
    }

    fn store_session(&self, session: Session) -> Result<(), ClientError> {
        let mut cached = self
            .session
            .lock()
            .map_err(|_| ClientError::StatePoisoned)?;
        *cached = Some(session);
        Ok(())
    }

    /// Drops the cached session, so that the next call looks the server up again.
    fn forget_session(&self) {
        // A poisoned lock is not cleared here: the next call reports the poison instead.
        if let Ok(mut cached) = self.session.lock() {
            *cached = None;
        }
    }
}

impl From<DiscoveryError> for ClientError {
    fn from(error: DiscoveryError) -> Self {
        Self::Discovery(error)
    }
}

impl Reply {
    /// The body as a JSON object.
    fn into_object(self) -> Result<Value, ClientError> {
        let value: Value =
            serde_json::from_slice(&self.body).map_err(|error| ClientError::MalformedReply {
                status: self.status,
                message: format!("the body is not JSON ({error})"),
            })?;
        if value.is_object() {
            Ok(value)
        } else {
            Err(ClientError::MalformedReply {
                status: self.status,
                message: "the body is JSON, but not an object".to_owned(),
            })
        }
    }

    fn into_status_error(self) -> ClientError {
        ClientError::Status {
            status: self.status,
            body: String::from_utf8_lossy(&self.body).into_owned(),
        }
    }
}

/// Whether a status means that the server refused the session token, rather than the request itself.
fn is_refusal(status: StatusCode) -> bool {
    status == StatusCode::UNAUTHORIZED
}

/// The request for one call: the path under the server's address, the `Host` header that the API
/// accepts, the session header and token when there is a session, and a JSON body when there is one.
fn build_request(
    url: &ServerUrl,
    method: Method,
    path: &str,
    session: Option<&Session>,
    body: Option<Bytes>,
) -> Result<Request<Full<Bytes>>, ClientError> {
    let invalid = |message: String| ClientError::InvalidRequest {
        url: url.clone(),
        message,
    };
    let uri = format!("{url}{path}")
        .parse::<Uri>()
        .map_err(|error| invalid(error.to_string()))?;
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::HOST, url.authority());
    if let Some(session) = session {
        builder = builder.header(session.header.clone(), session.token.clone());
    }
    if body.is_some() {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
    }
    builder
        .body(Full::new(body.unwrap_or_default()))
        .map_err(|error| invalid(error.to_string()))
}

/// The request envelope of the operation contract, as the server reads it.
#[must_use]
pub fn operation_envelope(request_id: &str, operation: &str, parameters: Value) -> Value {
    json!({
        "contract": CONTRACT,
        "request_id": request_id,
        "profile": PROFILE,
        "operation": operation,
        "parameters": parameters,
    })
}

/// A new request id: 32 random lowercase hexadecimal digits, which the server accepts.
///
/// # Errors
///
/// Returns [`ClientError::RandomSource`] when the operating system has no random bytes to give.
pub fn new_request_id() -> Result<String, ClientError> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(ClientError::RandomSource)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// Why a call to the running server failed. Its `Display` text is the message that a tool reports.
#[derive(Debug)]
#[non_exhaustive]
pub enum ClientError {
    /// The server's address could not be found.
    Discovery(DiscoveryError),
    /// Nothing answered at the address: the connection was refused or could not be made.
    Unreachable { url: ServerUrl, source: LegacyError },
    /// The connection failed after it was made, before the reply was read.
    Transport { url: ServerUrl, message: String },
    /// The request could not be built for the address.
    InvalidRequest { url: ServerUrl, message: String },
    /// The server answered with a status that the call does not accept.
    Status { status: StatusCode, body: String },
    /// The reply is larger than [`MAX_REPLY_BYTES`]. It was not read, so nothing was truncated.
    ReplyTooLarge,
    /// The reply is not the JSON that the call expects.
    MalformedReply { status: StatusCode, message: String },
    /// The server that answered at a recorded address is not the server that wrote the record: that
    /// server stopped, and another one now uses its port.
    NotTheRecordedServer { url: ServerUrl },
    /// The operating system had no random bytes for a request id.
    RandomSource(getrandom::Error),
    /// The session cache is unusable, because a call panicked while it held the cache.
    StatePoisoned,
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Discovery(error) => write!(f, "{error}"),
            Self::Unreachable { url, .. } => write!(
                f,
                "No running Chemifree API server answered at {url}. Start chemifree-api for that data directory, or check --server-url."
            ),
            Self::Transport { url, message } => write!(
                f,
                "The request to the Chemifree API server at {url} failed: {message}."
            ),
            Self::InvalidRequest { url, message } => {
                write!(f, "The request to {url} could not be built: {message}.")
            }
            Self::Status { status, body } if body.is_empty() => {
                write!(f, "The Chemifree API server answered HTTP {status}.")
            }
            Self::Status { status, body } => {
                write!(f, "The Chemifree API server answered HTTP {status}: {body}")
            }
            Self::NotTheRecordedServer { url } => write!(
                f,
                "The Chemifree API server at {url} is not the server recorded for this data directory: that server has stopped, and another one uses its port. Start chemifree-api for this data directory."
            ),
            Self::ReplyTooLarge => write!(
                f,
                "The Chemifree API server's reply is larger than {MAX_REPLY_BYTES} bytes, so it was not read."
            ),
            Self::MalformedReply { status, message } => write!(
                f,
                "The Chemifree API server answered HTTP {status} with a reply that cannot be used: {message}."
            ),
            Self::RandomSource(error) => {
                write!(f, "No random bytes were available for a request id: {error}.")
            }
            Self::StatePoisoned => f.write_str(
                "The client's session state is unusable after an internal failure; restart chemifree-mcp.",
            ),
        }
    }
}

impl std::error::Error for ClientError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Discovery(error) => Some(error),
            Self::Unreachable { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_envelope_has_exactly_the_contract_fields() {
        let envelope = operation_envelope(
            "0123456789abcdef0123456789abcdef",
            "document.snapshot",
            json!({"document_id": "d1"}),
        );
        assert_eq!(
            envelope,
            json!({
                "contract": 1,
                "request_id": "0123456789abcdef0123456789abcdef",
                "profile": "preserve-semantics",
                "operation": "document.snapshot",
                "parameters": {"document_id": "d1"}
            })
        );
    }

    #[test]
    fn a_new_request_id_is_32_lowercase_hex_digits() {
        let id = new_request_id().expect("the operating system has random bytes");
        assert_eq!(id.len(), 32, "request id {id:?}");
        assert!(
            id.bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
            "request id {id:?} must be lowercase hexadecimal"
        );
    }

    #[test]
    fn two_new_request_ids_differ() {
        let first = new_request_id().expect("the operating system has random bytes");
        let second = new_request_id().expect("the operating system has random bytes");
        assert_ne!(first, second, "a request id must not repeat");
    }

    #[test]
    fn a_reply_must_be_a_json_object() {
        let reply = Reply {
            status: StatusCode::OK,
            body: Bytes::from_static(b"[1,2]"),
        };
        assert!(
            matches!(reply.into_object(), Err(ClientError::MalformedReply { .. })),
            "an array is not an operation reply"
        );
    }
}
