//! A backend's library once it is open: what `Backend::open` returns, and what loads models. The engine keeps one per
//! backend while any model loaded from it is in memory, and drops it after the last one (see *`open` and `load`* in
//! `backend.rs`).

use std::sync::Arc;

use async_trait::async_trait;

use crate::backend::BackendModel;
use crate::catalog::{BuildEntry, ModelEntry};
use crate::host::{Accelerator, Host};
use crate::install::Installed;
use crate::maybe_send::{MaybeSend, MaybeSync};
use crate::Result;

/// A backend's open library (a native shared library, a JavaScript module, a provider's API): it loads models, and
/// outlives every model it loaded. Dropping it closes it.
#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
pub(crate) trait Library: MaybeSend + MaybeSync {
    /// Loads an installed build, as `load` says: the model's files, each by its key, on the accelerator `probe` found,
    /// and hands back something that transcribes, speaks or detects speech. It downloads nothing, reads nothing outside
    /// the files, keeps nothing, and fails with a stable code (see *`open` and `load`* in `backend.rs`).
    async fn load(&self, load: Load<'_>) -> Result<Box<dyn BackendModel>>;
}

/// What a library loads: the catalogue's model and its build, the accelerator it runs on, its installed files (none for
/// a remote build) and the host, through which a remote model makes its calls and finds its key.
#[derive(Clone, Copy)]
pub(crate) struct Load<'a> {
    pub(crate) model: &'a ModelEntry,
    pub(crate) build: &'a BuildEntry,
    pub(crate) accelerator: Accelerator,
    pub(crate) files: &'a Installed,
    pub(crate) host: &'a Arc<dyn Host>,
}
