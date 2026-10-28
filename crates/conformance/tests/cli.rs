use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Value, json};

fn run(arguments: &[&str], directory: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_chemifree-conformance"))
        .args(arguments)
        .current_dir(directory)
        .output()
        .unwrap()
}

/// Writes a one-case suite; the case passes unless `fail` is set.
fn suite(directory: &Path, fail: bool) -> PathBuf {
    let revision = if fail { 1 } else { 0 };
    let case = json!({
        "id": "one", "title": "T", "requirements": ["OPS-02"], "axis": "graph", "kind": "accept",
        "steps": [{ "call": "create", "doc": "d", "expect": { "document": { "revision": revision } } }],
    });
    fs::write(directory.join("one.json"), case.to_string()).unwrap();
    let path = directory.join("suite.json");
    fs::write(
        &path,
        json!({ "suite_version": "9", "cases": ["one.json"] }).to_string(),
    )
    .unwrap();
    path
}

fn report(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn out_writes_the_report_to_the_file_only() {
    let directory = tempfile::tempdir().unwrap();
    suite(directory.path(), false);
    let output = run(
        &["suite.json", "--out", "reports/run.json"],
        directory.path(),
    );
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stdout.is_empty(), "stdout: {:?}", output.stdout);
    let written = report(&directory.path().join("reports/run.json"));
    assert_eq!(written["suite_version"], "9");
    assert_eq!(written["counts"]["accept"]["passed"], 1);
}

#[test]
fn without_out_the_report_goes_to_standard_output() {
    let directory = tempfile::tempdir().unwrap();
    suite(directory.path(), false);
    let output = run(&["suite.json"], directory.path());
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let printed: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(printed["counts"]["total"]["passed"], 1);
}

#[test]
fn a_failing_case_exits_1_and_still_writes_the_report() {
    let directory = tempfile::tempdir().unwrap();
    suite(directory.path(), true);
    let output = run(&["suite.json", "--out", "run.json"], directory.path());
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let written = report(&directory.path().join("run.json"));
    assert_eq!(written["counts"]["accept"]["failed"], 1);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("FAILED one: create:"), "{stderr}");
}

#[test]
fn a_suite_that_cannot_load_exits_1_and_writes_nothing() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("suite.json"), "{}").unwrap();
    let output = run(&["suite.json", "--out", "run.json"], directory.path());
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(!directory.path().join("run.json").exists());
}

#[test]
fn usage_errors_exit_2_without_running_or_printing_a_report() {
    let directory = tempfile::tempdir().unwrap();
    suite(directory.path(), false);
    let usages: [&[&str]; 4] = [
        &["suite.json", "--out"],
        &["--out"],
        &["suite.json", "--verbose"],
        &["suite.json", "other.json"],
    ];
    for arguments in usages {
        let output = run(arguments, directory.path());
        assert_eq!(output.status.code(), Some(2), "{arguments:?}: {output:?}");
        assert!(output.stdout.is_empty(), "{arguments:?} printed to stdout");
    }
    let names: Vec<_> = fs::read_dir(directory.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(
        names.len(),
        2,
        "nothing but the suite and its case: {names:?}"
    );
}
