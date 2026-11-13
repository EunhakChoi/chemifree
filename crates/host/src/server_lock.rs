//! The data directory's server lock: an operating-system advisory lock on `<data>/server.lock`
//! that lets one API server serve a data directory at a time. Only the HTTP API takes it.

use std::error::Error;
use std::fmt;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process;

/// The most bytes of a record that are read. A record this module writes is far shorter; the bound
/// keeps a tampered file from being read whole.
const RECORD_LIMIT: u64 = 1024;
/// The longest address a record may show.
const MAX_URL_BYTES: usize = 64;
/// The only address form a record may show: the server listens on loopback.
const URL_PREFIX: &str = "http://127.0.0.1:";

/// The data directory's server lock, held for as long as this value lives.
///
/// The lock is taken on `<data>/server.lock`. The operating system releases it when its holder
/// exits, however it exits, so a killed server never blocks the next start. The file is never
/// deleted or truncated on drop: deleting a lock file that another process is about to open could
/// let two holders in. The record in the file is informational. A refused server reads it while
/// the lock is held. The holder rewrites the record right after taking the lock, so a refused
/// server shows the current holder's record, except in the instant before that rewrite, when it
/// may still show the record a killed server left.
#[derive(Debug)]
#[must_use = "if unused the lock is released immediately"]
pub struct ServerLock {
    file: File,
    path: PathBuf,
}

impl ServerLock {
    /// The name of the lock file in the data directory.
    pub const FILE_NAME: &str = "server.lock";

    /// Takes the lock of `data_dir`, creating the directory and any missing parents first. The
    /// lock is held until the returned value is dropped.
    ///
    /// The record is replaced with this process's pid only after the lock is taken, so a refused
    /// process never erases the record of the holder.
    ///
    /// # Errors
    ///
    /// Returns [`ServerLockError::Held`] when another live process holds the lock, and
    /// [`ServerLockError::Io`] when the directory or the lock file cannot be created, locked or
    /// written.
    pub fn acquire(data_dir: &Path) -> Result<Self, ServerLockError> {
        create_private_dir(data_dir).map_err(|error| ServerLockError::Io {
            path: data_dir.to_path_buf(),
            error,
        })?;
        let path = data_dir.join(Self::FILE_NAME);
        let file = open_lock_file(&path).map_err(|error| ServerLockError::Io {
            path: path.clone(),
            error,
        })?;
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                return Err(ServerLockError::Held {
                    data_dir: data_dir.to_path_buf(),
                    holder: read_holder(&path),
                });
            }
            Err(TryLockError::Error(error)) => return Err(ServerLockError::Io { path, error }),
        }
        replace_record(&file, &format!("pid={}\n", process::id())).map_err(|error| {
            ServerLockError::Io {
                path: path.clone(),
                error,
            }
        })?;
        Ok(Self { file, path })
    }

    /// Records `url` next to this process's pid, replacing the record that `acquire` wrote. A
    /// refused server names the holder from this record.
    ///
    /// # Errors
    ///
    /// Returns the I/O error from rewriting the lock file. The lock stays held either way.
    pub fn record_url(&self, url: &str) -> io::Result<()> {
        replace_record(&self.file, &format!("pid={}\nurl={url}\n", process::id()))
    }

    /// The path of the lock file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// What a lock record says about the process that wrote it. A field is `None` unless the record
/// states a valid value for it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LockHolder {
    /// The holder's process id.
    pub pid: Option<u32>,
    /// The holder's address, such as `http://127.0.0.1:3001`.
    pub url: Option<String>,
}

impl fmt::Display for LockHolder {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.pid {
            Some(pid) => write!(formatter, "pid {pid}")?,
            None => formatter.write_str("pid unknown")?,
        }
        match &self.url {
            Some(url) => write!(formatter, ", {url}"),
            None => formatter.write_str(", address not recorded"),
        }
    }
}

/// Why the server lock could not be taken.
#[derive(Debug)]
pub enum ServerLockError {
    /// Another live process holds the lock. `holder` is what its record says about it.
    Held {
        /// The data directory, as it was passed to [`ServerLock::acquire`].
        data_dir: PathBuf,
        /// The holder's recorded pid and address, each `None` when the record does not state it.
        holder: LockHolder,
    },
    /// The lock file or its directory could not be created, locked or written.
    Io {
        /// The file or directory that failed.
        path: PathBuf,
        /// The underlying I/O error.
        error: io::Error,
    },
}

