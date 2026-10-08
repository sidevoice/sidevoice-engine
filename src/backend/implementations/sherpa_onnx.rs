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
//! - Whisper (catalogue): `encoder`, `decoder` and `tokens`.
//! - Kokoro (catalogue): `model`, `voices`, `tokens`, and `espeak-ng-data`, a directory.
//!
//! Which a build is follows from its files ([`Kind::of`]).
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
            Kind::Whisper => Box::new(Whisper::load(files, provider)?),
            Kind::Kokoro => Box::new(Kokoro::load(files, provider)?),
        })
    }
}

/// Which of the models this backend runs a build is: it follows from the build's files (see *Files*).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Whisper,
    Kokoro,
}

impl Kind {
    /// `encoder` makes it Whisper, `voices` Kokoro; anything else is `unsupported-model`.
    fn of(files: &Installed) -> Result<Self> {
        if files.file("encoder").is_some() {
            Ok(Self::Whisper)
        } else if files.file("voices").is_some() {
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
