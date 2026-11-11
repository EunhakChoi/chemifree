//! What the CLI binary assembles at startup, pinned from the real binary: the codecs it attaches,
//! the file-backed preferences of its data directory, the default compute limits, and the
//! startup prune of abandoned temporary files, which the CLI does not run (the API does).

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, SystemTime};

use serde_json::{Value, json};

/// Environment variables that select the data directory or an optional provider. Every child
/// starts without them, so the developer's shell cannot change what the binary assembles.
const SELECTING_VARIABLES: [&str; 7] = [
    "CHEMIFREE_DATA_DIR",
    "CHEMIFREE_INCHI_LIBRARY",
    "CHEMIFREE_INCHI_VERSION",
    "CHEMIFREE_INCHI_SHA256",
    "CHEMIFREE_INCHI_TIMEOUT_SECONDS",
    "CHEMIFREE_MODEL_DATA_ROOT",
    "CHEMIFREE_MODEL_TRUSTED_REGISTRY",
];

/// The label of the `format_conversion` capability for the codecs this binary attaches.
const FORMAT_LABEL: &str = "MOL/SDF, CDXML, CDX, SMILES and SVG subsets";

/// The largest `time_limit_ms` that the default compute limits accept.
const DEFAULT_MAX_TIME_MS: u64 = 8_000;

/// Two hours: older than the hour after which a store prunes its temporary files.
const STALE_AGE: Duration = Duration::from_secs(2 * 60 * 60);

/// A `chemifree` command for `data`, without the selecting variables.
fn cli_command(data: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_chemifree"));
    for key in SELECTING_VARIABLES {
        command.env_remove(key);
    }
    command.arg("--data-dir").arg(data);
    command
}

