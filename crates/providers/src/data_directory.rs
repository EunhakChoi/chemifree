//! A [`DataSource`] over one explicitly configured directory. It reads only the paths it is
//! asked for, never searches, and never executes anything.

use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use chemifree_application::{
    ActiveDataError, DataRegistry, DataSource, SourceError, TrustedRegistry,
};
use chemifree_domain::data;

/// Reads an explicitly selected trusted descriptor and DATA root.
#[must_use]
pub fn configure_selected_registry(root: Option<&Path>, descriptor: Option<&Path>) -> DataRegistry {
    match (root, descriptor) {
        (None, None) => DataRegistry::not_configured(),
        (Some(root), Some(descriptor)) => load_selected_registry(root, descriptor),
        _ => DataRegistry::refused(ActiveDataError::RegistryConfigurationIncomplete),
    }
}

/// Reads an explicitly selected trusted descriptor and DATA root.
#[must_use]
pub fn load_selected_registry(
    root: impl Into<PathBuf>,
    descriptor: impl AsRef<Path>,
) -> DataRegistry {
    let Ok(file) = open_descriptor(descriptor.as_ref()) else {
        return DataRegistry::refused(ActiveDataError::RegistryUnavailable);
    };
    let Ok(metadata) = file.metadata() else {
        return DataRegistry::refused(ActiveDataError::RegistryUnavailable);
    };
    if !metadata.is_file() || metadata.len() > chemifree_application::MAX_MANIFEST_BYTES {
        return DataRegistry::refused(ActiveDataError::RegistryUnavailable);
    }
    let mut bytes = Vec::new();
    if file
        .take(chemifree_application::MAX_MANIFEST_BYTES + 1)
        .read_to_end(&mut bytes)
        .is_err()
        || bytes.len() as u64 > chemifree_application::MAX_MANIFEST_BYTES
    {
        return DataRegistry::refused(ActiveDataError::RegistryUnavailable);
    }
    let Ok(registry) = TrustedRegistry::parse(&bytes) else {
        return DataRegistry::refused(ActiveDataError::RegistryInvalid);
    };
    let source = DirectoryDataSource::new(root).with_root_binding(registry.root_binding_id());
    registry.load(&source)
}

#[cfg(target_os = "linux")]
fn open_descriptor(path: &Path) -> io::Result<File> {
    use rustix::fs::{Mode, OFlags, open};
    let fd = open(
        path,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NONBLOCK,
        Mode::empty(),
    )
    .map_err(|error| io::Error::from_raw_os_error(error.raw_os_error()))?;
    Ok(fd.into())
}

#[cfg(not(target_os = "linux"))]
fn open_descriptor(_path: &Path) -> io::Result<File> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

/// Reads files below one data directory, refusing any path that resolves outside it (through
/// `..` or a symbolic link). The directory is never discovered: the host names it.
#[derive(Clone)]
pub struct DirectoryDataSource {
    root: PathBuf,
    root_binding_id: Option<String>,
    trusted_public_manifest_sha256: Option<String>,
    opened_root: Arc<OnceLock<Result<File, SourceError>>>,
}

impl DirectoryDataSource {
    /// Construction performs no I/O.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            root_binding_id: None,
            trusted_public_manifest_sha256: None,
            opened_root: Arc::new(OnceLock::new()),
        }
    }

    /// Associates an explicitly selected directory with an identity from a trusted registry.
    /// The caller is responsible for binding that identity to this specific local root.
    #[must_use]
    pub fn with_root_binding(mut self, id: impl Into<String>) -> Self {
        self.root_binding_id = Some(id.into());
        self
    }

    /// Binds a public fixture manifest to an exact hash obtained from a trusted authority.
    #[must_use]
    pub fn with_trusted_public_manifest(mut self, sha256: impl Into<String>) -> Self {
        self.trusted_public_manifest_sha256 = Some(sha256.into());
        self
    }
}

impl DataSource for DirectoryDataSource {
    fn trusted_public_manifest_sha256(&self) -> Option<&str> {
        self.trusted_public_manifest_sha256.as_deref()
    }

