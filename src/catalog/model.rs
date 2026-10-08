//! A model of the catalogue, with every build of it; inside, `capability` (what a model can do) and `build` (one way
//! to run it).

use serde::Deserialize;

mod build;
mod capability;

pub use build::{Build, Memory, MemorySource, ModelFile, Requires};
pub use capability::Capability;

/// A model, with every build of it the catalogue knows. Languages and licence are the model's, not its family's: they
/// vary within a family (English-only and multilingual Whisper models).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Model {
    /// Its stable id: "whisper-small", "kokoro-82m-v0.19", ... The app translates what it shows from it.
    pub id: String,
    /// What it can do.
    pub capabilities: Vec<Capability>,
    /// Its size, in millions of parameters.
    pub parameters_m: u32,
    /// The languages it handles: BCP 47 tags, or "multi" for a model that handles many (multilingual Whisper).
    pub languages: Vec<String>,
    /// Its licence, as an SPDX id, from its source.
    pub license: String,
    /// Every way to run it, in no particular order: which one fits best here is the resolver's to rank.
    pub builds: Vec<Build>,
}
