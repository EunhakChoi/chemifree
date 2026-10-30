//! Application-owned storage port. Infrastructure providers implement it.

use std::fmt;

use chemifree_domain::document::{Document, DocumentId};

pub trait DocumentRepository: Send + Sync {
    /// Returns a fresh identifier that no stored document uses.
    fn allocate_id(&self) -> Result<DocumentId, StorageError>;

    /// Identifiers of stored documents, including ones that may fail to load.
    fn list_ids(&self) -> Result<Vec<DocumentId>, StorageError>;

    /// `Ok(None)` means no document is stored under this identifier.
    fn load(&self, id: &DocumentId) -> Result<Option<Document>, StorageError>;

    /// The stored revision, used to notice another writer's save. Providers should answer
    /// without restoring the whole document.
    fn stored_revision(&self, id: &DocumentId) -> Result<Option<u64>, StorageError> {
        Ok(self.load(id)?.map(|document| document.revision()))
    }

    /// Durably and atomically replaces the stored document; returns only after the commit.
    ///
    /// `base_revision: None` requires that nothing is stored yet. `Some(revision)` requires
    /// the stored revision to equal it, or the stored document to already equal `document`
    /// (a retried save whose acknowledgement was lost). Otherwise this is a conflict and
    /// nothing is written. The check and the commit are one step for every writer of the same
    /// storage, including other processes.
    ///
    /// A recovery draft this repository owns for the document is removed with the commit: it
    /// never outlives the save, even if the process ends between the two.
    fn save(&self, document: &Document, base_revision: Option<u64>) -> Result<(), StorageError>;

    /// Who holds the document's recovery draft, if there is one.
    fn draft_status(&self, id: &DocumentId) -> Result<DraftStatus, StorageError>;

    /// Durably replaces the document's recovery draft and keeps it owned by this repository.
    ///
    /// Fails with `Locked` when another running process owns the draft, with `Conflict`
    /// when an abandoned draft is present (that one must be recovered or discarded first), and
    /// with `Missing` when the document is not stored (for example, trashed).
    fn write_draft(&self, draft: &Draft) -> Result<(), StorageError>;

    /// Takes over an abandoned draft (or one already owned) and returns it; `Ok(None)` means
    /// there is none. The inner error describes a draft that cannot be read. Fails with
    /// `Locked` when another running process owns it.
    fn claim_draft(&self, id: &DocumentId) -> Result<Option<Result<Draft, String>>, StorageError>;

    /// Prepares an owned, readable draft for recovery. A provider that migrates an older draft
    /// preserves its original bytes here before a recovery can replace or remove it. Stores
    /// without an older representation have nothing to preserve.
    fn prepare_draft_for_recovery(&self, _id: &DocumentId) -> Result<(), StorageError> {
        Ok(())
    }

    /// Gives up ownership without removing the draft, so it is abandoned again.
    fn release_draft(&self, id: &DocumentId);

    /// Removes the draft this repository owns, and gives up ownership. Succeeds when there is
    /// nothing owned to remove.
    fn remove_draft(&self, id: &DocumentId) -> Result<(), StorageError>;

    /// When the stored document was last committed, in milliseconds since the Unix epoch.
    fn saved_at_ms(&self, id: &DocumentId) -> Result<Option<u64>, StorageError>;

    /// Moves the stored document to the trash in one step, so it is either stored or trashed,
    /// never both or neither, even if the process ends meanwhile.
    ///
    /// Requires the stored revision to equal `base_revision` (else `Conflict`) and no recovery
    /// draft for the document: `DraftPending` when an abandoned one exists, `Locked` when
    /// another running process owns one. The check and the move are one step for every writer
    /// of the same storage. While the document is trashed, no draft can be written for it.
    fn trash(&self, id: &DocumentId, base_revision: u64) -> Result<(), StorageError>;

    /// Moves a trashed document back. `Missing` when it is not in the trash.
    fn restore(&self, id: &DocumentId) -> Result<(), StorageError>;

    /// Trashed documents, with when each was trashed.
    fn list_trash(&self) -> Result<Vec<TrashEntry>, StorageError>;

    /// A trashed document as it was stored. `Ok(None)` means it is not in the trash.
    fn load_trashed(&self, id: &DocumentId) -> Result<Option<Document>, StorageError>;
}

/// A trashed document's identifier and when it was trashed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrashEntry {
    pub id: DocumentId,
    /// Milliseconds since the Unix epoch, when known.
    pub deleted_at_ms: Option<u64>,
}

/// An autosaved working copy, stored apart from the committed document so unsaved work
/// survives the process that held it.
#[derive(Clone, Debug, PartialEq)]
pub struct Draft {
    /// The working copy as it was, with history and identifier counters.
    pub document: Document,
    /// The stored revision the working copy started from (its last open or save).
    pub base_revision: u64,
    /// When the draft was written, in milliseconds since the Unix epoch.
    pub saved_at_ms: u64,
}

/// What a draft says about itself without restoring its document.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DraftInfo {
    pub revision: u64,
    pub base_revision: u64,
    pub saved_at_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DraftStatus {
    None,
    /// This repository owns it: the draft of a working copy open in this process.
    Owned,
    /// Another running process owns it.
    Locked,
    /// Its owner ended without saving or discarding. The error describes an unreadable draft.
    Abandoned(Result<DraftInfo, String>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StorageError {
    /// The stored document is not the expected base; another writer saved first.
    Conflict { stored_revision: Option<u64> },
    /// Stored data exists but is not a valid document.
    Corrupt(String),
    /// Another running process holds what this operation needs.
    Locked(String),
    /// Storage refused the write: missing permission or a read-only file system. Retrying
    /// helps only after that is fixed; any previously committed document is intact.
    Denied(String),
    /// Reading or writing failed; any previously committed document is intact.
    Unavailable(String),
    /// No document is stored (or trashed, for a restore) under this identifier.
    Missing,
    /// An abandoned recovery draft holds unsaved work for the document; it must be recovered
    /// or discarded first.
    DraftPending,
}

impl fmt::Display for StorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Conflict {
                stored_revision: Some(revision),
                ..
            } => write!(formatter, "storage holds revision {revision}"),
            Self::Conflict { .. } => write!(formatter, "storage holds a different document"),
            Self::Corrupt(message)
            | Self::Locked(message)
            | Self::Denied(message)
            | Self::Unavailable(message) => formatter.write_str(message),
            Self::Missing => formatter.write_str("no such document is stored"),
            Self::DraftPending => {
                formatter.write_str("an abandoned recovery draft holds unsaved work")
            }
        }
    }
}

impl std::error::Error for StorageError {}
