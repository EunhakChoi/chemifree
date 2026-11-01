//! Document settings and application preferences through the shared application, on the real
//! file stores: settings are the document's own (edited by `update_settings`, reset, or imported
//! as one undoable step), new documents copy the preferred settings once when they are created,
//! and preferences never reach an existing document, its file, or its history, whichever
//! process changes them.

use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use chemifree_application::{
    Application, ApplicationError, CommandDto, ErrorCode, PreferenceRepository, Preferences,
    PreferencesChange, PreferencesResponse, SettingsSource, StorageError,
};
use chemifree_domain::settings::SettingsError;
use chemifree_providers::{FileDocumentRepository, FilePreferenceStore, LocalCapabilities};
use serde_json::{Value, json};
use tempfile::TempDir;

type App = Application<LocalCapabilities, FileDocumentRepository>;

/// An application on `data`, as one process of the API or the CLI starts it.
fn process(data: &Path) -> App {
    Application::new(LocalCapabilities, FileDocumentRepository::new(data))
        .with_preferences(FilePreferenceStore::new(data))
}

fn application() -> (App, TempDir) {
    let data = tempfile::tempdir().unwrap();
    (process(data.path()), data)
}

fn commands(value: Value) -> Vec<CommandDto> {
    serde_json::from_value(value).unwrap()
}

fn change(value: Value) -> PreferencesChange {
    serde_json::from_value(value).unwrap()
}

fn settings(app: &App, id: &str) -> Value {
    serde_json::to_value(app.document(id).unwrap()).unwrap()["document"]["settings"].clone()
}

fn json_of(response: &PreferencesResponse) -> Value {
    serde_json::to_value(response).unwrap()
}

fn created(app: &App, title: &str) -> String {
    app.create_document(None, Some(title)).unwrap().document.id
}

fn defaults() -> Value {
    json!({
        "bond_length_pt": 30.0, "line_width_pt": 1.2, "label_font": "sans-serif",
        "label_size_pt": 12.0, "carbon_labels": "default", "display_unit": "pt"
    })
}

fn stored_file(data: &Path, id: &str) -> Vec<u8> {
    fs::read(data.join("documents").join(format!("{id}.json"))).unwrap()
}

#[test]
fn a_new_document_without_preferences_has_the_built_in_defaults() {
    let (app, _data) = application();
    let id = created(&app, "Plain");
    assert_eq!(settings(&app, &id), defaults());
    let preferences = json_of(&app.preferences().unwrap());
    assert_eq!(preferences["revision"], 0);
    assert_eq!(preferences["ui"], json!({"theme": "system"}));
    assert_eq!(preferences["new_documents"], json!({}));
    assert_eq!(preferences["new_document_settings"], defaults());
}

#[test]
fn update_settings_is_an_undoable_edit_and_refused_values_carry_their_reason() {
    let (app, _data) = application();
    let id = created(&app, "Styled");
    let edited = app
        .apply_transaction(
            &id,
            None,
            0,
            commands(
                json!([{"op": "update_settings", "line_width_pt": 2.5, "label_font": "serif"}]),
            ),
        )
        .unwrap();
    assert_eq!(edited.document.revision, 1);
    assert_eq!(
        serde_json::to_value(&edited.document.settings).unwrap(),
        json!({
            "bond_length_pt": 30.0, "line_width_pt": 2.5, "label_font": "serif",
            "label_size_pt": 12.0, "carbon_labels": "default", "display_unit": "pt"
        })
    );
    let unchanged = app
        .apply_transaction(
            &id,
            None,
            1,
            commands(json!([{"op": "update_settings", "line_width_pt": 2.5}])),
        )
        .unwrap();
    assert_eq!(
        unchanged.document.revision, 1,
        "an update that changes nothing is a no-op"
    );

    let error = app
        .apply_transaction(
            &id,
            None,
            1,
            commands(json!([
                {"op": "update_settings", "display_unit": "mm"},
                {"op": "update_settings", "bond_length_pt": 201}
            ])),
        )
        .unwrap_err();
    assert_eq!(
        (error.code, error.command_index, error.reason_code()),
        (
            ErrorCode::InvalidCommand,
            Some(1),
            Some(SettingsError::BondLengthOutOfRange.code())
        )
    );
    assert_eq!(settings(&app, &id)["display_unit"], "pt", "nothing changed");

    app.undo(&id, None, 1).unwrap();
    assert_eq!(settings(&app, &id), defaults());
    assert!(
        serde_json::from_value::<Vec<CommandDto>>(
            json!([{"op": "update_settings", "label_font": "fantasy"}])
        )
        .is_err(),
        "a value outside a closed set makes the body malformed"
    );
}

