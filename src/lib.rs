//! The brain of local models: which models exist, which build fits on this device, which to use for each stage, and
//! the state each one is in.
//!
//! The platform is injected and mandatory: [`Engine::new`] takes a [`Host`] (capabilities, storage, downloads) and,
//! optionally, extra [`CatalogSource`]s. A native build ships one, [`NativeHost`], which keeps its files in a directory
//! the app chooses; any other host can stand in for it. Backends are not passed in: they belong to the engine, and
//! which ones a build contains is decided when it is compiled ([`Engine::backends`] names them). Nothing is downloaded
//! or loaded until [`Engine::prepare`], and what goes unused is unloaded from memory again ([`BuildState`]).
//!
//! Compiled to wasm32, the crate is also the npm package `@sidevoice/engine`: `web` is its bridge to JavaScript, and
//! exists in no other build.
#![warn(missing_docs)]

/// The attribute that makes the engine's async traits implementable; see [`Host`] for how to apply it.
#[doc(no_inline)]
pub use async_trait::async_trait;

mod backend;
mod catalog;
mod engine;
mod host;
mod install;
mod maybe_send;
mod resolver;
#[cfg(test)]
mod test_support;
#[cfg(web)]
mod web;

pub use backend::BackendId;
pub use catalog::{Build, CatalogFragment, CatalogSource, Model, Problem, Task};
pub use engine::{
    BuildState, ConfigError, Engine, Handle, Preferences, Selection, DEFAULT_IDLE_UNLOAD,
};
#[cfg(native)]
pub use host::NativeHost;
pub use host::{Accelerator, Capabilities, Download, Fetcher, Host, Runs, Storage, StorageWriter};
pub use install::{Artifact, Cancel, Progress, ProgressSink};
pub use maybe_send::{MaybeSend, MaybeSync};
pub use resolver::{Offer, Reason, Rejection};

/// What the engine's operations fail with. Errors carry a stable code, never text: clients translate it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Error {
    /// The stable code, such as `"not-implemented"`.
    pub code: &'static str,
}

impl Error {
    /// An error with this code.
    #[must_use]
    pub const fn new(code: &'static str) -> Self {
        Self { code }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code)
    }
}

impl std::error::Error for Error {}

/// The engine's result: [`Error`] unless said otherwise.
pub type Result<T, E = Error> = std::result::Result<T, E>;