impl fmt::Display for ServerLockError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Held { data_dir, holder } => write!(
                formatter,
                "the data directory {} is already used by another Chemifree API server ({holder})",
                data_dir.display()
            ),
            Self::Io { path, error } => write!(
                formatter,
                "could not take the server lock {}: {error}",
                path.display()
            ),
        }
    }
}

impl Error for ServerLockError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Held { .. } => None,
            Self::Io { error, .. } => Some(error),
        }
    }
}

/// Creates `path` and any missing parents, private to the owner on unix.
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

/// Opens the lock file, creating it if needed. It is never truncated here: only a caller that
/// holds the lock replaces the record, through [`replace_record`].
fn open_lock_file(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

/// Replaces the whole record with `record`. Callers must hold the lock, because the truncation
/// would otherwise erase the holder's record.
fn replace_record(file: &File, record: &str) -> io::Result<()> {
    let mut writer = file;
    writer.seek(SeekFrom::Start(0))?;
    file.set_len(0)?;
    writer.write_all(record.as_bytes())
}

/// Reads the record of the current holder. It is called only when another process holds the lock,
/// so the record is that holder's, except in the instant before the holder rewrites it (see
/// [`ServerLock`]). A file that cannot be read gives an empty record.
fn read_holder(path: &Path) -> LockHolder {
    let Ok(file) = File::open(path) else {
        return LockHolder::default();
    };
    let mut record = String::new();
    if file.take(RECORD_LIMIT).read_to_string(&mut record).is_err() {
        return LockHolder::default();
    }
    parse_holder(&record)
}

/// Reads the `pid=` and `url=` lines of a record. Other lines, and values that fail their checks,
/// are ignored.
fn parse_holder(record: &str) -> LockHolder {
    let mut holder = LockHolder::default();
    for line in record.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key {
            "pid" => {
                let pid: Result<u32, _> = value.parse();
                if let Ok(pid) = pid {
                    holder.pid = Some(pid);
                }
            }
            "url" => {
                if let Some(url) = accepted_url(value) {
                    holder.url = Some(url.to_owned());
                }
            }
            _ => {}
        }
    }
    holder
}

