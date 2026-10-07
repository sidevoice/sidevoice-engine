use serde::Deserialize;

use crate::install::Artifact;

/// One way to run a model: the backend that runs it, its precision, what it strictly needs, the memory it takes, and
/// its files. Which accelerators it runs on and its file format are its backend's to know, not the build's.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Build {
    /// Its stable id, unique in the catalogue: "whisper-small/sherpa-onnx-int8", ... Editorial data and benchmarks
    /// are keyed by it, outside the catalogue.
    pub id: String,
    /// The id of the backend that runs it ([`Engine::backends`](crate::Engine::backends), `backends.json`).
    pub backend: String,
    /// The precision of its weights.
    pub precision: Precision,
    /// Hard constraints only; absent, there are none.
    #[serde(default)]
    pub requires: Requires,
    /// The memory it takes to run, and where that figure comes from.
    pub memory: Memory,
    /// Every file the backend needs to load it, configuration and tokenizer included.
    pub files: Vec<ModelFile>,
}

/// The precision of a build's weights, from a fixed vocabulary: never read from file names. Adding one is a change to
/// the engine; it is `#[non_exhaustive]`, so that is not a breaking change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Precision {
    /// 16-bit floating point.
    Fp16,
    /// 8-bit integers (ONNX dynamic quantisation, as sherpa-onnx exports it).
    Int8,
    /// 8-bit quantisation, as transformers.js names it (`dtype: "q8"`).
    Q8,
    /// ggml's 5-bit quantisation, type 1 (whisper.cpp).
    #[serde(rename = "q5_1")]
    Q5_1,
}

/// What a build strictly needs beyond its backend and its memory: only constraints that make it unusable when not
/// met, never preferences. Each is optional; absent, it does not constrain.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Requires {
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

/// One file of a build, pinned: its URL names an immutable revision, and its digest and size are the file's.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelFile {
    /// What the backend finds it by: a role ("encoder", "tokens", ...), or its path inside the model's directory
    /// when the backend reads a directory ("espeak-ng-data/en_dict").
    pub key: String,
    /// Where it is downloaded from, at a pinned revision.
    pub url: String,
    /// Its SHA-256 digest, in lowercase hex.
    pub sha256: String,
    /// Its size, in bytes.
    pub bytes: u64,
}

impl ModelFile {
    /// What the installer downloads for it.
    pub(crate) fn artifact(&self) -> Artifact {
        Artifact {
            key: self.key.clone(),
            url: self.url.clone(),
            sha256: self.sha256.clone(),
        }
    }
}
