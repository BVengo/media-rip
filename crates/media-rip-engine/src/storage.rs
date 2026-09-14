//! Owned staging directories and atomic publication on the destination filesystem.
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_STAGING_ID: AtomicU64 = AtomicU64::new(0);

pub(crate) struct StagingDirectory(PathBuf);

impl StagingDirectory {
    pub(crate) fn new(parent: &Path) -> io::Result<Self> {
        for _ in 0..100 {
            let id = NEXT_STAGING_ID.fetch_add(1, Ordering::Relaxed);
            let path = parent.join(format!(".mediarip-{}-{id}", std::process::id()));
            let mut builder = fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not reserve a staging directory",
        ))
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }

    /// The staged file must be closed by its producer before publication.
    pub(crate) fn publish(
        &self,
        staged: &Path,
        destination: &Path,
        overwrite: bool,
    ) -> io::Result<()> {
        File::open(staged)?.sync_all()?;
        if overwrite {
            // Atomic replacement preserves the old file if publication fails.
            fs::rename(staged, destination)?;
        } else {
            // Unlike exists() + rename(), link creation cannot replace a file
            // created by another writer between preflight and publication.
            fs::hard_link(staged, destination)?;
        }
        File::open(destination.parent().unwrap_or(Path::new(".")))?.sync_all()
    }
}

impl Drop for StagingDirectory {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            log::warn!(
                "Could not remove staging directory {}: {error}",
                self.0.display()
            );
        }
    }
}

/// Save settings or cache data without exposing partial writes. Files are private
/// from creation, including when the data contains provider credentials.
pub fn atomic_write(path: &Path, data: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let staging = StagingDirectory::new(parent)?;
    let staged = staging.path().join("data");
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&staged)?;
    file.write_all(data)?;
    file.sync_all()?;
    drop(file);
    staging.publish(&staged, path, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publication_never_clobbers_a_concurrent_writer() {
        let directory = StagingDirectory::new(&std::env::temp_dir()).unwrap();
        let staging = StagingDirectory::new(directory.path()).unwrap();
        let staged = staging.path().join("output");
        let destination = directory.path().join("final");
        fs::write(&staged, b"new").unwrap();
        fs::write(&destination, b"existing").unwrap();
        assert_eq!(
            staging
                .publish(&staged, &destination, false)
                .unwrap_err()
                .kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read(&destination).unwrap(), b"existing");
        staging.publish(&staged, &destination, true).unwrap();
        assert_eq!(fs::read(destination).unwrap(), b"new");
    }

    #[test]
    fn failed_or_cancelled_work_is_removed_on_drop() {
        let directory = StagingDirectory::new(&std::env::temp_dir()).unwrap();
        let path = directory.path().to_owned();
        fs::write(path.join("partial"), b"incomplete").unwrap();
        drop(directory);
        assert!(!path.exists());
    }

    #[test]
    fn concurrent_settings_writes_publish_complete_values() {
        let directory = StagingDirectory::new(&std::env::temp_dir()).unwrap();
        let path = directory.path().join("settings");
        std::thread::scope(|scope| {
            for byte in 0..8u8 {
                let path = &path;
                scope.spawn(move || atomic_write(path, &vec![byte; 8192]).unwrap());
            }
        });
        let data = fs::read(&path).unwrap();
        assert_eq!(data.len(), 8192);
        assert!(data.iter().all(|byte| *byte == data[0]));
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
}
