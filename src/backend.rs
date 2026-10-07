//! Backends: what runs models (sherpa-onnx, whisper.cpp, MLX, transformers.js, ...). This file is the interface every
//! backend implements ([`Backend`], with its data in [`BackendSpec`]); the engine does the matching, ranking,
//! selection and installing for every backend alike (`crate::resolver`, `crate::install`). Which models a backend
//! runs is the catalogue's to say, and which library files it needs is data too (`backends.json`). Backends belong to
//! the engine: none of this is public, except a backend's id.
//!
//! Inside: `spec` (a backend as data, and its id), `runtime` (the library files each backend needs per platform, from
//! `backends.json`), `requirement` (what the machine must meet, and the common requirements), `registry` (how the
//! backends of this build are found) and `implementations` (one file per backend).

use async_trait::async_trait;

use crate::catalog::Build;
use crate::host::{Accelerator, Capabilities};
use crate::install::Installed;
use crate::maybe_send::{MaybeSend, MaybeSync};
use crate::Result;

mod implementations;
mod loaded_model;
mod registry;
mod requirement;
mod runtime;
mod spec;
#[cfg(test)]
mod tests;

pub(crate) use loaded_model::LoadedModel;
pub(crate) use registry::{built_in, find, BackendFactory};
#[allow(
    unused_imports,
    reason = "a common requirement no backend declares yet"
)]
pub(crate) use requirement::MinCores;
pub(crate) use requirement::{MinMemoryMb, Requirement};
pub(crate) use runtime::runtime_files;
pub use spec::BackendId;
pub(crate) use spec::BackendSpec;

/// What runs models: its data ([`BackendSpec`]), which of its accelerators work here, and loading a build.
#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
pub(crate) trait Backend: MaybeSend + MaybeSync {
    /// What it is and needs.
    fn spec(&self) -> &BackendSpec;

    /// Which of the declared accelerators work here. By default, the ones the host reports. A backend overrides it
    /// when only trying can tell (a CUDA driver, a WebGPU adapter, CoreML). The engine caches the answer, and keeps
    /// only what the host reports: a probe narrows, it never adds an accelerator the host did not see.
    fn probe(&self, caps: &Capabilities) -> Vec<Accelerator> {
        self.spec()
            .accelerators
            .iter()
            .copied()
            .filter(|accelerator| caps.has(*accelerator))
            .collect()
    }

    /// Loads an installed build (its model files and this backend's own, each by name in `files`) on one of the
    /// accelerators `probe` found, and hands back something that transcribes or speaks.
    async fn load(
        &self,
        build: &Build,
        accelerator: Accelerator,
        files: &Installed,
    ) -> Result<Box<dyn LoadedModel>>;
}
