//! A local model as this engine sees it, what the local catalogue lists ([`Catalog::models`](crate::Catalog::models)):
//! its catalogue data, whether it is installed, every build of it ranked with whether it runs here and why not, and
//! the build the engine recommends.

use crate::catalog::{Capability, SpeedRange, Voice};
use crate::host::Accelerator;
use crate::resolver::Reason;

/// A model of the local catalogue, here: what [`ModelInfo`](crate::ModelInfo) says of every model, and the local
/// specifics (its family, size, licence, builds and what is installed).
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
    /// The speeds it takes, as its family declares them with their source; `None`, it takes no speed.
    pub speed: Option<SpeedRange>,
    /// Whether one of its builds is installed.
    pub installed: bool,
    /// Every build of it, ranked: those that run here first.
    pub builds: Vec<ModelBuild>,
    /// The build the engine would use: the first that runs here, if any does.
    pub recommended_build: Option<String>,
}

/// One build of a model, here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelBuild {
    /// Its stable id.
    pub id: String,
    /// The backend that runs it.
    pub backend: String,
    /// The accelerator it would run on here, when it runs here.
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
