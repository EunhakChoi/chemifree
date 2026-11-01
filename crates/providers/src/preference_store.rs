//! The preferences of a data directory, kept in `<root>/preferences.json`, apart from every
//! document.
//!
//! A write takes the advisory lock `locks/preferences.lock`, checks the stored revision against
//! the base the caller names, and replaces the file atomically (a temporary file beside it,
//! flushed, then renamed over it), so writers in every process take turns and a stale one is
//! refused. A missing file means nothing is stored: revision 0 and the defaults. A file this
//! build cannot read is reported as corrupt, never replaced by defaults.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use chemifree_application::{
    MassPrecision, PreferenceRepository, Preferences, StorageError, Theme,
};
use chemifree_domain::settings::{Settings, SettingsEdit};
use serde::{Deserialize, Serialize};

use crate::file_format::{StoredCarbonLabels, StoredFont, StoredUnit};
use crate::file_store::{
    prune_stale_temporaries_in, unavailable, wait_for_lock, write_atomically, write_failed,
};

const FILE_NAME: &str = "preferences.json";
const LOCK_FILE: &str = "locks/preferences.lock";
const FORMAT: &str = "chemifree.preferences";
const FORMAT_VERSION: u32 = 2;
/// Preferences are a few hundred bytes; anything far larger is not a preferences file.
const MAX_PREFERENCES_BYTES: u64 = 64 * 1024;

/// Preferences stored as one JSON file in a data directory.
#[derive(Clone, Debug)]
pub struct FilePreferenceStore {
    root: PathBuf,
}

impl FilePreferenceStore {
    /// Uses `<root>/preferences.json` and `<root>/locks/preferences.lock`. Nothing is touched
    /// until the first write.
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    /// The preferences file.
    pub fn path(&self) -> PathBuf {
        self.root.join(FILE_NAME)
    }

    /// Removes temporary files that interrupted preference writes abandoned more than an hour
    /// ago.
    pub fn prune_stale_temporaries(&self) {
        prune_stale_temporaries_in(&self.root, ".preferences.");
    }

    fn lock(&self) -> Result<File, StorageError> {
        wait_for_lock(&self.root.join(LOCK_FILE), || "the preferences".to_owned())
    }

    fn read(&self) -> Result<Option<Preferences>, StorageError> {
        let path = self.path();
        let file = match File::open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(unavailable("open", &path, &error)),
        };
        let mut bytes = Vec::new();
        file.take(MAX_PREFERENCES_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| unavailable("read", &path, &error))?;
        if bytes.len() as u64 > MAX_PREFERENCES_BYTES {
            return Err(StorageError::Corrupt(format!(
                "{} exceeds {MAX_PREFERENCES_BYTES} bytes",
                path.display()
            )));
        }
        decode(&bytes)
            .map(Some)
            .map_err(|reason| StorageError::Corrupt(format!("{}: {reason}", path.display())))
    }
}

impl PreferenceRepository for FilePreferenceStore {
    fn load(&self) -> Result<Option<Preferences>, StorageError> {
        self.read()
    }

