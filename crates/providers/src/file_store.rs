//! Local JSON document storage with atomic replace-on-save, cross-process save locks, and
//! recovery drafts.
//!
//! Layout under the data directory:
//!
//! - `documents/<id>.json`: the committed document. A save writes a uniquely named temporary
//!   file in the same directory, flushes it to disk, renames it over the target (the commit
//!   point), and then syncs the directory where supported. A failure before the rename leaves
//!   the previously committed file intact; leftover temporary files are pruned later.
//! - `locks/<id>.lock`: an advisory OS lock held from the base-revision check through the
//!   commit, so saves from every process are serialized per document. The operating system
//!   releases it when its holder exits, however it exits. Lock files are never deleted:
//!   deleting a lock file while another process waits on it would let two holders in.
//! - `drafts/<id>.json`: the recovery draft of a working copy with unsaved changes, and
//!   `drafts/<id>.owner`, whose lock its owning process holds while it runs. A draft whose
//!   owner lock is free was abandoned by a process that ended.
//! - `drafts/<id>.committing`: an owned draft set aside during a save. If the saving process
//!   ends before removing it, the next lock holder settles it: the commit happened exactly
//!   when storage no longer holds the draft's base revision.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use chemifree_application::{DocumentRepository, Draft, DraftInfo, DraftStatus, StorageError};
use chemifree_domain::document::{Document, DocumentId};

use crate::file_format;

const DOCUMENTS_DIR: &str = "documents";
const DRAFTS_DIR: &str = "drafts";
const LOCKS_DIR: &str = "locks";
const EXTENSION: &str = "json";
const TEMPORARY_SUFFIX: &str = ".tmp";
/// Larger files are refused before parsing.
pub const MAX_DOCUMENT_BYTES: u64 = 64 * 1024 * 1024;
const STALE_TEMPORARY_AGE: Duration = Duration::from_secs(60 * 60);
/// How long a save waits for another process's save of the same document. It stays well
/// below the web client's 10 s request deadline, so a timeout reaches it as `storage_failed`.
pub const SAVE_LOCK_WAIT: Duration = Duration::from_secs(5);
const LOCK_POLL: Duration = Duration::from_millis(10);
/// Debug builds pause this many milliseconds inside the save lock, between the base check and
/// the commit, so tests can race saves and kill a process that holds the lock.
pub const COMMIT_DELAY_VARIABLE: &str = "CHEMIFREE_TEST_COMMIT_DELAY_MS";

static TEMPORARY_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug)]
pub struct FileDocumentRepository {
    directory: PathBuf,
    drafts: PathBuf,
    locks: PathBuf,
    /// Revisions read by `stored_revision`, keyed by a stamp of the file, so polling an
    /// unchanged file does not re-read it.
    probes: Arc<Mutex<HashMap<DocumentId, (FileStamp, u64)>>>,
    /// Locked owner files of the drafts this repository owns. Dropping one releases it.
    owned: Arc<Mutex<HashMap<DocumentId, File>>>,
    /// Held from checking whether this store owns a draft until it has acted on taking the
    /// owner lock. An OS file lock refuses every other handle, including this process's own,
    /// so without it a concurrent request here would read another request's brief probe as
    /// a different running process. Order: this, then an owner lock, then a save lock
    /// (`save` takes only the save lock).
    taking_ownership: Arc<Mutex<()>>,
}

/// Length, modification time, and on Unix the inode. Saves replace the file by rename, so
/// every commit by this store (in any process) gets a new inode even within one clock tick.
#[derive(Clone, Copy, Debug, PartialEq)]
struct FileStamp(u64, SystemTime, u64);

impl FileStamp {
    fn of(metadata: &fs::Metadata) -> Option<Self> {
        #[cfg(unix)]
        let inode = std::os::unix::fs::MetadataExt::ino(metadata);
        #[cfg(not(unix))]
        let inode = 0;
        Some(Self(metadata.len(), metadata.modified().ok()?, inode))
    }
}

