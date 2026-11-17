//! The HTTP client of the running Chemifree API server.
//!
//! The client makes the calls that the web editor makes: `GET /api/v1/session` for the session header
//! name and token, `GET /api/v1/documents`, and `POST /api/v1/operations` for one request of the
//! operation contract. It keeps one session, the one from the server it last reached, and sends it
//! with every call. A server that refuses the session (401, the answer to a missing or stale token) gets
//! one fresh session and one more send of the same request, and no more. A 403 is the server's Host or
//! Origin guard, which a fresh token cannot change, so it is reported as it is. A transport failure drops
//! the session, so the next call looks the server up again. There is no client-side timeout: an operation
//! may run for as long as its own time limit allows, and the server answers when it is done.
//!
//! An edit whose outcome is not known is remembered with the instance of the server process that took its
//! send, because the server remembers a request id only within its process. Such an edit is sent again only
//! to that process. When another process answers, the call is refused with [`ClientError::ServerRestarted`]
//! before the edit is sent again.

use std::collections::VecDeque;
use std::fmt;
use std::sync::{Mutex, MutexGuard};

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
/// The session header of the Chemifree API (`chemifree_api::SESSION_HEADER`).
const SESSION_HEADER: &str = "x-chemifree-session";
/// The operation profile that every request asks for.
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
    /// The edits whose outcome is not known. The lock is never held across an await.
    uncertain: Mutex<UncertainEdits>,
}

/// A session of one server: the header that carries the token, and the token. The token is marked
/// sensitive, so the `Debug` output of the header value does not print it.
#[derive(Clone, Debug)]
struct Session {
    url: ServerUrl,
    /// The instance that the record names for this address, when the address was found through a record.
    expected_instance: Option<String>,
    /// The instance that the server named in its session reply: the process that issued the token.
    served_instance: Option<String>,
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

/// The most edits whose outcome is remembered. When the list is full, its oldest entry is dropped, and a repeat of
/// that edit is no longer guarded.
const UNCERTAIN_LIMIT: usize = 256;

/// An edit whose outcome is not known, and the server process that its last send reached.
#[derive(Clone, Debug)]
struct Uncertain {
    request_id: String,
    /// The instance that the process named in its session reply, or `None` when the process named none.
    instance: Option<String>,
}

/// The edits whose outcome is not known, oldest first. A reply that settles an edit removes its entry.
#[derive(Debug, Default)]
struct UncertainEdits {
    entries: VecDeque<Uncertain>,
}

impl UncertainEdits {
    fn get(&self, request_id: &str) -> Option<&Uncertain> {
        self.entries
            .iter()
            .find(|entry| entry.request_id == request_id)
    }

    /// Remembers that the edit `request_id` may have been applied by the process `instance`.
    fn remember(&mut self, request_id: &str, instance: Option<String>) {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.request_id == request_id)
        {
            entry.instance = instance;
            return;
        }
        if self.entries.len() == UNCERTAIN_LIMIT {
            self.entries.pop_front();
        }
        self.entries.push_back(Uncertain {
            request_id: request_id.to_owned(),
            instance,
        });
    }

    /// Forgets the edit `request_id`, whose outcome a reply has settled.
    fn settle(&mut self, request_id: &str) {
        self.entries.retain(|entry| entry.request_id != request_id);
    }
}

