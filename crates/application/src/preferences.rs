//! Application preferences: one set per data directory, kept apart from every document.
//!
//! Preferences hold what the user interface uses (its theme), the settings new documents start
//! from, and the place reserved for computation preferences. A new document copies the built-in
//! defaults overlaid with `new_documents` once, when it is created; from then on its settings
//! are its own, so changing preferences never alters an existing document, its file, or its
//! history. Mass-result display decimals are stored here and copied into new results; broader
//! computation precision and NMR settings remain unavailable and cannot be stored.
//!
//! Every write names the revision it is based on; a different stored revision is a conflict,
//! whichever process wrote it.

use std::collections::VecDeque;
use std::sync::{Mutex, MutexGuard, PoisonError};

use chemifree_domain::settings::{Settings, SettingsEdit};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::API_VERSION;
use crate::contract::{
    SettingsDto, SettingsEditDto, settings_edit_view, settings_view, to_settings_edit,
};
use crate::documents::{REPLAY_LIMIT, validate_request_id};
use crate::error::{ApplicationError, ErrorCode, StorageReason};
use crate::repository::StorageError;
use crate::turns::Turns;

/// The colour theme of the user interface.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    /// Follow the system's light or dark setting.
    #[default]
    System,
    Light,
    Dark,
}

/// The stored preferences of one data directory.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Preferences {
    /// Increases by one with every change; 0 while nothing is stored.
    pub revision: u64,
    pub theme: Theme,
    /// The settings fields new documents start from; the rest take the built-in defaults.
    pub new_documents: SettingsEdit,
    /// Decimal places for displaying new mass results, independent of document style.
    pub mass_precision: MassPrecision,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MassPrecision {
    pub decimals: u8,
}

impl Default for MassPrecision {
    fn default() -> Self {
        Self { decimals: 2 }
    }
}

impl Preferences {
    /// The settings a document created now starts from.
    ///
    /// # Errors
    ///
    /// A `new_documents` value out of range (which a stored file may hold if it was edited by
    /// hand) is refused.
    pub fn new_document_settings(&self) -> Result<Settings, ApplicationError> {
        Settings::default()
            .edited(&self.new_documents)
            .map_err(|error| ApplicationError {
                invalid_settings: Some(error),
                ..ApplicationError::new(
                    ErrorCode::InvalidRequest,
                    format!("new_documents: {error}"),
                )
            })
    }

    /// Whether `other` holds the same preferences, whatever their revisions.
    fn same_as(&self, other: &Self) -> bool {
        self.theme == other.theme
            && self.new_documents == other.new_documents
            && self.mass_precision == other.mass_precision
    }
}

/// Application-owned storage port for preferences.
pub trait PreferenceRepository: Send + Sync {
    /// The stored preferences; `Ok(None)` means none are stored (revision 0, defaults).
    fn load(&self) -> Result<Option<Preferences>, StorageError>;

    /// Durably and atomically replaces the stored preferences with `preferences`, if storage
    /// holds `base_revision` (0: nothing stored); otherwise this is a `Conflict` naming the
    /// stored revision and nothing is written. The check and the write are one step for every
    /// writer of the same storage, including other processes.
    fn save(&self, preferences: &Preferences, base_revision: u64) -> Result<(), StorageError>;
}

/// Preferences kept in memory for the life of the application: for in-process callers, such
/// as tests, that give no store. Entry adapters give the data directory's store.
#[derive(Debug, Default)]
pub struct MemoryPreferences(Mutex<Option<Preferences>>);

impl PreferenceRepository for MemoryPreferences {
    fn load(&self) -> Result<Option<Preferences>, StorageError> {
        Ok(self
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone())
    }

    fn save(&self, preferences: &Preferences, base_revision: u64) -> Result<(), StorageError> {
        let mut stored = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let revision = stored.as_ref().map_or(0, |stored| stored.revision);
        if revision != base_revision {
            return Err(StorageError::Conflict {
                stored_revision: Some(revision),
            });
        }
        *stored = Some(preferences.clone());
        Ok(())
    }
}

