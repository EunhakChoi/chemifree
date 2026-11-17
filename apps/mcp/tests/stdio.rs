//! The `chemifree-mcp` binary over stdio: the refusal of a bad address at startup, an MCP session against a
//! live server, and a session against an address with no server.

mod common;

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use common::{TestServer, closed_port, create_document};
use serde_json::{Value, json};

/// The longest wait for one reply. A reply is normally immediate; the limit only keeps a broken process
/// from hanging the test.
const REPLY_TIMEOUT: Duration = Duration::from_secs(30);

/// The instructions that the server sends in its `initialize` reply.
const INSTRUCTIONS: &str = "Read-only tools for a running local Chemifree server: list documents, read a document snapshot and its revision, and inspect the operation catalog.";

/// A `chemifree-mcp` process whose replies are read on a thread. It is killed and reaped when dropped.
struct McpProcess {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<std::io::Result<String>>,
}

impl McpProcess {
    /// Starts the binary with `args`, and with no server address or data directory from the environment.
    fn start(args: &[&str]) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_chemifree-mcp"))
            .args(args)
            .env_remove("CHEMIFREE_SERVER_URL")
            .env_remove("CHEMIFREE_DATA_DIR")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("the chemifree-mcp binary starts");
        let stdin = child.stdin.take().expect("the input is piped");
        let stdout = child.stdout.take().expect("the output is piped");
        let (sender, lines) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            stdin: Some(stdin),
            lines,
        }
    }

    fn send(&mut self, message: &Value) {
        let stdin = self.stdin.as_mut().expect("the input is open until finish");
        writeln!(stdin, "{message}").expect("the message is written");
        stdin.flush().expect("the message is flushed");
    }

    fn receive(&mut self) -> Value {
        let line = self
            .lines
            .recv_timeout(REPLY_TIMEOUT)
            .unwrap_or_else(|error| panic!("no reply line within {REPLY_TIMEOUT:?}: {error}"))
            .expect("the reply line is read");
        serde_json::from_str(&line)
            .unwrap_or_else(|error| panic!("the reply is not JSON ({error}): {line}"))
    }

    /// Closes the input, which ends the session, and waits for the process to exit.
    fn finish(&mut self) -> ExitStatus {
        self.stdin.take();
        self.child.wait().expect("the process can be waited for")
    }
}

impl Drop for McpProcess {
    fn drop(&mut self) {
        // A process still running when a test ends would outlive it, so it is killed. The errors are
        // ignored: a process that has already exited cannot be killed, and that is the usual case here.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Runs one step of the session on a blocking thread, so that the in-process server keeps running on
/// the runtime, and returns the process with the step's value.
async fn step<T, F>(mut process: McpProcess, run: F) -> (McpProcess, T)
where
    T: Send + 'static,
    F: FnOnce(&mut McpProcess) -> T + Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        let value = run(&mut process);
        (process, value)
    })
    .await
    .unwrap_or_else(|error| std::panic::resume_unwind(error.into_panic()))
}

fn initialize_request() -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "chemifree-mcp-test", "version": "0.0.0"}
        }
    })
}

fn initialized_notification() -> Value {
    json!({"jsonrpc": "2.0", "method": "notifications/initialized"})
}

fn call_request(id: u64, name: &str, arguments: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": {"name": name, "arguments": arguments}
    })
}