impl FileDocumentRepository {
    /// Uses `<root>/documents`, `<root>/drafts`, and `<root>/locks`. Nothing is touched until
    /// the first write, which creates the directories (owner-only on Unix).
    pub fn new(root: &Path) -> Self {
        Self {
            directory: root.join(DOCUMENTS_DIR),
            drafts: root.join(DRAFTS_DIR),
            locks: root.join(LOCKS_DIR),
            probes: Arc::default(),
            owned: Arc::default(),
            taking_ownership: Arc::default(),
        }
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    fn path(&self, id: &DocumentId) -> PathBuf {
        self.directory.join(format!("{id}.{EXTENSION}"))
    }

    fn draft_path(&self, id: &DocumentId) -> PathBuf {
        self.drafts.join(format!("{id}.{EXTENSION}"))
    }

    fn set_aside_path(&self, id: &DocumentId) -> PathBuf {
        self.drafts.join(format!("{id}.committing"))
    }

    /// Removes temporary files that interrupted writes abandoned more than an hour ago.
    /// Younger ones may belong to a write in progress in another process.
    pub fn prune_stale_temporaries(&self) {
        let now = SystemTime::now();
        for directory in [&self.directory, &self.drafts] {
            let Ok(entries) = fs::read_dir(directory) else {
                continue;
            };
            for entry in entries.flatten() {
                let name = entry.file_name();
                let is_temporary = name
                    .to_str()
                    .is_some_and(|name| name.starts_with('.') && name.ends_with(TEMPORARY_SUFFIX));
                let stale = entry
                    .metadata()
                    .and_then(|metadata| metadata.modified())
                    .ok()
                    .and_then(|modified| now.duration_since(modified).ok())
                    .is_some_and(|age| age > STALE_TEMPORARY_AGE);
                if is_temporary && stale {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
    }

    fn read(&self, id: &DocumentId) -> Result<Option<Document>, StorageError> {
        let path = self.path(id);
        let Some(bytes) = self.read_bytes(&path)? else {
            return Ok(None);
        };
        let document = file_format::decode(&bytes).map_err(StorageError::Corrupt)?;
        if document.id() != id {
            return Err(StorageError::Corrupt(format!(
                "{} contains document {}",
                path.display(),
                document.id()
            )));
        }
        Ok(Some(document))
    }

    /// The stored revision, read from the file without restoring the document.
    fn read_revision(&self, path: &Path) -> Result<Option<u64>, StorageError> {
        self.read_bytes(path)?
            .map(|bytes| file_format::decode_revision(&bytes).map_err(StorageError::Corrupt))
            .transpose()
    }

    fn read_bytes(&self, path: &Path) -> Result<Option<Vec<u8>>, StorageError> {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(unavailable("open", path, &error)),
        };
        let mut bytes = Vec::new();
        file.take(MAX_DOCUMENT_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| unavailable("read", path, &error))?;
        if bytes.len() as u64 > MAX_DOCUMENT_BYTES {
            return Err(StorageError::Corrupt(format!(
                "{} exceeds {MAX_DOCUMENT_BYTES} bytes",
                path.display()
            )));
        }
        Ok(Some(bytes))
    }

    /// Writes beside `target` and renames over it; see the module notes.
    fn write_atomically(&self, target: &Path, bytes: &[u8]) -> io::Result<()> {
        let directory = target.parent().unwrap_or(Path::new("."));
        create_private_dir(directory)?;
        let temporary = directory.join(format!(
            ".{}.{}.{}{TEMPORARY_SUFFIX}",
            target
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("document"),
            std::process::id(),
            TEMPORARY_COUNTER.fetch_add(1, Ordering::Relaxed),
        ));
        let result = (|| {
            let mut file = create_private_file(&temporary)?;
            file.write_all(bytes)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temporary, target)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
            return result;
        }
        // The rename is the commit point: the new file is in place and is what the next read
        // returns, so the write must not be reported as failed after it. Syncing the
        // directory makes the rename survive power loss where the platform supports it; if it
        // cannot, the write stays committed with that weaker guarantee.
        let _ = sync_directory(directory);
        Ok(())
    }

    /// Waits for the document's save lock. The returned file holds it until dropped.
    fn lock_document(&self, id: &DocumentId) -> Result<File, StorageError> {
        let path = self.locks.join(format!("{id}.lock"));
        let file = open_lock_file(&path).map_err(|error| unavailable("lock", &path, &error))?;
        let deadline = Instant::now() + SAVE_LOCK_WAIT;
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(file),
                Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                    std::thread::sleep(LOCK_POLL);
                }
                Err(TryLockError::WouldBlock) => {
                    return Err(StorageError::Locked(format!(
                        "another process has been saving document {id} for over {} seconds",
                        SAVE_LOCK_WAIT.as_secs()
                    )));
                }
                Err(TryLockError::Error(error)) => return Err(unavailable("lock", &path, &error)),
            }
        }
    }

    fn owned(&self) -> MutexGuard<'_, HashMap<DocumentId, File>> {
        self.owned.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Serializes taking draft owner locks within this store; see `taking_ownership`.
    fn taking_ownership(&self) -> MutexGuard<'_, ()> {
        self.taking_ownership
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn owns(&self, id: &DocumentId) -> bool {
        self.owned().contains_key(id)
    }

