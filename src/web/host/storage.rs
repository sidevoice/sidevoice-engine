//! The web build's storage: the Origin Private File System (OPFS), laid out as natively, as Hugging Face's hub cache.
//! Under one directory per engine (`sidevoice-engine/`):
//!
//! - `blobs/<name>`: each file once, under the name the installer gives it (its SHA-256). It is written under a
//!   temporary name beside it and moved to its own only when committed, so a stored name is always a whole file.
//! - `models/<build id>/<path>`: a build's folder, each file at its original path (`onnx/model_q8.onnx`). OPFS has no
//!   links, so each is a copy of its blob. A folder is made in `partial/<nonce>/`, and its files are moved into
//!   `models/` when it is committed.
//! - `folders.json`: which blobs each stored folder holds. Writing it is what commits a folder (it is replaced whole,
//!   by a move), and it answers [`Storage::find_folder`] and [`Storage::is_linked`]: a folder not in it is not stored,
//!   whatever files `models/` holds.
//!
//! Archives are unpacked natively only, so there are no trees here. The index is kept consistent within a page; two
//! pages of one origin installing at once are not coordinated.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use wasm_bindgen::JsValue;

use super::fetcher::StreamDownload;
use crate::web::opfs::{self, Directory, FileHandle, Writable};
use crate::{async_trait, Download, Error, FolderWriter, Result, Storage, StorageWriter};

#[cfg(test)]
mod tests;

/// The OPFS directory the engine keeps its files in, under the origin's root.
const DIRECTORY: &str = "sidevoice-engine";
const BLOBS: &str = "blobs";
const MODELS: &str = "models";
const PARTIAL: &str = "partial";
const INDEX: &str = "folders.json";

/// Which blobs each stored build folder holds, by build id: `folders.json`.
type Index = BTreeMap<String, BTreeSet<String>>;

/// Files in OPFS, under one directory (`sidevoice-engine` under the origin's root). A location is the path from the
/// OPFS root (`sidevoice-engine/blobs/<name>`, `sidevoice-engine/models/<build id>/<path>`), which `opfs::open` opens.
/// Fails with `storage-failed` where the page has no OPFS (Node, an old browser, a private window that refuses it) or
/// the medium fails, `storage-name-invalid` for a name or a path that breaks [`Storage`]'s rules, and
/// `archive-unsupported` for trees.
pub(super) struct WebStorage {
    directory: &'static str,
    /// The directory, once opened.
    opened: RefCell<Option<Directory>>,
    /// Held while `folders.json` is read and written again, so that two changes in this page do not lose one.
    index: Rc<async_lock::Mutex<()>>,
}

impl WebStorage {
    /// The engine's storage, in the `sidevoice-engine` directory.
    pub(super) fn new() -> Self {
        Self::in_directory(DIRECTORY)
    }

    /// Storage in the OPFS directory `directory` (tests keep theirs apart).
    pub(super) fn in_directory(directory: &'static str) -> Self {
        Self {
            directory,
            opened: RefCell::new(None),
            index: Rc::new(async_lock::Mutex::new(())),
        }
    }

    /// The directory, opened (and created) the first time it is needed.
    async fn directory(&self) -> Result<Directory> {
        if let Some(directory) = self.opened.borrow().clone() {
            return Ok(directory);
        }
        let root = opfs::root().await.map_err(failed)?;
        let directory = root.directory(self.directory).await.map_err(failed)?;
        *self.opened.borrow_mut() = Some(directory.clone());
        Ok(directory)
    }

    /// The subdirectory `name` of the engine's directory, created if it is not there.
    async fn part(&self, name: &str) -> Result<Directory> {
        self.directory()
            .await?
            .directory(name)
            .await
            .map_err(failed)
    }

    fn location(&self, path: &str) -> String {
        format!("{}/{path}", self.directory)
    }

    /// The stored folders, as `folders.json` lists them; none if it is not there yet.
    async fn index(&self) -> Result<Index> {
        read_index(&self.directory().await?).await
    }
}

#[async_trait(?Send)]
impl Storage for WebStorage {
    async fn find(&self, name: &str) -> Result<Option<String>> {
        check(name)?;
        let blobs = self.part(BLOBS).await?;
        let found = blobs.file(name, false).await.map_err(failed)?;
        Ok(found.map(|_| self.location(&format!("{BLOBS}/{name}"))))
    }