#[test]
fn a_server_address_outside_the_loopback_form_is_refused_at_startup() {
    let output = Command::new(env!("CARGO_BIN_EXE_chemifree-mcp"))
        .args(["--server-url", "http://localhost:8080"])
        .env_remove("CHEMIFREE_SERVER_URL")
        .env_remove("CHEMIFREE_DATA_DIR")
        .output()
        .expect("the chemifree-mcp binary runs");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(
        output.status.code(),
        Some(2),
        "a usage error exits with status 2; stderr: {stderr}"
    );
    assert!(
        stderr.contains("--server-url"),
        "the error names the option, got: {stderr}"
    );
    assert!(
        output.stdout.is_empty(),
        "nothing is written to the protocol channel before a session starts"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn serves_the_read_only_tools_over_stdio_against_a_live_server() {
    let server = TestServer::start().await;
    let document_id = create_document(&server.http(), "stdio-create-1").await;
    let process = McpProcess::start(&["--server-url", server.url.as_str()]);

    let (process, initialized) = step(process, |process| {
        process.send(&initialize_request());
        process.receive()
    })
    .await;
    assert_eq!(initialized["id"], 1, "{initialized}");
    assert_eq!(
        initialized["result"]["protocolVersion"], "2025-06-18",
        "the server answers with the version the client asked for: {initialized}"
    );
    assert_eq!(initialized["result"]["serverInfo"]["name"], "chemifree");
    assert_eq!(
        initialized["result"]["serverInfo"]["version"],
        env!("CARGO_PKG_VERSION")
    );
    assert_eq!(initialized["result"]["instructions"], INSTRUCTIONS);

    let (process, ()) = step(process, |process| {
        process.send(&initialized_notification());
    })
    .await;
    let (process, listed) = step(process, |process| {
        process.send(&json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}));
        process.receive()
    })
    .await;
    let tools = listed["result"]["tools"]
        .as_array()
        .expect("the tool list is an array");
    let mut names: Vec<&str> = tools
        .iter()
        .map(|tool| tool["name"].as_str().expect("each tool has a name"))
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "chemifree_capabilities",
            "chemifree_document_snapshot",
            "chemifree_documents_list",
        ]
    );
    for tool in tools {
        assert_eq!(
            tool["annotations"]["readOnlyHint"], true,
            "{} must be marked read-only",
            tool["name"]
        );
    }

    let (process, called) = step(process, |process| {
        process.send(&call_request(3, "chemifree_documents_list", json!({})));
        process.receive()
    })
    .await;
    assert_eq!(called["id"], 3, "{called}");
    assert_eq!(called["result"]["isError"], false, "{called}");
    assert_eq!(
        called["result"]["content"][0]["text"], "1 document",
        "{called}"
    );
    let content_json: Value = serde_json::from_str(
        called["result"]["content"][1]["text"]
            .as_str()
            .expect("the second content block is the reply as JSON text"),
    )
    .expect("the second content block is JSON");
    assert_eq!(
        content_json, called["result"]["structuredContent"],
        "a client that reads only content gets the whole reply"
    );
    assert_eq!(
        called["result"]["structuredContent"]["documents"]
            .as_array()
            .map(Vec::len),
        Some(1),
        "the structured reply is the server's document list: {called}"
    );

    let (process, snapshot) = step(process, move |process| {
        process.send(&call_request(
            4,
            "chemifree_document_snapshot",
            json!({"document_id": document_id}),
        ));
        process.receive()
    })
    .await;
    assert_eq!(snapshot["id"], 4, "{snapshot}");
    assert_eq!(snapshot["result"]["isError"], false, "{snapshot}");
    assert_eq!(
        snapshot["result"]["content"][0]["text"], "document.snapshot: outcome ok, revision 0",
        "a new document is at revision 0: {snapshot}"
    );
    assert_eq!(
        snapshot["result"]["structuredContent"]["operation"], "document.snapshot",
        "{snapshot}"
    );

    let (process, capabilities) = step(process, |process| {
        process.send(&call_request(5, "chemifree_capabilities", json!({})));
        process.receive()
    })
    .await;
    assert_eq!(capabilities["id"], 5, "{capabilities}");
    assert_eq!(capabilities["result"]["isError"], false, "{capabilities}");
    assert_eq!(
        capabilities["result"]["content"][0]["text"],
        "capabilities.inspect: outcome ok, revision n/a",
        "{capabilities}"
    );

    let (_, status) = step(process, |process| process.finish()).await;
    assert!(
        status.success(),
        "the process exits cleanly when its input closes, got {status}"
    );
}

#[test]
fn an_address_with_no_server_is_a_tool_error_over_stdio() {
    let url = format!("http://127.0.0.1:{}", closed_port());
    let mut process = McpProcess::start(&["--server-url", url.as_str()]);
    process.send(&initialize_request());
    let initialized = process.receive();
    assert_eq!(initialized["id"], 1, "{initialized}");
    process.send(&initialized_notification());

    process.send(&call_request(2, "chemifree_documents_list", json!({})));
    let called = process.receive();

    assert_eq!(called["id"], 2, "{called}");
    assert_eq!(called["result"]["isError"], true, "{called}");
    assert_eq!(
        called["result"]["content"][0]["text"],
        format!(
            "No running Chemifree API server answered at {url}. Start chemifree-api for that data directory, or check --server-url."
        ),
        "{called}"
    );
    let status = process.finish();
    assert!(
        status.success(),
        "the process exits cleanly when its input closes, got {status}"
    );
}