    /// Takes the draft's owner lock if no running process holds it.
    fn try_own(&self, id: &DocumentId) -> Result<Option<File>, StorageError> {
        let path = self.drafts.join(format!("{id}.owner"));
        let file = open_lock_file(&path).map_err(|error| unavailable("lock", &path, &error))?;
        match file.try_lock() {
            Ok(()) => Ok(Some(file)),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(error)) => Err(unavailable("lock", &path, &error)),
        }
    }

    /// Takes ownership of an unowned draft slot, settling a save its last owner left
    /// unfinished. `Ok(None)` means another running process owns it.
    fn adopt(&self, id: &DocumentId) -> Result<Option<File>, StorageError> {
        let Some(owner) = self.try_own(id)? else {
            return Ok(None);
        };
        if exists(&self.set_aside_path(id))? {
            let _lock = self.lock_document(id)?;
            self.settle_interrupted_save(id)?;
        }
        Ok(Some(owner))
    }

    /// Restores and validates a draft file's contents, including that it belongs to `id`.
    fn decode_draft(&self, id: &DocumentId, bytes: &[u8]) -> Result<Draft, String> {
        let draft = file_format::decode_draft(bytes)?;
        if draft.document.id() != id {
            return Err(format!(
                "{} contains document {}",
                self.draft_path(id).display(),
                draft.document.id()
            ));
        }
        Ok(draft)
    }

    /// Settles a draft set aside by a save whose process ended before removing it. Called with
    /// the save lock held, so no commit can happen meanwhile.
    fn settle_interrupted_save(&self, id: &DocumentId) -> Result<(), StorageError> {
        let set_aside = self.set_aside_path(id);
        let Some(bytes) = self.read_bytes(&set_aside)? else {
            return Ok(());
        };
        let stored = self.read_revision(&self.path(id))?;
        // An unreadable draft is kept, so it is reported rather than silently dropped.
        let committed = file_format::decode_draft_info(&bytes)
            .is_ok_and(|info| stored != Some(info.base_revision));
        let result = if committed {
            fs::remove_file(&set_aside)
        } else {
            fs::rename(&set_aside, self.draft_path(id))
        };
        result.map_err(|error| unavailable("settle", &set_aside, &error))?;
        let _ = sync_directory(&self.drafts);
        Ok(())
    }
}

impl DocumentRepository for FileDocumentRepository {
    fn allocate_id(&self) -> Result<DocumentId, StorageError> {
        for _ in 0..8 {
            let mut bytes = [0_u8; 8];
            getrandom::fill(&mut bytes).map_err(|error| {
                StorageError::Unavailable(format!("no system randomness: {error}"))
            })?;
            let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
            let id = DocumentId::parse(&format!("doc-{hex}"))
                .expect("generated identifiers are canonical");
            if !self.path(&id).exists() {
                return Ok(id);
            }
        }
        Err(StorageError::Unavailable(
            "could not allocate an unused document identifier".into(),
        ))
    }

