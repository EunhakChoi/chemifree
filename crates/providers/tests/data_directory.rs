//! The data registry loader over real directories: the committed synthetic fixture verifies,
//! and each kind of damage to a copy of it is refused with its own reason, without activating
//! anything that depends on what was refused.

use std::fs;
use std::path::{Path, PathBuf};

use chemifree_application::{
    BindingState, CheckState, DataSource, DataStatus, DataValue, DistributionPolicy, FeatureData,
    LoadedFile, ManifestState, load_data, load_data_snapshot,
};
use chemifree_providers::{DirectoryDataSource, load_selected_registry};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

const TABLE: &str = "tables/example-rows.jsonl";

#[cfg(target_os = "linux")]
#[test]
fn selected_root_remains_the_same_directory_across_reads() {
    let parent = tempfile::tempdir().unwrap();
    let selected = parent.path().join("selected");
    fs::create_dir(&selected).unwrap();
    fs::write(selected.join("first.json"), b"first").unwrap();
    fs::write(selected.join("second.json"), b"original").unwrap();
    let source = DirectoryDataSource::new(&selected);
    assert_eq!(source.read("first.json", 100).unwrap(), b"first");
    fs::rename(&selected, parent.path().join("renamed")).unwrap();
    fs::create_dir(&selected).unwrap();
    fs::write(selected.join("second.json"), b"substitute").unwrap();
    assert_eq!(source.read("second.json", 100).unwrap(), b"original");
}

#[cfg(target_os = "linux")]
#[test]
fn a_fifo_selected_as_the_data_root_is_refused_without_blocking() {
    use rustix::fs::{CWD, Mode, OFlags, mkfifoat, open};
    use std::sync::mpsc;
    use std::time::Duration;

    let parent = tempfile::tempdir().unwrap();
    let fifo = parent.path().join("not-a-directory");
    mkfifoat(CWD, &fifo, Mode::RUSR | Mode::WUSR).unwrap();
    let source = DirectoryDataSource::new(&fifo);
    let (sender, receiver) = mpsc::sync_channel(1);
    let reader =
        std::thread::spawn(move || sender.send(source.read("manifest.json", 100)).unwrap());
    let result = receiver.recv_timeout(Duration::from_secs(5));
    // If a defective reader blocks opening the FIFO, keep both ends open so it can finish and
    // be joined before asserting the timeout; no background thread survives the test.
    let unblock = result
        .is_err()
        .then(|| open(&fifo, OFlags::RDWR | OFlags::NONBLOCK, Mode::empty()).unwrap());
    reader.join().unwrap();
    drop(unblock);
    assert!(matches!(
        result,
        Ok(Err(chemifree_application::SourceError::RootUnusable))
    ));
}

#[test]
fn missing_selected_registry_is_reported_as_refused() {
    let dir = tempfile::tempdir().unwrap();
    let registry = load_selected_registry(dir.path(), dir.path().join("missing.json"));
    assert_eq!(
        registry.status().state,
        chemifree_application::RegistryState::Refused
    );
    assert_eq!(registry.status().reason, Some("registry_unavailable"));
    assert!(registry.datasets().is_empty());
}

#[test]
fn verified_snapshot_keeps_typed_rows_and_never_exposes_a_partial_dataset() {
    let dir = copy();
    let source = trusted_source(dir.path());
    let snapshot = load_data_snapshot(&source, DistributionPolicy::PublicOnly);
    let Some(LoadedFile::Table(table)) = snapshot.file("synthetic-example-table", TABLE) else {
        panic!("the complete verified table must be available to consumers");
    };
    assert_eq!(table.rows.len(), 3);
    assert_eq!(table.rows[1][1], DataValue::Integer(5));
    assert!(matches!(
        snapshot.file(
            "synthetic-example-parameters",
            "parameters/example-settings.json"
        ),
        Some(LoadedFile::Parameters(_))
    ));
    fs::write(dir.path().join(TABLE), b"changed").unwrap();
    let next = load_data_snapshot(&source, DistributionPolicy::PublicOnly);
    assert!(next.file("synthetic-example-table", TABLE).is_none());
    assert_eq!(next.status().bindings[1].state, BindingState::Inactive);
    assert!(matches!(
        snapshot.file("synthetic-example-table", TABLE),
        Some(LoadedFile::Table(_))
    ));
    assert!(
        next.file(
            "synthetic-example-parameters",
            "parameters/example-settings.json"
        )
        .is_some()
    );
}

