use serde::Deserialize;

use crate::host::Accelerator;
use crate::install::Artifact;

/// One way to run a model: the backend that runs it, its precision, what it strictly needs, the memory it takes, and
/// its files. Which accelerators it runs on and its file format are its backend's to know, short of a hard restriction
/// in `requires`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildEntry {
    /// Its stable id, unique in the catalogue: "whisper-small/sherpa-onnx-int8", ... Editorial data and benchmarks
    /// are keyed by it, outside the catalogue.
    pub id: String,
    /// The id of the backend that runs it ([`Engine::backends`](crate::Engine::backends)).
    pub backend: String,
    /// The format's own name for its precision, as its backend uses it: "int8", "q8", "fp16", "q5_1", "4bit", ....
    /// Informational only, to tell builds apart (the transformers.js loader may pass it on as its `dtype`): nothing
    /// else interprets it.
    pub precision: String,
    /// Hard constraints only; absent, there are none.
    #[serde(default)]
    pub requires: Requires,
    /// The memory it takes to run, and where that figure comes from.
    pub memory: Memory,
    /// Every file the backend needs to load it, configuration and tokenizer included.
    pub files: Vec<ModelFile>,
}

/// What a build strictly needs beyond its backend and its memory: only constraints that make it unusable when not
/// met, never preferences. Each is optional; absent, it does not constrain.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Requires {
    /// The only accelerators it can run on, when its backend runs on more than it can take (Kokoro on sherpa-onnx: Core
    /// ML aborts the process, so the CPU only). Empty, any its backend runs on.
    #[serde(default)]
    pub accelerators: Vec<Accelerator>,
    /// WebGPU features the adapter must have, as WebGPU names them: "shader-f16", ...
    #[serde(default)]
    pub webgpu_features: Vec<String>,
    /// On WebAssembly, the most memory it may take, in MB: wasm32 cannot hand out more than about 2 GiB at once.
    pub wasm_max_mb: Option<u32>,
}

/// The memory a build takes to run, and its provenance.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Memory {
    /// In MB.
    pub mb: u32,
    /// Where the figure comes from.
    pub source: MemorySource,
    /// How it was obtained: "weights size + 30%", the machine it was measured on, the page that declares it, ...
    pub basis: String,
}

/// Where a build's memory figure comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemorySource {
    /// Derived from something else, such as the size of the weights (`cargo xtask pin-catalog` writes these).
    Estimated,
    /// What the model's publisher says.
    Declared,
    /// Measured by running it.
    Measured,
}

/// One file of a build, pinned: its URL names an immutable revision (or is marked `mutable`), and its digest and size
/// are what it serves.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelFile {
    /// What the backend finds it by: "encoder", "tokens", "espeak-ng-data" (a directory), ...
    pub key: String,
    /// Where it is downloaded from: pinned to a revision, unless `mutable`.
    pub url: String,
    /// Its SHA-256 digest, in lowercase hex: what is downloaded from `url`, the archive when there is one.
    pub sha256: String,
    /// Its size, in bytes: what is downloaded from `url`.
    pub bytes: u64,
    /// When `url` is an archive, the file or directory inside it that `key` names. Several keys may name parts of one
    /// archive: they repeat its `url`, `sha256` and `bytes`, and it is downloaded and unpacked once, by its digest.
    pub archive_path: Option<String>,
    /// Whether `url` can serve other bytes over time (a GitHub release asset, which can be uploaded again): then only
    /// `sha256` pins it.
    #[serde(default)]
    pub mutable: bool,
}

impl ModelFile {
    /// What the installer downloads for it: the file, or the member of the archive at `url`.
    pub(crate) fn artifact(&self) -> Artifact {
        Artifact {
            key: self.key.clone(),
            url: self.url.clone(),
            sha256: self.sha256.clone(),
            archive_path: self.archive_path.clone(),
        }
    }
}
