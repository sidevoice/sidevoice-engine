//! A model of the catalogue, with every build of it; inside, `capability` (what a model can do) and `build` (one way
//! to run it).

use serde::Deserialize;

mod build;
mod capability;
mod voice;

pub(crate) use build::CALL_ARGUMENTS;
pub use build::{BuildEntry, Memory, MemorySource, ModelFile, Requires};
pub use capability::Capability;
pub use voice::{Gender, Voice};

/// A model, with every build of it the catalogue knows. Languages and licence are the model's, not its family's: they
/// vary within a family (English-only and multilingual Whisper models).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelEntry {
    /// Its stable id: "whisper-small", "kokoro-82m-v0.19", ... The app translates what it shows from it.
    pub id: String,
    /// What it can do.
    pub capabilities: Vec<Capability>,
    /// Its size, in millions of parameters.
    pub parameters_m: u32,
    /// The languages it handles, each a BCP 47 tag ("en-US", "es", ...): never "multi", and never an accent the source
    /// does not state.
    pub languages: Vec<String>,
    /// Where `languages` comes from, when it is generated rather than written: `cargo xtask pin-catalog` reads the
    /// list from it (Whisper's, from openai/whisper's tokenizer at a pinned commit).
    #[serde(default)]
    pub languages_source: Option<String>,
    /// Its voices, as its source declares them, for a text-to-speech model whose source does; a voice the backend has
    /// and the catalogue does not describe gets the model's languages and no gender.
    #[serde(default)]
    pub voices: Vec<Voice>,
    /// Its licence, as an SPDX id, from its source.
    pub license: String,
    /// Every way to run it, in no particular order: which one fits best here is the resolver's to rank.
    pub builds: Vec<BuildEntry>,
}
