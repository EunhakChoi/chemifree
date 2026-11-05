//! Application errors shared by every entry adapter.

use std::fmt;

use chemifree_domain::scene::SceneError;
use chemifree_domain::settings::SettingsError;
use chemifree_domain::states::StateError;
use chemifree_domain::stereo::StereoError;
use chemifree_domain::stereo_drawing::NotDerivable;
use serde::Serialize;

use crate::API_VERSION;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorCode {
    /// A request field is malformed (identifier, title, request ID).
    InvalidRequest,
    /// A command is malformed or violates a document rule; see `command_index`, and `reason`
    /// for a refused combination of recorded states.
    InvalidCommand,
    DocumentNotFound,
    /// `expected_revision` differs from the session's current revision.
    RevisionConflict,
    /// A `request_id` was reused for a different request.
    RequestIdReused,
    NothingToUndo,
    NothingToRedo,
    /// Closing would discard unsaved changes without explicit consent.
    UnsavedChanges,
    /// Stored data changed since this session last loaded or saved it.
    SaveConflict,
    /// The storage provider failed, or the document stayed busy for longer than a request
    /// waits; `reason` says which. The session keeps its unsaved changes.
    StorageFailed,
    DocumentUnreadable,
    ResourceLimit,
    /// An abandoned recovery draft must be recovered or discarded before new work is
    /// autosaved over it.
    RecoveryPending,
    /// Another running process owns the recovery draft.
    DraftInUse,
    /// Storage changed since the draft's base; it can be recovered only as a copy.
    DraftOutdated,
    DraftNotFound,
    /// A request with this `request_id` is still running; ask again for its outcome.
    RequestInProgress,
    /// The request sets a preference whose feature is not implemented, so it is not stored.
    SettingNotImplemented,
    /// The stored preferences cannot be read; nothing that needs them is done.
    PreferencesUnreadable,
    /// A computation was stopped by its time limit; nothing was changed. Whether an input
    /// finishes in time depends on the machine, unlike a work-limit refusal.
    TimeLimit,
    /// A computation was stopped because the caller went away; nothing was changed.
    Cancelled,
}

impl ErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidRequest => "invalid_request",
            Self::InvalidCommand => "invalid_command",
            Self::DocumentNotFound => "document_not_found",
            Self::RevisionConflict => "revision_conflict",
            Self::RequestIdReused => "request_id_reused",
            Self::NothingToUndo => "nothing_to_undo",
            Self::NothingToRedo => "nothing_to_redo",
            Self::UnsavedChanges => "unsaved_changes",
            Self::SaveConflict => "save_conflict",
            Self::StorageFailed => "storage_failed",
            Self::DocumentUnreadable => "document_unreadable",
            Self::ResourceLimit => "resource_limit",
            Self::RecoveryPending => "recovery_pending",
            Self::DraftInUse => "draft_in_use",
            Self::DraftOutdated => "draft_outdated",
            Self::DraftNotFound => "draft_not_found",
            Self::RequestInProgress => "request_in_progress",
            Self::SettingNotImplemented => "setting_not_implemented",
            Self::PreferencesUnreadable => "preferences_unreadable",
            Self::TimeLimit => "time_limit",
            Self::Cancelled => "cancelled",
        }
    }
}

/// Why storage failed, so a client can tell whether retrying can help.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageReason {
    /// Storage refused the write (permissions or a read-only file system). Retrying helps
    /// only after that is fixed.
    ReadOnly,
    /// The document stayed busy (another request's turn, or another process's save lock) for
    /// longer than a request waits. Nothing was changed; retrying later can succeed.
    Busy,
    /// Any other read or write failure.
    Io,
}

impl StorageReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read_only",
            Self::Busy => "busy",
            Self::Io => "io",
        }
    }
}

