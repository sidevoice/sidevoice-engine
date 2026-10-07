//! The brain of local models: which models exist, which build fits on this device, which to use for each stage, and
//! the state each one is in.
//!
//! The platform is injected and mandatory: [`Engine::new`] takes a [`Host`] (capabilities, storage, downloads) and,
//! optionally, extra [`CatalogSource`]s. Backends are not passed in: they belong to the engine, and which ones a build
//! contains is decided when it is compiled ([`built_in`]). Nothing is downloaded or loaded until [`Engine::prepare`].

pub use async_trait::async_trait;

mod backend;
mod catalog;
mod engine;
#[cfg(test)]
mod fakes;
mod host;
mod install;
mod lifecycle;
mod model;
mod offer;
mod resolver;

pub use backend::{
    built_in, Backend, BackendFactory, BackendId, BackendSpec, MinCores, MinMemoryMb, Requirement,
};
pub use catalog::{Build, Catalog, CatalogFragment, CatalogSource, Family, Model, Problem, Task};
pub use engine::{ConfigError, Engine, Handle, Preferences, Selection};
pub use host::{Accelerator, Capabilities, Fetcher, Host, Runs, Storage};
pub use install::{Artifact, Installed, Installer};
pub use lifecycle::BuildState;
pub use model::{LoadedModel, Synthesizer, Transcriber};
pub use offer::{Offer, Reason, Rejection};

/// What the engine's operations fail with. Errors carry a stable code, never text: clients translate it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub code: &'static str,
}

impl Error {
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

pub type Result<T, E = Error> = std::result::Result<T, E>;