#[test]
fn new_documents_copy_the_preferred_settings_once_and_preferences_never_reach_them_later() {
    let data = tempfile::tempdir().unwrap();
    let app = process(data.path());
    let preferred = app
        .set_preferences(
            None,
            0,
            change(json!({"ui": {"theme": "dark"}, "new_documents": {"bond_length_pt": 40, "carbon_labels": "all"}})),
        )
        .unwrap();
    assert_eq!(preferred.revision, 1);
    let first = created(&app, "First");
    let first_settings = settings(&app, &first);
    assert_eq!(first_settings["bond_length_pt"], 40.0);
    assert_eq!(first_settings["carbon_labels"], "all");
    assert_eq!(
        first_settings["label_font"], "sans-serif",
        "unnamed fields are defaults"
    );
    let first_file = stored_file(data.path(), &first);

    // Another process changes the preferences.
    let other = process(data.path());
    other
        .set_preferences(
            None,
            1,
            change(json!({"ui": {"theme": "light"}, "new_documents": {"bond_length_pt": 25, "label_size_pt": 9}})),
        )
        .unwrap();

    // The open document, its file, and its history are untouched; a new document gets the new
    // preferences, also in the first process, which reads them from storage.
    assert_eq!(settings(&app, &first), first_settings);
    assert_eq!(stored_file(data.path(), &first), first_file);
    assert_eq!(app.document(&first).unwrap().session.undo_depth, 0);
    let second = created(&app, "Second");
    assert_eq!(settings(&app, &second)["bond_length_pt"], 25.0);
    assert_eq!(settings(&app, &second)["label_size_pt"], 9.0);
    assert_eq!(settings(&app, &second)["carbon_labels"], "default");

    // After reopening in yet another process, each document still has its own settings.
    let reopened = process(data.path());
    assert_eq!(settings(&reopened, &first), first_settings);
    assert_eq!(settings(&reopened, &second)["bond_length_pt"], 25.0);
    assert_eq!(
        json_of(&reopened.preferences().unwrap())["ui"]["theme"],
        "light"
    );
}

#[test]
fn a_stale_preferences_revision_is_a_conflict_whichever_process_wrote_first() {
    let data = tempfile::tempdir().unwrap();
    let (one, two) = (process(data.path()), process(data.path()));
    assert_eq!(one.preferences().unwrap().revision, 0);
    assert_eq!(two.preferences().unwrap().revision, 0);
    two.set_preferences(
        None,
        0,
        change(json!({"ui": {"theme": "dark"}, "new_documents": {}})),
    )
    .unwrap();
    let error = one
        .set_preferences(
            None,
            0,
            change(json!({"ui": {"theme": "light"}, "new_documents": {}})),
        )
        .unwrap_err();
    assert_eq!(
        (error.code, error.current_revision),
        (ErrorCode::RevisionConflict, Some(1))
    );
    assert_eq!(json_of(&one.preferences().unwrap())["ui"]["theme"], "dark");
}

#[test]
fn computation_preferences_are_refused_as_not_implemented_and_never_stored() {
    let (app, data) = application();
    for computation in [
        json!({"nmr": {"solvent": "CDCl3"}}),
        json!({"numeric_precision": {"decimals": 2}}),
        // Naming a setting refuses the write whatever its value, null included.
        json!({"nmr": null}),
        json!({"numeric_precision": null}),
    ] {
        let error = app
            .set_preferences(
                None,
                0,
                change(json!({"ui": {"theme": "dark"}, "new_documents": {}, "computation": computation})),
            )
            .unwrap_err();
        assert_eq!(
            error.code,
            ErrorCode::SettingNotImplemented,
            "{computation}"
        );
    }
    assert_eq!(app.preferences().unwrap().revision, 0, "nothing was stored");
    assert!(!data.path().join("preferences.json").exists());
    // An empty section names nothing, so it is accepted.
    app.set_preferences(
        None,
        0,
        change(json!({"ui": {"theme": "dark"}, "new_documents": {}, "computation": {}})),
    )
    .unwrap();

    let reported = json_of(&app.preferences().unwrap())["computation"].clone();
    for (setting, followup) in [("nmr", "P66"), ("numeric_precision", "P60")] {
        assert_eq!(reported[setting]["status"], "not_implemented", "{setting}");
        assert_eq!(reported[setting]["owner"], "preferences", "{setting}");
        assert_eq!(reported[setting]["followup"], followup, "{setting}");
    }
}