    fn list_ids(&self) -> Result<Vec<DocumentId>, StorageError> {
        let entries = match fs::read_dir(&self.directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(unavailable("list", &self.directory, &error)),
        };
        let mut ids = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| unavailable("list", &self.directory, &error))?;
            let name = entry.file_name();
            let id = name
                .to_str()
                .and_then(|name| name.strip_suffix(".json"))
                .and_then(DocumentId::parse);
            ids.extend(id);
        }
        ids.sort();
        Ok(ids)
    }

    /// Answers from the cache while the file's stamp is unchanged. A rewrite in place by
    /// another program that keeps the stamp could be missed until the next change; that only
    /// delays a refresh, because saves check the base without the cache.
    fn stored_revision(&self, id: &DocumentId) -> Result<Option<u64>, StorageError> {
        let path = self.path(id);
        let stamp = match fs::metadata(&path) {
            Ok(metadata) => FileStamp::of(&metadata),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(unavailable("inspect", &path, &error)),
        };
        let probes = || self.probes.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(stamp) = stamp
            && let Some(&(known, revision)) = probes().get(id)
            && known == stamp
        {
            return Ok(Some(revision));
        }
        // Read without holding the cache lock; probes of other documents proceed meanwhile.
        let revision = self.read_revision(&path)?;
        match (stamp, revision) {
            (Some(stamp), Some(revision)) => probes().insert(id.clone(), (stamp, revision)),
            _ => probes().remove(id),
        };
        Ok(revision)
    }

    fn load(&self, id: &DocumentId) -> Result<Option<Document>, StorageError> {
        self.read(id)
    }

    fn save(&self, document: &Document, base_revision: Option<u64>) -> Result<(), StorageError> {
        let id = document.id();
        let target = self.path(id);
        let _lock = self.lock_document(id)?;
        self.settle_interrupted_save(id)?;
        // The revision probe settles the common case without restoring the stored document.
        let stored_revision = self.read_revision(&target)?;
        let base_matches = match (stored_revision, base_revision) {
            (None, None) => true,
            (Some(stored), Some(base)) if stored == base => true,
            // A retried save whose acknowledgement was lost: the file already holds it.
            (Some(_), Some(_)) => self.read(id)?.as_ref() == Some(document),
            _ => false,
        };
        if !base_matches {
            return Err(StorageError::Conflict { stored_revision });
        }
        let bytes = file_format::encode(document);
        // Never commit a file this store would refuse to read back.
        if bytes.len() as u64 > MAX_DOCUMENT_BYTES {
            return Err(StorageError::Unavailable(format!(
                "the document encodes to {} bytes, over the {MAX_DOCUMENT_BYTES}-byte limit",
                bytes.len()
            )));
        }
        // The owned draft is set aside before the commit and removed after it, so a draft
        // left behind by a crash in between is recognized as superseded.
        let owned = self.owns(id);
        let set_aside = self.set_aside_path(id);
        let draft_set_aside = owned
            && match fs::rename(self.draft_path(id), &set_aside) {
                Ok(()) => true,
                Err(error) if error.kind() == io::ErrorKind::NotFound => false,
                Err(error) => return Err(unavailable("set aside", &set_aside, &error)),
            };
        pause_for_tests();
        if let Err(error) = self.write_atomically(&target, &bytes) {
            if draft_set_aside {
                let _ = fs::rename(&set_aside, self.draft_path(id));
            }
            return Err(unavailable("save", &target, &error));
        }
        if draft_set_aside {
            // A leftover is settled as superseded by the next save lock holder.
            let _ = fs::remove_file(&set_aside);
            let _ = sync_directory(&self.drafts);
        }
        if owned {
            self.owned().remove(id);
        }
        Ok(())
    }

    fn draft_status(&self, id: &DocumentId) -> Result<DraftStatus, StorageError> {
        let _taking = self.taking_ownership();
        if self.owns(id) {
            return Ok(DraftStatus::Owned);
        }
        let draft = self.draft_path(id);
        if !exists(&draft)? && !exists(&self.set_aside_path(id))? {
            return Ok(DraftStatus::None);
        }
        let Some(_owner) = self.adopt(id)? else {
            return Ok(DraftStatus::Locked);
        };
        Ok(match self.read_bytes(&draft) {
            Ok(None) => DraftStatus::None,
            // A draft is recoverable only if it restores completely, so the state offered
            // matches what `claim_draft` will return.
            Ok(Some(bytes)) => {
                DraftStatus::Abandoned(self.decode_draft(id, &bytes).map(|draft| DraftInfo {
                    revision: draft.document.revision(),
                    base_revision: draft.base_revision,
                    saved_at_ms: draft.saved_at_ms,
                }))
            }
            Err(StorageError::Corrupt(message)) => DraftStatus::Abandoned(Err(message)),
            Err(error) => return Err(error),
        })
    }

    fn write_draft(&self, draft: &Draft) -> Result<(), StorageError> {
        let id = draft.document.id();
        let path = self.draft_path(id);
        let taking = self.taking_ownership();
        if !self.owns(id) {
            let Some(owner) = self.adopt(id)? else {
                return Err(StorageError::Locked(format!(
                    "another running process holds the recovery draft of document {id}"
                )));
            };
            if exists(&path)? {
                return Err(StorageError::Conflict {
                    stored_revision: None,
                });
            }
            self.owned().insert(id.clone(), owner);
        }
        drop(taking);
        let bytes = file_format::encode_draft(draft);
        if bytes.len() as u64 > MAX_DOCUMENT_BYTES {
            return Err(StorageError::Unavailable(format!(
                "the draft encodes to {} bytes, over the {MAX_DOCUMENT_BYTES}-byte limit",
                bytes.len()
            )));
        }
        self.write_atomically(&path, &bytes)
            .map_err(|error| unavailable("write draft", &path, &error))
    }

    fn claim_draft(&self, id: &DocumentId) -> Result<Option<Result<Draft, String>>, StorageError> {
        let taking = self.taking_ownership();
        if !self.owns(id) {
            let Some(owner) = self.adopt(id)? else {
                return Err(StorageError::Locked(format!(
                    "another running process holds the recovery draft of document {id}"
                )));
            };
            self.owned().insert(id.clone(), owner);
        }
        drop(taking);
        let path = self.draft_path(id);
        let bytes = match self.read_bytes(&path) {
            Ok(Some(bytes)) => bytes,
            Ok(None) => {
                self.release_draft(id);
                return Ok(None);
            }
            Err(StorageError::Corrupt(message)) => return Ok(Some(Err(message))),
            Err(error) => {
                self.release_draft(id);
                return Err(error);
            }
        };
        Ok(Some(self.decode_draft(id, &bytes)))
    }

    fn release_draft(&self, id: &DocumentId) {
        self.owned().remove(id);
    }

    fn remove_draft(&self, id: &DocumentId) -> Result<(), StorageError> {
        if !self.owns(id) {
            return Ok(());
        }
        for path in [self.draft_path(id), self.set_aside_path(id)] {
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(unavailable("remove", &path, &error)),
            }
        }
        let _ = sync_directory(&self.drafts);
        self.owned().remove(id);
        Ok(())
    }
}

