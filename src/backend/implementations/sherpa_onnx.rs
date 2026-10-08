// Linked only where the `sherpa-onnx` feature (default) brings the official crate in, natively: build.rs.
#![cfg(sherpa_onnx)]
//! sherpa-onnx: speech to text with Whisper and text to speech with Kokoro, on ONNX Runtime.
//!
//! # Binding
//!
//! The official `sherpa-onnx` crate (k2-fsa), pinned to one exact version in Cargo.toml, through its safe API
//! (`OfflineRecognizer`, `OfflineTts`). Its build script downloads sherpa-onnx's prebuilt static libraries for the
//! target (ONNX Runtime included) and links them into the app: in this first phase the backend is linked, not
//! downloaded when a model needs it, and `backends.json` lists nothing to download for it (`[]`). So `open` has
//! nothing to open: its library is the linked one, and the contract (`open`, then the library's `load`) stays as it
//! is for when loading the runtime on demand comes back (sidevoice-engine#33). Building without the `sherpa-onnx`
//! feature leaves the backend out, and ONNX Runtime with it.
//!
//! # Files
//!
//! A build's files are sherpa-onnx's config, as data: each file's key in the catalogue is the config field that receives
//! it, relative to the root of its capability (`config.rs`), and `load` fills that field, on the engine's provider and
//! threads, with everything else sherpa-onnx's default (the sample rates and feature sizes it reads from the model).
//!
//! - Whisper: `whisper.encoder`, `whisper.decoder` and `tokens`, under `OfflineRecognizerConfig.model_config`.
//! - Kokoro: `kokoro.model`, `kokoro.voices`, `kokoro.tokens` and `kokoro.data_dir` (espeak-ng's data, a directory),
//!   under `OfflineTtsConfig.model`.
//!
//! The family follows from the keys ([`Kind::of`]), and only what sherpa-onnx's config does not cover is code: Whisper
//! takes the language per call (`whisper.rs`), Kokoro names its voices (`kokoro.rs`).
//!
//! # Accelerators
//!
//! The CPU only, in this phase. The static libraries the crate links are built without ONNX Runtime's Core ML
//! execution provider: asked for it, sherpa-onnx logs "Fallback to cpu" and runs on the CPU. So the backend declares
//! the CPU alone rather than claim Core ML. The shared libraries it downloaded before did run Whisper on Core ML;
//! Core ML comes back with them (sidevoice-engine#33). CUDA needs sherpa-onnx's CUDA builds, which are not linked
//! either.
//!
//! Kokoro must stay off Core ML whatever the libraries: creating its TTS on Core ML throws a C++ exception that the
//! C API does not catch, and an exception that reaches Rust aborts the process. `load` refuses it with
//! `unsupported-accelerator` before creating it, and Kokoro's catalogue builds accept the CPU only.

use async_trait::async_trait;

use crate::backend::{Backend, BackendFactory, BackendSpec, Library, LoadedModel};
use crate::catalog::Build;
use crate::host::Accelerator;
use crate::install::Installed;
use crate::{Error, Result};

mod config;
#[cfg(test)]
mod inference_tests;
mod kokoro;
mod model_metadata;
#[cfg(test)]
mod tests;
mod whisper;

use kokoro::Kokoro;
use whisper::Whisper;

struct SherpaOnnx;

const SPEC: BackendSpec = BackendSpec {
    id: "sherpa-onnx",
    // Core ML is not in the linked libraries (see *Accelerators*).
    accelerators: &[Accelerator::Cpu],
    requirements: &[],
};

inventory::submit! { BackendFactory(|| Box::new(SherpaOnnx)) }

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl Backend for SherpaOnnx {
    fn spec(&self) -> &BackendSpec {
        &SPEC
    }

    /// The library is linked: there is nothing to open, and nothing installed is read.
    async fn open(&self, _files: &Installed) -> Result<Box<dyn Library>> {
        Ok(Box::new(Linked))
    }
}

/// The linked sherpa-onnx, which loads the model.
struct Linked;

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl Library for Linked {
    async fn load(
        &self,
        _build: &Build,
        accelerator: Accelerator,
        files: &Installed,
    ) -> Result<Box<dyn LoadedModel>> {
        let kind = Kind::of(files)?;
        let provider = kind.provider(accelerator)?;
        Ok(match kind {
            Kind::Whisper => Box::new(Whisper::load(config::recognizer(files, provider)?)?),
            Kind::Kokoro => Box::new(Kokoro::load(config::tts(files, provider)?)?),
        })
    }
}

/// Which family a build is, for what its config does not cover: it follows from its files' keys (see *Files*).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Whisper,
    Kokoro,
}

impl Kind {
    /// A `whisper.` key makes it Whisper, a `kokoro.` one Kokoro; anything else is `unsupported-model`.
    fn of(files: &Installed) -> Result<Self> {
        let has = |prefix: &str| files.files.keys().any(|key| key.starts_with(prefix));
        if has("whisper.") {
            Ok(Self::Whisper)
        } else if has("kokoro.") {
            Ok(Self::Kokoro)
        } else {
            Err(Error::new("unsupported-model"))
        }
    }

    /// The execution provider this model runs on with `accelerator`, or `unsupported-accelerator`, checked before the
    /// model is created: Kokoro on Core ML throws from inside the library, which aborts the process (see
    /// *Accelerators*).
    fn provider(self, accelerator: Accelerator) -> Result<&'static str> {
        if self == Self::Kokoro && accelerator != Accelerator::Cpu {
            return Err(Error::new("unsupported-accelerator"));
        }
        provider(accelerator)
    }
}

/// ONNX Runtime's name for `accelerator`'s execution provider, or `unsupported-accelerator` for one this backend does
/// not declare.
fn provider(accelerator: Accelerator) -> Result<&'static str> {
    match accelerator {
        Accelerator::Cpu => Ok("cpu"),
        Accelerator::CoreMl => Ok("coreml"),
        _ => Err(Error::new("unsupported-accelerator")),
    }
}

/// `text` as the crate takes it: the C API cannot take a NUL inside it, and the crate panics on one, so it is refused
/// here first (`invalid-text`).
fn text(text: &str) -> Result<String> {
    if text.contains('\0') {
        return Err(Error::new("invalid-text"));
    }
    Ok(text.to_owned())
}

/// The threads ONNX Runtime runs a model on: the machine's, up to 4, past which these small models gain little.
fn num_threads() -> i32 {
    std::thread::available_parallelism().map_or(1, |n| i32::try_from(n.get().min(4)).unwrap_or(1))
}
