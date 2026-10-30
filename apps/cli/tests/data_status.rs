use std::process::Command;

use serde_json::Value;

#[test]
fn unavailable_selected_registry_is_reported_and_does_not_block_documents() {
    let dir = tempfile::tempdir().unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_chemifree"))
            .env_remove("CHEMIFREE_MODEL_DATA_ROOT")
            .env_remove("CHEMIFREE_MODEL_TRUSTED_REGISTRY")
            .arg("--data-dir")
            .arg(dir.path().join("documents"))
            .arg("--model-data-root")
            .arg(dir.path().join("model"))
            .arg("--model-trusted-registry")
            .arg(dir.path().join("missing.json"))
            .args(args)
            .output()
            .unwrap()
    };
    let output = run(&["data", "status", "--json"]);
    assert!(output.status.success());
    let status: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(status["state"], "refused");
    assert_eq!(status["reason"], "registry_unavailable");
    assert_eq!(status["engine"], "not_implemented");
    let output = run(&["documents", "create", "--json"]);
    assert!(output.status.success());
}