fn unavailable(action: &str, path: &Path, error: &io::Error) -> StorageError {
    StorageError::Unavailable(format!("could not {action} {}: {error}", path.display()))
}

#[cfg(unix)]
fn create_private_dir(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
}

#[cfg(not(unix))]
fn create_private_dir(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)
}

fn exists(path: &Path) -> Result<bool, StorageError> {
    path.try_exists()
        .map_err(|error| unavailable("inspect", path, &error))
}

/// Opens (creating if needed) a lock file without truncating it.
fn open_lock_file(path: &Path) -> io::Result<File> {
    if let Some(directory) = path.parent() {
        create_private_dir(directory)?;
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

fn pause_for_tests() {
    #[cfg(debug_assertions)]
    if let Some(milliseconds) = std::env::var(COMMIT_DELAY_VARIABLE)
        .ok()
        .and_then(|value| value.parse().ok())
    {
        std::thread::sleep(Duration::from_millis(milliseconds));
    }
}

fn create_private_file(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

/// Windows has no portable directory handle sync; the rename itself is the commit.
#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> io::Result<()> {
    Ok(())
}

/// Platform user-data directory for Chemifree, or `None` if the environment gives none.
pub fn default_data_dir() -> Option<PathBuf> {
    let from = |variable: &str| {
        std::env::var_os(variable)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    };
    let base = if cfg!(windows) {
        from("APPDATA")
    } else if cfg!(target_os = "macos") {
        from("HOME").map(|home| home.join("Library").join("Application Support"))
    } else {
        from("XDG_DATA_HOME").or_else(|| from("HOME").map(|home| home.join(".local").join("share")))
    }?;
    Some(base.join("chemifree"))
}

#[cfg(test)]
mod tests {
    use chemifree_domain::document::{
        AtomId, AtomRef, BondOrder, BondTo, Command, HISTORY_LIMIT, MAX_ATOMS, MAX_BONDS,
        MAX_COMMANDS, MIN_CHARGE, Point, Vector,
    };
    use chemifree_domain::element::Element;

    use super::*;

    fn scratch() -> (tempfile::TempDir, FileDocumentRepository) {
        let directory = tempfile::tempdir().unwrap();
        let repository = FileDocumentRepository::new(directory.path());
        assert!(
            !repository.directory().exists(),
            "construction has no side effects"
        );
        assert_eq!(repository.list_ids().unwrap(), []);
        (directory, repository)
    }

    fn drawn(repository: &FileDocumentRepository) -> Document {
        let mut document = Document::new(repository.allocate_id().unwrap(), "Stored").unwrap();
        document
            .apply(&[
                Command::AddAtom {
                    element: Element::CARBON,
                    // Needs the full round-trip float parser to reload bit-exactly.
                    position: Point::new(150.8, 226.399_999_999_999_98).unwrap(),
                    bond_to: None,
                },
                Command::AddAtom {
                    element: Element::from_symbol("Cl").unwrap(),
                    position: Point::new(30.0, 0.1).unwrap(),
                    bond_to: Some(BondTo {
                        atom: AtomRef::Id(AtomId::from_serial(1).unwrap()),
                        order: BondOrder::Triple,
                    }),
                },
            ])
            .unwrap();
        document
            .apply(&[Command::Rename {
                title: "Stored twice".into(),
            }])
            .unwrap();
        document.undo().unwrap();
        document
    }

    #[test]
    fn saved_documents_load_identically_with_history() {
        let (_directory, repository) = scratch();
        let document = drawn(&repository);
        repository.save(&document, None).unwrap();
        assert_eq!(
            repository.load(document.id()).unwrap(),
            Some(document.clone())
        );
        assert_eq!(repository.list_ids().unwrap(), [document.id().clone()]);

        let reopened = FileDocumentRepository::new(_directory.path());
        let mut loaded = reopened.load(document.id()).unwrap().unwrap();
        assert_eq!((loaded.undo_depth(), loaded.redo_depth()), (1, 1));
        loaded.redo().unwrap();
        assert_eq!(loaded.title(), "Stored twice");
    }

    /// Entity and history limits together must keep every document within the read limit,
    /// or a save could commit a file that can never be opened again.
    #[test]
    fn a_document_at_every_limit_encodes_within_the_read_limit() {
        let mut document =
            Document::new(DocumentId::parse("doc-limits").unwrap(), "Limits").unwrap();
        let atom = |serial: usize| AtomRef::Id(AtomId::from_serial(serial as u32).unwrap());
        let ids = |range: std::ops::Range<usize>| {
            range.map(|serial| AtomId::from_serial(serial as u32).unwrap())
        };
        for batch in (0..MAX_ATOMS).collect::<Vec<_>>().chunks(MAX_COMMANDS) {
            let commands: Vec<_> = batch
                .iter()
                .map(|&index| Command::AddAtom {
                    element: Element::CARBON,
                    // Long shortest-round-trip decimals, near the coordinate bound.
                    position: Point::new(
                        -99_999.0 + index as f64 * 0.013,
                        99_999.0 - index as f64 * 0.017,
                    )
                    .unwrap(),
                    bond_to: None,
                })
                .collect();
            document.apply(&commands).unwrap();
        }
        let pairs: Vec<_> = (1..MAX_ATOMS)
            .map(|serial| (serial, serial + 1))
            .chain((1..MAX_ATOMS - 1).map(|serial| (serial, serial + 2)))
            .take(MAX_BONDS)
            .collect();
        for batch in pairs.chunks(MAX_COMMANDS) {
            let commands: Vec<_> = batch
                .iter()
                .map(|&(begin, end)| Command::AddBond {
                    begin: atom(begin),
                    end: atom(end),
                    order: BondOrder::Triple,
                })
                .collect();
            document.apply(&commands).unwrap();
        }
        for batch in (1..=MAX_ATOMS).collect::<Vec<_>>().chunks(MAX_COMMANDS) {
            let commands: Vec<_> = batch
                .iter()
                .map(|&serial| Command::UpdateAtom {
                    atom: AtomId::from_serial(serial as u32).unwrap(),
                    element: Some(Element::from_symbol("Og").unwrap()),
                    charge: Some(MIN_CHARGE),
                    isotope: Some(Some(999)),
                })
                .collect();
            document.apply(&commands).unwrap();
        }
        // Moving every atom repeatedly fills the history with the largest entries.
        for _ in 0..HISTORY_LIMIT {
            document
                .apply(&[Command::MoveAtoms {
                    atoms: ids(1..MAX_ATOMS + 1).collect(),
                    delta: Vector {
                        dx: 0.000_123_456_789,
                        dy: -0.000_987_654_321,
                    },
                }])
                .unwrap();
        }
        let bytes = file_format::encode(&document);
        assert!(
            (bytes.len() as u64) < MAX_DOCUMENT_BYTES,
            "{} bytes exceed {MAX_DOCUMENT_BYTES}",
            bytes.len()
        );
        assert_eq!(file_format::decode(&bytes).unwrap(), document);
    }

    #[test]
    fn stored_revision_reads_the_committed_revision_only() {
        let (_directory, repository) = scratch();
        let document = drawn(&repository);
        assert_eq!(repository.stored_revision(document.id()), Ok(None));
        repository.save(&document, None).unwrap();
        assert_eq!(repository.stored_revision(document.id()), Ok(Some(3)));
        fs::write(repository.path(document.id()), b"{}").unwrap();
        assert!(matches!(
            repository.stored_revision(document.id()),
            Err(StorageError::Corrupt(_))
        ));
    }

    #[test]
    fn saves_check_the_base_revision() {
        let (_directory, repository) = scratch();
        let mut document = drawn(&repository);
        repository.save(&document, None).unwrap();
        assert_eq!(
            repository.save(&document, None),
            Err(StorageError::Conflict {
                stored_revision: Some(3)
            })
        );
        let base = document.revision();
        document.redo().unwrap();
        repository.save(&document, Some(base)).unwrap();
        // A retried save of the same content succeeds even though the base moved.
        repository.save(&document, Some(base)).unwrap();
        document.undo().unwrap();
        assert!(matches!(
            repository.save(&document, Some(base)),
            Err(StorageError::Conflict { .. })
        ));
    }

    #[test]
    fn a_failed_write_keeps_the_committed_file_and_leaves_no_temporary() {
        let (_directory, repository) = scratch();
        let mut document = drawn(&repository);
        repository.save(&document, None).unwrap();
        let committed = fs::read(repository.path(document.id())).unwrap();

        // Replacing a directory with a file fails on every platform, even for root.
        let blocked = repository.directory().join("blocked.json");
        fs::create_dir(&blocked).unwrap();
        let result = repository.write_atomically(&blocked, b"{}");
        assert!(result.is_err());

        document.redo().unwrap();
        let base = document.revision() - 1;
        let path = repository.path(document.id());
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(matches!(
            repository.save(&document, Some(base)),
            Err(StorageError::Unavailable(_))
        ));
        fs::remove_dir(&path).unwrap();
        fs::write(&path, &committed).unwrap();
        repository.save(&document, Some(base)).unwrap();

        let leftovers: Vec<_> = fs::read_dir(repository.directory())
            .unwrap()
            .flatten()
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .ends_with(TEMPORARY_SUFFIX)
            })
            .collect();
        assert!(leftovers.is_empty());
    }

    #[test]
    fn unreadable_files_are_reported_not_loaded() {
        let (_directory, repository) = scratch();
        let document = drawn(&repository);
        repository.save(&document, None).unwrap();
        let path = repository.path(document.id());
        let valid = fs::read_to_string(&path).unwrap();

        for corrupt in [
            "not json".to_owned(),
            valid.replace("\"format_version\": 1", "\"format_version\": 99"),
            valid.replacen("\"begin\": \"a1\"", "\"begin\": \"a9\"", 1),
            valid.replace(document.id().as_str(), "doc-0000000000000000"),
        ] {
            fs::write(&path, corrupt).unwrap();
            assert!(
                matches!(
                    repository.load(document.id()),
                    Err(StorageError::Corrupt(_))
                ),
                "corrupt file was accepted"
            );
        }
        let abandoned = repository.directory().join(".doc-x.1.2.tmp");
        fs::write(&abandoned, "partial").unwrap();
        fs::write(repository.directory().join("README.txt"), "ignored").unwrap();
        assert_eq!(repository.list_ids().unwrap(), [document.id().clone()]);
        repository.prune_stale_temporaries();
        assert!(
            abandoned.exists(),
            "a recent temporary may belong to a live save"
        );
    }

    fn draft_of(document: &Document, base_revision: u64) -> Draft {
        Draft {
            document: document.clone(),
            base_revision,
            saved_at_ms: 1_700_000_000_000,
        }
    }

    #[test]
    fn a_save_waits_for_another_stores_lock_and_then_sees_its_commit() {
        let (directory, first) = scratch();
        let second = FileDocumentRepository::new(directory.path());
        let mut document = drawn(&first);
        first.save(&document, None).unwrap();
        let base = document.revision();
        let mut theirs = document.clone();
        theirs.redo().unwrap();
        document
            .apply(&[Command::Rename {
                title: "Mine".into(),
            }])
            .unwrap();

        // `first` holds the lock while it commits; `second` saves from the same base.
        let lock = first.lock_document(document.id()).unwrap();
        let waiting = std::thread::spawn(move || second.save(&theirs, Some(base)));
        std::thread::sleep(Duration::from_millis(200));
        assert!(!waiting.is_finished(), "the second save waits for the lock");
        first
            .write_atomically(&first.path(document.id()), &file_format::encode(&document))
            .unwrap();
        drop(lock);
        assert_eq!(
            waiting.join().unwrap(),
            Err(StorageError::Conflict {
                stored_revision: Some(document.revision())
            })
        );
        assert_eq!(first.load(document.id()).unwrap(), Some(document));
    }

    #[test]
    fn requests_in_one_process_never_mistake_its_own_probe_for_another_owner() {
        let (directory, writer) = scratch();
        let document = drawn(&writer);
        writer.save(&document, None).unwrap();
        writer.write_draft(&draft_of(&document, 1)).unwrap();
        drop(writer); // Its process ended: the draft is abandoned.

        // Concurrent requests in one process (for example, two list requests from one page)
        // each inspect the abandoned draft through the same store.
        let store = FileDocumentRepository::new(directory.path());
        let threads = 8;
        let probes = 200;
        let start = Arc::new(std::sync::Barrier::new(threads));
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                let store = store.clone();
                let id = document.id().clone();
                let start = Arc::clone(&start);
                std::thread::spawn(move || {
                    start.wait();
                    let mut misread = Vec::new();
                    for _ in 0..probes {
                        match store.draft_status(&id) {
                            Ok(DraftStatus::Abandoned(Ok(_))) => {}
                            other => misread.push(other),
                        }
                    }
                    misread
                })
            })
            .collect();
        let misread: Vec<_> = workers
            .into_iter()
            .flat_map(|worker| worker.join().unwrap())
            .collect();
        assert!(
            misread.is_empty(),
            "{} of {} probes misread the abandoned draft, e.g. {:?}",
            misread.len(),
            threads * probes,
            misread.first()
        );
    }

    #[test]
    fn a_draft_is_owned_until_its_store_ends_then_can_be_claimed_once() {
        let (directory, writer) = scratch();
        let document = drawn(&writer);
        writer.save(&document, None).unwrap();
        let draft = draft_of(&document, 1);
        writer.write_draft(&draft).unwrap();
        assert_eq!(writer.draft_status(document.id()), Ok(DraftStatus::Owned));

        let other = FileDocumentRepository::new(directory.path());
        assert_eq!(other.draft_status(document.id()), Ok(DraftStatus::Locked));
        assert!(matches!(
            other.write_draft(&draft),
            Err(StorageError::Locked(_))
        ));
        assert!(matches!(
            other.claim_draft(document.id()),
            Err(StorageError::Locked(_))
        ));

        drop(writer); // The process ends: the operating system releases its locks.
        assert_eq!(
            other.draft_status(document.id()),
            Ok(DraftStatus::Abandoned(Ok(
                chemifree_application::DraftInfo {
                    revision: 3,
                    base_revision: 1,
                    saved_at_ms: draft.saved_at_ms,
                }
            )))
        );
        assert_eq!(
            other.write_draft(&draft),
            Err(StorageError::Conflict {
                stored_revision: None
            }),
            "an abandoned draft is never overwritten by new work"
        );
        assert_eq!(
            other.claim_draft(document.id()),
            Ok(Some(Ok(draft.clone())))
        );
        assert_eq!(other.draft_status(document.id()), Ok(DraftStatus::Owned));
        let third = FileDocumentRepository::new(directory.path());
        assert_eq!(third.draft_status(document.id()), Ok(DraftStatus::Locked));
        other.remove_draft(document.id()).unwrap();
        assert_eq!(third.draft_status(document.id()), Ok(DraftStatus::None));
    }

    #[test]
    fn a_save_removes_only_the_draft_its_store_owns() {
        let (directory, owner) = scratch();
        let mut document = drawn(&owner);
        owner.save(&document, None).unwrap();
        owner.write_draft(&draft_of(&document, 3)).unwrap();
        let base = document.revision();
        document.redo().unwrap();

        let other = FileDocumentRepository::new(directory.path());
        other.save(&document, Some(base)).unwrap();
        assert_eq!(other.draft_status(document.id()), Ok(DraftStatus::Locked));

        let base = document.revision();
        document.undo().unwrap();
        owner.save(&document, Some(base)).unwrap();
        assert_eq!(owner.draft_status(document.id()), Ok(DraftStatus::None));
        assert!(!owner.draft_path(document.id()).exists());
        assert!(!owner.set_aside_path(document.id()).exists());
    }

    /// A process can end between setting its draft aside and removing it. The next store
    /// keeps the draft exactly when the commit did not happen.
    #[test]
    fn a_save_interrupted_around_its_commit_settles_the_draft() {
        for committed in [false, true] {
            let (directory, owner) = scratch();
            let mut document = drawn(&owner);
            owner.save(&document, None).unwrap();
            let base = document.revision();
            owner.write_draft(&draft_of(&document, base)).unwrap();
            fs::rename(
                owner.draft_path(document.id()),
                owner.set_aside_path(document.id()),
            )
            .unwrap();
            if committed {
                document.redo().unwrap();
                owner
                    .write_atomically(&owner.path(document.id()), &file_format::encode(&document))
                    .unwrap();
            }
            drop(owner);

            let next = FileDocumentRepository::new(directory.path());
            let status = next.draft_status(document.id()).unwrap();
            if committed {
                assert_eq!(status, DraftStatus::None, "the commit superseded the draft");
            } else {
                assert!(
                    matches!(status, DraftStatus::Abandoned(Ok(_))),
                    "{status:?}"
                );
            }
            assert!(!next.set_aside_path(document.id()).exists());
        }
    }

    #[test]
    fn an_unreadable_draft_is_reported_and_can_be_removed() {
        let (_directory, store) = scratch();
        let document = drawn(&store);
        store.save(&document, None).unwrap();
        let path = store.draft_path(document.id());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{ not a draft").unwrap();
        assert!(matches!(
            store.draft_status(document.id()),
            Ok(DraftStatus::Abandoned(Err(_)))
        ));
        assert!(matches!(store.claim_draft(document.id()), Ok(Some(Err(_)))));
        store.remove_draft(document.id()).unwrap();
        assert!(!path.exists());
        assert_eq!(store.draft_status(document.id()), Ok(DraftStatus::None));
    }

    #[test]
    fn a_draft_with_a_valid_header_but_bad_contents_is_unreadable() {
        let (_directory, store) = scratch();
        let document = drawn(&store);
        store.save(&document, None).unwrap();
        let other = Document::new(store.allocate_id().unwrap(), "Other").unwrap();
        let path = store.draft_path(document.id());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let header_only = r#"{"format":"chemifree.draft","format_version":1,"base_revision":0,
            "saved_at_ms":0,"document":{"revision":3},"history":{}}"#;
        for bytes in [
            header_only.as_bytes().to_vec(),
            file_format::encode_draft(&draft_of(&other, 0)),
        ] {
            fs::write(&path, bytes).unwrap();
            assert!(matches!(
                store.draft_status(document.id()),
                Ok(DraftStatus::Abandoned(Err(_)))
            ));
            assert!(matches!(store.claim_draft(document.id()), Ok(Some(Err(_)))));
            store.release_draft(document.id());
        }
    }

    #[test]
    fn drafts_round_trip_with_history() {
        let (_directory, store) = scratch();
        let document = drawn(&store);
        store.save(&document, None).unwrap();
        let draft = draft_of(&document, 0);
        store.write_draft(&draft).unwrap();
        store.release_draft(document.id());
        assert_eq!(store.claim_draft(document.id()), Ok(Some(Ok(draft))));
    }
}