#[test]
fn one_bad_file_hides_every_file_in_its_dataset() {
    let dir = copy();
    edit_manifest(dir.path(), |manifest| {
        let second_file = manifest["datasets"][1]["files"][0].clone();
        manifest["datasets"][0]["files"]
            .as_array_mut()
            .unwrap()
            .push(second_file);
    });
    fs::write(dir.path().join(TABLE), b"corrupt").unwrap();
    let snapshot = load_data_snapshot(&trusted_source(dir.path()), DistributionPolicy::PublicOnly);
    assert!(snapshot.file("synthetic-example-table", TABLE).is_none());
    assert!(
        snapshot
            .file(
                "synthetic-example-table",
                "parameters/example-settings.json"
            )
            .is_none(),
        "a valid sibling is not exposed from a refused dataset"
    );
    assert!(
        snapshot
            .file(
                "synthetic-example-parameters",
                "parameters/example-settings.json"
            )
            .is_some(),
        "an independent verified dataset remains available"
    );
}

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/model-data")
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// A copy of the fixture to damage.
fn copy() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    copy_tree(&fixture_dir(), dir.path());
    dir
}

fn load(dir: &Path) -> DataStatus {
    load_data(
        &trusted_source(dir),
        DistributionPolicy::RequireVerifiedRights,
    )
}

/// The fixture author supplies its current test manifest identity as the trust anchor.
fn trusted_source(dir: &Path) -> DirectoryDataSource {
    let manifest = fs::read(dir.join("manifest.json")).unwrap_or_default();
    let digest: String = Sha256::digest(&manifest)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    DirectoryDataSource::new(dir).with_trusted_public_manifest(digest)
}

