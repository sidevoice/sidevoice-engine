//! The web build's storage: the Origin Private File System (OPFS), one directory per engine, each file stored under its
//! name as the installer gives it (its SHA-256, content-addressed, as natively). A file is written under a temporary
//! name and moved to its own only when committed, so a stored name is always a whole file; dropped before that, the
//! temporary file is discarded. Archives are unpacked natively only, so there are no trees here.

use std::cell::RefCell;

use wasm_bindgen::JsValue;

use super::fetcher::StreamDownload;
use crate::web::opfs::{self, Directory, FileHandle, Writable};
use crate::{async_trait, Download, Error, Result, Storage, StorageWriter};

#[cfg(test)]
mod tests;

/// The OPFS directory the engine keeps its files in, under the origin's root.
const DIRECTORY: &str = "sidevoice-engine";

/// Files in OPFS, in one directory (`sidevoice-engine` under the origin's root). A location is the path from the OPFS
/// root (`sidevoice-engine/<name>`), which `opfs::open` opens. Fails with `storage-failed` where the page has no OPFS
/// (Node, an old browser, a private window that refuses it) or the medium fails, and `storage-name-invalid` for a
/// name that breaks [`Storage`]'s rules.
pub(super) struct WebStorage {
    directory: &'static str,
    /// The directory, once opened.
    opened: RefCell<Option<Directory>>,
}

impl WebStorage {
    /// The engine's storage, in the `sidevoice-engine` directory.
    pub(super) const fn new() -> Self {
        Self::in_directory(DIRECTORY)
    }

    /// Storage in the OPFS directory `directory` (tests keep theirs apart).
    pub(super) const fn in_directory(directory: &'static str) -> Self {
        Self {
            directory,
            opened: RefCell::new(None),
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

    fn location(&self, name: &str) -> String {
        format!("{}/{name}", self.directory)
    }
}

#[async_trait(?Send)]
impl Storage for WebStorage {
    async fn find(&self, name: &str) -> Result<Option<String>> {
        check(name)?;
        let directory = self.directory().await?;
        let found = directory.file(name, false).await.map_err(failed)?;
        Ok(found.map(|_| self.location(name)))
    }

    async fn find_member(&self, _tree: &str, _path: &str) -> Result<Option<String>> {
        // Trees are unpacked archives, which the web build never installs (`install/plan.rs` refuses them first).
        Err(Error::new("archive-unsupported"))
    }

    async fn create(&self, name: &str) -> Result<Box<dyn StorageWriter>> {
        check(name)?;
        let directory = self.directory().await?;
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
            location: self.location(name),
        }))
    }

    async fn read(&self, name: &str) -> Result<Box<dyn Download>> {
        check(name)?;
        let directory = self.directory().await?;
        let file = directory.file(name, false).await.map_err(failed)?;
        let blob = file
            .ok_or(Error::new("storage-failed"))?
            .blob()
            .await
            .map_err(failed)?;
        Ok(Box::new(StreamDownload::of_blob(&blob)))
    }

    async fn remove(&self, name: &str) -> Result<()> {
        check(name)?;
        let directory = self.directory().await?;
        directory.remove(name).await.map_err(failed)
    }
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

/// The temporary name `name` is written under: never a valid name (it has a `.`), so `find` never sees it, and one
/// per writer, so two writers of one name never share it.
fn partial_name(name: &str) -> String {
    let nonce = (js_sys::Math::random() * f64::from(u32::MAX)) as u32;
    format!("{name}.partial-{nonce:08x}")
}

/// `storage-name-invalid` unless `name` is a name [`Storage`] takes: ASCII letters, digits, `-` and `_`.
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

/// The medium failed: `storage-failed`, with the cause in the console.
fn failed(error: JsValue) -> Error {
    web_sys::console::warn_2(&"sidevoice-engine: OPFS failed:".into(), &error);
    Error::new("storage-failed")
}
