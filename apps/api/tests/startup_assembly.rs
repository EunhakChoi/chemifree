//! What the API binary assembles at startup, observed on the real process: the stale-temporary
//! prune of its stores, every format codec with file-backed preferences, and the optional
//! provider selection. Each case starts the binary on an ephemeral port and speaks HTTP to it.

use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use serde_json::{Value, json};

/// Bounds every wait on the process, so a hung API fails the test instead of stalling it.
const TIMEOUT: Duration = Duration::from_secs(15);
/// Past the one-hour age at which the stores prune an abandoned temporary.
const STALE_AGE: Duration = Duration::from_secs(2 * 60 * 60);
/// The `format_conversion` label with every codec attached.
const ALL_CODECS_LABEL: &str = "MOL/SDF, CDXML, CDX, SMILES and SVG subsets";
/// The stderr line for a partial optional InChI registration.
const INCHI_REFUSAL: &str = "Optional InChI provider unavailable (inchi_registration_invalid): Optional InChI registration requires --inchi-library, --inchi-version (1.07.5), and --inchi-sha256 together.";
/// Environment variables that select host settings. Each child starts without them.
const SELECTION_ENV: [&str; 10] = [
    "CHEMIFREE_COMPUTE_CONCURRENCY",
    "CHEMIFREE_DATA_DIR",
    "CHEMIFREE_MODEL_DATA_ROOT",
    "CHEMIFREE_MODEL_TRUSTED_REGISTRY",
    "CHEMIFREE_INCHI_LIBRARY",
    "CHEMIFREE_INCHI_VERSION",
    "CHEMIFREE_INCHI_SHA256",
    "CHEMIFREE_INCHI_TIMEOUT_SECONDS",
    "CHEMIFREE_LOG_REQUESTS",
    "CHEMIFREE_WEB_DIR",
];

/// A running API process on an ephemeral port. Dropping it kills and reaps the child, also
/// when an assertion fails first.
struct Api {
    child: Child,
    port: u16,
    /// Every stderr line up to and including the documents line.
    startup: Vec<String>,
}

impl Drop for Api {
    fn drop(&mut self) {
        // The child may have exited already; killing and reaping it is what matters here.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Api {
    /// Starts the binary with `--port 0`, `--data-dir data` and `extra`, and returns once it
    /// has printed its documents line.
    fn start(data: &Path, extra: &[&OsStr]) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_chemifree-api"));
        for name in SELECTION_ENV {
            command.env_remove(name);
        }
        let mut child = command
            .args(["--port", "0", "--data-dir"])
            .arg(data)
            .args(extra)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the API binary starts");
        let stderr = child.stderr.take().expect("stderr is piped");
        let (sender, lines) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stderr).lines() {
                let Ok(line) = line else { break };
                // The receiver may be gone by now; the pipe is still read to its end, so the
                // child never blocks on it.
                let _ = sender.send(line);
            }
        });
        let mut api = Self {
            child,
            port: 0,
            startup: Vec::new(),
        };
        api.read_startup(&lines);
        api
    }

    /// Records stderr up to the documents line, which the binary prints last, and the port
    /// named by its listening line.
    fn read_startup(&mut self, lines: &Receiver<String>) {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            let wait = deadline.saturating_duration_since(Instant::now());
            let line = match lines.recv_timeout(wait) {
                Ok(line) => line,
                Err(RecvTimeoutError::Timeout) => panic!(
                    "no documents line within {TIMEOUT:?}; stderr so far: {:?}",
                    self.startup
                ),
                Err(RecvTimeoutError::Disconnected) => panic!(
                    "the API exited before its documents line; stderr: {:?}",
                    self.startup
                ),
            };
            let done = line.starts_with("Documents: ");
            self.startup.push(line);
            if done {
                break;
            }
        }
        let port: u16 = self
            .startup
            .iter()
            .find_map(|line| line.strip_prefix("Chemifree API listening on http://127.0.0.1:"))
            .expect("the listening line names the loopback address")
            .parse()
            .expect("the listening line ends with a port number");
        self.port = port;
    }

    /// The origin of the API's own loopback address, which writes must present.
    fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// The session header name and token that writes must present (`GET /api/v1/session`).
    fn session(&self) -> (String, String) {
        let session = self.send("GET", "/api/v1/session", &[], None).json();
        let header = session["header"]
            .as_str()
            .expect("the session names its header");
        let token = session["token"]
            .as_str()
            .expect("the session carries its token");
        (header.to_owned(), token.to_owned())
    }

    /// The stderr lines printed before the listening line.
    fn before_listening(&self) -> Vec<&str> {
        self.startup
            .iter()
            .take_while(|line| !line.starts_with("Chemifree API listening"))
            .map(String::as_str)
            .collect()
    }

    /// Sends one request on a new connection and reads the reply to its end.
    fn send(
        &self,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: Option<&Value>,
    ) -> Reply {
        let payload = body.map(Value::to_string).unwrap_or_default();
        let mut lines = vec![
            format!("{method} {path} HTTP/1.1"),
            format!("Host: 127.0.0.1:{}", self.port),
            "Connection: close".to_owned(),
        ];
        lines.extend(
            headers
                .iter()
                .map(|(name, value)| format!("{name}: {value}")),
        );
        if body.is_some() {
            lines.push("Content-Type: application/json".to_owned());
            lines.push(format!("Content-Length: {}", payload.len()));
        }
        let request = format!("{}\r\n\r\n{payload}", lines.join("\r\n"));
        let mut stream =
            TcpStream::connect(("127.0.0.1", self.port)).expect("the API accepts connections");
        stream
            .set_read_timeout(Some(TIMEOUT))
            .expect("a read timeout can be set");
        stream
            .write_all(request.as_bytes())
            .expect("the request is written");
        let mut raw = Vec::new();
        stream
            .read_to_end(&mut raw)
            .expect("the reply is read to the end of the connection");
        Reply::parse(&raw)
    }
}