/// Runs one command to completion, which must succeed, and returns what it printed.
fn run_ok(data: &Path, arguments: &[&str]) -> Output {
    let output = cli_command(data).args(arguments).output().unwrap();
    assert!(
        output.status.success(),
        "`chemifree {}` must succeed: {}",
        arguments.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

/// A child process that is killed and reaped if the test ends before it is waited for.
struct KillOnDrop(Option<Child>);

impl KillOnDrop {
    fn wait_with_output(mut self) -> Output {
        self.0
            .take()
            .expect("the child is waited for once")
            .wait_with_output()
            .unwrap()
    }
}

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            // Best effort: the child may have exited already, and nothing else can be done.
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Sends each request as one line on standard input, closes it, and returns one parsed reply
/// per request. The process must exit successfully.
fn operations(data: &Path, requests: &[Value]) -> Vec<Value> {
    let mut child = KillOnDrop(Some(
        cli_command(data)
            .arg("operations")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    ));
    let mut input = child.0.as_mut().unwrap().stdin.take().unwrap();
    for request in requests {
        writeln!(input, "{request}").unwrap();
    }
    drop(input);
    let output = child.wait_with_output();
    assert!(
        output.status.success(),
        "`chemifree operations` must succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let replies: Vec<Value> = std::str::from_utf8(&output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(replies.len(), requests.len(), "one reply per request");
    replies
}

/// The temporary files that one store can leave behind: a stale and a young file per store.
struct Temporaries {
    stale_document: PathBuf,
    young_document: PathBuf,
    stale_preferences: PathBuf,
    young_preferences: PathBuf,
}

/// Creates one abandoned temporary file per store, named as each store names its temporaries:
/// the documents store prunes names that start with `.` and end with `.tmp`, and the preferences
/// store prunes names that start with `.preferences.`. The stale files are two hours old; the
/// young files are written now.
fn plant_temporaries(data: &Path) -> Temporaries {
    let documents = data.join("documents");
    fs::create_dir_all(&documents).unwrap();
    let stale = SystemTime::now() - STALE_AGE;
    let temporaries = Temporaries {
        stale_document: documents.join(".stale-document.tmp"),
        young_document: documents.join(".young-document.tmp"),
        stale_preferences: data.join(".preferences.stale.tmp"),
        young_preferences: data.join(".preferences.young.tmp"),
    };
    for path in [&temporaries.stale_document, &temporaries.stale_preferences] {
        File::create(path).unwrap().set_modified(stale).unwrap();
    }
    for path in [&temporaries.young_document, &temporaries.young_preferences] {
        File::create(path).unwrap();
    }
    temporaries
}

/// A `chem.aromaticity_convert` request to Kekule form at revision 0, with `time_limit_ms`.
fn aromaticity_request(request_id: &str, document_id: &str, time_limit_ms: u64) -> Value {
    json!({
        "contract": 1,
        "request_id": request_id,
        "profile": "preserve-semantics",
        "operation": "chem.aromaticity_convert",
        "parameters": {
            "document_id": document_id,
            "expected_revision": 0,
            "to": "kekule",
            "time_limit_ms": time_limit_ms,
        },
    })
}

/// The CLI keeps abandoned temporary files: it has no startup prune, unlike the API.
#[test]
fn cli_leaves_abandoned_temporaries_in_place() {
    let data = tempfile::tempdir().unwrap();
    let temporaries = plant_temporaries(data.path());
    run_ok(data.path(), &["documents", "list", "--json"]);
    assert!(
        temporaries.stale_document.exists(),
        "the stale documents temporary {} must remain",
        temporaries.stale_document.display()
    );
    assert!(
        temporaries.stale_preferences.exists(),
        "the stale preferences temporary {} must remain",
        temporaries.stale_preferences.display()
    );
    assert!(
        temporaries.young_document.exists(),
        "the young documents temporary {} must remain",
        temporaries.young_document.display()
    );
    assert!(
        temporaries.young_preferences.exists(),
        "the young preferences temporary {} must remain",
        temporaries.young_preferences.display()
    );
}

/// The codecs the CLI attaches are named by the `format_conversion` capability label.
#[test]
fn cli_attaches_every_codec() {
    let data = tempfile::tempdir().unwrap();
    let output = run_ok(data.path(), &["capabilities", "--json"]);
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    let entry = response["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["id"] == "format_conversion")
        .expect("the capabilities list format_conversion");
    assert_eq!(
        entry["label"], FORMAT_LABEL,
        "the label must name the codecs the CLI attaches"
    );
}

/// Preferences saved by one process are read back by the next one.
#[test]
fn cli_preferences_are_file_backed() {
    let data = tempfile::tempdir().unwrap();
    run_ok(
        data.path(),
        &[
            "preferences",
            "set",
            "--expected-revision",
            "0",
            "--preferences",
            r#"{"ui":{"theme":"dark"},"new_documents":{}}"#,
        ],
    );
    // The next run is a new process, so it can report the theme only from the stored file.
    let shown = run_ok(data.path(), &["preferences", "show", "--json"]);
    let response: Value = serde_json::from_slice(&shown.stdout).unwrap();
    assert_eq!(
        response["ui"]["theme"], "dark",
        "a later process must read the saved theme"
    );
    assert_eq!(response["revision"], 1, "the saved change is revision 1");
    assert!(
        data.path().join("preferences.json").is_file(),
        "the preferences must be stored in preferences.json"
    );
}

/// The default compute limits refuse a time limit above 8000 ms and accept exactly 8000 ms.
#[test]
fn cli_uses_default_compute_limits() {
    let data = tempfile::tempdir().unwrap();
    let create = run_ok(
        data.path(),
        &["documents", "create", "--title", "T", "--json"],
    );
    let document: Value = serde_json::from_slice(&create.stdout).unwrap();
    let document_id = document["document"]["id"]
        .as_str()
        .expect("the created document has a string id");
    let replies = operations(
        data.path(),
        &[
            aromaticity_request("above-the-maximum", document_id, DEFAULT_MAX_TIME_MS + 1),
            aromaticity_request("at-the-maximum", document_id, DEFAULT_MAX_TIME_MS),
        ],
    );
    let refused = &replies[0];
    assert_eq!(
        refused["outcome"], "error",
        "a time limit above the default maximum is refused"
    );
    assert!(
        refused["result"].is_null(),
        "a refused request has no result"
    );
    let diagnostic = &refused["diagnostics"][0];
    assert_eq!(diagnostic["code"], "invalid_request");
    assert_eq!(diagnostic["path"], "parameters.time_limit_ms");
    assert_eq!(diagnostic["details"]["maximum_ms"], DEFAULT_MAX_TIME_MS);
    assert_eq!(
        replies[1]["outcome"], "ok",
        "the default maximum itself is accepted"
    );
}
