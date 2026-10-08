//! The native host's storage: a directory, laid out as Hugging Face's hub cache. Blobs (stored files and trees) are
//! `blobs/<name>`; build folders are `models/<build id>/`, each file in them a hard link to its blob (or, where the
//! file system refuses the link, a copy). One being written is `partial/<name>.<process>.<n>` until it is committed,
//! which renames it into place (atomic on one file system), and removed if it is dropped first. File operations are
//! short and blocking; the long wait, the network, is elsewhere.

use std::fs::{self, File};
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{
    async_trait, Download, Error, FolderWriter, Result, Storage, StorageWriter, TreeWriter,
};

/// The size of the parts a stored file is read back in.
const PART: usize = 64 * 1024;

/// Files kept in a directory.
#[derive(Debug)]
pub(super) struct Directory {
    blobs: PathBuf,
    models: PathBuf,
    partial: PathBuf,
}

impl Directory {
    /// The storage in `root`, whose three subdirectories are created if they do not exist.
    pub(super) fn new(root: PathBuf) -> Result<Self> {
        if root.to_str().is_none() {
            return Err(Error::new("storage-path-not-utf8"));
        }
        let storage = Self {
            blobs: root.join("blobs"),
            models: root.join("models"),
            partial: root.join("partial"),
        };
        for dir in [&storage.blobs, &storage.models, &storage.partial] {
            fs::create_dir_all(dir).map_err(|_| failed())?;
        }
        Ok(storage)
    }

    /// Where the blob `name` is stored, if it is a valid name.
    fn path(&self, name: &str) -> Result<PathBuf> {
        blob(&self.blobs, name)
    }

    /// Where the build folder `name` is stored, if it is a valid name: one directory per segment of the build id.
    fn folder(&self, name: &str) -> Result<PathBuf> {
        let mut path = self.models.clone();
        for segment in name.split('/') {
            let valid = !matches!(segment, "" | "." | "..")
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'));
            if !valid {
                return Err(invalid());
            }
            path.push(segment);
        }
        Ok(path)
    }

    /// A fresh place in `partial/` for what will be stored as `name`.
    fn partial(&self, name: &str) -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        self.partial.join(format!(
            "{}.{}.{n}",
            name.replace('/', "+"),
            std::process::id()
        ))
    }
}

/// Where the blob `name` is stored under `blobs`, if it is a valid name.
fn blob(blobs: &Path, name: &str) -> Result<PathBuf> {
    let valid = !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
    if valid {
        Ok(blobs.join(name))
    } else {
        Err(invalid())
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

    async fn find_folder(&self, name: &str) -> Result<Option<String>> {
        let path = self.folder(name)?;
        Ok(path.is_dir().then(|| text(&path)))
    }

    async fn find_in_folder(&self, name: &str, path: &str) -> Result<Option<String>> {
        let member = inside(&self.folder(name)?, path)?;
        Ok(member.exists().then(|| text(&member)))
    }

    async fn create_folder(&self, name: &str) -> Result<Box<dyn FolderWriter>> {
        let target = self.folder(name)?;
        let partial = self.partial(name);
        fs::create_dir(&partial).map_err(|_| failed())?;
        Ok(Box::new(Folder {
            blobs: self.blobs.clone(),
            partial,
            target,
            committed: false,
        }))
    }

    async fn remove_folder(&self, name: &str) -> Result<()> {
        let path = self.folder(name)?;
        match fs::remove_dir_all(&path) {
            Err(error) if error.kind() != ErrorKind::NotFound => return Err(failed()),
            _ => {}
        }
        // The model's directory, once its last build is gone: `remove_dir` refuses one that is not empty.
        let mut parent = path.parent();
        while let Some(dir) = parent.filter(|dir| *dir != self.models) {
            if fs::remove_dir(dir).is_err() {
                break;
            }
            parent = dir.parent();
        }
        Ok(())
    }

    async fn is_linked(&self, name: &str) -> Result<bool> {
        let path = self.path(name)?;
        Ok(path.exists() && linked(&path))
    }
}

/// Whether a file at or below `path` has another hard link: a build folder's. Where the platform does not say (only
/// Unix exposes the count in stable Rust), it is taken as linked, so that a blob is kept rather than lost.
fn linked(path: &Path) -> bool {
    if path.is_dir() {
        return fs::read_dir(path)
            .into_iter()
            .flatten()
            .flatten()
            .any(|entry| linked(&entry.path()));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        fs::metadata(path).is_ok_and(|meta| meta.nlink() > 1)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// A build folder being made in `partial/`.
struct Folder {
    blobs: PathBuf,
    partial: PathBuf,
    target: PathBuf,
    committed: bool,
}

#[async_trait]
impl FolderWriter for Folder {
    async fn link(&mut self, path: &str, blob_name: &str, member: Option<&str>) -> Result<()> {
        let mut source = blob(&self.blobs, blob_name)?;
        if let Some(member) = member {
            source = inside(&source, member)?;
        }
        let target = inside(&self.partial, path)?;
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|_| failed())?;
        }
        place(&source, &target)
    }

    async fn commit(mut self: Box<Self>) -> Result<String> {
        if let Some(parent) = self.target.parent() {
            fs::create_dir_all(parent).map_err(|_| failed())?;
        }
        if fs::rename(&self.partial, &self.target).is_err() && !self.target.is_dir() {
            return Err(failed());
        }
        // Renamed, or another one was stored first (same build, same files): either way this one is done.
        self.committed = true;
        let _ = fs::remove_dir_all(&self.partial);
        Ok(text(&self.target))
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_dir_all(&self.partial);
        }
    }
}

/// `source`, a file or a directory, at `target`: each file a hard link to it, or a copy where linking fails.
fn place(source: &Path, target: &Path) -> Result<()> {
    if source.is_dir() {
        fs::create_dir_all(target).map_err(|_| failed())?;
        for entry in fs::read_dir(source).map_err(|_| failed())? {
            let entry = entry.map_err(|_| failed())?;
            place(&entry.path(), &target.join(entry.file_name()))?;
        }
        return Ok(());
    }
    if !source.is_file() {
        return Err(failed());
    }
    if fs::hard_link(source, target).is_err() {
        fs::copy(source, target).map_err(|_| failed())?;
    }
    Ok(())
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