    async fn find_member(&self, _tree: &str, _path: &str) -> Result<Option<String>> {
        // Trees are unpacked archives, which the web build never installs (`install/plan.rs` refuses them first).
        Err(Error::new("archive-unsupported"))
    }

    async fn create(&self, name: &str) -> Result<Box<dyn StorageWriter>> {
        check(name)?;
        let directory = self.part(BLOBS).await?;
        let partial = partial_name(name);
        let file = directory
            .file(&partial, true)
            .await
            .map_err(failed)?
            .ok_or(Error::new("storage-failed"))?;
        let writable = file.writable().await.map_err(failed)?;
        Ok(Box::new(WebStorageWriter {
            open: Some(Open {
                directory,
                file,
                writable,
                partial,
            }),
            name: name.to_owned(),
            location: self.location(&format!("{BLOBS}/{name}")),
        }))
    }

    async fn read(&self, name: &str) -> Result<Box<dyn Download>> {
        check(name)?;
        let blobs = self.part(BLOBS).await?;
        let file = blobs.file(name, false).await.map_err(failed)?;
        let blob = file
            .ok_or(Error::new("storage-failed"))?
            .blob()
            .await
            .map_err(failed)?;
        Ok(Box::new(StreamDownload::of_blob(&blob)))
    }

    async fn remove(&self, name: &str) -> Result<()> {
        check(name)?;
        let blobs = self.part(BLOBS).await?;
        blobs.remove(name).await.map_err(failed)
    }

    async fn find_folder(&self, name: &str) -> Result<Option<String>> {
        check_folder(name)?;
        let stored = self.index().await?.contains_key(name);
        Ok(stored.then(|| self.location(&format!("{MODELS}/{name}"))))
    }

    async fn find_in_folder(&self, name: &str, path: &str) -> Result<Option<String>> {
        check_folder(name)?;
        check_path(path)?;
        if !self.index().await?.contains_key(name) {
            return Ok(None);
        }
        let (parents, file) = path.rsplit_once('/').unwrap_or(("", path));
        let models = self.part(MODELS).await?;
        let Some(directory) = models
            .at(&format!("{name}/{parents}"), false)
            .await
            .map_err(failed)?
        else {
            return Ok(None);
        };
        let there = directory.file(file, false).await.map_err(failed)?.is_some()
            || directory.existing(file).await.map_err(failed)?.is_some();
        Ok(there.then(|| self.location(&format!("{MODELS}/{name}/{path}"))))
    }

    async fn create_folder(&self, name: &str) -> Result<Box<dyn FolderWriter>> {
        check_folder(name)?;
        let staging = partial_name("folder");
        let partial = self.part(PARTIAL).await?;
        let staged = partial.directory(&staging).await.map_err(failed)?;
        Ok(Box::new(WebFolder {
            name: name.to_owned(),
            root: self.directory().await?,
            blobs: self.part(BLOBS).await?,
            partial,
            staging,
            staged,
            files: Vec::new(),
            linked: BTreeSet::new(),
            index: Rc::clone(&self.index),
            location: self.location(&format!("{MODELS}/{name}")),
            committed: false,
        }))
    }

    async fn remove_folder(&self, name: &str) -> Result<()> {
        check_folder(name)?;
        let root = self.directory().await?;
        {
            // Out of the index first: from then on the folder is not stored, whatever is left of its files.
            let _index = self.index.lock().await;
            let mut index = read_index(&root).await?;
            if index.remove(name).is_some() {
                write_index(&root, &index).await?;
            }
        }
        let models = self.part(MODELS).await?;
        let segments: Vec<&str> = name.split('/').collect();
        let (last, parents) = segments.split_last().expect("a checked name has a segment");
        let Some(parent) = models.at(&parents.join("/"), false).await.map_err(failed)? else {
            return Ok(());
        };
        parent.remove_recursively(last).await.map_err(failed)?;
        // The folders above it, left empty, go too.
        for depth in (1..segments.len()).rev() {
            let (above, below) = (segments[..depth - 1].join("/"), segments[depth - 1]);
            let Some(directory) = models.at(&above, false).await.map_err(failed)? else {
                break;
            };
            let Some(child) = directory.existing(below).await.map_err(failed)? else {
                break;
            };
            if !child.names().await.map_err(failed)?.is_empty() {
                break;
            }
            directory.remove(below).await.map_err(failed)?;
        }
        Ok(())
    }

