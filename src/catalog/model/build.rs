use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer};

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
    /// sherpa-onnx builds only: where each argument of a call goes, for a model that reads it from its config rather than
    /// per call. Keyed by the interface's argument name ([`CALL_ARGUMENTS`]: `language`), each the sherpa-onnx config
    /// paths (below `OfflineRecognizerConfig.model_config`) that take its value: Whisper's `whisper.language`, Canary's
    /// `canary.src_lang` and `canary.tgt_lang` (it transcribes when both are the language). One path or a list. Empty,
    /// no argument reaches the config (Whisper then detects the language). sidevoice-engine#46.
    #[serde(default, deserialize_with = "call_params")]
    pub call_params: BTreeMap<String, Vec<String>>,
}

/// The arguments of a call a build's `call_params` may name: `language`, the BCP 47 tag `Stt::transcribe` takes.
pub(crate) const CALL_ARGUMENTS: &[&str] = &["language"];

/// `call_params` as written: each argument's path, or its list of paths.
fn call_params<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<String, Vec<String>>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Paths {
        One(String),
        Many(Vec<String>),
    }
    let written = BTreeMap::<String, Paths>::deserialize(deserializer)?;
    Ok(written
        .into_iter()
        .map(|(argument, paths)| match paths {
            Paths::One(path) => (argument, vec![path]),
            Paths::Many(paths) => (argument, paths),
        })
        .collect())
}

/// What a build strictly needs beyond its backend and its memory: only constraints that make it unusable when not
/// met, never preferences. Each is optional; absent, it does not constrain.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Requires {
    /// The only accelerators it can run on, when its backend runs on more than it can take (Kokoro on sherpa-onnx: Core
    /// ML aborts the process, so the CPU only; transformers.js's fp16 builds, its WebGPU variant: WebGPU only). Empty,
    /// any its backend runs on.
    #[serde(default)]
    pub accelerators: Vec<Accelerator>,
    /// WebGPU features the adapter must have, as WebGPU names them: "shader-f16", .... Not checked yet: hosts do not
    /// report the adapter's features, so a build that needs them also requires the WebGPU accelerator.
    #[serde(default)]
    pub webgpu_features: Vec<String>,
    /// On WebAssembly, the most memory it may take, in MB: a cap declared for the build, not measured from the page.
    /// wasm32's linear memory tops out at 4 GiB (what Chrome allows; mobile browsers give a page far less), and a model
    /// fits in less than that: ONNX Runtime Web reads its weights into ArrayBuffers (at most about 2 GB each in Chrome),
    /// and an ONNX file over 2 GB needs external data. Today's transformers.js builds declare 2048. In a page, a build
    /// whose `memory` is more is rejected (`wasm-memory`).
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