impl ApiClient {
    /// A client that finds its server through `discovery`. Nothing is contacted until a call is made.
    #[must_use]
    pub fn new(discovery: Discovery) -> Self {
        Self {
            http: Client::builder(TokioExecutor::new()).build_http(),
            discovery,
            session: Mutex::new(None),
            uncertain: Mutex::new(UncertainEdits::default()),
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
    /// 400). The request id is `request_id` when one is given, and otherwise a new random one. A call
    /// that fails may still have applied the operation: see [`ClientError::may_have_taken_effect`]. An edit goes
    /// through [`ApiClient::run_edit`], which guards a repeat of an edit whose outcome is not known.
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

    /// Runs one edit of the contract under the caller's request id, and returns the server's reply, as JSON.
    ///
    /// An edit whose outcome is not known (see [`ClientError::may_have_taken_effect`]) is remembered with the
    /// server process that its last send reached. The server remembers a request id only within its process, so
    /// such an edit is sent again only to that process, and a call that finds another process is refused with
    /// [`ClientError::ServerRestarted`] before the edit is sent again. A reply that settles the outcome forgets the
    /// request id.
    ///
    /// # Errors
    ///
    /// Returns the errors of [`ApiClient::run_operation`], and [`ClientError::ServerRestarted`] when the server
    /// that answers is not the process that may have taken the edit.
    pub async fn run_edit(
        &self,
        operation: &str,
        parameters: Value,
        request_id: &str,
    ) -> Result<Value, ClientError> {
        let body = Bytes::from(operation_envelope(request_id, operation, parameters).to_string());
        let uncertain = self.lock_uncertain()?.get(request_id).cloned();
        let first = self.edit_session(request_id, uncertain.as_ref()).await?;
        let sent = self
            .exchange(
                &first.url,
                Method::POST,
                OPERATIONS_PATH,
                Some(&first),
                Some(body.clone()),
            )
            .await;
        let (session, reply) = match sent {
            // The session guard refuses a request before the operation runs, so the refused edit was not
            // applied. It is sent once more, with a fresh session that the same checks accept.
            Ok(reply) if is_refusal(reply.status) => {
                let fresh = match &uncertain {
                    Some(entry) => {
                        self.session_for_replay(request_id, entry.instance.as_deref())
                            .await?
                    }
                    None => {
                        self.fetch_session(&first.url, first.expected_instance.as_deref())
                            .await?
                    }
                };
                let reply = self
                    .exchange(
                        &fresh.url,
                        Method::POST,
                        OPERATIONS_PATH,
                        Some(&fresh),
                        Some(body),
                    )
                    .await;
                (fresh, reply)
            }
            reply => (first, reply),
        };
        let outcome = match reply {
            Ok(reply) => {
                let status = reply.status;
                let value = match status {
                    StatusCode::OK | StatusCode::BAD_REQUEST => reply.into_object(),
                    _ => Err(reply.into_status_error()),
                };
                value.map(|value| (status, value))
            }
            Err(error) => Err(error),
        };
        match &outcome {
            Ok((status, value)) if reply_settles_edit(*status, value) => {
                self.lock_uncertain()?.settle(request_id);
            }
            Err(error) if error.may_have_taken_effect() => {
                self.lock_uncertain()?
                    .remember(request_id, session.served_instance.clone());
            }
            // A protocol error or a reused request id says nothing about an earlier send of the id, so the id
            // keeps its entry. A failure that sent nothing, or that was refused with 413, adds none.
            Ok(_) | Err(_) => {}
        }
        outcome.map(|(_, value)| value)
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
            .fetch_session(&session.url, session.expected_instance.as_deref())
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

    /// The session that an edit is sent with: for an edit whose outcome is not known, a session of the process that
    /// may have taken it (see [`Self::session_for_replay`]), and otherwise the cached session.
    async fn edit_session(
        &self,
        request_id: &str,
        uncertain: Option<&Uncertain>,
    ) -> Result<Session, ClientError> {
        match uncertain {
            Some(entry) => {
                self.session_for_replay(request_id, entry.instance.as_deref())
                    .await
            }
            None => self.current_session().await,
        }
    }

    /// A fresh session of the located server, which must be the process that `recorded` names. An edit whose
    /// outcome is not known is replayed only by that process, so any other process is refused here, before the
    /// edit is sent, with [`ClientError::ServerRestarted`].
    async fn session_for_replay(
        &self,
        request_id: &str,
        recorded: Option<&str>,
    ) -> Result<Session, ClientError> {
        let located = self.discovery.locate().await?;
        let session = self
            .fetch_session(&located.url, located.instance.as_deref())
            .await?;
        if may_resend(recorded, session.served_instance.as_deref()) {
            Ok(session)
        } else {
            Err(ClientError::ServerRestarted {
                request_id: request_id.to_owned(),
            })
        }
    }

    fn lock_uncertain(&self) -> Result<MutexGuard<'_, UncertainEdits>, ClientError> {
        self.uncertain
            .lock()
            .map_err(|_| ClientError::StatePoisoned)
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
            expected_instance: instance.map(str::to_owned),
            served_instance: parsed.instance,
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

/// Whether an edit that the server process `recorded` may have applied can be sent to the process `served` now.
/// A request id is remembered only within its process, so only that process can replay the edit. A process that
/// names no instance cannot be shown to be the same one.
#[must_use]
fn may_resend(recorded: Option<&str>, served: Option<&str>) -> bool {
    matches!((recorded, served), (Some(recorded), Some(served)) if recorded == served)
}

/// Whether an edit reply settles the outcome of the edit, so that its request id is forgotten: an operation result
/// other than a reuse of the id. A protocol error (HTTP 400) is refused before the operation runs, and a reuse does
/// not give the outcome of the edit that first used the id, so neither settles an earlier send of the id.
#[must_use]
fn reply_settles_edit(status: StatusCode, reply: &Value) -> bool {
    status == StatusCode::OK && !request_id_reused(reply)
}

/// Whether an operation reply reports that the request id was used before, with other arguments.
fn request_id_reused(reply: &Value) -> bool {
    reply["diagnostics"].as_array().is_some_and(|diagnostics| {
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic["code"] == "request_id_reused")
    })
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
    /// The server that answers is not the process that may have applied the edit `request_id`, whose outcome is
    /// not known. That process remembered the request id, and this one does not, so applying the edit here could repeat it.
    ServerRestarted { request_id: String },
    /// The operating system had no random bytes for a request id.
    RandomSource(getrandom::Error),
    /// The session cache is unusable, because a call panicked while it held the cache.
    StatePoisoned,
}

impl ClientError {
    /// Whether the failed call may have reached the server and applied its operation, so that its outcome
    /// is not known. That holds for a failure after the connection was made, for a reply that was too
    /// large or not usable, and for every status but 413. The server refuses a body over its size limit
    /// before the operation runs, so a 413 means that it was not applied. Refusals before the handler, such
    /// as 401 and 403, also count as possibly applied, and so does a failure while the session is fetched,
    /// before the operation is sent; those answers are conservative. A refused connection sent nothing. A call
    /// that [`ClientError::ServerRestarted`] refused was either not sent or refused by the session guard before
    /// the operation ran, so it was not applied.
    #[must_use]
    pub fn may_have_taken_effect(&self) -> bool {
        match self {
            Self::Transport { .. } | Self::ReplyTooLarge | Self::MalformedReply { .. } => true,
            Self::Status { status, .. } => *status != StatusCode::PAYLOAD_TOO_LARGE,
            Self::Discovery(_)
            | Self::Unreachable { .. }
            | Self::InvalidRequest { .. }
            | Self::NotTheRecordedServer { .. }
            | Self::ServerRestarted { .. }
            | Self::RandomSource(_)
            | Self::StatePoisoned => false,
        }
    }
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
            Self::ServerRestarted { request_id } => write!(
                f,
                "The Chemifree API server restarted after request {request_id} may have taken effect, so it no longer remembers that request and would apply it again. Read the document (chemifree_documents_list or chemifree_document_snapshot) to learn whether it was applied."
            ),
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

    #[test]
    fn only_a_failure_after_a_request_may_have_reached_the_server_may_have_taken_effect() {
        let url: ServerUrl = "http://127.0.0.1:8080"
            .parse()
            .expect("the test address is a loopback address");
        let cases = [
            (
                ClientError::Transport {
                    url: url.clone(),
                    message: "connection reset".to_owned(),
                },
                true,
            ),
            (ClientError::ReplyTooLarge, true),
            (
                ClientError::MalformedReply {
                    status: StatusCode::OK,
                    message: "the body is not JSON".to_owned(),
                },
                true,
            ),
            (
                ClientError::Status {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    body: String::new(),
                },
                true,
            ),
            (
                ClientError::Status {
                    status: StatusCode::UNAUTHORIZED,
                    body: String::new(),
                },
                true,
            ),
            (
                ClientError::Status {
                    status: StatusCode::PAYLOAD_TOO_LARGE,
                    body: String::new(),
                },
                false,
            ),
            (ClientError::NotTheRecordedServer { url }, false),
            (
                ClientError::ServerRestarted {
                    request_id: "req-1".to_owned(),
                },
                false,
            ),
            (
                ClientError::Discovery(DiscoveryError::NoDataDirectory),
                false,
            ),
            (ClientError::StatePoisoned, false),
        ];
        for (error, expected) in cases {
            assert_eq!(error.may_have_taken_effect(), expected, "{error}");
        }
    }

    #[test]
    fn may_resend_only_to_the_process_that_took_the_send() {
        let cases = [
            (Some("a1"), Some("a1"), true),
            (Some("a1"), Some("b2"), false),
            (Some("a1"), None, false),
            (None, Some("a1"), false),
            (None, None, false),
        ];
        for (recorded, served, expected) in cases {
            assert_eq!(
                may_resend(recorded, served),
                expected,
                "recorded {recorded:?}, served {served:?}"
            );
        }
    }

    #[test]
    fn only_an_operation_result_that_does_not_reuse_the_request_id_settles_an_edit() {
        let applied = json!({"envelope": "operation_result", "outcome": "ok"});
        let refused = json!({
            "envelope": "operation_result",
            "outcome": "refused",
            "diagnostics": [{"code": "revision_conflict"}]
        });
        let reused = json!({
            "envelope": "operation_result",
            "outcome": "refused",
            "diagnostics": [{"code": "request_id_reused"}]
        });
        let protocol = json!({"envelope": "protocol_error", "outcome": "error"});
        let cases = [
            (StatusCode::OK, &applied, true),
            (StatusCode::OK, &refused, true),
            (StatusCode::OK, &reused, false),
            (StatusCode::BAD_REQUEST, &protocol, false),
        ];
        for (status, reply, expected) in cases {
            assert_eq!(
                reply_settles_edit(status, reply),
                expected,
                "status {status}, reply {reply}"
            );
        }
    }

    #[test]
    fn remembered_edits_are_found_and_settled_by_request_id_and_bounded() {
        let mut edits = UncertainEdits::default();
        edits.remember("edit-1", Some("a1".to_owned()));
        edits.remember("edit-1", Some("a1".to_owned()));
        assert_eq!(edits.entries.len(), 1, "one request id is one entry");
        assert_eq!(
            edits
                .get("edit-1")
                .and_then(|entry| entry.instance.as_deref()),
            Some("a1")
        );
        edits.settle("edit-1");
        assert!(edits.get("edit-1").is_none(), "a settled edit is forgotten");

        for index in 0..=UNCERTAIN_LIMIT {
            edits.remember(&format!("edit-{index}"), Some("a1".to_owned()));
        }
        assert_eq!(
            edits.entries.len(),
            UNCERTAIN_LIMIT,
            "the list holds at most {UNCERTAIN_LIMIT} edits"
        );
        assert!(
            edits.get("edit-0").is_none(),
            "the oldest edit is dropped when the list is full"
        );
        assert!(
            edits.get(&format!("edit-{UNCERTAIN_LIMIT}")).is_some(),
            "the newest edit is kept"
        );
    }

    #[test]
    fn the_restarted_server_message_names_the_request_and_the_documents_to_read() {
        let error = ClientError::ServerRestarted {
            request_id: "req-1".to_owned(),
        };
        assert_eq!(
            error.to_string(),
            "The Chemifree API server restarted after request req-1 may have taken effect, so it no longer remembers that request and would apply it again. Read the document (chemifree_documents_list or chemifree_document_snapshot) to learn whether it was applied."
        );
    }
}