/// One reply. The server frames each reply with Content-Length, which the parser checks.
struct Reply {
    status: u16,
    body: Vec<u8>,
}

impl Reply {
    fn parse(raw: &[u8]) -> Self {
        let split = raw
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .expect("the reply has a header block");
        let head = String::from_utf8_lossy(&raw[..split]);
        let body = raw[split + 4..].to_vec();
        let status_line = head.lines().next().unwrap_or_default();
        let status: u16 = status_line
            .split(' ')
            .nth(1)
            .and_then(|code| code.parse().ok())
            .unwrap_or_else(|| panic!("a status code in {status_line:?}"));
        let length: usize = head
            .lines()
            .skip(1)
            .filter_map(|line| line.split_once(':'))
            .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
            .expect("the reply states its Content-Length")
            .1
            .trim()
            .parse()
            .expect("the Content-Length is a number");
        assert_eq!(
            body.len(),
            length,
            "the body is exactly Content-Length bytes"
        );
        Self { status, body }
    }

    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or_else(|error| {
            panic!(
                "the body is JSON ({error}): {:?}",
                String::from_utf8_lossy(&self.body)
            )
        })
    }
}

/// Creates an empty file that looks like an interrupted write from two hours ago.
fn stale_temporary(path: &Path) {
    File::create(path)
        .expect("the temporary file can be created")
        .set_modified(SystemTime::now() - STALE_AGE)
        .expect("its modification time can be set");
}

/// Creates an empty file that looks like a write still in progress.
fn young_temporary(path: &Path) {
    File::create(path).expect("the temporary file can be created");
}

#[test]
fn startup_prunes_abandoned_temporaries_and_names_the_documents_directory() {
    let directory = tempfile::tempdir().expect("a temporary data directory");
    let data = directory.path();
    let documents = data.join("documents");
    fs::create_dir_all(&documents).expect("the documents directory exists before startup");
    // The names follow the `.<stem>.<pid>.<counter>.tmp` pattern of an interrupted write.
    let stale_document = documents.join(".drawing.4242.0.tmp");
    let young_document = documents.join(".drawing.4242.1.tmp");
    let stale_preferences = data.join(".preferences.4242.0.tmp");
    let young_preferences = data.join(".preferences.4242.1.tmp");
    stale_temporary(&stale_document);
    young_temporary(&young_document);
    stale_temporary(&stale_preferences);
    young_temporary(&young_preferences);

    let api = Api::start(data, &[]);

    assert!(
        !stale_document.exists(),
        "startup removes the abandoned document temporary"
    );
    assert!(
        young_document.exists(),
        "startup keeps a document temporary that may be in progress"
    );
    assert!(
        !stale_preferences.exists(),
        "startup removes the abandoned preferences temporary"
    );
    assert!(
        young_preferences.exists(),
        "startup keeps a preferences temporary that may be in progress"
    );
    assert_eq!(
        api.startup,
        [
            format!("Chemifree API listening on http://127.0.0.1:{}", api.port),
            format!("Documents: {}", documents.display()),
        ],
        "a clean start prints only the listening line and the documents line"
    );
}

