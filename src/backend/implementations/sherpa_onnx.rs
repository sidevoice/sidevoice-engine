// The sherpa-onnx library exists on every native platform.
#![cfg(native)]
//! sherpa-onnx: speech to text with Whisper and text to speech with Kokoro, on ONNX Runtime.
//!
//! # Binding
//!
//! Our own thin FFI over sherpa-onnx's C API (`c_api`, hand-written from the header), with the library that
//! `backends.json` pins opened at run time with `libloading` (`library`). Not a crate such as `sherpa-rs`: those link
//! the library at build time, or download it while building, so the app would carry ONNX Runtime whether or not
//! anyone loads a model, and the version would be the crate's rather than `backends.json`'s. Here the library is
//! one more installed file, fetched on demand like a model and opened by the first model that needs it.
//!
//! # Files
//!
//! - `library` (`backends.json`): where the installer unpacked the platform's archive (the C API's library is found
//!   below it, in `lib/`), or the C API's library itself; ONNX Runtime is beside it.
//! - Whisper (catalogue): `encoder`, `decoder` and `tokens`.
//! - Kokoro (catalogue): `model`, `voices`, `tokens`, and `espeak-ng-data`, a directory.
//!
//! Which of the two a build is follows from its files: `encoder` makes it Whisper, `voices` Kokoro.
//!
//! # Accelerators
//!
//! Core ML on macOS (ONNX Runtime's Core ML execution provider, which runs on the CPU whatever it cannot place) and
//! the CPU everywhere. CUDA needs sherpa-onnx's CUDA builds, which `backends.json` does not pin yet.
//!
//! Kokoro runs on the CPU only: creating its TTS on Core ML throws a C++ exception that the C API does not catch,
//! and an exception that reaches Rust aborts the process. `load` refuses it with `unsupported-accelerator` before the
//! library is touched, and Kokoro's catalogue builds should accept the CPU only.

use std::ffi::CString;
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;

use crate::backend::LoadedModel;
use crate::backend::{Backend, BackendFactory, BackendSpec};
use crate::catalog::Build;
use crate::host::Accelerator;
use crate::install::Installed;
use crate::{Error, Result};

mod c_api;
#[cfg(test)]
mod inference_tests;
mod kokoro;
mod library;
mod model_metadata;
#[cfg(test)]
mod tests;
mod whisper;

use kokoro::Kokoro;
use library::Api;
use whisper::Whisper;

struct SherpaOnnx;

const SPEC: BackendSpec = BackendSpec {
    id: "sherpa-onnx",
    accelerators: &[Accelerator::CoreMl, Accelerator::Cpu],
    requirements: &[],
};

inventory::submit! { BackendFactory(|| Box::new(SherpaOnnx)) }

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl Backend for SherpaOnnx {
    fn spec(&self) -> &BackendSpec {
        &SPEC
    }

    async fn load(
        &self,
        _build: &Build,
        accelerator: Accelerator,
        files: &Installed,
    ) -> Result<Box<dyn LoadedModel>> {
        let provider = provider(accelerator)?;
        let is_whisper = files.file("encoder").is_some();
        if !is_whisper && files.file("voices").is_none() {
            return Err(Error::new("unsupported-model"));
        }
        // Kokoro on Core ML throws from inside the library, which aborts the process (see *Accelerators*).
        if !is_whisper && accelerator != Accelerator::Cpu {
            return Err(Error::new("unsupported-accelerator"));
        }
        let api = Arc::new(Api::open(Path::new(path(files, "library")?))?);
        Ok(if is_whisper {
            Box::new(Whisper::load(api, files, provider)?)
        } else {
            Box::new(Kokoro::load(api, files, provider)?)
        })
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

/// Where the host keeps the installed file `name`, or `file-not-installed`.
fn path<'a>(files: &'a Installed, name: &str) -> Result<&'a str> {
    files.file(name).ok_or(Error::new("file-not-installed"))
}

/// `text` for the C API, which cannot take a NUL inside it (`invalid-text`).
fn c_string(text: &str) -> Result<CString> {
    CString::new(text).map_err(|_| Error::new("invalid-text"))
}

/// The threads ONNX Runtime runs a model on: the machine's, up to 4, past which these small models gain little.
fn num_threads() -> i32 {
    std::thread::available_parallelism().map_or(1, |n| i32::try_from(n.get().min(4)).unwrap_or(1))
}
