//! Closing the connection of a running `chem.aromaticity_convert` request stops its computation.
//!
//! The request is a document of 21 disjoint ring systems that each exhaust the 100,000-step
//! Kekulé search budget. An optimised build computes it in about 0.2 s and a debug build in
//! about four seconds. A second request with the same `request_id` and a time limit of 0 tells
//! whether the first still runs: it is answered `request_in_progress` until the first has
//! ended, and `time_limit` after.

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chemifree_api::{ApiState, LocalApplication, SESSION_HEADER, router};
use chemifree_application::ComputeLimits;
use chemifree_providers::{FileDocumentRepository, LocalCapabilities};
use serde_json::{Value, json};

const TOKEN: &str = "disconnect-token";
const COPIES: usize = 21;
const REQUEST: &str = "slow";
/// How long the conversion has been running when the client closes its connection. An optimised
/// build (`CARGO_PROFILE_TEST_OPT_LEVEL=1`) takes about 0.2 s in all and a debug build about four
/// seconds, so the close finds the computation running in either.
const RUNNING_BEFORE_CLOSE: Duration = Duration::from_millis(100);

/// An aromatic system of 93 atoms and 121 bonds with no Kekulé assignment, found only after
/// more than the 100,000-step budget: the search is exhausted, which takes the same time every
/// run. The same edges as the domain test `a_kekule_search_that_exhausts_the_step_budget`.
const EDGES: [(usize, usize); 121] = [
    (0, 15),
    (0, 31),
    (0, 34),
    (1, 39),
    (1, 52),
    (2, 53),
    (2, 73),
    (2, 75),
    (3, 9),
    (3, 44),
    (3, 68),
    (4, 33),
    (4, 35),
    (4, 73),
    (5, 21),
    (5, 42),
    (5, 61),
    (6, 28),
    (6, 62),
    (6, 92),
    (7, 68),
    (7, 71),
    (8, 25),
    (8, 31),
    (8, 67),
    (9, 18),
    (9, 45),
    (10, 29),
    (10, 41),
    (10, 43),
    (11, 45),
    (11, 81),
    (11, 87),
    (12, 43),
    (12, 56),
    (13, 30),
    (13, 65),
    (13, 84),
    (14, 67),
    (14, 78),
    (14, 91),
    (15, 21),
    (15, 22),
    (16, 55),
    (16, 59),
    (16, 67),
    (17, 61),
    (17, 86),
    (17, 92),
    (18, 71),
    (19, 22),
    (19, 79),
    (19, 87),
    (20, 24),
    (20, 49),
    (20, 89),
    (21, 70),
    (22, 48),
    (23, 59),
    (23, 63),
    (24, 77),
    (24, 90),
    (25, 27),
    (26, 48),
    (26, 66),
    (26, 75),
    (27, 34),
    (28, 29),
    (29, 86),
    (30, 35),
    (30, 52),
    (31, 80),
    (32, 55),
    (32, 63),
    (33, 61),
    (33, 84),
    (34, 87),
    (35, 46),
    (36, 45),
    (36, 79),
    (37, 58),
    (37, 65),
    (37, 72),
    (38, 47),
    (38, 83),
    (39, 65),
    (40, 41),
    (40, 50),
    (40, 56),
    (41, 58),
    (42, 48),
    (42, 73),
    (44, 77),
    (44, 81),
    (46, 53),
    (47, 77),
    (49, 54),
    (50, 72),
    (51, 57),
    (51, 60),
    (51, 74),
    (54, 85),
    (55, 82),
    (57, 66),
    (58, 76),
    (60, 64),
    (62, 78),
    (64, 88),
    (66, 79),
    (69, 74),
    (69, 88),
    (70, 80),
    (70, 92),
    (74, 75),
    (76, 84),
    (76, 86),
    (78, 80),
    (81, 89),
    (82, 91),
    (83, 90),
    (85, 90),
];

fn operation(request_id: &str, document: &str, parameters: Value) -> Vec<u8> {
    let mut parameters = parameters;
    parameters["document_id"] = document.into();
    json!({"contract": 1, "request_id": request_id, "profile": "preserve-semantics",
           "operation": "chem.aromaticity_convert", "parameters": parameters})
    .to_string()
    .into_bytes()
}