#[test]
fn mass_display_precision_is_persisted_and_old_writes_preserve_it() {
    let (app, data) = application();
    let document = created(&app, "Existing");
    let before = app.document(&document).unwrap();
    let original_file = stored_file(data.path(), &document);
    assert_eq!(
        json_of(&app.preferences().unwrap())["computation"]["mass_precision"]["decimals"],
        2
    );

    let request = |precision| {
        change(
            json!({"ui":{"theme":"dark"},"new_documents":{},"computation":{"mass_precision":precision}}),
        )
    };
    let saved = app
        .set_preferences(None, 0, request(json!({"decimals":5})))
        .unwrap();
    assert_eq!(
        json_of(&saved)["computation"]["mass_precision"]["decimals"],
        5
    );
    assert_eq!(
        json_of(&process(data.path()).preferences().unwrap())["computation"]["mass_precision"]["decimals"],
        5
    );

    for invalid in [
        json!(null),
        json!({"decimals":9}),
        json!({"decimals":-1}),
        json!({"decimals":2,"other":1}),
    ] {
        assert_eq!(
            app.set_preferences(None, 1, request(invalid))
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(app.preferences().unwrap().revision, 1);
    }
    let old_client = change(json!({"ui":{"theme":"light"},"new_documents":{}}));
    let after_old = app.set_preferences(None, 1, old_client).unwrap();
    assert_eq!(
        json_of(&after_old)["computation"]["mass_precision"]["decimals"],
        5
    );

    let adopted = app.preferences_from_document(None, 2, &document).unwrap();
    assert_eq!(
        json_of(&adopted)["computation"]["mass_precision"]["decimals"],
        5
    );
    let reset = app.reset_preferences(None, adopted.revision).unwrap();
    assert_eq!(
        json_of(&reset)["computation"]["mass_precision"]["decimals"],
        2
    );
    assert_eq!(
        serde_json::to_value(app.document(&document).unwrap()).unwrap(),
        serde_json::to_value(before).unwrap()
    );
    assert_eq!(stored_file(data.path(), &document), original_file);
}

#[test]
fn preferred_settings_outside_their_range_are_refused_with_their_reason() {
    let (app, _data) = application();
    let error = app
        .set_preferences(
            None,
            0,
            change(json!({"ui": {"theme": "system"}, "new_documents": {"label_size_pt": 100}})),
        )
        .unwrap_err();
    assert_eq!(
        (error.code, error.reason_code()),
        (ErrorCode::InvalidRequest, Some("label_size_out_of_range"))
    );
    assert_eq!(app.preferences().unwrap().revision, 0);
}

#[test]
fn preference_writes_replay_by_request_id_and_unchanged_ones_keep_the_revision() {
    let (app, _data) = application();
    let body = json!({"ui": {"theme": "dark"}, "new_documents": {}});
    let first = app
        .set_preferences(Some("p1"), 0, change(body.clone()))
        .unwrap();
    let again = app
        .set_preferences(Some("p1"), 0, change(body.clone()))
        .unwrap();
    assert_eq!(json_of(&first), json_of(&again));
    let reused: ApplicationError = app
        .set_preferences(
            Some("p1"),
            0,
            change(json!({"ui": {"theme": "light"}, "new_documents": {}})),
        )
        .unwrap_err();
    assert_eq!(reused.code, ErrorCode::RequestIdReused);
    let same = app.set_preferences(None, 1, change(body)).unwrap();
    assert_eq!(
        same.revision, 1,
        "the same preferences are not a new revision"
    );
}

#[test]
fn document_settings_reset_to_the_defaults_or_the_preferences_as_one_undoable_step() {
    let (app, _data) = application();
    app.set_preferences(
        None,
        0,
        change(json!({"ui": {"theme": "system"}, "new_documents": {"line_width_pt": 3}})),
    )
    .unwrap();
    let id = created(&app, "Reset");
    app.apply_transaction(
        &id,
        None,
        0,
        commands(json!([{"op": "update_settings", "label_size_pt": 20, "display_unit": "in"}])),
    )
    .unwrap();

    let reset = app
        .reset_settings(&id, None, 1, SettingsSource::Preferences)
        .unwrap();
    assert_eq!(reset.document.revision, 2);
    assert_eq!(reset.session.undo_depth, 2, "one step");
    let preferred = settings(&app, &id);
    assert_eq!(preferred["line_width_pt"], 3.0);
    assert_eq!(preferred["label_size_pt"], 12.0);

    app.reset_settings(&id, None, 2, SettingsSource::Defaults)
        .unwrap();
    assert_eq!(settings(&app, &id), defaults());
    let again = app
        .reset_settings(&id, None, 3, SettingsSource::Defaults)
        .unwrap();
    assert_eq!(
        again.document.revision, 3,
        "resetting what is already the default changes nothing"
    );

    app.undo(&id, None, 3).unwrap();
    assert_eq!(settings(&app, &id), preferred);
    let error = app
        .reset_settings(&id, None, 1, SettingsSource::Defaults)
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::RevisionConflict);
}

