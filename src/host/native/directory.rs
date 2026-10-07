//! The native host's storage: a directory. Stored files are `files/<name>`; a file being written is
//! `partial/<name>.<process>.<n>` until it is committed, which renames it into place (atomic on one file system), and
//! removed if it is dropped first. File operations are short and blocking; the long wait, the network, is elsewhere.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{async_trait, Error, Result, Storage, StorageWriter};

/// Files kept in a directory.
#[derive(Debug)]
pub(super) struct Directory {
    files: PathBuf,
    partial: PathBuf,
}

impl Directory {
    /// The storage in `root`, whose two subdirectories are created if they do not exist.
    pub(super) fn new(root: PathBuf) -> Result<Self> {
        if root.to_str().is_none() {
            return Err(Error::new("storage-path-not-utf8"));
        }
        let storage = Self {
            files: root.join("files"),
            partial: root.join("partial"),
        };
        for dir in [&storage.files, &storage.partial] {
            fs::create_dir_all(dir).map_err(|_| failed())?;
        }
        Ok(storage)
    }

    /// Where `name` is stored, if it is a valid name.
    fn path(&self, name: &str) -> Result<PathBuf> {
        let valid = !name.is_empty()
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
        if valid {
            Ok(self.files.join(name))
        } else {
            Err(Error::new("storage-name-invalid"))
        }
    }
}

#[async_trait]
impl Storage for Directory {
    async fn find(&self, name: &str) -> Result<Option<String>> {
        let path = self.path(name)?;
        Ok(path.is_file().then(|| text(&path)))
    }

    async fn create(&self, name: &str) -> Result<Box<dyn StorageWriter>> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let target = self.path(name)?;
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let partial = self
            .partial
            .join(format!("{name}.{}.{n}", std::process::id()));
        let file = File::create(&partial).map_err(|_| failed())?;
        Ok(Box::new(Writer {
            file: Some(file),
            partial,
            target,
        }))
    }
}

/// A file being written into `partial/`.
struct Writer {
    /// `None` once committed.
    file: Option<File>,
    partial: PathBuf,
    target: PathBuf,
}

#[async_trait]
impl StorageWriter for Writer {
    async fn write(&mut self, bytes: &[u8]) -> Result<()> {
        let file = self.file.as_mut().ok_or_else(failed)?;
        file.write_all(bytes).map_err(|_| failed())
    }

    async fn commit(mut self: Box<Self>) -> Result<String> {
        let file = self.file.take().ok_or_else(failed)?;
        let synced = file.sync_all();
        drop(file);
        if synced
            .and_then(|()| fs::rename(&self.partial, &self.target))
            .is_err()
        {
            let _ = fs::remove_file(&self.partial);
            return Err(failed());
        }
        Ok(text(&self.target))
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        if self.file.take().is_some() {
            let _ = fs::remove_file(&self.partial);
        }
    }
}

/// `path` as text: always UTF-8, since the root is (checked in [`Directory::new`]) and names are ASCII.
fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn failed() -> Error {
    Error::new("storage-failed")
}
