//! Stopping the host cancels its computations. After `ApiState::cancel_computations`, an
//! aromaticity conversion over HTTP is answered `cancelled` with status 200 and changes nothing,
//! while the read routes, health and a read-only operation are still served.
//!
//! No test waits on the clock: the cancel comes before the request, so the refusal is immediate.
//! The only wait is the 30 s bound on a reply, which a passing test never reaches.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use chemifree_api::{ApiState, LocalApplication, SESSION_HEADER, router};
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};

const TOKEN: &str = "shutdown-token";
/// Long enough for any reply that is not blocked; short enough to end a failing run.
const WAIT: Duration = Duration::from_secs(30);

/// A loopback server running on its own runtime, which stops when this is dropped. The test keeps
/// the state, as `main` keeps the state it hands to its shutdown hook.
struct Served {
    port: u16,
    state: Arc<ApiState>,
    _runtime: tokio::runtime::Runtime,
}

fn serve(application: LocalApplication) -> Served {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let state = Arc::new(ApiState::new(application, TOKEN.into(), port, &[]).unwrap());
    let routes = router(Arc::clone(&state));
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.spawn(async move {
        let listener = tokio::net::TcpListener::from_std(listener).unwrap();
        axum::serve(listener, routes).await.unwrap();
    });
    Served {
        port,
        state,
        _runtime: runtime,
    }
}

/// Creates a document holding a Kekulé benzene at revision 1: six carbons with alternating double
/// and single bonds, which the conversion to aromatic bonds would change.
fn kekule_benzene(application: &LocalApplication) -> String {
    let id = application
        .create_document(None, Some("benzene"))
        .unwrap()
        .document
        .id;
    let mut commands: Vec<Value> = (0..6)
        .map(|i| json!({"op": "add_atom", "element": "C", "position": {"x": i * 20, "y": 0}}))
        .collect();
    commands.extend((0..6).map(|i| {
        let order = if i % 2 == 0 { "double" } else { "single" };
        json!({"op": "add_bond", "begin": format!("${i}"), "end": format!("${}", (i + 1) % 6), "order": order})
    }));
    let request = json!({"contract": 1, "request_id": "draw", "profile": "preserve-semantics",
        "operation": "document.transaction",
        "parameters": {"document_id": id, "expected_revision": 0, "commands": commands}});
    let reply = serde_json::to_value(application.execute(request.to_string().as_bytes())).unwrap();
    assert_eq!(reply["outcome"], "ok", "{reply}");
    id
}

/// A `chem.aromaticity_convert` request that would turn the benzene's bonds aromatic.
fn convert(document: &str) -> Vec<u8> {
    json!({"contract": 1, "request_id": "convert", "profile": "preserve-semantics",
        "operation": "chem.aromaticity_convert",
        "parameters": {"document_id": document, "expected_revision": 1, "to": "aromatic"}})
    .to_string()
    .into_bytes()
}

/// A read-only `document.snapshot` request.
fn snapshot(document: &str) -> Vec<u8> {
    json!({"contract": 1, "request_id": "snapshot", "profile": "preserve-semantics",
        "operation": "document.snapshot", "parameters": {"document_id": document}})
    .to_string()
    .into_bytes()
}

/// Sends one HTTP/1.1 request that asks for the connection to close, and returns the status code
/// and the JSON body of the reply.
fn exchange(port: u16, head: &str, body: &[u8]) -> (u16, Value) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(WAIT)).unwrap();
    stream.write_all(head.as_bytes()).unwrap();
    stream.write_all(body).unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap();
    let split = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("an HTTP reply")
        + 4;
    let status_line = String::from_utf8_lossy(&raw[..split]);
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .expect("a status code")
        .parse()
        .unwrap();
    (status, serde_json::from_slice(&raw[split..]).unwrap())
}

/// `GET` of `path`, which needs no session token.
fn get(port: u16, path: &str) -> (u16, Value) {
    let head =
        format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    exchange(port, &head, &[])
}

/// `POST /api/v1/operations`, which needs the session token.
fn operate(port: u16, body: &[u8]) -> (u16, Value) {
    let head = format!(
        "POST /api/v1/operations HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{SESSION_HEADER}: {TOKEN}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    exchange(port, &head, body)
}

#[test]
fn after_cancel_computations_a_conversion_over_http_is_answered_200_cancelled_and_changes_nothing()
{
    let data = tempfile::tempdir().unwrap();
    let application =
        LocalApplication::new(LocalCapabilities, FileDocumentRepository::new(data.path()))
            .with_compute_concurrency(1);
    let document = kekule_benzene(&application);
    let served = serve(application);
    let (status, before) = get(served.port, &format!("/api/v1/documents/{document}"));
    assert_eq!(status, 200, "{before}");
    assert_eq!(before["document"]["revision"], 1, "{before}");

    served.state.cancel_computations();

    let (status, reply) = operate(served.port, &convert(&document));
    assert_eq!(
        status, 200,
        "a stopped computation is an operation reply, not an HTTP error: {reply}"
    );
    assert_eq!(reply["outcome"], "error", "{reply}");
    assert_eq!(reply["diagnostics"][0]["code"], "cancelled", "{reply}");
    assert!(reply["result"].is_null(), "{reply}");
    let (status, after) = get(served.port, &format!("/api/v1/documents/{document}"));
    assert_eq!(status, 200, "{after}");
    assert_eq!(after["document"]["revision"], 1, "{after}");
    assert_eq!(
        after, before,
        "a refused conversion must leave the document exactly as it was"
    );
}

#[test]
fn after_cancel_computations_reads_and_health_are_still_served() {
    let data = tempfile::tempdir().unwrap();
    let application =
        LocalApplication::new(LocalCapabilities, FileDocumentRepository::new(data.path()))
            .with_compute_concurrency(1);
    let document = kekule_benzene(&application);
    let served = serve(application);

    served.state.cancel_computations();

    let (status, health) = get(served.port, "/api/v1/health");
    assert_eq!(status, 200, "{health}");
    assert_eq!(health["status"], "ok", "{health}");
    let (status, read) = get(served.port, &format!("/api/v1/documents/{document}"));
    assert_eq!(status, 200, "{read}");
    assert_eq!(read["document"]["id"], document.as_str(), "{read}");
    assert_eq!(read["document"]["revision"], 1, "{read}");
    let (status, list) = get(served.port, "/api/v1/documents");
    assert_eq!(status, 200, "{list}");
    assert_eq!(list["documents"][0]["id"], document.as_str(), "{list}");
    assert_eq!(list["documents"][0]["revision"], 1, "{list}");
    let (status, reply) = operate(served.port, &snapshot(&document));
    assert_eq!(status, 200, "{reply}");
    assert_eq!(reply["outcome"], "ok", "{reply}");
    assert_eq!(reply["document_revision"], 1, "{reply}");
}
