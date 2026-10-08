//! Where a host keeps files, in the layout of Hugging Face's hub cache: blobs, each stored once under a name the
//! installer gives it (its digest), and build folders (`models/<build id>/`), where each of a build's files sits under
//! its original name, as a link to its blob. A blob is a file or, in a native build, a tree (an unpacked archive).
//! Storage does not know what a file is, and a name is only ever stored once its bytes have been checked.

use async_trait::async_trait;

use crate::host::Download;
use crate::maybe_send::{MaybeSend, MaybeSync};
use crate::Result;

/// Where engine packages and models are kept: blobs, each file or tree under a name, and build folders made of links
/// to them. Implemented with [`async_trait`](crate::async_trait), as [`Host`](crate::Host) shows.
///
/// A blob's name is made of ASCII letters, digits, `-` and `_` (the installer uses lowercase hex digests). A folder's
/// name is a build id: `/`-separated segments of ASCII letters, digits, `.`, `-` and `_`, none of them `.` or `..`.
/// A path inside a tree or a folder is relative and `/`-separated, without empty, `.` or `..` segments (the installer
/// checks it before it gets here). What is stored is stored whole or not at all: what [`Storage::create`],
/// `Storage::create_tree` or [`Storage::create_folder`] writes is only stored under its name when it is committed.
/// Errors are stable codes: `storage-failed` when the medium fails, `storage-name-invalid` for a name or a path that
/// breaks the rules above.
///
/// Trees are unpacked archives, which only native builds install: in a native build
/// (`#[cfg(not(target_arch = "wasm32"))]`), a storage also opens a stored file for reading and creates a tree, both
/// synchronously, because the installer unpacks on a blocking thread of its own.
///
/// A folder holds each file as a link to its blob where the medium has links (natively, a hard link: no privilege
/// needed, on one volume), so a file shared by two builds is kept once; where it has none (the browser's OPFS), or
/// where linking fails, as a copy, which costs the space twice but works the same.
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

    /// The file stored as `name`, a part at a time.
    async fn read(&self, name: &str) -> Result<Box<dyn Download>>;

    /// Removes the file or tree stored as `name`; nothing happens if there is none.
    async fn remove(&self, name: &str) -> Result<()>;

    /// The file stored as `name`, read synchronously.
    #[cfg(native)]
    fn open(&self, name: &str) -> Result<Box<dyn std::io::Read + Send>>;

    /// A new tree to be stored as `name`, written a file at a time, synchronously. Nothing is stored as `name` until
    /// it is [committed](TreeWriter::commit); dropped before that, what was written is discarded.
    #[cfg(native)]
    fn create_tree(&self, name: &str) -> Result<Box<dyn TreeWriter>>;

    /// Where the build folder `name` is kept, if it is stored, complete.
    async fn find_folder(&self, name: &str) -> Result<Option<String>>;

    /// Where the file or directory at `path` inside the build folder `name` is kept, if both exist.
    async fn find_in_folder(&self, name: &str, path: &str) -> Result<Option<String>>;

    /// A new build folder to be stored as `name`, made of links to stored blobs. Nothing is stored as `name` until it
    /// is [committed](FolderWriter::commit); dropped before that, what was linked is discarded (the blobs stay).
    async fn create_folder(&self, name: &str) -> Result<Box<dyn FolderWriter>>;

    /// Removes the build folder `name`, and the folders above it left empty; nothing happens if there is none. The
    /// blobs it linked stay.
    async fn remove_folder(&self, name: &str) -> Result<()>;

    /// Whether a build folder links the blob `name` (a file, or any file of a tree). A copy made where linking failed
    /// does not count: the blob is then not needed by it. `false` if there is no such blob.
    async fn is_linked(&self, name: &str) -> Result<bool>;
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

/// A tree being written to [`Storage`], synchronously, stored under its name only when committed. Paths follow the
/// rules of [`Storage`]; parent directories are created as needed. Native builds only, as trees are.
#[cfg(native)]
pub trait TreeWriter: Send {
    /// Creates the directory at `path`.
    fn directory(&mut self, path: &str) -> Result<()>;

    /// Starts the file at `path`, empty: what [`TreeWriter::write`] writes goes into it, until the next file.
    fn file(&mut self, path: &str) -> Result<()>;

    /// Appends `bytes` to the file started last.
    fn write(&mut self, bytes: &[u8]) -> Result<()>;

    /// Stores the tree under its name and says where it is kept, as [`Storage::find`] would. If another tree was
    /// stored under that name meanwhile, that one is kept: same name, same content.
    fn commit(self: Box<Self>) -> Result<String>;
}

/// A build folder being made of links to stored blobs, stored under its name only when committed.
#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
pub trait FolderWriter: MaybeSend {
    /// Puts the blob `blob` at `path` in the folder, or, with a `member`, the file or directory at that path inside
    /// the tree `blob` (a directory with every file below it). Parent directories are created as needed.
    async fn link(&mut self, path: &str, blob: &str, member: Option<&str>) -> Result<()>;

    /// Stores the folder under its name and says where it is kept, as [`Storage::find_folder`] would. If another
    /// folder was stored under that name meanwhile, that one is kept: same build, same files.
    async fn commit(self: Box<Self>) -> Result<String>;
}