fn edit_manifest(dir: &Path, edit: impl FnOnce(&mut Value)) {
    let path = dir.join("manifest.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    edit(&mut manifest);
    fs::write(path, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
}

/// Writes `bytes` to the table file and declares them in the manifest with their real size and
/// hash, so only what the bytes hold can be refused.
fn replace_table(dir: &Path, bytes: &[u8]) {
    fs::write(dir.join(TABLE), bytes).unwrap();
    let sha256: String = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    edit_manifest(dir, |manifest| {
        let file = &mut manifest["datasets"][0]["files"][0];
        file["bytes"] = bytes.len().into();
        file["sha256"] = sha256.into();
    });
}

/// The manifest-level refusal of a load: its code, with nothing listed or active.
fn manifest_refusal(status: &DataStatus) -> &'static str {
    assert_eq!(status.manifest.state, ManifestState::Refused, "{status:#?}");
    assert!(status.datasets.is_empty() && status.bindings.is_empty());
    assert!(
        status
            .features
            .iter()
            .all(|feature| feature.data == FeatureData::Unavailable),
        "a refused manifest activates nothing"
    );
    status.manifest.reason.expect("a refusal has a reason")
}

/// Per dataset (state, reason) and per binding (feature, state, reason).
type Summary = (
    Vec<(CheckState, Option<&'static str>)>,
    Vec<(String, BindingState, Option<&'static str>)>,
);

fn summary(status: &DataStatus) -> Summary {
    assert_eq!(
        status.manifest.state,
        ManifestState::Verified,
        "{status:#?}"
    );
    (
        status
            .datasets
            .iter()
            .map(|dataset| (dataset.state, dataset.reason))
            .collect(),
        status
            .bindings
            .iter()
            .map(|binding| (binding.feature.clone(), binding.state, binding.reason))
            .collect(),
    )
}

fn active_features(status: &DataStatus) -> Vec<&'static str> {
    status
        .features
        .iter()
        .filter(|feature| feature.data == FeatureData::Active)
        .map(|feature| feature.id)
        .collect()
}

/// The table dataset refused for `reason`: both bindings use it, so neither is active, though
/// the parameter dataset verified.
fn assert_table_refused(status: &DataStatus, reason: &'static str) {
    assert_eq!(
        summary(status),
        (
            vec![
                (CheckState::Refused, Some(reason)),
                (CheckState::Verified, None)
            ],
            vec![
                (
                    "fixture.table".to_owned(),
                    BindingState::Inactive,
                    Some("dataset_refused")
                ),
                (
                    "fixture.composite".to_owned(),
                    BindingState::Inactive,
                    Some("dataset_refused")
                ),
            ]
        ),
        "{status:#?}"
    );
    assert!(active_features(status).is_empty(), "no partial activation");
    assert_eq!(status.datasets[0].files[0].reason, Some(reason));
}

#[test]
fn the_committed_fixture_verifies_and_activates_its_bindings() {
    let status = load(&fixture_dir());
    assert_eq!(status.manifest.version, Some(1));
    assert_eq!(
        summary(&status),
        (
            vec![(CheckState::Verified, None), (CheckState::Verified, None)],
            vec![
                ("fixture.table".to_owned(), BindingState::Active, None),
                ("fixture.composite".to_owned(), BindingState::Active, None),
            ]
        )
    );
    let records: Vec<Option<u64>> = status
        .datasets
        .iter()
        .map(|dataset| dataset.files[0].records)
        .collect();
    assert_eq!(records, [Some(3), Some(2)], "rows and fields decoded");
    assert_eq!(status.datasets[0].distribution, "public");
    assert!(
        active_features(&status).is_empty(),
        "fixtures do not activate scientific features"
    );
    assert!(
        status
            .features
            .iter()
            .all(|feature| feature.engine == "not_implemented"),
        "verified data runs nothing"
    );
    assert!(status.is_fully_verified());
    let public_only = load_data(
        &trusted_source(&fixture_dir()),
        DistributionPolicy::PublicOnly,
    );
    assert!(public_only.is_fully_verified(), "the fixture is public");
}

#[test]
fn self_declared_generic_columns_do_not_activate_a_scientific_feature() {
    let dir = copy();
    edit_manifest(dir.path(), |manifest| {
        manifest["bindings"][0]["feature"] = "prediction.nmr".into();
    });
    let status = load(dir.path());
    let binding = &status.bindings[0];
    assert_eq!(binding.feature, "prediction.nmr");
    assert_eq!(binding.state, BindingState::Inactive);
    assert_eq!(binding.reason, Some("profile_unsupported"));
    assert!(
        !active_features(&status).contains(&"prediction.nmr"),
        "a self-declared toy table is not an NMR model"
    );
}

#[test]
fn a_rights_note_cannot_authorize_an_unreviewed_personal_dataset() {
    let dir = copy();
    edit_manifest(dir.path(), |manifest| {
        manifest["datasets"][1]["distribution"] = "personal".into();
        manifest["datasets"][1]["rights"] = "self-declared permission".into();
    });
    let status = load(dir.path());
    assert_eq!(status.datasets[1].state, CheckState::Refused);
    assert_eq!(status.datasets[1].reason, Some("rights_unverified"));
    assert_eq!(status.bindings[1].state, BindingState::Inactive);
}

#[test]
fn a_public_label_without_a_trusted_manifest_cannot_authorize_loading() {
    let dir = copy();
    edit_manifest(dir.path(), |manifest| {
        manifest["datasets"][0]["distribution"] = "public".into();
        manifest["datasets"][0]["rights"] = "self-declared public data".into();
    });
    let source = DirectoryDataSource::new(dir.path());
    let status = load_data(&source, DistributionPolicy::RequireVerifiedRights);
    assert_eq!(status.manifest.state, ManifestState::Refused);
    assert_eq!(status.manifest.reason, Some("rights_unverified"));
    assert!(status.datasets.is_empty());

    let dir = copy();
    let source = trusted_source(dir.path());
    edit_manifest(dir.path(), |manifest| {
        manifest["datasets"][0]["rights"] = "changed after approval".into()
    });
    let status = load_data(&source, DistributionPolicy::PublicOnly);
    assert_eq!(status.manifest.state, ManifestState::Refused);
    assert_eq!(status.manifest.reason, Some("rights_unverified"));
}

#[test]
fn an_empty_registry_is_not_reported_as_fully_verified_data() {
    let dir = copy();
    edit_manifest(dir.path(), |manifest| {
        manifest["datasets"] = json!([]);
        manifest["bindings"] = json!([]);
    });
    let status = load(dir.path());
    assert!(!status.is_fully_verified());
}

#[test]
fn a_missing_or_unusable_data_directory_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        manifest_refusal(&load(&dir.path().join("absent"))),
        "data_root_invalid"
    );
    let file = dir.path().join("a-file");
    fs::write(&file, b"{}").unwrap();
    assert_eq!(manifest_refusal(&load(&file)), "data_root_invalid");
    assert_eq!(manifest_refusal(&load(dir.path())), "manifest_missing");
}

