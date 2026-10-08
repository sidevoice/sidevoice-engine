// Linked only where the `sherpa-onnx` feature (default) brings the official crate in, natively: build.rs.
#![cfg(sherpa_onnx)]
//! sherpa-onnx, on ONNX Runtime: speech to text with Whisper and with transducers (NeMo's FastConformer and Parakeet),
//! and text to speech with Kokoro, VITS (Piper's voices) and Supertonic.
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
//! Every text-to-speech model speaks through `synthesizer.rs`, which names the voices and tells a model its language;
//! a model's file only builds its config.
//!
//! # Files
//!
//! - Whisper (catalogue): `encoder`, `decoder` and `tokens`.
//! - A transducer: `encoder`, `decoder`, `joiner` and `tokens`.
//! - Kokoro: `model`, `voices`, `tokens`, and `espeak-ng-data`, a directory; a multilingual one, `lexicon` too.
//! - VITS: `model`, `tokens` and `espeak-ng-data`.
//! - Supertonic: `duration_predictor`, `text_encoder`, `vector_estimator`, `vocoder`, `tts_json`, `unicode_indexer`
//!   and `voice_style`.
//!
//! Which a build is follows from its files ([`Kind::of`]).
//!
//! # Accelerators
//!
//! The CPU only, in this phase. The static libraries the crate links are built without ONNX Runtime's Core ML
//! execution provider: asked for it, sherpa-onnx logs "Fallback to cpu" and runs on the CPU. So the backend declares
//! the CPU alone rather than claim Core ML. The shared libraries it downloaded before did run Whisper, the transducers,
//! Piper and Supertonic on Core ML; Core ML comes back with them (sidevoice-engine#33). CUDA needs sherpa-onnx's CUDA
//! builds, which are not linked either.
//!
//! Kokoro must stay off Core ML whatever the libraries: creating its TTS on Core ML throws a C++ exception that the
//! C API does not catch, and an exception that reaches Rust aborts the process. `load` refuses it with
//! `unsupported-accelerator` before creating it, and Kokoro's catalogue builds accept the CPU only.

use async_trait::async_trait;
use sherpa_onnx::OfflineRecognizer;

use crate::backend::{Backend, BackendFactory, BackendSpec, Library, LoadedModel};
use crate::catalog::Build;
use crate::host::Accelerator;
use crate::install::Installed;
use crate::{Error, Result};

#[cfg(test)]
mod inference_tests;
mod kokoro;
mod model_metadata;
mod supertonic;
mod synthesizer;
#[cfg(test)]
mod tests;
mod transducer;
mod vits;
mod whisper;

use transducer::Transducer;
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
            Kind::Whisper => Box::new(Whisper::load(files, provider)?),
            Kind::Transducer => Box::new(Transducer::load(files, provider)?),
            Kind::Kokoro => Box::new(kokoro::load(files, provider)?),
            Kind::Vits => Box::new(vits::load(files, provider)?),
            Kind::Supertonic => Box::new(supertonic::load(files, provider)?),
        })
    }
}

/// Which of the models this backend runs a build is: it follows from the build's files (see *Files*).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Whisper,
    Transducer,
    Kokoro,
    Vits,
    Supertonic,
}

impl Kind {
    /// By the file only it has: `joiner` makes it a transducer, an `encoder` without one Whisper, `voices` Kokoro,
    /// `duration_predictor` Supertonic, and a `model` with none of those VITS; anything else is `unsupported-model`.
    fn of(files: &Installed) -> Result<Self> {
        let has = |key| files.file(key).is_some();
        if has("joiner") {
            Ok(Self::Transducer)
        } else if has("encoder") {
            Ok(Self::Whisper)
        } else if has("voices") {
            Ok(Self::Kokoro)
        } else if has("duration_predictor") {
            Ok(Self::Supertonic)
        } else if has("model") {
            Ok(Self::Vits)
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
/// not name.
fn provider(accelerator: Accelerator) -> Result<&'static str> {
    match accelerator {
        Accelerator::Cpu => Ok("cpu"),
        Accelerator::CoreMl => Ok("coreml"),
        _ => Err(Error::new("unsupported-accelerator")),
    }
}

/// The sample rate the speech-to-text models take, as `SttModel::transcribe` does.
const SAMPLE_RATE: i32 = 16_000;

/// What `recognizer` hears in `pcm` (mono, at [`SAMPLE_RATE`]), on the calling thread: the engine decides where to run
/// it. Fails with `transcription-failed`.
fn transcribe(recognizer: &OfflineRecognizer, pcm: &[f32]) -> Result<String> {
    let stream = recognizer.create_stream();
    stream.accept_waveform(SAMPLE_RATE, pcm);
    recognizer.decode(&stream);
    let result = stream
        .get_result()
        .ok_or(Error::new("transcription-failed"))?;
    Ok(result.text.trim().to_owned())
}

/// The recognizer `config` describes, or `model-load-failed`.
fn recognizer(config: &sherpa_onnx::OfflineRecognizerConfig) -> Result<OfflineRecognizer> {
    OfflineRecognizer::create(config).ok_or(Error::new("model-load-failed"))
}

/// Where the host keeps the installed file `name`, for the crate: `file-not-installed`, or `invalid-text` for a path
/// the C API cannot take.
fn path(files: &Installed, name: &str) -> Result<String> {
    let path = files.file(name).ok_or(Error::new("file-not-installed"))?;
    text(path)
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

/// The primary language subtag of the BCP 47 tag `tag`, lower-cased: `es-ES` → `es`.
fn primary_language(tag: &str) -> String {
    tag.split(['-', '_'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}
