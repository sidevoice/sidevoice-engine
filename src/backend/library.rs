//! A backend's library once it is open: what `Backend::open` returns, and what loads models. The engine keeps one per
//! backend while any model loaded from it is in memory, and drops it after the last one (see *`open` and `load`* in
//! `backend.rs`).

use async_trait::async_trait;

use crate::backend::BackendModel;
use crate::catalog::BuildEntry;
use crate::host::Accelerator;
use crate::install::Installed;
use crate::maybe_send::{MaybeSend, MaybeSync};
use crate::Result;

/// A backend's open library (a native shared library, a JavaScript module): it loads models, and outlives every
/// model it loaded. Dropping it closes it.
#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
pub(crate) trait Library: MaybeSend + MaybeSync {
    /// Loads an installed build on `accelerator`, one that `probe` found: the model's files, each by its key in
    /// `files`, and hands back something that transcribes or speaks. It downloads nothing, reads nothing outside
    /// `files`, keeps nothing, and fails with a stable code (see *`open` and `load`* in `backend.rs`).
    async fn load(
        &self,
        build: &BuildEntry,
        accelerator: Accelerator,
        files: &Installed,
    ) -> Result<Box<dyn BackendModel>>;
}