fn draw_systems(application: &LocalApplication, document: &str) {
    for copy in 0..COPIES {
        let mut commands: Vec<Value> = (0..93)
            .map(|i| json!({"op": "add_atom", "element": "C", "position": {"x": i, "y": copy}}))
            .collect();
        commands.extend(EDGES.iter().map(|&(a, b)| {
            json!({"op": "add_bond", "begin": format!("${a}"), "end": format!("${b}"), "order": "aromatic"})
        }));
        let request = json!({"contract": 1, "request_id": format!("draw-{copy}"),
            "profile": "preserve-semantics", "operation": "document.transaction",
            "parameters": {"document_id": document, "expected_revision": copy, "commands": commands}});
        let reply =
            serde_json::to_value(application.execute(request.to_string().as_bytes())).unwrap();
        assert_eq!(reply["outcome"], "ok", "{reply}");
    }
}

/// Sends the request and returns the connection without reading the reply.
fn send(port: u16, body: &[u8]) -> TcpStream {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let head = format!(
        "POST /api/v1/operations HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{SESSION_HEADER}: {TOKEN}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).unwrap();
    stream.write_all(body).unwrap();
    stream
}

/// The JSON body of the reply, once the server has closed the connection.
fn reply(mut stream: TcpStream) -> Value {
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap();
    let split = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("an HTTP reply")
        + 4;
    assert!(
        raw.starts_with(b"HTTP/1.1 200"),
        "{}",
        String::from_utf8_lossy(&raw)
    );
    serde_json::from_slice(&raw[split..]).unwrap()
}

/// Whether the server has already answered or closed the connection.
fn replied(stream: &TcpStream) -> bool {
    stream.set_nonblocking(true).unwrap();
    let mut byte = [0_u8; 1];
    let ready = match stream.peek(&mut byte) {
        Ok(_) => true,
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => false,
        Err(error) => panic!("reading the connection failed: {error}"),
    };
    stream.set_nonblocking(false).unwrap();
    ready
}

/// The code the second request is answered with.
fn probe(port: u16, document: &str) -> String {
    let body = operation(
        REQUEST,
        document,
        json!({"expected_revision": COPIES, "to": "kekule", "time_limit_ms": 0}),
    );
    let reply = reply(send(port, &body));
    reply["diagnostics"][0]["code"].as_str().unwrap().to_owned()
}

#[test]
fn closing_the_connection_stops_a_running_conversion() {
    let data = tempfile::tempdir().unwrap();
    // Longer than the test can wait, so that only a cancel ends the computation early.
    let application =
        LocalApplication::new(LocalCapabilities, FileDocumentRepository::new(data.path()))
            .with_compute_limits(ComputeLimits {
                default_time_ms: 60_000,
                max_time_ms: 60_000,
            });
    let document = application
        .create_document(None, Some("slow"))
        .unwrap()
        .document
        .id;
    draw_systems(&application, &document);

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let state = ApiState::new(application, TOKEN.into(), port, &[]).unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.spawn(async move {
        let listener = tokio::net::TcpListener::from_std(listener).unwrap();
        axum::serve(listener, router(Arc::new(state)))
            .await
            .unwrap();
    });

    let slow = operation(
        REQUEST,
        &document,
        json!({"expected_revision": COPIES, "to": "kekule"}),
    );
    // One deadline for every start, resends included: a request that has already finished is
    // answered by replay, and resending it would otherwise go on forever.
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut started = Instant::now();
    let mut request = send(port, &slow);
    // The computation is running once the same request ID is answered as in progress. The probe
    // goes out at once, while the computation still has most of its work to do. A probe that is
    // still running when the request arrives makes the request the refused one; its reply is then
    // already there, and the request is sent again.
    loop {
        let code = probe(port, &document);
        if code == "request_in_progress" {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the conversion was not seen running within 30 s; the same request ID was last answered {code}"
        );
        if replied(&request) {
            request = send(port, &slow);
            started = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    // It really is a long computation: it is still running when the close comes (see
    // `RUNNING_BEFORE_CLOSE`).
    std::thread::sleep(RUNNING_BEFORE_CLOSE.saturating_sub(started.elapsed()));
    assert_eq!(
        probe(port, &document),
        "request_in_progress",
        "the input must compute for longer than the cancel is given, took {:?}",
        started.elapsed()
    );

    request.shutdown(Shutdown::Both).unwrap();
    drop(request);
    let closed = Instant::now();
    let code = loop {
        let code = probe(port, &document);
        if code != "request_in_progress" {
            break code;
        }
        assert!(
            closed.elapsed() < Duration::from_secs(1),
            "the computation still ran {:?} after the client closed its connection",
            closed.elapsed()
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(
        code, "time_limit",
        "after the close the probe must compute anew with its limit of 0, got {code}"
    );
    runtime.shutdown_timeout(Duration::from_secs(5));
}