#[test]
fn the_assembled_api_attaches_every_codec_and_file_preferences() {
    let directory = tempfile::tempdir().expect("a temporary data directory");
    let data = directory.path();
    let api = Api::start(data, &[]);

    let capabilities = api.send("GET", "/api/v1/capabilities", &[], None).json();
    let conversion = capabilities["capabilities"]
        .as_array()
        .expect("the capabilities are a list")
        .iter()
        .find(|item| item["id"] == "format_conversion")
        .expect("format_conversion is listed");
    assert_eq!(
        conversion["label"], ALL_CODECS_LABEL,
        "with every codec attached, the label names each of them"
    );

    let data_status = api.send("GET", "/api/v1/data", &[], None).json();
    assert_eq!(
        data_status,
        json!({"api_version": "v1", "state": "not_configured", "reason": null,
            "registrations": [], "engine": "not_implemented"}),
        "no DATA selection is configured by default"
    );

    let (header, token) = api.session();
    let origin = api.origin();
    let change = json!({"request_id": "startup-1", "expected_revision": 0,
        "ui": {"theme": "dark"}, "new_documents": {"bond_length_pt": 40}});
    let reply = api.send(
        "PUT",
        "/api/v1/preferences",
        &[
            (header.as_str(), token.as_str()),
            ("origin", origin.as_str()),
        ],
        Some(&change),
    );
    assert_eq!(
        reply.status, 200,
        "a session-authorized preferences write is accepted"
    );
    let preferences = reply.json();
    assert_eq!(
        preferences["revision"], 1,
        "the write moves preferences to revision 1"
    );
    assert!(
        data.join("preferences.json").exists(),
        "file-backed preferences are saved as preferences.json in the data directory"
    );
}

#[test]
fn optional_provider_selection_errors_are_reported_and_the_api_still_serves() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let root = directory.path();
    let library = root.join("missing.so");
    let model_root = root.join("missing");
    let api = Api::start(
        &root.join("data"),
        &[
            OsStr::new("--inchi-library"),
            library.as_os_str(),
            OsStr::new("--model-data-root"),
            model_root.as_os_str(),
        ],
    );

    // Each observation is checked before the test fails, so one regression names every
    // observation it changes.
    let mut mismatches = Vec::new();
    let before = api.before_listening();
    if before != [INCHI_REFUSAL] {
        mismatches.push(format!("stderr before the listening line: {before:?}"));
    }
    let data_status = api.send("GET", "/api/v1/data", &[], None).json();
    let incomplete = json!({"api_version": "v1", "state": "refused",
        "reason": "registry_configuration_incomplete", "registrations": [],
        "engine": "not_implemented"});
    if data_status != incomplete {
        mismatches.push(format!("DATA status: {data_status}"));
    }
    let (header, token) = api.session();
    let origin = api.origin();
    let request = json!({"contract": 1, "request_id": "inchi", "profile": "preserve-semantics",
        "operation": "chem.inchi", "parameters": {"document_id": "not-created"}});
    let reply = api.send(
        "POST",
        "/api/v1/operations",
        &[
            (header.as_str(), token.as_str()),
            ("origin", origin.as_str()),
        ],
        Some(&request),
    );
    if reply.status != 200 {
        mismatches.push(format!("operation reply status: {}", reply.status));
    }
    let outcome = reply.json();
    let diagnostic = &outcome["diagnostics"][0];
    if outcome["outcome"] != "unsupported" {
        mismatches.push(format!("operation outcome: {}", outcome["outcome"]));
    }
    if diagnostic["code"] != "inchi_unavailable" {
        mismatches.push(format!("diagnostic code: {}", diagnostic["code"]));
    }
    if diagnostic["details"]["reason_code"] != "inchi_registration_invalid" {
        mismatches.push(format!(
            "diagnostic reason_code: {}",
            diagnostic["details"]["reason_code"]
        ));
    }
    assert!(
        mismatches.is_empty(),
        "the optional provider selection changed:\n{}",
        mismatches.join("\n")
    );
}
