//! The native host's storage: a directory. Stored files and trees are `files/<name>`; one being written is
//! `partial/<name>.<process>.<n>` until it is committed, which renames it into place (atomic on one file system), and
//! removed if it is dropped first. File operations are short and blocking; the long wait, the network, is elsewhere.

use std::fs::{self, File};
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{async_trait, Download, Error, Result, Storage, StorageWriter, TreeWriter};

/// The size of the parts a stored file is read back in.
const PART: usize = 64 * 1024;

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
            Err(invalid())
        }
    }

    /// A fresh place in `partial/` for what will be stored as `name`.
    fn partial(&self, name: &str) -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        self.partial
            .join(format!("{name}.{}.{n}", std::process::id()))
    }
}

#[async_trait]
impl Storage for Directory {
    async fn find(&self, name: &str) -> Result<Option<String>> {
        let path = self.path(name)?;
        Ok(path.exists().then(|| text(&path)))
    }

    async fn find_member(&self, tree: &str, path: &str) -> Result<Option<String>> {
        let member = inside(&self.path(tree)?, path)?;
        Ok(member.exists().then(|| text(&member)))
    }

    async fn create(&self, name: &str) -> Result<Box<dyn StorageWriter>> {
        let target = self.path(name)?;
        let partial = self.partial(name);
        let file = File::create(&partial).map_err(|_| failed())?;
        Ok(Box::new(Writer {
            file: Some(file),
            partial,
            target,
        }))
    }

    async fn read(&self, name: &str) -> Result<Box<dyn Download>> {
        let file = File::open(self.path(name)?).map_err(|_| failed())?;
        let size = file.metadata().map_err(|_| failed())?.len();
        Ok(Box::new(Reader { file, size }))
    }

    async fn remove(&self, name: &str) -> Result<()> {
        let path = self.path(name)?;
        let removed = if path.is_dir() {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_file(&path)
        };
        match removed {
            Err(error) if error.kind() != ErrorKind::NotFound => Err(failed()),
            _ => Ok(()),
        }
    }

    fn open(&self, name: &str) -> Result<Box<dyn Read + Send>> {
        Ok(Box::new(
            File::open(self.path(name)?).map_err(|_| failed())?,
        ))
    }

    fn create_tree(&self, name: &str) -> Result<Box<dyn TreeWriter>> {
        let target = self.path(name)?;
        let partial = self.partial(name);
        fs::create_dir(&partial).map_err(|_| failed())?;
        Ok(Box::new(Tree {
            partial,
            target,
            file: None,
            committed: false,
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

/// A tree being written into `partial/`.
struct Tree {
    partial: PathBuf,
    target: PathBuf,
    /// The file started last.
    file: Option<File>,
    committed: bool,
}

impl TreeWriter for Tree {
    fn directory(&mut self, path: &str) -> Result<()> {
        self.file = None;
        fs::create_dir_all(inside(&self.partial, path)?).map_err(|_| failed())
    }

    fn file(&mut self, path: &str) -> Result<()> {
        self.file = None;
        let path = inside(&self.partial, path)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|_| failed())?;
        }
        self.file = Some(File::create(path).map_err(|_| failed())?);
        Ok(())
    }

    fn write(&mut self, bytes: &[u8]) -> Result<()> {
        let file = self.file.as_mut().ok_or_else(failed)?;
        file.write_all(bytes).map_err(|_| failed())
    }

    fn commit(mut self: Box<Self>) -> Result<String> {
        if let Some(file) = self.file.take() {
            file.sync_all().map_err(|_| failed())?;
        }
        if fs::rename(&self.partial, &self.target).is_err() && !self.target.is_dir() {
            return Err(failed());
        }
        // Renamed, or another one was stored first (same name, same content): either way this one is done.
        self.committed = true;
        let _ = fs::remove_dir_all(&self.partial);
        Ok(text(&self.target))
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        if !self.committed {
            self.file = None;
            let _ = fs::remove_dir_all(&self.partial);
        }
    }
}

/// A stored file being read back.
struct Reader {
    file: File,
    size: u64,
}

#[async_trait]
impl Download for Reader {
    fn size(&self) -> Option<u64> {
        Some(self.size)
    }

    async fn chunk(&mut self) -> Result<Option<Vec<u8>>> {
        let mut part = vec![0; PART];
        loop {
            match self.file.read(&mut part) {
                Ok(0) => return Ok(None),
                Ok(read) => {
                    part.truncate(read);
                    return Ok(Some(part));
                }
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(_) => return Err(failed()),
            }
        }
    }
}

/// `path`, relative and `/`-separated, inside `root`; checked again here, whatever the caller checked.
fn inside(root: &Path, path: &str) -> Result<PathBuf> {
    let mut joined = root.to_path_buf();
    for segment in path.split('/') {
        let plain = !matches!(segment, "" | "." | "..")
            && !segment
                .chars()
                .any(|c| c == '\\' || c == ':' || c.is_control());
        if !plain {
            return Err(invalid());
        }
        joined.push(segment);
    }
    Ok(joined)
}

/// `path` as text: always UTF-8, since the root is (checked in [`Directory::new`]), names are ASCII and member paths
/// are `str`.
fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn failed() -> Error {
    Error::new("storage-failed")
}

fn invalid() -> Error {
    Error::new("storage-name-invalid")
}
