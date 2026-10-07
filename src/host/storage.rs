//! Where a host keeps files: by name, whole or not at all. A stored name is a file or a tree (a directory of files, an
//! unpacked archive). Storage does not know what a file is: the installer names each one after its digest
//! (content-addressed), and a name is only ever stored once its bytes have been checked.

use async_trait::async_trait;

use crate::host::Download;
use crate::maybe_send::{MaybeSend, MaybeSync};
use crate::Result;

/// Where engine packages and models are kept, each file or tree under a name. Implemented with
/// [`async_trait`](crate::async_trait), as [`Host`](crate::Host) shows.
///
/// Names are made of ASCII letters, digits, `-` and `_` (the installer uses lowercase hex digests). A path inside a
/// tree is relative and `/`-separated, without empty, `.` or `..` segments (the installer checks it before it gets
/// here). What is stored is stored whole or not at all: what [`Storage::create`] or [`Storage::create_tree`] writes is
/// only stored under its name when it is committed. Errors are stable codes: `storage-failed` when the medium fails,
/// `storage-name-invalid` for a name or a path that breaks the rules above.
#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
pub trait Storage: MaybeSend + MaybeSync {
    /// Where the file or tree stored as `name` is kept (a path, an OPFS name, ...), if it is stored, complete.
    async fn find(&self, name: &str) -> Result<Option<String>>;

    /// Where the file or directory at `path` inside the tree stored as `tree` is kept, if both exist.
    async fn find_member(&self, tree: &str, path: &str) -> Result<Option<String>>;

    /// A new file to be stored as `name`, written in parts. Nothing is stored as `name` until it is
    /// [committed](StorageWriter::commit); dropped before that, what was written is discarded.
    async fn create(&self, name: &str) -> Result<Box<dyn StorageWriter>>;

    /// A new tree to be stored as `name`, written a file at a time. Nothing is stored as `name` until it is
    /// [committed](TreeWriter::commit); dropped before that, what was written is discarded.
    async fn create_tree(&self, name: &str) -> Result<Box<dyn TreeWriter>>;

    /// The file stored as `name`, a part at a time.
    async fn read(&self, name: &str) -> Result<Box<dyn Download>>;

    /// Removes the file or tree stored as `name`; nothing happens if there is none.
    async fn remove(&self, name: &str) -> Result<()>;
}

/// A file being written to [`Storage`], stored under its name only when committed.
#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
pub trait StorageWriter: MaybeSend {
    /// Appends `bytes`.
    async fn write(&mut self, bytes: &[u8]) -> Result<()>;

    /// Stores what was written under its name, replacing a file already stored there, and says where it is kept, as
    /// [`Storage::find`] would.
    async fn commit(self: Box<Self>) -> Result<String>;
}

/// A tree being written to [`Storage`], stored under its name only when committed. Paths follow the rules of
/// [`Storage`]; parent directories are created as needed.
#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
pub trait TreeWriter: MaybeSend {
    /// Creates the directory at `path`.
    async fn directory(&mut self, path: &str) -> Result<()>;

    /// Starts the file at `path`, empty: what [`TreeWriter::write`] writes goes into it, until the next file.
    async fn file(&mut self, path: &str) -> Result<()>;

    /// Appends `bytes` to the file started last.
    async fn write(&mut self, bytes: &[u8]) -> Result<()>;

    /// Stores the tree under its name and says where it is kept, as [`Storage::find`] would. If another tree was
    /// stored under that name meanwhile, that one is kept: same name, same content.
    async fn commit(self: Box<Self>) -> Result<String>;
}