    async fn is_linked(&self, name: &str) -> Result<bool> {
        check(name)?;
        let index = self.index().await?;
        Ok(index.values().any(|blobs| blobs.contains(name)))
    }
}

/// `folders.json` in `root`, read; empty if it is not there.
async fn read_index(root: &Directory) -> Result<Index> {
    let Some(file) = root.file(INDEX, false).await.map_err(failed)? else {
        return Ok(Index::new());
    };
    let blob = file.blob().await.map_err(failed)?;
    let text = wasm_bindgen_futures::JsFuture::from(blob.text())
        .await
        .map_err(failed)?;
    let text = text.as_string().ok_or(Error::new("storage-failed"))?;
    serde_json::from_str(&text).map_err(|error| {
        web_sys::console::warn_1(&format!("sidevoice-engine: {INDEX} unreadable: {error}").into());
        Error::new("storage-failed")
    })
}

/// Replaces `folders.json` in `root` with `index`, whole: written beside it, then moved over it.
async fn write_index(root: &Directory, index: &Index) -> Result<()> {
    let json = serde_json::to_vec(index).map_err(|_| Error::new("storage-failed"))?;
    let partial = partial_name("folders");
    let file = root
        .file(&partial, true)
        .await
        .map_err(failed)?
        .ok_or(Error::new("storage-failed"))?;
    let written = async {
        let writable = file.writable().await?;
        writable.append(&json).await?;
        writable.finish().await?;
        file.rename(root, INDEX).await
    }
    .await;
    if let Err(error) = written {
        let _ = root.remove(&partial).await;
        return Err(failed(error));
    }
    Ok(())
}

/// A file being written: to a temporary file beside where it goes, moved to its name when committed.
struct WebStorageWriter {
    /// What is open until it is committed or dropped.
    open: Option<Open>,
    name: String,
    location: String,
}

struct Open {
    directory: Directory,
    file: FileHandle,
    writable: Writable,
    partial: String,
}

#[async_trait(?Send)]
impl StorageWriter for WebStorageWriter {
    async fn write(&mut self, bytes: &[u8]) -> Result<()> {
        let open = self.open.as_ref().ok_or(Error::new("storage-failed"))?;
        open.writable.append(bytes).await.map_err(failed)
    }

    async fn commit(mut self: Box<Self>) -> Result<String> {
        let open = self.open.take().ok_or(Error::new("storage-failed"))?;
        let committed = async {
            open.writable.finish().await?;
            open.file.rename(&open.directory, &self.name).await
        }
        .await;
        if let Err(error) = committed {
            discard(open);
            return Err(failed(error));
        }
        Ok(std::mem::take(&mut self.location))
    }
}

impl Drop for WebStorageWriter {
    /// Dropped uncommitted: what was written is discarded, and the temporary file removed, once the page gets to it.
    fn drop(&mut self) {
        if let Some(open) = self.open.take() {
            discard(open);
        }
    }
}

/// Aborts the stream and removes the temporary file, in the background: dropping cannot wait.
fn discard(open: Open) {
    wasm_bindgen_futures::spawn_local(async move {
        // Either may fail once the other has (a closed stream cannot be aborted): the removal is what matters.
        let _ = open.writable.discard().await;
        let _ = open.directory.remove(&open.partial).await;
    });
}

/// A build folder being made: each file copied from its blob into `partial/<staging>/`, moved into `models/` and listed
/// in `folders.json` when committed, and the staging directory removed either way.
struct WebFolder {
    name: String,
    root: Directory,
    blobs: Directory,
    partial: Directory,
    staging: String,
    staged: Directory,
    /// Each file's path in the folder, in the order linked.
    files: Vec<String>,
    /// The blobs copied in.
    linked: BTreeSet<String>,
    index: Rc<async_lock::Mutex<()>>,
    location: String,
    committed: bool,
}

