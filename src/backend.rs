//! Backends: what runs models (sherpa-onnx, whisper.cpp, MLX, transformers.js, ...). This file is the interface every
//! backend implements ([`Backend`], with its data in [`BackendSpec`]); the engine does the matching, ranking,
//! selection and installing for every backend alike (`crate::resolver`, `crate::install`). Which models a backend
//! runs is the catalogue's to say, and which files it downloads is data too.
//!
//! Inside: `requirement` (what the machine must meet, and the common requirements), `registry` (how the backends of
//! this build are found) and `implementations` (one file per backend).

use async_trait::async_trait;

use crate::catalog::Build;
use crate::host::{Accelerator, Capabilities};
use crate::install::Installed;
use crate::model::LoadedModel;
use crate::Result;

mod implementations;
mod registry;
mod requirement;
#[cfg(test)]
mod tests;

pub use registry::{built_in, BackendFactory};
pub use requirement::{MinCores, MinMemoryMb, Requirement};

/// A backend's stable id, as catalogue builds name it: "sherpa-onnx", "whisper-cpp", "mlx", ...
pub type BackendId = &'static str;

/// What a backend is and needs, as data. Adding a backend is mostly filling this in.
pub struct BackendSpec {
    pub id: BackendId,
    /// The accelerators it can run on, best first: the default is the first one that works here.
    pub accelerators: &'static [Accelerator],
    /// What the machine must meet, whatever the model: each one a check on the capabilities.
    pub requirements: &'static [&'static dyn Requirement],
}

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
pub trait Backend: Send + Sync {
    fn spec(&self) -> &BackendSpec;

    /// Which of the declared accelerators work here. By default, the ones the host reports. A backend overrides it
    /// when only trying can tell (a CUDA driver, a WebGPU adapter, CoreML). The engine caches the answer.
    fn probe(&self, caps: &Capabilities) -> Vec<Accelerator> {
        self.spec()
            .accelerators
            .iter()
            .copied()
            .filter(|accelerator| caps.has(*accelerator))
            .collect()
    }

    /// Loads an installed build (its model files and this backend's library, by name in `files`) on one of the
    /// accelerators `probe` found, and hands back something that transcribes or speaks.
    async fn load(
        &self,
        build: &Build,
        accelerator: Accelerator,
        files: &Installed,
    ) -> Result<Box<dyn LoadedModel>>;
}
