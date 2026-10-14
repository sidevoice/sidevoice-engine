//! A local model as this engine sees it, what the local catalogue lists ([`Catalog::models`](crate::Catalog::models)):
//! its catalogue data, whether it is installed and when it was last loaded, every build of it ranked with whether it
//! runs here and why not, and the build the engine recommends.

use crate::catalog::{Capability, SpeedRange, Voice};
use crate::host::{Accelerator, AcceleratorClass};
use crate::resolver::Reason;

/// A model of the local catalogue, here: what [`ModelInfo`](crate::ModelInfo) says of every model, and the local
/// specifics (its family, size, licence, builds, what is installed and when it was last loaded).
#[derive(Debug, Clone, PartialEq)]
pub struct LocalModelInfo {
    /// Its stable id: what [`Engine::install`](crate::Engine::install) and [`Engine::load`](crate::Engine::load) take.
    pub id: String,
    /// The id of the family it belongs to in the catalogue: "whisper", "kokoro", ...
    pub family: String,
    /// What it can do.
    pub capabilities: Vec<Capability>,
    /// Its size, in millions of parameters.
    pub parameters_m: u32,
    /// The languages it handles: BCP 47 tags.
    pub languages: Vec<String>,
    /// Its licence, as an SPDX id.
    pub license: String,
    /// Its voices, as the catalogue declares them (a text-to-speech model whose source declares them).
    pub voices: Vec<Voice>,
    /// The speeds it takes, as its family declares them; `None`, it takes no speed.
    pub speed: Option<SpeedRange>,
    /// Whether one of its builds is installed.
    pub installed: bool,
    /// When it was last loaded ([`Engine::load`](crate::Engine::load), any build), in milliseconds since the Unix epoch,
    /// by the host's clock ([`Host::now_ms`](crate::Host::now_ms)); kept in the host's storage, so it outlives the app.
    /// `None` when it is not installed, or has not been loaded since it was (or since the engine began noting it).
    pub last_used_at: Option<u64>,
    /// Every build of it, ranked: those that run here first.
    pub builds: Vec<ModelBuild>,
    /// The build the engine recommends: the best ranked of those that run here, if any does. Loading the model without
    /// naming a build ([`Engine::load`](crate::Engine::load)) loads it, unless another build that runs here is
    /// installed: then that one.
    pub recommended_build: Option<String>,
}

/// One build of a model, here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelBuild {
    /// Its stable id.
    pub id: String,
    /// The backend that runs it.
    pub backend: String,
    /// The accelerator it would run on here, when it runs here; its class is [`ModelBuild::accelerator_class`].
    pub accelerator: Option<Accelerator>,
    /// The format's own name for its precision ("int8", "fp16", ...): informational.
    pub precision: String,
    /// What installing it downloads, in bytes (an archive several files share counts once).
    pub download_bytes: u64,
    /// The memory it takes to run, in MB.
    pub memory_mb: u32,
    /// Whether it runs here.
    pub available: bool,
    /// Why it does not, when it does not: stable codes the app translates.
    pub reasons: Vec<Reason>,
    /// Whether all its files are installed.
    pub installed: bool,
}

impl ModelBuild {
    /// The class of the accelerator it would run on here (a GPU, a neural engine, a CPU), when it runs here: what the
    /// builds that run are ranked by first ([`Accelerator::class`]). Derived from `accelerator` each time it is asked,
    /// so the two never disagree; JavaScript gets it as `acceleratorClass`.
    #[must_use]
    pub fn accelerator_class(&self) -> Option<AcceleratorClass> {
        self.accelerator.map(Accelerator::class)
    }
}