#[async_trait(?Send)]
impl FolderWriter for WebFolder {
    async fn link(&mut self, path: &str, blob: &str, member: Option<&str>) -> Result<()> {
        check(blob)?;
        check_path(path)?;
        if member.is_some() {
            return Err(Error::new("archive-unsupported"));
        }
        let source = self
            .blobs
            .file(blob, false)
            .await
            .map_err(failed)?
            .ok_or(Error::new("storage-failed"))?;
        let (parents, name) = path.rsplit_once('/').unwrap_or(("", path));
        let copied = async {
            let directory = self.staged.at(parents, true).await?.ok_or_else(missing)?;
            let file = directory.file(name, true).await?.ok_or_else(missing)?;
            let writable = file.writable().await?;
            writable.append_blob(&source.blob().await?).await?;
            writable.finish().await
        }
        .await;
        copied.map_err(failed)?;
        self.files.push(path.to_owned());
        self.linked.insert(blob.to_owned());
        Ok(())
    }

    async fn commit(mut self: Box<Self>) -> Result<String> {
        let models = self.root.directory(MODELS).await.map_err(failed)?;
        for path in &self.files {
            let (parents, name) = path.rsplit_once('/').unwrap_or(("", path));
            let moved = async {
                let from = self.staged.at(parents, false).await?.ok_or_else(missing)?;
                let file = from.file(name, false).await?.ok_or_else(missing)?;
                let to = models
                    .at(&format!("{}/{parents}", self.name), true)
                    .await?
                    .ok_or_else(missing)?;
                file.rename(&to, name).await
            }
            .await;
            moved.map_err(failed)?;
        }
        {
            let _index = self.index.lock().await;
            let mut index = read_index(&self.root).await?;
            // Another folder of this build stored meanwhile is kept: same build, same files.
            if !index.contains_key(&self.name) {
                index.insert(self.name.clone(), std::mem::take(&mut self.linked));
                write_index(&self.root, &index).await?;
            }
        }
        self.committed = true;
        let _ = self.partial.remove_recursively(&self.staging).await;
        Ok(std::mem::take(&mut self.location))
    }
}

impl Drop for WebFolder {
    /// Dropped uncommitted: the staging directory is removed, once the page gets to it. `models/` was never touched.
    fn drop(&mut self) {
        if !self.committed {
            let partial = self.partial.clone();
            let staging = std::mem::take(&mut self.staging);
            wasm_bindgen_futures::spawn_local(async move {
                let _ = partial.remove_recursively(&staging).await;
            });
        }
    }
}

/// The temporary name `name` is written under: never a valid name (it has a `.`), so `find` never sees it, and one
/// per writer, so two writers of one name never share it.
fn partial_name(name: &str) -> String {
    let nonce = (js_sys::Math::random() * f64::from(u32::MAX)) as u32;
    format!("{name}.partial-{nonce:08x}")
}

/// `storage-name-invalid` unless `name` is a blob's name [`Storage`] takes: ASCII letters, digits, `-` and `_`.
fn check(name: &str) -> Result<()> {
    let valid = !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
    if valid {
        Ok(())
    } else {
        Err(Error::new("storage-name-invalid"))
    }
}

/// `storage-name-invalid` unless `name` is a folder's name [`Storage`] takes: `/`-separated segments of ASCII
/// letters, digits, `.`, `-` and `_`, none of them `.` or `..`.
fn check_folder(name: &str) -> Result<()> {
    let valid = name.split('/').all(|segment| {
        !matches!(segment, "" | "." | "..")
            && segment
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    });
    if valid {
        Ok(())
    } else {
        Err(Error::new("storage-name-invalid"))
    }
}

/// `storage-name-invalid` unless `path` is a path inside a folder: relative, `/`-separated, without empty, `.` or
/// `..` segments.
fn check_path(path: &str) -> Result<()> {
    let valid = path
        .split('/')
        .all(|segment| !matches!(segment, "" | "." | "..") && !segment.contains('\\'));
    if valid {
        Ok(())
    } else {
        Err(Error::new("storage-name-invalid"))
    }
}

/// An entry this storage made is gone: the medium failed under it.
fn missing() -> JsValue {
    JsValue::from_str("an entry of the folder being made is missing")
}

/// The medium failed: `storage-failed`, with the cause in the console.
fn failed(error: JsValue) -> Error {
    web_sys::console::warn_2(&"sidevoice-engine: OPFS failed:".into(), &error);
    Error::new("storage-failed")
}
