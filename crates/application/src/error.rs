//! Application errors shared by every entry adapter.

use std::fmt;

use serde::Serialize;

use crate::API_VERSION;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorCode {
    /// A request field is malformed (identifier, title, request ID).
    InvalidRequest,
    /// A command is malformed or violates a document rule; see `command_index`.
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
    /// The storage provider failed; the session keeps its unsaved changes.
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
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationError {
    pub code: ErrorCode,
    pub message: String,
    pub current_revision: Option<u64>,
    pub command_index: Option<usize>,
}

impl ApplicationError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            current_revision: None,
            command_index: None,
        }
    }

    pub fn to_response(&self) -> ErrorResponse {
        ErrorResponse {
            api_version: API_VERSION,
            code: self.code.as_str(),
            message: self.message.clone(),
            current_revision: self.current_revision,
            command_index: self.command_index,
        }
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
}
