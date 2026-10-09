//! The brain of voice models, local and remote. Two siblings produce models: the catalogue, of local models (which
//! exist, which build of each runs on this device, which are installed: [`Engine::models`], [`Engine::load`], a
//! [`LocalModel`]), and the remote providers (which models the app's key may use, listed live:
//! [`Engine::providers`], [`Engine::remote`], a [`RemoteModel`]). Both hand out the same capability interfaces,
//! [`Stt`], [`Tts`], [`Vad`] and [`EndOfTurn`]: transcribing with Whisper or with OpenAI is the same call.
//!
//! The platform is injected and mandatory: [`Engine::new`] takes a [`Host`] (capabilities, storage, downloads, API
//! calls, keys) and the [`CatalogSource`]s to merge, usually the [`BundledCatalog`] and any others. A native build
//! ships one host, [`NativeHost`], which keeps its files in a directory the app chooses; any other host can stand in
//! for it. Backends and providers are not passed in: they belong to the engine, and which ones a build contains is
//! decided when it is compiled ([`Engine::backends`], [`Engine::providers`]). Nothing is downloaded or loaded until
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
    BuildEntry, BundledCatalog, Capability, CatalogFragment, CatalogSource, Family, Gender, Memory,
    MemorySource, ModelEntry, ModelFile, Problem, Requires, Voice,
};
pub use engine::{ConfigError, Engine, LocalModel, Model, ModelBuild};
pub use host::{
    Accelerator, Capabilities, Credentials, Download, Fetcher, FolderWriter, Host, HttpClient,
    HttpRequest, HttpResponse, NoCredentials, Runs, Storage, StorageWriter,
};
#[cfg(native)]
pub use host::{NativeHost, TreeWriter};
pub use install::{Artifact, Cancel, Progress, ProgressSink};
pub use maybe_send::{MaybeSend, MaybeSync};
pub use provider::{Provider, ProviderModel, RemoteModel};
pub use resolver::Reason;

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
