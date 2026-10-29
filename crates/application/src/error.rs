//! Application errors shared by every entry adapter.

use std::fmt;

use chemifree_domain::document::EditError;
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

/// Why a command's atom or bond state was refused (`invalid_command`): a stable code that
/// says which values do not hold together. Only these refusals carry one; other invalid
/// commands are explained by their message alone.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandReason {
    /// A stated hydrogen count above 16.
    HydrogenCountOutOfRange,
    /// `hydrogens.mode` is `specified` without a `count`.
    HydrogenCountMissing,
    /// `hydrogens.mode` is `inferred` or `unknown` with a `count`.
    HydrogenCountUnexpected,
    /// A query atom with a nonzero charge.
    QueryAtomCharge,
    /// A query atom with an isotope.
    QueryAtomIsotope,
    /// A query list (elements of a query atom, orders of a query bond) with no entry.
    QueryListEmpty,
    /// A query list naming an entry twice.
    QueryListRepeated,
    /// A query atom's element list longer than 10.
    QueryListTooLong,
    /// `elements`, `negated`, or `orders` given with a query kind other than `list`.
    QueryListUnexpected,
    /// Both `element` and `query` were given for one atom.
    ElementAndQuery,
    /// `add_atom` gave neither `element` nor `query`.
    AtomKindMissing,
    /// `order` is `query` without a `query` expression.
    BondQueryMissing,
    /// A `query` expression with an `order` other than `query`.
    BondQueryUnexpected,
    /// An attachment point outside 1–4.
    AttachmentPointOutOfRange,
    /// An attachment point given twice.
    AttachmentPointRepeated,
}

impl CommandReason {
    pub const ALL: [Self; 15] = [
        Self::HydrogenCountOutOfRange,
        Self::HydrogenCountMissing,
        Self::HydrogenCountUnexpected,
        Self::QueryAtomCharge,
        Self::QueryAtomIsotope,
        Self::QueryListEmpty,
        Self::QueryListRepeated,
        Self::QueryListTooLong,
        Self::QueryListUnexpected,
        Self::ElementAndQuery,
        Self::AtomKindMissing,
        Self::BondQueryMissing,
        Self::BondQueryUnexpected,
        Self::AttachmentPointOutOfRange,
        Self::AttachmentPointRepeated,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::HydrogenCountOutOfRange => "hydrogen_count_out_of_range",
            Self::HydrogenCountMissing => "hydrogen_count_missing",
            Self::HydrogenCountUnexpected => "hydrogen_count_unexpected",
            Self::QueryAtomCharge => "query_atom_charge",
            Self::QueryAtomIsotope => "query_atom_isotope",
            Self::QueryListEmpty => "query_list_empty",
            Self::QueryListRepeated => "query_list_repeated",
            Self::QueryListTooLong => "query_list_too_long",
            Self::QueryListUnexpected => "query_list_unexpected",
            Self::ElementAndQuery => "element_and_query",
            Self::AtomKindMissing => "atom_kind_missing",
            Self::BondQueryMissing => "bond_query_missing",
            Self::BondQueryUnexpected => "bond_query_unexpected",
            Self::AttachmentPointOutOfRange => "attachment_point_out_of_range",
            Self::AttachmentPointRepeated => "attachment_point_repeated",
        }
    }

    /// The reason a domain refusal carries, if it is one of these.
    pub fn of(error: &EditError) -> Option<Self> {
        Some(match error {
            EditError::HydrogenCountOutOfRange(_) => Self::HydrogenCountOutOfRange,
            EditError::QueryAtomCharge => Self::QueryAtomCharge,
            EditError::QueryAtomIsotope => Self::QueryAtomIsotope,
            EditError::EmptyQueryList => Self::QueryListEmpty,
            EditError::RepeatedQueryEntry => Self::QueryListRepeated,
            EditError::QueryListTooLong => Self::QueryListTooLong,
            EditError::AttachmentPointOutOfRange(_) => Self::AttachmentPointOutOfRange,
            EditError::RepeatedAttachmentPoint(_) => Self::AttachmentPointRepeated,
            _ => return None,
        })
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
    /// Set with `InvalidCommand` when an atom or bond state was refused. The envelope reports
    /// it as `reason`, which `StorageFailed` uses for its own reasons.
    pub command_reason: Option<CommandReason>,
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

    /// The `reason` the error envelope and the operation diagnostic report.
    pub fn reason_code(&self) -> Option<&'static str> {
        self.reason
            .map(StorageReason::as_str)
            .or(self.command_reason.map(CommandReason::as_str))
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
}