    fn root_binding_id(&self) -> Option<&str> {
        self.root_binding_id.as_deref()
    }

    fn read(&self, path: &str, limit: u64) -> Result<Vec<u8>, SourceError> {
        self.read_with_hook(path, limit, || {})
    }
}

impl DirectoryDataSource {
    fn read_with_hook(
        &self,
        path: &str,
        limit: u64,
        after_root_open: impl FnOnce(),
    ) -> Result<Vec<u8>, SourceError> {
        let root = self
            .opened_root
            .get_or_init(|| {
                let root = open_root(&self.root)?;
                if !root
                    .metadata()
                    .map_err(|_| SourceError::RootUnusable)?
                    .is_dir()
                {
                    return Err(SourceError::RootUnusable);
                }
                Ok(root)
            })
            .as_ref()
            .map_err(Clone::clone)?;
        // The application checks paths first; this keeps the guarantee for any caller.
        if data::check_relative_path(path).is_err() {
            return Err(SourceError::Escapes);
        }
        // Open relative to the already-open directory, with containment enforced atomically by
        // the kernel. A canonicalize-then-open check can race with a replaced symbolic link.
        after_root_open();
        let file = open_beneath(root, path)?;
        let metadata = file.metadata().map_err(|error| io_error(&error))?;
        if !metadata.is_file() {
            return Err(SourceError::NotAFile);
        }
        if metadata.len() > limit {
            return Err(SourceError::TooLarge);
        }
        // The file may grow after the size check; the read stops one byte past the limit.
        let mut bytes = Vec::new();
        file.take(limit.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|error| io_error(&error))?;
        if bytes.len() as u64 > limit {
            return Err(SourceError::TooLarge);
        }
        Ok(bytes)
    }
}

#[cfg(target_os = "linux")]
fn open_root(path: &Path) -> Result<File, SourceError> {
    use rustix::fs::{Mode, OFlags, open};
    let fd = open(
        path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NONBLOCK,
        Mode::empty(),
    )
    .map_err(|_| SourceError::RootUnusable)?;
    Ok(fd.into())
}

#[cfg(not(target_os = "linux"))]
fn open_root(_path: &Path) -> Result<File, SourceError> {
    Err(SourceError::RootUnusable)
}

#[cfg(target_os = "linux")]
fn open_beneath(root: &File, path: &str) -> Result<File, SourceError> {
    use rustix::fs::{Mode, OFlags, ResolveFlags, openat2};
    use rustix::io::Errno;
    let fd = openat2(
        root,
        path,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NONBLOCK,
        Mode::empty(),
        ResolveFlags::BENEATH | ResolveFlags::NO_MAGICLINKS,
    )
    .map_err(|error| match error {
        Errno::XDEV | Errno::LOOP => SourceError::Escapes,
        Errno::NOENT | Errno::NOTDIR => SourceError::Missing,
        _ => io_error(&io::Error::from_raw_os_error(error.raw_os_error())),
    })?;
    Ok(fd.into())
}

#[cfg(not(target_os = "linux"))]
fn open_beneath(_root: &File, _path: &str) -> Result<File, SourceError> {
    // Until the platform has an equivalent atomic containment implementation, fail closed.
    Err(SourceError::Io(
        "atomic directory containment is unsupported on this platform".into(),
    ))
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn a_link_swapped_after_validation_cannot_read_an_outside_file() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let target = root.path().join("value.json");
        fs::write(&target, b"inside").unwrap();
        fs::write(outside.path().join("value.json"), b"outside").unwrap();
        let source = DirectoryDataSource::new(root.path());
        let result = source.read_with_hook("value.json", 100, || {
            fs::remove_file(&target).unwrap();
            std::os::unix::fs::symlink(outside.path().join("value.json"), &target).unwrap();
        });
        assert!(matches!(result, Err(SourceError::Escapes)));
    }
}

/// An I/O failure, described without the path: statuses never show where data lives.
fn io_error(error: &io::Error) -> SourceError {
    SourceError::Io(error.kind().to_string())
}