/// The user interface's preferences.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UiPreferencesDto {
    pub theme: Theme,
}

/// Preferences as `GET /api/v1/preferences` reports them. Added within v1.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PreferencesResponse {
    pub api_version: &'static str,
    pub revision: u64,
    pub ui: UiPreferencesDto,
    /// The fields new documents start from, as stored; the rest are the built-in defaults.
    pub new_documents: SettingsEditDto,
    /// The settings a document created now starts from: the defaults overlaid with
    /// `new_documents`.
    pub new_document_settings: SettingsDto,
    pub computation: ComputationView,
}

/// Computation preferences, including unavailable general settings and mass display decimals.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ComputationView {
    pub nmr: ComputationSetting,
    pub numeric_precision: ComputationSetting,
    /// Added within v1; older clients ignore this scoped setting.
    pub mass_precision: MassPrecisionView,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MassPrecisionView {
    pub status: &'static str,
    pub owner: &'static str,
    pub decimals: u8,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ComputationSetting {
    /// Always `not_implemented` so far: nothing is stored or applied.
    pub status: &'static str,
    /// Who owns the setting once it exists: `preferences` (this data directory), never a
    /// document.
    pub owner: &'static str,
    /// The plan entry expected to implement it (docs/roadmap/).
    pub followup: &'static str,
    pub note: &'static str,
}

const NMR: ComputationSetting = ComputationSetting {
    status: "not_implemented",
    owner: "preferences",
    followup: "P66",
    note: "NMR solvent and correction settings will be stored here, apart from documents; \
           results will record the settings they used. No NMR prediction runs yet.",
};

const NUMERIC_PRECISION: ComputationSetting = ComputationSetting {
    status: "not_implemented",
    owner: "preferences",
    followup: "P60",
    note: "General computation precision remains unimplemented. Mass display decimals are available separately as mass_precision.",
};

/// The preferences a write sets: all of them, replacing what is stored.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PreferencesChange {
    pub ui: UiPreferencesDto,
    pub new_documents: SettingsEditDto,
    /// Unavailable settings are refused; an omitted mass setting preserves its current value.
    #[serde(default)]
    pub computation: ComputationChange,
}

/// Computation preferences a client asked to set. A field is `Some` whenever the body names it,
/// including `null`, so invalid values cannot be mistaken for an omitted setting.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ComputationChange {
    #[serde(default, deserialize_with = "named")]
    pub nmr: Option<Value>,
    #[serde(default, deserialize_with = "named")]
    pub numeric_precision: Option<Value>,
    #[serde(default, deserialize_with = "named")]
    pub mass_precision: Option<Value>,
}

/// A field that is present, whatever its value: unlike `Option`'s own rule, `null` is `Some`.
fn named<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(deserializer).map(Some)
}

/// Body of `PUT /api/v1/preferences`. Added within v1.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreferencesRequest {
    pub request_id: String,
    pub expected_revision: u64,
    pub ui: UiPreferencesDto,
    pub new_documents: SettingsEditDto,
    #[serde(default)]
    pub computation: ComputationChange,
}

impl PreferencesRequest {
    pub fn change(&self) -> PreferencesChange {
        PreferencesChange {
            ui: self.ui.clone(),
            new_documents: self.new_documents.clone(),
            computation: self.computation.clone(),
        }
    }
}

/// Body of `POST /api/v1/preferences/import`: new documents start from `document`'s settings.
/// Added within v1.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportPreferencesRequest {
    pub request_id: String,
    pub expected_revision: u64,
    pub document: String,
}

/// What a preferences write asked for, compared when a `request_id` is seen again.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum PreferenceMutation {
    Set {
        expected_revision: u64,
        change: PreferencesChange,
    },
    Reset {
        expected_revision: u64,
    },
    FromDocument {
        expected_revision: u64,
        document: String,
    },
}

/// The preferences use cases over a store, with the replies of recent writes by `request_id`.
pub(crate) struct PreferenceService {
    store: Box<dyn PreferenceRepository>,
    /// Writes in this process take turns; writes of other processes are serialized by the store.
    writes: Mutex<()>,
    /// The replay log is never held during I/O, so a completed request can be answered while
    /// another write waits for storage.
    replays: Mutex<VecDeque<(String, PreferenceMutation, PreferencesResponse)>>,
    turns: Turns,
}