/// Native clipboard refusal details, boxed in the shared error to keep ordinary results compact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClipboardRefusal {
    pub reason: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationError {
    pub code: ErrorCode,
    pub message: String,
    pub current_revision: Option<u64>,
    pub command_index: Option<usize>,
    /// Set with `StorageFailed`.
    pub reason: Option<StorageReason>,
    /// Set with `InvalidCommand` when a combination of recorded states was refused; sent as
    /// `reason`, like a storage reason.
    pub invalid_state: Option<StateError>,
    /// Set with `InvalidCommand` when a scene rule was refused.
    pub invalid_scene: Option<SceneError>,
    /// The invalid document or new-document style value, when one was refused.
    pub invalid_settings: Option<SettingsError>,
    /// Set with `InvalidCommand` when a stereo mark or configuration was refused.
    pub invalid_stereo: Option<StereoError>,
    /// Set with `InvalidCommand` when `set_configuration_from_drawing` found that the drawing
    /// does not determine a configuration.
    pub invalid_derivation: Option<NotDerivable>,
    /// Set with `InvalidCommand` when the wire refused a stereo group value (the code of a
    /// number outside the 32-bit signed range); sent as `reason`.
    pub invalid_stereo_group: Option<&'static str>,
    /// Set when the chosen aromaticity model cannot safely convert the recorded graph.
    pub invalid_aromaticity: Option<&'static str>,
    /// A bounded native clipboard refusal.
    pub invalid_clipboard: Option<Box<ClipboardRefusal>>,
    /// Set with `TimeLimit`: the time limit, in milliseconds, that stopped the computation.
    pub limit_ms: Option<u64>,
}

impl ApplicationError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            current_revision: None,
            command_index: None,
            reason: None,
            invalid_state: None,
            invalid_scene: None,
            invalid_settings: None,
            invalid_stereo: None,
            invalid_derivation: None,
            invalid_stereo_group: None,
            invalid_aromaticity: None,
            invalid_clipboard: None,
            limit_ms: None,
        }
    }

    /// `StorageFailed` with its reason.
    pub fn storage_failed(reason: StorageReason, message: impl Into<String>) -> Self {
        Self {
            reason: Some(reason),
            ..Self::new(ErrorCode::StorageFailed, message)
        }
    }

    pub fn to_response(&self) -> ErrorResponse {
        ErrorResponse {
            api_version: API_VERSION,
            code: self.code.as_str(),
            message: self.message.clone(),
            current_revision: self.current_revision,
            command_index: self.command_index,
            reason: self.reason_code(),
        }
    }

    /// The envelope's `reason`: why storage failed, or which combination of states was refused.
    pub fn reason_code(&self) -> Option<&'static str> {
        self.reason
            .map(StorageReason::as_str)
            .or(self.invalid_state.map(StateError::code))
            .or(self.invalid_scene.map(SceneError::code))
            .or(self.invalid_settings.map(SettingsError::code))
            .or(self.invalid_stereo.map(StereoError::code))
            .or(self.invalid_derivation.map(NotDerivable::code))
            .or(self.invalid_stereo_group)
            .or(self.invalid_aromaticity)
            .or(self.invalid_clipboard.as_deref().map(|error| error.reason))
    }
}

impl fmt::Display for ApplicationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for ApplicationError {}

/// v1 error envelope. Optional fields are omitted when they do not apply.
#[derive(Clone, Debug, Serialize)]
pub struct ErrorResponse {
    pub api_version: &'static str,
    pub code: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_revision: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command_index: Option<usize>,
    /// With `storage_failed`, a [`StorageReason`]; with `invalid_command`, a [`StateError`] code
    /// when a combination of recorded states was refused, or a [`StereoError`] code when a
    /// stereo mark or configuration was.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boxed_clipboard_refusal_preserves_the_public_error_envelope() {
        let error = ApplicationError {
            invalid_clipboard: Some(Box::new(ClipboardRefusal {
                reason: "clipboard_fragment_invalid",
            })),
            ..ApplicationError::new(ErrorCode::InvalidCommand, "Malformed native clipboard data")
        };
        assert_eq!(error.clone(), error);
        assert_eq!(
            serde_json::to_value(error.to_response()).unwrap(),
            serde_json::json!({
                "api_version":"v1", "code":"invalid_command", "message":"Malformed native clipboard data",
                "reason":"clipboard_fragment_invalid"
            })
        );
    }
}
