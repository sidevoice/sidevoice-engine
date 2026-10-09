// Native only: whisper-rs compiles whisper.cpp for the target (its dependency is native-only in Cargo.toml).
#![cfg(native)]
//! whisper.cpp, on ggml: speech to text with Whisper's ggml builds (the catalogue's `whisper-cpp` builds, files from
//! ggerganov/whisper.cpp). One whole turn at a time, in the language asked for or the one it detects, decoded greedily,
//! without timestamps.
//!
//! # Binding
//!
//! `whisper-rs`, pinned to one exact version in Cargo.toml, through its safe API (`WhisperContext`, `WhisperState`,
//! `FullParams`). Its build script compiles the whisper.cpp and ggml it bundles with CMake and links them statically:
//! in this first phase the backend is linked, as sherpa-onnx is, and nothing is downloaded for it. So `open` has nothing
//! to open, and the contract (`open`, then the library's `load`) stays as it is for when loading it on demand comes
//! (sidevoice-engine#33).
//!
//! whisper.cpp logs what it does to the standard error (the model it reads, the backend it runs on), as it comes: the
//! engine does not redirect it.
//!
//! # Files
//!
//! `model`: the ggml file, which holds the weights, the vocabulary and the mel filters.
//!
//! # Accelerators
//!
//! Metal on Apple silicon, the only build that compiles ggml's Metal backend in (the `metal` feature of whisper-rs, in
//! Cargo.toml), and the CPU everywhere. On Metal the model asks whisper.cpp for its GPU; should Metal not start,
//! whisper.cpp says so in its log and runs on the CPU. Windows is not built in CI: it should compile with a C++
//! toolchain and CMake, and is untested. CUDA and Vulkan are features of whisper-rs that are not turned on.
//!
//! # Languages
//!
//! Whisper reads the primary language subtag of the tag it is given (`es-ES` → `es`); one it does not know fails with
//! `unsupported-language`. With none, it detects the language from the first 30 seconds.

use std::collections::HashMap;
use std::ffi::c_int;

use async_trait::async_trait;
use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState,
};

use crate::backend::loaded_model::SttModel;
use crate::backend::registry::BackendFactory;
use crate::backend::{Backend, BackendModel, BackendSpec, Library, Load};
use crate::host::Accelerator;
use crate::install::Installed;
use crate::{Error, Result};

#[cfg(test)]
mod tests;

struct WhisperCpp;

/// Metal where it is compiled in (see *Accelerators*), then the CPU.
#[cfg(apple_silicon)]
const ACCELERATORS: &[Accelerator] = &[Accelerator::Metal, Accelerator::Cpu];
#[cfg(not(apple_silicon))]
const ACCELERATORS: &[Accelerator] = &[Accelerator::Cpu];

const SPEC: BackendSpec = BackendSpec {
    id: "whisper-cpp",
    name: "whisper.cpp",
    description: "Whisper in C/C++ on ggml, through whisper-rs, linked into native builds for now \
                  (sidevoice-engine#33).",
    upstream: "https://github.com/ggml-org/whisper.cpp",
    accelerators: ACCELERATORS,
    requirements: &[],
    provider: None,
};

inventory::submit! { BackendFactory(|| Box::new(WhisperCpp)) }

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl Backend for WhisperCpp {
    fn spec(&self) -> &BackendSpec {
        &SPEC
    }

    /// The library is linked: there is nothing to open, and nothing installed is read.
    async fn open(&self, _files: &Installed) -> Result<Box<dyn Library>> {
        Ok(Box::new(Linked))
    }
}

/// The linked whisper.cpp, which loads the model.
struct Linked;

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl Library for Linked {
    async fn load(&self, load: Load<'_>) -> Result<Box<dyn BackendModel>> {
        Ok(Box::new(Whisper::load(load.files, load.accelerator)?))
    }
}

/// A Whisper model in memory: its state (the buffers one transcription runs in, reused by the next, which waits for it)
/// and the decoding parameters for each language it has been asked for.
struct Whisper {
    state: WhisperState,
    /// By whisper.cpp's language id, `None` for detecting it. whisper-rs gives up the language's C string it hands
    /// whisper.cpp each time a language is set, so each language's parameters are made once and copied after that.
    params: HashMap<Option<c_int>, FullParams<'static, 'static>>,
}

impl Whisper {
    /// Reads the `model` in `files` on `accelerator`: `file-not-installed`, `unsupported-accelerator`, or
    /// `model-load-failed`.
    fn load(files: &Installed, accelerator: Accelerator) -> Result<Self> {
        let path = files
            .file("model")
            .ok_or(Error::new("file-not-installed"))?;
        let use_gpu = match accelerator {
            Accelerator::Metal if cfg!(apple_silicon) => true,
            Accelerator::Cpu => false,
            _ => return Err(Error::new("unsupported-accelerator")),
        };
        if path.contains('\0') {
            return Err(Error::new("model-load-failed"));
        }
        let mut context = WhisperContextParameters::default();
        context.use_gpu(use_gpu);
        let context = WhisperContext::new_with_params(path, context)
            .map_err(|_| Error::new("model-load-failed"))?;
        let state = context
            .create_state()
            .map_err(|_| Error::new("model-load-failed"))?;
        Ok(Self {
            state,
            params: HashMap::new(),
        })
    }

    /// The parameters for `language`: greedy, no timestamps, no context carried over from the previous turn, nothing
    /// printed, and whisper.cpp's own thread count (the machine's, up to 4).
    fn params(&mut self, language: Option<&str>) -> Result<FullParams<'static, 'static>> {
        let id = language.map(language_id).transpose()?;
        let params = self.params.entry(id).or_insert_with(|| {
            let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
            params.set_language(Some(
                id.and_then(whisper_rs::get_lang_str).unwrap_or("auto"),
            ));
            params.set_no_timestamps(true);
            params.set_no_context(true);
            params.set_print_special(false);
            params.set_print_progress(false);
            params.set_print_realtime(false);
            params.set_print_timestamps(false);
            params
        });
        Ok(params.clone())
    }
}

/// whisper.cpp's id for the language of the BCP 47 tag `tag`, by its primary subtag, lower-cased (`es-ES` → `es`), or
/// `unsupported-language`.
fn language_id(tag: &str) -> Result<c_int> {
    let code = tag
        .split(['-', '_'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    // A NUL would make whisper-rs panic; whisper.cpp's codes are letters only.
    if code.is_empty() || !code.bytes().all(|byte| byte.is_ascii_lowercase()) {
        return Err(Error::new("unsupported-language"));
    }
    whisper_rs::get_lang_id(&code).ok_or(Error::new("unsupported-language"))
}

impl BackendModel for Whisper {
    fn as_stt(&mut self) -> Option<&mut dyn SttModel> {
        Some(self)
    }

    fn memory_mb(&self) -> Option<u32> {
        None
    }
}

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl SttModel for Whisper {
    /// On the calling thread: the engine decides where to run it. Silence (no samples) is heard as nothing.
    async fn transcribe(&mut self, pcm: &[f32], language: Option<&str>) -> Result<String> {
        let params = self.params(language)?;
        if pcm.is_empty() {
            return Ok(String::new());
        }
        self.state
            .full(params, pcm)
            .map_err(|_| Error::new("transcription-failed"))?;
        let mut text = String::new();
        for segment in self.state.as_iter() {
            let piece = segment
                .to_str_lossy()
                .map_err(|_| Error::new("transcription-failed"))?;
            text.push_str(&piece);
        }
        Ok(text.trim().to_owned())
    }
}