#[test]
fn another_documents_settings_are_imported_as_one_undoable_step() {
    let (app, _data) = application();
    let source = created(&app, "Source");
    app.apply_transaction(
        &source,
        None,
        0,
        commands(
            json!([{"op": "update_settings", "bond_length_pt": 50, "label_font": "monospace"}]),
        ),
    )
    .unwrap();
    let target = created(&app, "Target");
    let imported = app.import_settings(&target, None, 0, &source).unwrap();
    assert_eq!(imported.document.revision, 1);
    assert_eq!(settings(&app, &target), settings(&app, &source));
    let again = app.import_settings(&target, None, 1, &source).unwrap();
    assert_eq!(
        again.document.revision, 1,
        "importing the same settings changes nothing"
    );
    app.undo(&target, None, 1).unwrap();
    assert_eq!(settings(&app, &target), defaults());

    let missing = app
        .import_settings(&target, None, 2, "doc-0000000000000000")
        .unwrap_err();
    assert_eq!(missing.code, ErrorCode::DocumentNotFound);
    assert!(
        missing.message.contains("doc-0000000000000000"),
        "{}",
        missing.message
    );
}

#[test]
fn new_document_preferences_are_set_from_a_document_and_reset_to_the_defaults() {
    let (app, _data) = application();
    let id = created(&app, "Model");
    app.apply_transaction(
        &id,
        None,
        0,
        commands(json!([{"op": "update_settings", "label_size_pt": 14.5, "carbon_labels": "all"}])),
    )
    .unwrap();
    let from = app.preferences_from_document(None, 0, &id).unwrap();
    let from = json_of(&from);
    assert_eq!(from["revision"], 1);
    assert_eq!(
        from["new_documents"],
        settings(&app, &id),
        "every field is named"
    );
    assert_eq!(settings(&app, &created(&app, "Next")), settings(&app, &id));

    let reset = json_of(&app.reset_preferences(None, 1).unwrap());
    assert_eq!(reset["revision"], 2);
    assert_eq!(reset["new_documents"], json!({}));
    assert_eq!(reset["ui"], json!({"theme": "system"}));
    assert_eq!(settings(&app, &created(&app, "Plain")), defaults());
}

#[test]
fn a_copy_keeps_its_sources_settings() {
    let (app, _data) = application();
    let id = created(&app, "Original");
    app.apply_transaction(
        &id,
        None,
        0,
        commands(json!([{"op": "update_settings", "line_width_pt": 0.5}])),
    )
    .unwrap();
    app.set_preferences(
        None,
        0,
        change(json!({"ui": {"theme": "system"}, "new_documents": {"line_width_pt": 4}})),
    )
    .unwrap();
    let copy = app.save_as(&id, None, 1, None).unwrap();
    assert_eq!(
        serde_json::to_value(&copy.copy.document.settings).unwrap()["line_width_pt"],
        0.5
    );
}

