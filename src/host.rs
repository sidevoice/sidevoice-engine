//! The platform: what the place the engine runs in can see, keep and download. The engine ships one host per kind of
//! build, chosen by the same aliases as the backends (`native.rs`, [`NativeHost`], in every native build; the
//! browser's to come), and the [`Host`] interface stays open: tests and other platforms bring their own.
//!
//! Inside: `capabilities` (what a host reports), `platform` (which platform that is), `storage` and `fetcher` (where
//! files are kept, and how they arrive), and `native` (the native host).

use crate::maybe_send::{MaybeSend, MaybeSync};

mod capabilities;
mod fetcher;
#[cfg(native)]
mod native;
mod platform;
mod storage;

pub use capabilities::{Accelerator, Capabilities, Runs};
pub use fetcher::{Download, Fetcher};
#[cfg(native)]
pub use native::NativeHost;
pub(crate) use platform::Platform;
#[cfg(native)]
pub use storage::TreeWriter;
pub use storage::{FolderWriter, Storage, StorageWriter};

/// The facts, storage and downloads of the place the engine runs in. Without a host there is no engine: an app passes
/// the built-in one for its build ([`NativeHost`] natively) or its own.
///
/// A host must be `Send + Sync` in a native build and need not be in the web build ([`MaybeSend`], [`MaybeSync`]).
/// [`Storage`], [`StorageWriter`], [`Fetcher`] and [`Download`] are async traits: implement them with
/// the re-exported [`async_trait`](crate::async_trait) attribute, which must match the engine's on each target
/// (futures are `Send` in a native build, not on the web).:
///
/// ```
/// use sidevoice_engine::{async_trait, Download, Error, Fetcher, Result};
///
/// struct Offline;
///
/// #[cfg_attr(not(target_arch = "wasm32"), async_trait)]
/// #[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
/// impl Fetcher for Offline {
///     async fn fetch(&self, _url: &str) -> Result<Box<dyn Download>> {
///         Err(Error::new("download-failed"))
///     }
/// }
/// ```
///
/// In a native build a storage also opens stored files and writes trees synchronously ([`Storage`], `TreeWriter`): the
/// installer unpacks archives on a blocking thread. And the native engine's futures expect a Tokio runtime, as
/// [`NativeHost`] says.
pub trait Host: MaybeSend + MaybeSync {
    /// Known when the host is built: gathering it is the host's job, so asking is cheap and synchronous.
    fn capabilities(&self) -> Capabilities;
    /// Where engine packages and models live (a directory, OPFS, ...).
    fn storage(&self) -> &dyn Storage;
    /// Where files are downloaded from.
    fn fetcher(&self) -> &dyn Fetcher;
}