/// The address to show: a loopback HTTP origin of at most [`MAX_URL_BYTES`] printable ASCII
/// bytes. The record is a file that any process may edit, so anything else is refused, which keeps
/// control characters out of the terminal.
fn accepted_url(value: &str) -> Option<&str> {
    let accepted = value.starts_with(URL_PREFIX)
        && value.len() <= MAX_URL_BYTES
        && value.bytes().all(|byte| byte.is_ascii_graphic());
    accepted.then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The address that the tests record, in the form a server records its own.
    const ADDRESS: &str = "http://127.0.0.1:4242";

    /// The message that a refused server prints for a holder described as `holder`.
    fn refused_message(data_dir: &Path, holder: &str) -> String {
        format!(
            "the data directory {} is already used by another Chemifree API server ({holder})",
            data_dir.display()
        )
    }

    #[cfg(unix)]
    fn mode_of(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;

        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn acquire_creates_the_data_directory_and_records_only_the_pid() {
        let directory = tempfile::tempdir().unwrap();
        let data = directory.path().join("nested").join("data");

        let lock = ServerLock::acquire(&data).unwrap();

        assert!(data.is_dir(), "a missing data directory is created");
        assert_eq!(lock.path(), data.join("server.lock"));
        assert_eq!(
            fs::read_to_string(lock.path()).unwrap(),
            format!("pid={}\n", process::id()),
            "a new holder records its pid and nothing else"
        );
    }

    #[test]
    fn acquire_replaces_a_stale_record_left_by_a_killed_server() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("server.lock");
        // A killed server leaves its record behind. The next holder must replace all of it.
        let stale = format!("pid=999999\nurl={ADDRESS}\n{}", "x".repeat(64));
        fs::write(&path, stale).unwrap();

        let _lock = ServerLock::acquire(directory.path()).unwrap();

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            format!("pid={}\n", process::id()),
            "the stale record is replaced, not appended to"
        );
    }

    #[test]
    fn record_url_records_the_address_after_the_pid() {
        let directory = tempfile::tempdir().unwrap();
        let lock = ServerLock::acquire(directory.path()).unwrap();

        lock.record_url(ADDRESS).unwrap();

        assert_eq!(
            fs::read_to_string(lock.path()).unwrap(),
            format!("pid={}\nurl={ADDRESS}\n", process::id())
        );
    }

    #[test]
    fn second_acquire_is_refused_and_names_the_live_holder() {
        let directory = tempfile::tempdir().unwrap();
        let first = ServerLock::acquire(directory.path()).unwrap();
        first.record_url(ADDRESS).unwrap();

        let error = ServerLock::acquire(directory.path())
            .expect_err("a second acquire must not share a lock that is still held");

        assert_eq!(
            error.to_string(),
            refused_message(
                directory.path(),
                &format!("pid {}, {ADDRESS}", process::id())
            )
        );
        let ServerLockError::Held { data_dir, holder } = &error else {
            panic!("a live holder must report Held, got {error}");
        };
        assert_eq!(data_dir, directory.path());
        assert_eq!(
            holder,
            &LockHolder {
                pid: Some(process::id()),
                url: Some(ADDRESS.to_owned()),
            }
        );
    }

    #[test]
    fn a_holder_that_has_not_recorded_its_address_is_named_by_pid_alone() {
        let directory = tempfile::tempdir().unwrap();
        let _first = ServerLock::acquire(directory.path()).unwrap();

        let error = ServerLock::acquire(directory.path())
            .expect_err("a second acquire must not share a lock that is still held");

        assert_eq!(
            error.to_string(),
            refused_message(
                directory.path(),
                &format!("pid {}, address not recorded", process::id())
            )
        );
    }

    #[test]
    fn dropping_the_lock_lets_the_next_server_take_it() {
        let directory = tempfile::tempdir().unwrap();
        let first = ServerLock::acquire(directory.path()).unwrap();
        first.record_url(ADDRESS).unwrap();
        drop(first);

        let _second = ServerLock::acquire(directory.path())
            .expect("the lock is free once its holder is dropped");

        assert!(
            directory.path().join("server.lock").is_file(),
            "the lock file is kept, never deleted"
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_data_directory_and_lock_file_are_private_to_their_owner() {
        let directory = tempfile::tempdir().unwrap();
        let data = directory.path().join("data");
        let lock = ServerLock::acquire(&data).unwrap();

        assert_eq!(
            mode_of(&data),
            0o700,
            "a created data directory is owner-only"
        );
        assert_eq!(
            mode_of(lock.path()),
            0o600,
            "the lock file is readable and writable by its owner only"
        );
    }

    #[test]
    fn read_holder_of_an_empty_or_missing_record_names_nothing() {
        let directory = tempfile::tempdir().unwrap();
        let empty = directory.path().join("server.lock");
        File::create(&empty).unwrap();

        assert_eq!(
            read_holder(&empty),
            LockHolder::default(),
            "an empty record states no pid and no address"
        );
        assert_eq!(
            read_holder(&directory.path().join("missing.lock")),
            LockHolder::default(),
            "a record that cannot be opened states no pid and no address"
        );
        assert_eq!(
            LockHolder::default().to_string(),
            "pid unknown, address not recorded"
        );
    }

    #[test]
    fn parse_holder_accepts_only_loopback_http_addresses() {
        let cases = [
            ("url=http://127.0.0.1:4242", Some(ADDRESS)),
            ("url=http://localhost:4242", None),
            ("url=http://example.com:4242", None),
            ("url=https://127.0.0.1:4242", None),
            ("url=http://127.0.0.1:4242 x", None),
            ("url=http://127.0.0.1:4242\u{1b}[2J", None),
        ];
        for (record, url) in cases {
            assert_eq!(
                parse_holder(record).url.as_deref(),
                url,
                "the record {record:?} shows the address it should"
            );
        }
    }

    #[test]
    fn parse_holder_accepts_an_address_of_up_to_64_bytes() {
        let prefix = "http://127.0.0.1:";
        let at_limit = format!("{prefix}{}", "1".repeat(64 - prefix.len()));
        let over_limit = format!("{at_limit}1");
        assert_eq!(at_limit.len(), 64, "the fixture is exactly at the limit");

        assert_eq!(
            parse_holder(&format!("url={at_limit}\n")).url,
            Some(at_limit.clone()),
            "an address of 64 bytes is shown"
        );
        assert_eq!(
            parse_holder(&format!("url={over_limit}\n")).url,
            None,
            "an address of 65 bytes is not shown"
        );
    }

    #[test]
    fn parse_holder_ignores_unknown_and_malformed_lines() {
        let holder = parse_holder("status=busy\nnoise\npid=seven\nurl=http://127.0.0.1:4242\n");

        assert_eq!(
            holder,
            LockHolder {
                pid: None,
                url: Some(ADDRESS.to_owned()),
            },
            "a non-numeric pid is absent, and the other lines still count"
        );
        assert_eq!(parse_holder("status=busy\npid=77\n").pid, Some(77));
        assert_eq!(
            parse_holder("pid=4294967296\n").pid,
            None,
            "a pid must fit in u32"
        );
    }
}
