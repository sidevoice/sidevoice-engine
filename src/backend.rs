//! A backend runs models: sherpa-onnx, whisper.cpp, MLX, transformers.js, ... It describes itself as data
//! ([`BackendSpec`]: its accelerators and its requirements); the engine does the matching, ranking, selection and
//! installing for every backend alike (`crate::resolver`, `crate::install`). Which models a backend runs is the
//! catalogue's to say (each build names its backend), and which files it downloads is data too, not code. What only
//! the backend can do is check its accelerators for real (`probe`, when the default is not enough) and load a model
//! (`load`).

use async_trait::async_trait;

use crate::catalog::Build;
use crate::host::{Accelerator, Capabilities};
use crate::install::Installed;
use crate::resolver::Reason;
use crate::Result;

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

/// One condition the machine must meet. The engine has the common ones ([`MinMemoryMb`], [`MinCores`]); a backend can
/// write its own without changing the contract.
pub trait Requirement: Send + Sync {
    fn check(&self, caps: &Capabilities) -> Result<(), Reason>;
}

/// At least this much memory, in MB. Unknown memory passes.
pub struct MinMemoryMb(pub u32);

impl Requirement for MinMemoryMb {
    fn check(&self, caps: &Capabilities) -> Result<(), Reason> {
        match caps.memory_mb {
            Some(has) if has < self.0 => Err(Reason::numbers("memory", self.0, has)),
            _ => Ok(()),
        }
    }
}

/// At least this many CPU cores. Unknown cores pass.
pub struct MinCores(pub u32);

impl Requirement for MinCores {
    fn check(&self, caps: &Capabilities) -> Result<(), Reason> {
        match caps.cores {
            Some(has) if has < self.0 => Err(Reason::numbers("cores", self.0, has)),
            _ => Ok(()),
        }
    }
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

/// A model in memory. Transcribing and speaking are capabilities of the loaded model, not of the backend: one
/// backend can load models of both kinds.
pub trait LoadedModel: Send {
    fn as_transcriber(&mut self) -> Option<&mut dyn Transcriber> {
        None
    }
    fn as_synthesizer(&mut self) -> Option<&mut dyn Synthesizer> {
        None
    }
    fn memory_mb(&self) -> Option<u32>;
}

/// Speech to text, one whole turn at a time.
#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
pub trait Transcriber {
    /// `pcm`: mono 16 kHz samples. `language`: a BCP 47 tag, or `None` to detect it.
    async fn transcribe(&mut self, pcm: &[f32], language: Option<&str>) -> Result<String>;
}

/// Text to speech.
#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
pub trait Synthesizer {
    fn voices(&self) -> Vec<String>;
    async fn speak(&mut self, text: &str, voice: &str, speed: f32) -> Result<Vec<f32>>;
}
