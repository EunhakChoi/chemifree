//! Local JSON document storage with atomic replace-on-save.
//!
//! Layout: `<root>/documents/<id>.json`. A save writes a uniquely named temporary file in
//! the same directory, flushes it to disk, renames it over the target, and then syncs the
//! directory. A crash or failure at any point leaves the previously committed file intact;
//! leftover temporary files are ignored and pruned later.
//!
//! The base-revision check and the rename are not one atomic step across processes. Two
//! processes saving the same document in the same instant can still race; within one
//! process the application serializes saves.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime};

use chemifree_application::{DocumentRepository, StorageError};
use chemifree_domain::document::{Document, DocumentId};

use crate::file_format;

const DOCUMENTS_DIR: &str = "documents";
const EXTENSION: &str = "json";
const TEMPORARY_SUFFIX: &str = ".tmp";
/// Larger files are refused before parsing.
pub const MAX_DOCUMENT_BYTES: u64 = 64 * 1024 * 1024;
const STALE_TEMPORARY_AGE: Duration = Duration::from_secs(60 * 60);

static TEMPORARY_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug)]
pub struct FileDocumentRepository {
    directory: PathBuf,
    /// Revisions read by `stored_revision`, keyed by a stamp of the file, so polling an
    /// unchanged file does not re-read it.
    probes: Arc<Mutex<HashMap<DocumentId, (FileStamp, u64)>>>,
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
    /// Uses `<root>/documents`. Nothing is touched until the first save, which creates the
    /// directory (owner-only on Unix).
    pub fn new(root: &Path) -> Self {
        Self {
            directory: root.join(DOCUMENTS_DIR),
            probes: Arc::default(),
        }
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    fn path(&self, id: &DocumentId) -> PathBuf {
        self.directory.join(format!("{id}.{EXTENSION}"))
    }

    /// Removes temporary files that interrupted saves abandoned more than an hour ago.
    /// Younger ones may belong to a save in progress in another process.
    pub fn prune_stale_temporaries(&self) {
        let Ok(entries) = fs::read_dir(&self.directory) else {
            return;
        };
        let now = SystemTime::now();
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

    fn write_atomically(&self, target: &Path, bytes: &[u8]) -> io::Result<()> {
        let temporary = self.directory.join(format!(
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
            fs::rename(&temporary, target)?;
            sync_directory(&self.directory)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
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
        let target = self.path(document.id());
        // The revision probe settles the common case without restoring the stored document.
        let stored_revision = self.read_revision(&target)?;
        let base_matches = match (stored_revision, base_revision) {
            (None, None) => true,
            (Some(stored), Some(base)) if stored == base => true,
            // A retried save whose acknowledgement was lost: the file already holds it.
            (Some(_), Some(_)) => self.read(document.id())?.as_ref() == Some(document),
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
        create_private_dir(&self.directory)
            .and_then(|()| self.write_atomically(&target, &bytes))
            .map_err(|error| unavailable("save", &target, &error))
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
}
