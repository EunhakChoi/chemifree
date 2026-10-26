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

    /// Durably and atomically replaces the stored document; returns only after the commit.
    ///
    /// `base_revision: None` requires that nothing is stored yet. `Some(revision)` requires
    /// the stored revision to equal it, or the stored document to already equal `document`
    /// (a retried save whose acknowledgement was lost). Otherwise this is a conflict and
    /// nothing is written.
    fn save(&self, document: &Document, base_revision: Option<u64>) -> Result<(), StorageError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StorageError {
    /// The stored document is not the expected base; another writer saved first.
    Conflict { stored_revision: Option<u64> },
    /// Stored data exists but is not a valid document.
    Corrupt(String),
    /// Reading or writing failed; any previously committed document is intact.
    Unavailable(String),
}

impl fmt::Display for StorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Conflict {
                stored_revision: Some(revision),
                ..
            } => write!(formatter, "storage holds revision {revision}"),
            Self::Conflict { .. } => write!(formatter, "storage holds a different document"),
            Self::Corrupt(message) | Self::Unavailable(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for StorageError {}
