use std::process::Command;

use serde_json::{Value, json};

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

#[test]
fn missing_phase6_catalog_is_reported_without_blocking_documents() {
    let dir = tempfile::tempdir().unwrap();
    let model = dir.path().join("model");
    std::fs::create_dir(&model).unwrap();
    std::fs::create_dir(model.join("dataset")).unwrap();
    std::fs::write(model.join("manifest.json"), b"{}").unwrap();
    std::fs::write(model.join("review.json"), b"{}").unwrap();
    let registry = json!({
        "registry_version": 1, "root_binding_id": "fixture-root",
        "registrations": [{
            "manifest_path": "manifest.json", "review_path": "review.json",
            "data_root": "dataset", "source_registry_path": "catalog/registry.json", "format_profile_path": "catalog/profile.json",
            "trust": {"registration_id": "synthetic", "generation": 1, "dataset_id": "synthetic", "dataset_version": "1", "family_id": "synthetic", "profile_id": "private-naming-element-ranks-v1", "engine_contract_id": "synthetic", "manifest_sha256": "0".repeat(64), "manifest_file_sha256": "0".repeat(64), "review_sha256": "0".repeat(64), "source_registry_binding_id": "synthetic", "private_root_binding_id": "fixture-root"}
        }]
    });
    let descriptor = dir.path().join("registry.json");
    std::fs::write(&descriptor, serde_json::to_vec(&registry).unwrap()).unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_chemifree"))
            .env_remove("CHEMIFREE_MODEL_DATA_ROOT")
            .env_remove("CHEMIFREE_MODEL_TRUSTED_REGISTRY")
            .arg("--data-dir")
            .arg(dir.path().join("documents"))
            .arg("--model-data-root")
            .arg(&model)
            .arg("--model-trusted-registry")
            .arg(&descriptor)
            .args(args)
            .output()
            .unwrap()
    };
    let output = run(&["data", "status", "--json"]);
    assert!(output.status.success());
    let status: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(status["state"], "checked");
    assert_eq!(status["registrations"][0]["reason"], "data_unavailable");
    assert_eq!(status["engine"], "not_implemented");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("catalog/registry.json"));
    assert!(run(&["documents", "create", "--json"]).status.success());
}
