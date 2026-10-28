//! The binary's exit statuses and record, run as a subprocess.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::{Value, json};

fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_chemifree-conformance"))
}

fn write_suite(dir: &Path, cases: &[Value]) {
    let mut paths = Vec::new();
    for (index, case) in cases.iter().enumerate() {
        let path = format!("cases/{index}.json");
        fs::create_dir_all(dir.join("cases")).unwrap();
        fs::write(dir.join(&path), serde_json::to_vec(case).unwrap()).unwrap();
        paths.push(path);
    }
    let suite = json!({"suite_version": 1, "cases": paths});
    fs::write(dir.join("suite.json"), serde_json::to_vec(&suite).unwrap()).unwrap();
}

fn case(id: &str, revision: u64) -> Value {
    json!({
        "id": id, "title": "Create", "requirements": ["API-01"], "axis": "protocol",
        "kind": "accept",
        "steps": [{"call": "create", "expect": {"document": {"revision": revision}}}]
    })
}

fn pending(id: &str) -> Value {
    json!({
        "id": id, "title": "Later", "requirements": ["CHEM-01"], "axis": "numeric",
        "kind": "accept", "pending": {"followup": "P19", "reason": "not yet"}
    })
}

fn status(output: &Output) -> i32 {
    output.status.code().expect("the process exited")
}

#[test]
fn usage_errors_exit_2_without_running() {
    let dir = tempfile::tempdir().unwrap();
    write_suite(dir.path(), &[case("a", 0)]);
    let out = dir.path().join("report.json");
    let suite = dir.path().join("suite.json");
    let attempts: [Vec<&std::ffi::OsStr>; 4] = [
        vec![suite.as_os_str(), "--out".as_ref()],
        vec!["--bogus".as_ref(), suite.as_os_str()],
        vec!["--out".as_ref(), out.as_os_str()],
        vec![
            "--out".as_ref(),
            out.as_os_str(),
            suite.as_os_str(),
            suite.as_os_str(),
        ],
    ];
    for args in attempts {
        let output = binary().args(&args).output().unwrap();
        assert_eq!(status(&output), 2, "{args:?}");
        assert!(!out.exists(), "{args:?} must not write a record");
    }
}

#[test]
fn a_passing_run_writes_the_record_and_exits_0() {
    let dir = tempfile::tempdir().unwrap();
    write_suite(dir.path(), &[case("a", 0), pending("b")]);
    let out = dir.path().join("deep/report.json");
    let output = binary()
        .arg("--out")
        .arg(&out)
        .arg(dir.path().join("suite.json"))
        .output()
        .unwrap();
    assert_eq!(
        status(&output),
        0,
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let record: Value = serde_json::from_slice(&fs::read(&out).unwrap()).unwrap();
    assert_eq!(
        record["counts"],
        json!({
            "accept": {"passed": 1, "failed": 0, "not_run": 1},
            "reject": {"passed": 0, "failed": 0, "not_run": 0},
            "total": {"passed": 1, "failed": 0, "not_run": 1}
        })
    );
    assert_eq!(record["results"][1]["outcome"], "not_run");
    assert_eq!(
        record["results"][1]["pending"],
        json!({"followup": "P19", "reason": "not yet"})
    );
    let text = fs::read_to_string(&out).unwrap();
    let directory = dir.path().to_str().unwrap();
    assert!(
        !text.contains(directory),
        "the record must hold no absolute path"
    );
}

#[test]
fn a_failed_case_exits_1_and_is_recorded() {
    let dir = tempfile::tempdir().unwrap();
    write_suite(dir.path(), &[case("a", 7)]);
    let out = dir.path().join("report.json");
    let output = binary()
        .arg("--out")
        .arg(&out)
        .arg(dir.path().join("suite.json"))
        .output()
        .unwrap();
    assert_eq!(status(&output), 1);
    let record: Value = serde_json::from_slice(&fs::read(&out).unwrap()).unwrap();
    assert_eq!(record["results"][0]["outcome"], "failed");
    assert_eq!(record["counts"]["total"]["failed"], 1);
}

#[test]
fn an_invalid_suite_exits_1_without_a_record() {
    let dir = tempfile::tempdir().unwrap();
    write_suite(dir.path(), &[case("a", 0), case("a", 0)]);
    let out = dir.path().join("report.json");
    let output = binary()
        .arg("--out")
        .arg(&out)
        .arg(dir.path().join("suite.json"))
        .output()
        .unwrap();
    assert_eq!(status(&output), 1);
    assert!(!out.exists());
    assert!(String::from_utf8_lossy(&output.stderr).contains("repeated"));
}

#[test]
fn two_runs_write_identical_records() {
    let dir = tempfile::tempdir().unwrap();
    write_suite(dir.path(), &[case("a", 0), pending("b")]);
    let mut records = Vec::new();
    for name in ["one.json", "two.json"] {
        let out = dir.path().join(name);
        let output = binary()
            .arg("--out")
            .arg(&out)
            .arg(dir.path().join("suite.json"))
            .output()
            .unwrap();
        assert_eq!(status(&output), 0);
        records.push(fs::read(out).unwrap());
    }
    assert_eq!(records[0], records[1]);
}