#[test]
fn a_manifest_of_another_version_is_refused_whole() {
    let dir = copy();
    edit_manifest(dir.path(), |manifest| {
        manifest["manifest_version"] = 2.into()
    });
    let status = load(dir.path());
    assert_eq!(manifest_refusal(&status), "manifest_version_unsupported");
    assert!(
        status
            .manifest
            .message
            .as_deref()
            .unwrap()
            .contains("version 2")
    );
}

#[test]
fn a_malformed_manifest_is_refused_whole() {
    for (name, edit) in [
        (
            "an unknown field",
            Box::new(|manifest: &mut Value| manifest["note"] = "x".into())
                as Box<dyn FnOnce(&mut Value)>,
        ),
        (
            "a missing hash",
            Box::new(|manifest: &mut Value| {
                manifest["datasets"][0]["files"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("sha256");
            }),
        ),
        (
            "an uppercase hash",
            Box::new(|manifest: &mut Value| {
                let file = &mut manifest["datasets"][0]["files"][0];
                file["sha256"] = file["sha256"].as_str().unwrap().to_uppercase().into();
            }),
        ),
        (
            "an invalid dataset ID",
            Box::new(|manifest: &mut Value| manifest["datasets"][0]["id"] = "Not An ID".into()),
        ),
        (
            "an unknown distribution",
            Box::new(|manifest: &mut Value| {
                manifest["datasets"][0]["distribution"] = "shared".into();
            }),
        ),
        (
            "an empty rights note",
            Box::new(|manifest: &mut Value| manifest["datasets"][0]["rights"] = " ".into()),
        ),
    ] {
        let dir = copy();
        edit_manifest(dir.path(), edit);
        assert_eq!(
            manifest_refusal(&load(dir.path())),
            "manifest_invalid",
            "{name}"
        );
    }

    let dir = copy();
    fs::write(
        dir.path().join("manifest.json"),
        br#"{"manifest_version": 1, "manifest_version": 1, "datasets": [], "bindings": []}"#,
    )
    .unwrap();
    assert_eq!(
        manifest_refusal(&load(dir.path())),
        "manifest_invalid",
        "a repeated key"
    );
    fs::write(
        dir.path().join("manifest.json"),
        vec![b' '; 1024 * 1024 + 1],
    )
    .unwrap();
    assert_eq!(manifest_refusal(&load(dir.path())), "manifest_too_large");
}

#[test]
fn a_manifest_field_naming_code_is_refused() {
    let dir = copy();
    edit_manifest(dir.path(), |manifest| {
        manifest["datasets"][1]["files"][0]["Entry-Point"] = "run".into();
    });
    assert_eq!(manifest_refusal(&load(dir.path())), "executable_reference");
}

#[test]
fn duplicate_identifiers_and_too_many_datasets_are_refused_whole() {
    let dir = copy();
    edit_manifest(dir.path(), |manifest| {
        manifest["datasets"][1]["id"] = "synthetic-example-table".into();
    });
    assert_eq!(manifest_refusal(&load(dir.path())), "duplicate_id");

    let dir = copy();
    edit_manifest(dir.path(), |manifest| {
        let binding = manifest["bindings"][0].clone();
        manifest["bindings"].as_array_mut().unwrap().push(binding);
    });
    assert_eq!(
        manifest_refusal(&load(dir.path())),
        "duplicate_id",
        "a feature bound twice"
    );

    let dir = copy();
    edit_manifest(dir.path(), |manifest| {
        let dataset = manifest["datasets"][0].clone();
        let datasets: Vec<Value> = (0..257)
            .map(|index| {
                let mut copy = dataset.clone();
                copy["id"] = format!("synthetic-{index}").into();
                copy
            })
            .collect();
        manifest["datasets"] = datasets.into();
    });
    assert_eq!(manifest_refusal(&load(dir.path())), "limit_exceeded");
}

#[test]
fn a_binding_with_a_wrong_reference_stays_inactive_and_others_do_not() {
    for (edit, reason) in [
        (
            json!({"dataset": "synthetic-absent", "version": "1"}),
            "unknown_dataset",
        ),
        (
            json!({"dataset": "synthetic-example-parameters", "version": "2"}),
            "version_mismatch",
        ),
    ] {
        let dir = copy();
        edit_manifest(dir.path(), |manifest| {
            manifest["bindings"][1]["datasets"][1] = edit
        });
        let status = load(dir.path());
        assert_eq!(
            summary(&status).1,
            [
                ("fixture.table".to_owned(), BindingState::Active, None),
                (
                    "fixture.composite".to_owned(),
                    BindingState::Inactive,
                    Some(reason)
                ),
            ],
            "{reason}"
        );
        assert!(active_features(&status).is_empty());
    }

    let dir = copy();
    edit_manifest(dir.path(), |manifest| {
        manifest["bindings"][0]["feature"] = "layout.teleport".into();
    });
    let status = load(dir.path());
    assert_eq!(
        (
            status.bindings[0].state,
            status.bindings[0].reason,
            status.bindings[0].operation
        ),
        (BindingState::Inactive, Some("unknown_feature"), None)
    );
    assert!(active_features(&status).is_empty());
}

#[test]
fn a_hash_mismatch_refuses_the_dataset_and_every_binding_that_needs_it() {
    let dir = copy();
    let mut bytes = fs::read(dir.path().join(TABLE)).unwrap();
    let digit = bytes.iter().position(|byte| *byte == b'3').unwrap();
    bytes[digit] = b'4';
    fs::write(dir.path().join(TABLE), bytes).unwrap();
    assert_table_refused(&load(dir.path()), "hash_mismatch");
}

#[test]
fn a_missing_file_is_refused() {
    let dir = copy();
    fs::remove_file(dir.path().join(TABLE)).unwrap();
    assert_table_refused(&load(dir.path()), "file_missing");
}

#[test]
fn a_file_of_another_size_than_declared_is_refused() {
    let dir = copy();
    let bytes = fs::read(dir.path().join(TABLE)).unwrap();
    fs::write(dir.path().join(TABLE), &bytes[..bytes.len() - 10]).unwrap();
    assert_table_refused(&load(dir.path()), "size_mismatch");

    let dir = copy();
    let mut longer = bytes.clone();
    longer.extend_from_slice(b"{}\n");
    fs::write(dir.path().join(TABLE), longer).unwrap();
    assert_table_refused(&load(dir.path()), "size_mismatch");

    let dir = copy();
    edit_manifest(dir.path(), |manifest| {
        manifest["datasets"][0]["files"][0]["bytes"] = (64_u64 * 1024 * 1024 + 1).into();
    });
    assert_table_refused(&load(dir.path()), "file_too_large");
}

#[test]
fn truncated_or_corrupt_content_is_refused_even_when_its_hash_is_declared() {
    let bytes = fs::read(fixture_dir().join(TABLE)).unwrap();
    for (name, damaged) in [
        ("cut inside a row", bytes[..bytes.len() - 20].to_vec()),
        ("a row of the wrong type", {
            let text = String::from_utf8(bytes.clone()).unwrap();
            text.replace("\"size\": 5", "\"size\": \"5\"").into_bytes()
        }),
        ("a repeated unique key", {
            let text = String::from_utf8(bytes.clone()).unwrap();
            text.replace("synthetic-row-2", "synthetic-row-1")
                .into_bytes()
        }),
        ("not UTF-8", [&bytes[..], b"\xff\n"].concat()),
    ] {
        let dir = copy();
        replace_table(dir.path(), &damaged);
        assert_table_refused(&load(dir.path()), "content_invalid");
        let message = load(dir.path()).datasets[0].message.clone().unwrap();
        assert!(
            message.starts_with("tables/example-rows.jsonl: "),
            "{name}: {message}"
        );
    }
}

#[test]
fn executable_content_is_refused_by_signature_and_name() {
    for signature in [
        &b"\x7fELF\x02\x01\x01"[..],
        b"MZ\x90\x00",
        b"#!/bin/sh\necho\n",
    ] {
        let dir = copy();
        replace_table(dir.path(), signature);
        assert_table_refused(&load(dir.path()), "executable_content");
    }

    let dir = copy();
    fs::rename(dir.path().join(TABLE), dir.path().join("tables/rows.dll")).unwrap();
    edit_manifest(dir.path(), |manifest| {
        manifest["datasets"][0]["files"][0]["path"] = "tables/rows.dll".into();
    });
    assert_table_refused(&load(dir.path()), "executable_content");
}

#[test]
fn a_path_that_leaves_the_data_directory_is_refused() {
    for path in [
        "../outside.jsonl",
        "/etc/hostname",
        "tables\\example-rows.jsonl",
    ] {
        let dir = copy();
        edit_manifest(dir.path(), |manifest| {
            manifest["datasets"][0]["files"][0]["path"] = path.into();
        });
        assert_table_refused(&load(dir.path()), "path_invalid");
    }

    let dir = copy();
    fs::remove_file(dir.path().join(TABLE)).unwrap();
    fs::create_dir(dir.path().join(TABLE)).unwrap();
    assert_table_refused(&load(dir.path()), "not_a_file");
}

#[cfg(unix)]
#[test]
fn a_symbolic_link_out_of_the_data_directory_is_refused_and_one_inside_is_followed() {
    let outside = tempfile::tempdir().unwrap();
    let dir = copy();
    let table = dir.path().join(TABLE);
    fs::rename(&table, outside.path().join("rows.jsonl")).unwrap();
    std::os::unix::fs::symlink(outside.path().join("rows.jsonl"), &table).unwrap();
    assert_table_refused(&load(dir.path()), "path_escape");

    let dir = copy();
    let table = dir.path().join(TABLE);
    fs::rename(&table, dir.path().join("rows.jsonl")).unwrap();
    std::os::unix::fs::symlink("../rows.jsonl", &table).unwrap();
    assert!(
        load(dir.path()).is_fully_verified(),
        "a link within stays in"
    );
}

#[test]
fn an_unsupported_format_or_unusable_schema_is_refused() {
    let dir = copy();
    edit_manifest(dir.path(), |manifest| {
        manifest["datasets"][0]["files"][0]["format_version"] = 2.into();
    });
    assert_table_refused(&load(dir.path()), "format_unsupported");

    for (name, edit) in [
        (
            "a unique number column",
            json!([{"name": "weight", "type": "number", "unique": true}]),
        ),
        ("an unknown type", json!([{"name": "key", "type": "date"}])),
        (
            "a repeated column",
            json!([{"name": "key", "type": "string"}, {"name": "key", "type": "string"}]),
        ),
        ("no columns", json!([])),
    ] {
        let dir = copy();
        edit_manifest(dir.path(), |manifest| {
            manifest["datasets"][0]["files"][0]["columns"] = edit;
        });
        let status = load(dir.path());
        assert_eq!(
            status.datasets[0].reason,
            Some("schema_invalid"),
            "{name}: {status:#?}"
        );
    }
}

#[test]
fn personal_data_is_refused_where_only_public_data_is_accepted() {
    let dir = copy();
    edit_manifest(dir.path(), |manifest| {
        manifest["datasets"][1]["distribution"] = "personal".into();
    });
    let unreviewed = load(dir.path());
    assert_eq!(unreviewed.datasets[1].reason, Some("rights_unverified"));
    assert_eq!(
        unreviewed.datasets[1].files[0].state,
        CheckState::NotChecked
    );
    let status = load_data(&trusted_source(dir.path()), DistributionPolicy::PublicOnly);
    assert_eq!(
        summary(&status),
        (
            vec![
                (CheckState::Verified, None),
                (CheckState::Refused, Some("distribution_not_allowed"))
            ],
            vec![
                ("fixture.table".to_owned(), BindingState::Active, None),
                (
                    "fixture.composite".to_owned(),
                    BindingState::Inactive,
                    Some("dataset_refused")
                ),
            ]
        )
    );
    assert_eq!(status.datasets[1].files[0].state, CheckState::NotChecked);
}
