//! Application errors shared by every entry adapter.

use std::fmt;

use serde::Serialize;

use crate::API_VERSION;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorCode {
    /// A request field is malformed (identifier, title, request ID).
    InvalidRequest,
    /// A command is malformed or violates a document rule; see `command_index`, and `reason`
    /// for a refusal with a stable code (a combination of recorded states, a scene command, the
    /// last instance, or the selection).
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationError {
    pub code: ErrorCode,
    pub message: String,
    pub current_revision: Option<u64>,
    pub command_index: Option<usize>,
    /// Set with `StorageFailed`.
    pub reason: Option<StorageReason>,
    /// Set with `InvalidCommand` when the refusal has a stable code (see
    /// `chemifree_domain::document::EditError::reason`); sent as `reason`, like a storage
    /// reason.
    pub command_reason: Option<&'static str>,
}

impl ApplicationError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            current_revision: None,
            command_index: None,
            reason: None,
            command_reason: None,
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

    /// The envelope's `reason`: why storage failed, or why a command was refused.
    pub fn reason_code(&self) -> Option<&'static str> {
        self.reason
            .map(StorageReason::as_str)
            .or(self.command_reason)
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
    /// With `storage_failed`, a [`StorageReason`]; with `invalid_command`, the refusal's stable
    /// code when it has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
}