impl PreferenceService {
    pub(crate) fn new(store: Box<dyn PreferenceRepository>) -> Self {
        Self {
            store,
            writes: Mutex::new(()),
            replays: Mutex::new(VecDeque::new()),
            turns: Turns::default(),
        }
    }

    /// The stored preferences, or the defaults at revision 0 when none are stored.
    pub(crate) fn current(&self) -> Result<Preferences, ApplicationError> {
        Ok(self
            .store
            .load()
            .map_err(storage_error)?
            .unwrap_or_default())
    }

    pub(crate) fn get(&self) -> Result<PreferencesResponse, ApplicationError> {
        respond(&self.current()?)
    }

    pub(crate) fn set(
        &self,
        request_id: Option<&str>,
        expected_revision: u64,
        change: PreferencesChange,
    ) -> Result<PreferencesResponse, ApplicationError> {
        let unsupported = [
            ("nmr", change.computation.nmr.is_some(), &NMR),
            (
                "numeric_precision",
                change.computation.numeric_precision.is_some(),
                &NUMERIC_PRECISION,
            ),
        ];
        if let Some((name, _, setting)) = unsupported.into_iter().find(|(_, given, _)| *given) {
            return Err(ApplicationError::new(
                ErrorCode::SettingNotImplemented,
                format!(
                    "computation.{name} is not implemented, so it is not stored; it will belong \
                     to preferences ({})",
                    setting.followup
                ),
            ));
        }
        let mass_precision = change
            .computation
            .mass_precision
            .as_ref()
            .map(|value| {
                let precision: MassPrecision =
                    serde_json::from_value(value.clone()).map_err(|_| {
                        ApplicationError::new(
                            ErrorCode::InvalidRequest,
                            "computation.mass_precision must contain a decimal count from 0 to 8",
                        )
                    })?;
                if precision.decimals > 8 {
                    return Err(ApplicationError::new(
                        ErrorCode::InvalidRequest,
                        "computation.mass_precision.decimals must be from 0 to 8",
                    ));
                }
                Ok(precision)
            })
            .transpose()?;
        let mutation = PreferenceMutation::Set {
            expected_revision,
            change: change.clone(),
        };
        self.write(request_id, expected_revision, mutation, |current| {
            let next = Preferences {
                revision: current.revision,
                theme: change.ui.theme,
                new_documents: to_settings_edit(&change.new_documents),
                // A request from an older v1 client omits the new field and preserves it.
                mass_precision: mass_precision.unwrap_or(current.mass_precision),
            };
            next.new_document_settings()?;
            Ok(next)
        })
    }

    pub(crate) fn reset(
        &self,
        request_id: Option<&str>,
        expected_revision: u64,
    ) -> Result<PreferencesResponse, ApplicationError> {
        let mutation = PreferenceMutation::Reset { expected_revision };
        self.write(request_id, expected_revision, mutation, |current| {
            Ok(Preferences {
                revision: current.revision,
                ..Preferences::default()
            })
        })
    }

    /// Sets `new_documents` to every field of the settings `settings` reads, keeping the rest.
    pub(crate) fn adopt_document(
        &self,
        request_id: Option<&str>,
        expected_revision: u64,
        document: &str,
        settings: impl FnOnce() -> Result<Settings, ApplicationError>,
    ) -> Result<PreferencesResponse, ApplicationError> {
        let mutation = PreferenceMutation::FromDocument {
            expected_revision,
            document: document.to_owned(),
        };
        self.write(request_id, expected_revision, mutation, |current| {
            Ok(Preferences {
                new_documents: settings()?.to_edit(),
                ..current.clone()
            })
        })
    }

