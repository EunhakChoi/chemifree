//! Optional native registration must not make the operations host unusable.

use std::io::Write;
use std::process::{Command, Output, Stdio};

use serde_json::{Value, json};

fn run(options: &[String]) -> (Output, Vec<Value>) {
    let directory = tempfile::tempdir().unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_chemifree"));
    for key in [
        "CHEMIFREE_INCHI_LIBRARY",
        "CHEMIFREE_INCHI_VERSION",
        "CHEMIFREE_INCHI_SHA256",
        "CHEMIFREE_INCHI_TIMEOUT_SECONDS",
        "CHEMIFREE_MODEL_DATA_ROOT",
        "CHEMIFREE_MODEL_TRUSTED_REGISTRY",
    ] {
        command.env_remove(key);
    }
    let mut child = command
        .arg("--data-dir")
        .arg(directory.path())
        .args(options)
        .arg("operations")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    for (request, operation, parameters) in [
        ("inchi", "chem.inchi", json!({"document_id":"not-created"})),
        ("create", "document.create", json!({"title":"Still usable"})),
    ] {
        let body = json!({"contract":1,"request_id":request,"profile":"preserve-semantics","operation":operation,"parameters":parameters});
        writeln!(input, "{body}").unwrap();
    }
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let replies: Vec<Value> = String::from_utf8(output.stdout.clone())
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(replies.len(), 2);
    assert_eq!(replies[0]["diagnostics"][0]["code"], "inchi_unavailable");
    assert_eq!(replies[0]["outcome"], "unsupported");
    assert!(
        replies[0]["result"].is_null(),
        "unavailable native code must generate no identifier"
    );
    assert_eq!(replies[1]["result"]["document"]["title"], "Still usable");
    assert_eq!(replies[1]["document_revision"], 0);
    (output, replies)
}

fn selected(path: &std::path::Path, version: &str, digest: &str) -> Vec<String> {
    vec![
        "--inchi-library".into(),
        path.to_str().unwrap().into(),
        "--inchi-version".into(),
        version.into(),
        "--inchi-sha256".into(),
        digest.into(),
    ]
}

#[test]
fn an_unconfigured_host_starts_without_native_identifiers_or_registration_metadata() {
    let (output, replies) = run(&[]);
    assert!(
        output.stderr.is_empty(),
        "no selection needs no library warning"
    );
    assert!(
        replies[0]["diagnostics"][0]["details"]
            .get("reason_code")
            .is_none()
    );
}

#[test]
fn a_partial_opt_in_reports_the_missing_registration_without_stopping_the_host() {
    let (_, replies) = run(&["--inchi-version".into(), "1.07.5".into()]);
    let details = &replies[0]["diagnostics"][0]["details"];
    assert_eq!(details["reason_code"], "inchi_registration_invalid");
    assert!(details["reason"].as_str().unwrap().contains("together"));
}

#[test]
fn an_unsupported_declared_release_is_reported_without_inventing_a_detected_version() {
    let directory = tempfile::tempdir().unwrap();
    let (_, replies) = run(&selected(
        &directory.path().join("missing.so"),
        "1.07.4",
        &"0".repeat(64),
    ));
    assert_eq!(
        replies[0]["diagnostics"][0]["details"]["reason_code"],
        "inchi_registration_invalid"
    );
    assert!(replies[0]["result"].is_null());
}

#[test]
fn a_missing_explicit_library_leaves_the_operations_stream_usable() {
    let directory = tempfile::tempdir().unwrap();
    let (output, replies) = run(&selected(
        &directory.path().join("missing.so"),
        "1.07.5",
        &"0".repeat(64),
    ));
    assert_eq!(
        replies[0]["diagnostics"][0]["details"]["reason_code"],
        "inchi_unavailable"
    );
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("Optional InChI provider unavailable")
    );
}

#[cfg(target_os = "linux")]
#[test]
fn a_selected_file_with_a_valid_digest_but_no_library_symbols_is_nonfatal() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("not-a-library.so");
    std::fs::write(&path, b"not an InChI library").unwrap();
    let (_, replies) = run(&selected(
        &path,
        "1.07.5",
        "a511b4c1307624f35f774dbd61452af457c93d0e7092f4e4b2ca7b5dd0fb967d",
    ));
    assert_eq!(
        replies[0]["diagnostics"][0]["details"]["reason_code"],
        "inchi_unavailable"
    );
    assert!(
        !replies[0]["diagnostics"][0]["details"]["reason"]
            .as_str()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn an_invalid_timeout_is_a_nonfatal_provider_reason() {
    let directory = tempfile::tempdir().unwrap();
    let mut options = selected(
        &directory.path().join("missing.so"),
        "1.07.5",
        &"0".repeat(64),
    );
    options.extend(["--inchi-timeout-seconds".into(), "not-a-number".into()]);
    let (_, replies) = run(&options);
    assert_eq!(
        replies[0]["diagnostics"][0]["details"]["reason"],
        "InChI timeout must be an integer from 1 to 60 seconds."
    );
}

#[cfg(target_os = "linux")]
#[test]
fn a_digest_mismatch_is_reported_before_loading_the_selected_file() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("not-a-library.so");
    std::fs::write(&path, b"not an InChI library").unwrap();
    let (_, replies) = run(&selected(&path, "1.07.5", &"0".repeat(64)));
    let details = &replies[0]["diagnostics"][0]["details"];
    assert_eq!(details["reason_code"], "inchi_registration_invalid");
    assert!(
        details["reason"].as_str().unwrap().contains("SHA-256"),
        "{details}"
    );
}
