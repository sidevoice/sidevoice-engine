//! The brain of voice models, local and remote. Models come from catalogues, one interface ([`Catalog`], listed by
//! [`Engine::catalogs`]): the local catalogue (which models exist, which build of each runs on this device, which are
//! installed) and one per remote provider (which models the app's key may use, listed live). Each has a status, its
//! models, `refresh` and `load`, and every model loaded hands out the same capability interfaces, [`Stt`], [`Tts`],
//! [`Vad`] and [`EndOfTurn`]: transcribing with Whisper or with OpenAI is the same call.
//!
//! The platform is injected and mandatory: [`Engine::new`] takes a [`Host`] (capabilities, storage, downloads, API
//! calls, keys) and the [`CatalogSource`]s to merge, usually the [`BundledCatalog`] and any others. A native build
//! ships one host, [`NativeHost`], which keeps its files in a directory the app chooses; any other host can stand in
//! for it. Backends and providers are not passed in: they belong to the engine, and which ones a build contains is
//! decided when it is compiled ([`Engine::backends`], [`Engine::catalogs`]). Nothing is downloaded or loaded until
//! [`Engine::install`] or [`Engine::load`], and a [`LocalModel`] is unloaded when the last one of its build is dropped.
//!
//! Compiled to wasm32, the crate is also the npm package `@sidevoice/engine`: `web` is its bridge to JavaScript, and
//! exists in no other build.
#![warn(missing_docs)]

/// The attribute that makes the engine's async traits implementable; see [`Host`] for how to apply it.
#[doc(no_inline)]
pub use async_trait::async_trait;

mod backend;
mod capability;
mod catalog;
mod engine;
mod host;
mod install;
mod maybe_send;
mod provider;
mod resolver;
#[cfg(test)]
mod test_support;
#[cfg(web)]
mod web;

pub use backend::{BackendId, BackendInfo};
pub use capability::{
    Audio, EndOfTurn, Stt, Tts, Vad, VadEvent, VadFrame, VadOptions, VadOutput, VadStream,
};
pub use catalog::{
    BuildEntry, BundledCatalog, Capability, CatalogFragment, CatalogSource, Family, FamilySpeed,
    Gender, Memory, MemorySource, ModelEntry, ModelFile, Problem, Requires, SpeedRange, Voice,
};
pub use engine::{
    Catalog, CatalogStatus, ConfigError, Engine, LocalCatalog, LocalModel, LocalModelInfo, Model,
    ModelBuild, ModelInfo, RemoteCatalog, LOCAL_CATALOG,
};
pub use host::{
    Accelerator, Capabilities, Credentials, Download, Fetcher, FolderWriter, Host, HttpClient,
    HttpRequest, HttpResponse, NoCredentials, Runs, Storage, StorageWriter,
};
#[cfg(native)]
pub use host::{NativeHost, TreeWriter};
pub use install::{Artifact, Cancel, Progress, ProgressSink};
pub use maybe_send::{MaybeSend, MaybeSync};
pub use provider::{RemoteModel, RemoteModelInfo};
pub use resolver::Reason;

/// What the engine's operations fail with. Errors carry a stable code, never text: clients translate it. One that a
/// remote provider caused also carries what the provider said, its own code and message (`detail`), for the app to
/// show as it is (a toast, say): the engine neither interprets nor remembers it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Error {
    /// The stable code, such as `"not-implemented"`.
    pub code: &'static str,
    /// What a remote provider said when it refused a call, in its own words: `"<its code>: <its message>"`, or either.
    /// `None` for an error no provider answered.
    pub detail: Option<String>,
}

impl Error {
    /// An error with this code.
    #[must_use]
    pub const fn new(code: &'static str) -> Self {
        Self { code, detail: None }
    }

    /// An error with this code, and what the provider said.
    #[must_use]
    pub fn with_detail(code: &'static str, detail: Option<String>) -> Self {
        Self { code, detail }
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