    /// Replays a completed write, or checks the revision, makes the next preferences from the
    /// current ones, and stores them as the next revision unless they are the same.
    fn write(
        &self,
        request_id: Option<&str>,
        expected_revision: u64,
        mutation: PreferenceMutation,
        make: impl FnOnce(&Preferences) -> Result<Preferences, ApplicationError>,
    ) -> Result<PreferencesResponse, ApplicationError> {
        validate_request_id(request_id)?;
        let _running = self.turns.start(None, request_id).map_err(|_| {
            ApplicationError::new(
                ErrorCode::RequestInProgress,
                "a request with this request_id is still running; ask again for its outcome",
            )
        })?;
        let replays = self.lock();
        if let Some(request_id) = request_id
            && let Some((_, recorded, response)) =
                replays.iter().find(|(id, _, _)| id == request_id)
        {
            return if *recorded == mutation {
                Ok(response.clone())
            } else {
                Err(ApplicationError::new(
                    ErrorCode::RequestIdReused,
                    format!("request_id {request_id:?} was already used for a different request"),
                ))
            };
        }
        drop(replays);
        let _write = self.writes.lock().unwrap_or_else(PoisonError::into_inner);
        let current = self.current()?;
        if current.revision != expected_revision {
            return Err(conflict(current.revision, expected_revision));
        }
        let next = make(&current)?;
        let response = if next.same_as(&current) {
            respond(&current)?
        } else {
            let revision = current.revision.checked_add(1).ok_or_else(|| {
                ApplicationError::new(
                    ErrorCode::ResourceLimit,
                    "the preferences have reached their last revision",
                )
            })?;
            let next = Preferences { revision, ..next };
            self.store
                .save(&next, current.revision)
                .map_err(|error| match error {
                    StorageError::Conflict {
                        stored_revision: Some(stored),
                    } => conflict(stored, expected_revision),
                    error => storage_error(error),
                })?;
            respond(&next)?
        };
        if let Some(request_id) = request_id {
            let mut replays = self.lock();
            replays.push_back((request_id.to_owned(), mutation, response.clone()));
            if replays.len() > REPLAY_LIMIT {
                replays.pop_front();
            }
        }
        Ok(response)
    }

    /// A write that panicked recorded nothing, so the log is whole; recovering it is safe.
    fn lock(&self) -> MutexGuard<'_, VecDeque<(String, PreferenceMutation, PreferencesResponse)>> {
        self.replays.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn respond(preferences: &Preferences) -> Result<PreferencesResponse, ApplicationError> {
    Ok(PreferencesResponse {
        api_version: API_VERSION,
        revision: preferences.revision,
        ui: UiPreferencesDto {
            theme: preferences.theme,
        },
        new_documents: settings_edit_view(&preferences.new_documents),
        new_document_settings: settings_view(&preferences.new_document_settings()?),
        computation: ComputationView {
            nmr: NMR,
            numeric_precision: NUMERIC_PRECISION,
            mass_precision: MassPrecisionView {
                status: "available",
                owner: "preferences",
                decimals: preferences.mass_precision.decimals,
            },
        },
    })
}

fn conflict(stored: u64, expected: u64) -> ApplicationError {
    ApplicationError {
        current_revision: Some(stored),
        ..ApplicationError::new(
            ErrorCode::RevisionConflict,
            format!("the preferences are at revision {stored}, not {expected}"),
        )
    }
}

fn storage_error(error: StorageError) -> ApplicationError {
    let (reason, message) = match error {
        StorageError::Corrupt(message) => {
            return ApplicationError::new(
                ErrorCode::PreferencesUnreadable,
                format!("the stored preferences are unreadable: {message}"),
            );
        }
        StorageError::Conflict { .. } => {
            return ApplicationError::new(
                ErrorCode::RevisionConflict,
                "the stored preferences changed elsewhere; read them again",
            );
        }
        StorageError::Locked(message) => (StorageReason::Busy, message),
        StorageError::Denied(message) => (StorageReason::ReadOnly, message),
        StorageError::Unavailable(message) => (StorageReason::Io, message),
        error @ (StorageError::Missing | StorageError::DraftPending) => {
            (StorageReason::Io, error.to_string())
        }
    };
    ApplicationError::storage_failed(reason, format!("preferences: {message}"))
}