#[test]
fn a_create_already_answered_is_replayed_even_when_the_preferences_broke_since() {
    let (app, data) = application();
    let first = app.create_document(Some("create-1"), Some("Kept")).unwrap();
    fs::write(data.path().join("preferences.json"), b"{ not json").unwrap();

    let again = app
        .create_document(Some("create-1"), Some("Kept"))
        .expect("a create already answered is replayed without reading the preferences");
    assert_eq!(again.document.id, first.document.id);
    assert_eq!(again.document.revision, first.document.revision);
    let error = app
        .create_document(Some("create-2"), Some("New"))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::PreferencesUnreadable);
}

#[derive(Clone)]
struct GatedPreferences {
    block: Arc<AtomicBool>,
    entered: mpsc::Sender<()>,
    release: Arc<Mutex<()>>,
}

impl PreferenceRepository for GatedPreferences {
    fn load(&self) -> Result<Option<Preferences>, StorageError> {
        if self.block.load(Ordering::SeqCst) {
            self.entered.send(()).unwrap();
            drop(self.release.lock().unwrap());
        }
        Ok(None)
    }

    fn save(&self, _: &Preferences, _: u64) -> Result<(), StorageError> {
        Ok(())
    }
}

#[test]
fn a_slow_preference_read_during_create_does_not_block_another_document() {
    let directory = tempfile::tempdir().unwrap();
    let (entered, waiting) = mpsc::channel();
    let store = GatedPreferences {
        block: Arc::new(AtomicBool::new(false)),
        entered,
        release: Arc::new(Mutex::new(())),
    };
    let app = Arc::new(
        Application::new(
            LocalCapabilities,
            FileDocumentRepository::new(directory.path()),
        )
        .with_preferences(store.clone()),
    );
    let first = app.create_document(None, Some("Already open")).unwrap();
    let held = store.release.lock().unwrap();
    store.block.store(true, Ordering::SeqCst);
    let creating = std::thread::spawn({
        let app = Arc::clone(&app);
        move || app.create_document(Some("slow-create"), Some("Later"))
    });
    waiting.recv_timeout(Duration::from_secs(5)).unwrap();
    let (sent, answer) = mpsc::channel();
    let reading = std::thread::spawn({
        let app = Arc::clone(&app);
        let id = first.document.id.clone();
        move || sent.send(app.document(&id)).unwrap()
    });
    let before_release = answer.recv_timeout(Duration::from_secs(1)).ok();
    store.block.store(false, Ordering::SeqCst);
    drop(held);
    let result = before_release.as_ref().map(|answer| answer.is_ok());
    if before_release.is_none() {
        answer
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
    }
    reading.join().unwrap();
    creating.join().unwrap().unwrap();
    assert_eq!(
        result,
        Some(true),
        "the other document must not wait for preference I/O"
    );
}

#[test]
fn a_settings_request_marks_its_id_running_before_reading_preferences() {
    let directory = tempfile::tempdir().unwrap();
    let (entered, waiting) = mpsc::channel();
    let store = GatedPreferences {
        block: Arc::new(AtomicBool::new(false)),
        entered,
        release: Arc::new(Mutex::new(())),
    };
    let app = Arc::new(
        Application::new(
            LocalCapabilities,
            FileDocumentRepository::new(directory.path()),
        )
        .with_preferences(store.clone()),
    );
    let id = app.create_document(None, None).unwrap().document.id;
    let held = store.release.lock().unwrap();
    store.block.store(true, Ordering::SeqCst);
    let resetting = std::thread::spawn({
        let (app, id) = (Arc::clone(&app), id.clone());
        move || app.reset_settings(&id, Some("same-reset"), 0, SettingsSource::Preferences)
    });
    waiting.recv_timeout(Duration::from_secs(5)).unwrap();
    let (sent, answer) = mpsc::channel();
    let repeated = std::thread::spawn({
        let (app, id) = (Arc::clone(&app), id.clone());
        move || {
            sent.send(app.reset_settings(&id, Some("same-reset"), 0, SettingsSource::Preferences))
                .unwrap()
        }
    });
    let (sent_stale, stale_answer) = mpsc::channel();
    let stale = std::thread::spawn({
        let (app, id) = (Arc::clone(&app), id.clone());
        move || {
            sent_stale
                .send(app.reset_settings(&id, Some("stale-reset"), 1, SettingsSource::Preferences))
                .unwrap()
        }
    });
    let before_release = answer.recv_timeout(Duration::from_secs(1)).ok();
    let stale_before_release = stale_answer.recv_timeout(Duration::from_secs(1)).ok();
    store.block.store(false, Ordering::SeqCst);
    drop(held);
    if before_release.is_none() {
        let _ = answer.recv_timeout(Duration::from_secs(5));
    }
    if stale_before_release.is_none() {
        let _ = stale_answer.recv_timeout(Duration::from_secs(5));
    }
    repeated.join().unwrap();
    stale.join().unwrap();
    resetting.join().unwrap().unwrap();
    assert_eq!(
        before_release.unwrap().unwrap_err().code,
        ErrorCode::RequestInProgress
    );
    assert_eq!(
        stale_before_release.unwrap().unwrap_err().code,
        ErrorCode::RevisionConflict
    );
}