    fn save(&self, preferences: &Preferences, base_revision: u64) -> Result<(), StorageError> {
        if preferences.mass_precision.decimals > 8 {
            return Err(StorageError::Corrupt(
                "mass display decimals must be from 0 to 8".into(),
            ));
        }
        let _lock = self.lock()?;
        let stored_revision = self.read()?.map_or(0, |stored| stored.revision);
        if stored_revision != base_revision {
            return Err(StorageError::Conflict {
                stored_revision: Some(stored_revision),
            });
        }
        let path = self.path();
        // A v1 file has no mass setting. Preserve its exact bytes before the first v2
        // replacement, without overwriting an earlier saved migration of different bytes.
        match fs::read(&path) {
            Ok(bytes) => {
                let header: serde_json::Value = serde_json::from_slice(&bytes)
                    .map_err(|error| StorageError::Corrupt(error.to_string()))?;
                if header["format_version"] == 1 {
                    let mut retained = false;
                    for number in 1..=u32::MAX {
                        let name = if number == 1 {
                            "preferences.v1.json".to_owned()
                        } else {
                            format!("preferences.v1.{number}.json")
                        };
                        let kept = self.root.join("migrated").join(name);
                        match fs::read(&kept) {
                            Ok(existing) if existing == bytes => {
                                retained = true;
                                break;
                            }
                            Ok(_) => continue,
                            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                                write_atomically(&kept, &bytes).map_err(|error| {
                                    write_failed("keep old preferences as", &kept, &error)
                                })?;
                                retained = true;
                                break;
                            }
                            Err(error) => return Err(unavailable("read", &kept, &error)),
                        }
                    }
                    if !retained {
                        return Err(StorageError::Unavailable(
                            "no free preferences migration name".into(),
                        ));
                    }
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => (),
            Err(error) => return Err(unavailable("read", &path, &error)),
        }
        write_atomically(&path, &encode(preferences))
            .map_err(|error| write_failed("save", &path, &error))
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredPreferences {
    format: String,
    format_version: u32,
    revision: u64,
    ui: StoredUi,
    new_documents: StoredNewDocuments,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mass_precision: Option<MassPrecision>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredUi {
    theme: StoredTheme,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum StoredTheme {
    System,
    Light,
    Dark,
}

/// The settings fields new documents start from; absent fields take the built-in defaults.
#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredNewDocuments {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bond_length_pt: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    line_width_pt: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    label_font: Option<StoredFont>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    label_size_pt: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    carbon_labels: Option<StoredCarbonLabels>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    display_unit: Option<StoredUnit>,
}

fn encode(preferences: &Preferences) -> Vec<u8> {
    let edit = &preferences.new_documents;
    let stored = StoredPreferences {
        format: FORMAT.to_owned(),
        format_version: FORMAT_VERSION,
        revision: preferences.revision,
        ui: StoredUi {
            theme: match preferences.theme {
                Theme::System => StoredTheme::System,
                Theme::Light => StoredTheme::Light,
                Theme::Dark => StoredTheme::Dark,
            },
        },
        new_documents: StoredNewDocuments {
            bond_length_pt: edit.bond_length_pt,
            line_width_pt: edit.line_width_pt,
            label_font: edit.label_font.map(StoredFont::from),
            label_size_pt: edit.label_size_pt,
            carbon_labels: edit.carbon_labels.map(StoredCarbonLabels::from),
            display_unit: edit.display_unit.map(StoredUnit::from),
        },
        mass_precision: Some(preferences.mass_precision),
    };
    serde_json::to_vec(&stored).expect("preferences always serialize")
}

fn decode(bytes: &[u8]) -> Result<Preferences, String> {
    let stored: StoredPreferences =
        serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
    if stored.format != FORMAT {
        return Err(format!("format {:?} is not {FORMAT:?}", stored.format));
    }
    if !(1..=FORMAT_VERSION).contains(&stored.format_version) {
        return Err(format!(
            "format version {}; this build reads versions 1 and {FORMAT_VERSION}",
            stored.format_version
        ));
    }
    let mass_precision = match (stored.format_version, stored.mass_precision) {
        (1, None) => MassPrecision::default(),
        (2, Some(value)) if value.decimals <= 8 => value,
        _ => return Err("invalid mass precision for this preferences version".into()),
    };
    if stored.revision == 0 {
        return Err("a stored revision starts at 1".to_owned());
    }
    let new_documents = stored.new_documents;
    let edit = SettingsEdit {
        bond_length_pt: new_documents.bond_length_pt,
        line_width_pt: new_documents.line_width_pt,
        label_font: new_documents.label_font.map(Into::into),
        label_size_pt: new_documents.label_size_pt,
        carbon_labels: new_documents.carbon_labels.map(Into::into),
        display_unit: new_documents.display_unit.map(Into::into),
    };
    // Never hand out preferences a new document could not start from.
    Settings::default()
        .edited(&edit)
        .map_err(|error| format!("new_documents: {error}"))?;
    Ok(Preferences {
        revision: stored.revision,
        theme: match stored.ui.theme {
            StoredTheme::System => Theme::System,
            StoredTheme::Light => Theme::Light,
            StoredTheme::Dark => Theme::Dark,
        },
        new_documents: edit,
        mass_precision,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use chemifree_domain::settings::{DisplayUnit, LabelFont};

    use super::*;

    fn scratch() -> (tempfile::TempDir, FilePreferenceStore) {
        let directory = tempfile::tempdir().unwrap();
        let store = FilePreferenceStore::new(directory.path());
        (directory, store)
    }

    fn chosen(revision: u64) -> Preferences {
        Preferences {
            revision,
            theme: Theme::Dark,
            new_documents: SettingsEdit {
                bond_length_pt: Some(42.5),
                label_font: Some(LabelFont::Monospace),
                display_unit: Some(DisplayUnit::Mm),
                ..SettingsEdit::default()
            },
            mass_precision: MassPrecision::default(),
        }
    }

    #[test]
    fn nothing_is_stored_until_the_first_write_which_round_trips() {
        let (directory, store) = scratch();
        assert_eq!(store.load(), Ok(None));
        assert_eq!(
            fs::read_dir(directory.path()).unwrap().count(),
            0,
            "reading creates nothing"
        );

        store.save(&chosen(1), 0).unwrap();
        assert_eq!(store.load(), Ok(Some(chosen(1))));
        assert_eq!(
            fs::read_to_string(store.path()).unwrap(),
            r#"{"format":"chemifree.preferences","format_version":2,"revision":1,"ui":{"theme":"dark"},"new_documents":{"bond_length_pt":42.5,"label_font":"monospace","display_unit":"mm"},"mass_precision":{"decimals":2}}"#
        );
        let mut names: Vec<String> = fs::read_dir(directory.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(names, ["locks", "preferences.json"], "no temporary is left");
    }

    #[test]
    fn version_one_is_read_without_writing_then_preserved_before_upgrade() {
        let (directory, store) = scratch();
        let original = br#"{"format":"chemifree.preferences","format_version":1,"revision":1,"ui":{"theme":"dark"},"new_documents":{}}"#;
        fs::write(store.path(), original).unwrap();
        let mut preferences = store.load().unwrap().unwrap();
        assert_eq!(preferences.mass_precision.decimals, 2);
        assert_eq!(fs::read(store.path()).unwrap(), original);
        assert!(!directory.path().join("migrated").exists());
        preferences.mass_precision.decimals = 5;
        preferences.revision = 2;
        store.save(&preferences, 1).unwrap();
        assert_eq!(
            fs::read(directory.path().join("migrated/preferences.v1.json")).unwrap(),
            original
        );
        assert_eq!(store.load().unwrap().unwrap().mass_precision.decimals, 5);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&fs::read(store.path()).unwrap()).unwrap()
                ["format_version"],
            2
        );
    }

    #[test]
    fn a_write_from_a_stale_base_is_a_conflict_whichever_store_made_the_newer_one() {
        let (directory, mine) = scratch();
        // A second store over the same directory stands for another process.
        let theirs = FilePreferenceStore::new(directory.path());
        theirs.save(&chosen(1), 0).unwrap();
        let committed = fs::read(mine.path()).unwrap();

        let stale = Preferences {
            theme: Theme::Light,
            ..chosen(1)
        };
        assert_eq!(
            mine.save(&stale, 0),
            Err(StorageError::Conflict {
                stored_revision: Some(1)
            })
        );
        assert_eq!(
            fs::read(mine.path()).unwrap(),
            committed,
            "nothing is written"
        );

        mine.save(&chosen(2), 1).unwrap();
        assert_eq!(theirs.load(), Ok(Some(chosen(2))));
        assert_eq!(
            theirs.save(&chosen(2), 1),
            Err(StorageError::Conflict {
                stored_revision: Some(2)
            })
        );
    }

    #[test]
    fn a_failed_write_keeps_the_stored_preferences_and_leaves_no_temporary() {
        let (directory, store) = scratch();
        store.save(&chosen(1), 0).unwrap();
        let committed = fs::read(store.path()).unwrap();
        // A lock path that is a directory cannot be opened as a file, so the write stops
        // before anything is written.
        fs::remove_dir_all(directory.path().join("locks")).unwrap();
        fs::create_dir_all(directory.path().join(LOCK_FILE)).unwrap();
        assert!(
            matches!(store.save(&chosen(2), 1), Err(StorageError::Unavailable(_))),
            "the write fails"
        );
        assert_eq!(fs::read(store.path()).unwrap(), committed);
        let temporaries = fs::read_dir(directory.path())
            .unwrap()
            .filter(|entry| {
                entry
                    .as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".tmp")
            })
            .count();
        assert_eq!(temporaries, 0);
    }

    #[test]
    fn files_this_build_cannot_use_are_corrupt_not_defaults() {
        let valid = String::from_utf8(encode(&chosen(3))).unwrap();
        let cases = [
            ("not json".to_owned(), "expected"),
            (
                valid.replace("\"format_version\":2", "\"format_version\":3"),
                "this build reads versions 1 and 2",
            ),
            (
                valid.replace("chemifree.preferences", "chemifree.document"),
                "is not",
            ),
            (
                valid.replace("\"revision\":3", "\"revision\":0"),
                "starts at 1",
            ),
            (valid.replace("42.5", "250"), "bond length"),
            (valid.replace("\"dark\"", "\"sepia\""), "unknown variant"),
            (
                valid.replace("\"ui\":", "\"extra\":1,\"ui\":"),
                "unknown field",
            ),
        ];
        for (text, reason) in cases {
            let (_directory, store) = scratch();
            fs::write(store.path(), &text).unwrap();
            match store.load() {
                Err(StorageError::Corrupt(message)) => {
                    assert!(message.contains(reason), "{text}: {message}");
                }
                other => panic!("{text}: {other:?}"),
            }
            assert!(
                matches!(store.save(&chosen(4), 3), Err(StorageError::Corrupt(_))),
                "{text}: an unreadable file is never overwritten"
            );
            assert_eq!(fs::read_to_string(store.path()).unwrap(), text);
        }
    }
}
