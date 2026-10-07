//! Where a host keeps files: by name, whole or not at all. Storage does not know what a file is: the installer names
//! each one after its digest (content-addressed), and a name is only ever stored once its bytes have been checked.

use async_trait::async_trait;

use crate::maybe_send::{MaybeSend, MaybeSync};
use crate::Result;

/// Where engine packages and models are kept, each file under a name. Implemented with
/// [`async_trait`](crate::async_trait), as [`Host`](crate::Host) shows.
///
/// Names are made of ASCII letters, digits, `-` and `_` (the installer uses lowercase hex digests). A file is stored
/// whole or not at all: what [`Storage::create`] writes is only stored under its name when it is committed. Errors
/// are stable codes: `storage-failed` when the medium fails.
#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
pub trait Storage: MaybeSend + MaybeSync {
    /// Where the file stored as `name` is kept (a path, an OPFS name, ...), if it is stored, complete.
    async fn find(&self, name: &str) -> Result<Option<String>>;

    /// A new file to be stored as `name`, written in parts. Nothing is stored as `name` until it is
    /// [committed](StorageWriter::commit); dropped before that, what was written is discarded.
    async fn create(&self, name: &str) -> Result<Box<dyn StorageWriter>>;
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