#[test]
fn a_repeated_preference_write_is_answered_while_its_store_is_blocked() {
    let directory = tempfile::tempdir().unwrap();
    let (entered, waiting) = mpsc::channel();
    let store = GatedPreferences {
        block: Arc::new(AtomicBool::new(false)),
        entered,
        release: Arc::new(Mutex::new(())),
    };
    let app = Arc::new(
        Application::new(
            LocalCapabilities,
            FileDocumentRepository::new(directory.path()),
        )
        .with_preferences(store.clone()),
    );
    let previous = app.reset_preferences(Some("earlier-prefs"), 0).unwrap();
    let held = store.release.lock().unwrap();
    store.block.store(true, Ordering::SeqCst);
    let first = std::thread::spawn({
        let app = Arc::clone(&app);
        move || app.reset_preferences(Some("reset-prefs"), 0)
    });
    waiting.recv_timeout(Duration::from_secs(5)).unwrap();
    let (sent, answer) = mpsc::channel();
    let repeated = std::thread::spawn({
        let app = Arc::clone(&app);
        move || {
            sent.send(app.reset_preferences(Some("reset-prefs"), 0))
                .unwrap()
        }
    });
    let (replay_sent, replay_answer) = mpsc::channel();
    let replayed = std::thread::spawn({
        let app = Arc::clone(&app);
        move || {
            replay_sent
                .send(app.reset_preferences(Some("earlier-prefs"), 0))
                .unwrap()
        }
    });
    let before_release = answer.recv_timeout(Duration::from_secs(1)).ok();
    let replay_before_release = replay_answer.recv_timeout(Duration::from_secs(1)).ok();
    store.block.store(false, Ordering::SeqCst);
    drop(held);
    if before_release.is_none() {
        let _ = answer.recv_timeout(Duration::from_secs(5));
    }
    if replay_before_release.is_none() {
        let _ = replay_answer.recv_timeout(Duration::from_secs(5));
    }
    repeated.join().unwrap();
    replayed.join().unwrap();
    let completed = first.join().unwrap().unwrap();
    assert_eq!(
        before_release.unwrap().unwrap_err().code,
        ErrorCode::RequestInProgress
    );
    assert_eq!(replay_before_release.unwrap().unwrap(), previous);
    assert_eq!(
        app.reset_preferences(Some("reset-prefs"), 0).unwrap(),
        completed
    );
}

#[test]
fn exhausted_preference_revision_is_refused_without_writing() {
    let (app, data) = application();
    let path = data.path().join("preferences.json");
    let original = format!(
        "{{\"format\":\"chemifree.preferences\",\"format_version\":1,\"revision\":{},\"ui\":{{\"theme\":\"system\"}},\"new_documents\":{{}}}}",
        u64::MAX
    );
    fs::write(&path, &original).unwrap();
    let error = app
        .set_preferences(
            Some("revision-exhausted"),
            u64::MAX,
            change(json!({"ui": {"theme": "dark"}, "new_documents": {}})),
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::ResourceLimit);
    assert_eq!(fs::read_to_string(path).unwrap(), original);
}
