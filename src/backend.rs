//! Backends: what runs models (sherpa-onnx, whisper.cpp, MLX, transformers.js, ...). This file is the contract every
//! backend implements ([`Backend`], with its data in [`BackendSpec`]); the engine does the matching, ranking,
//! selection and installing for every backend alike (`crate::resolver`, `crate::install`). Backends belong to the
//! engine: none of this is public, except a backend's id.
//!
//! Inside: `runtime` (the library files each backend needs per platform, from `backends.json`), `requirement` (what
//! the machine must meet, and the common requirements), `registry` (how the backends of this build are found),
//! `library` (what `open` returns, which loads models), `loaded_model` (what `load` returns) and `implementations`
//! (one file per backend).
//!
//! # The contract
//!
//! A backend is its record, an optional `probe`, its `open` and its library's `load`. Everything else is somebody
//! else's:
//!
//! - which models it runs: each catalogue build names its backend ([`Build::backend`](crate::Build::backend));
//! - what it downloads: its entry in `backends.json`, read by `runtime`, with its files per platform, which the
//!   installer fetches next to the model's files; where the entry says it does not run (`null`), the resolver rejects
//!   its builds with `no-runtime-for-platform` before asking the backend anything;
//! - whether a build fits here, which build and accelerator win, and installing them: the resolver and the installer;
//! - when its library is opened and closed, and when a model leaves memory: the engine (see *`open` and `load`*).
//!
//! ## `spec`: the record
//!
//! A `const` [`BackendSpec`], returned by reference. It must be plain data: no I/O, no allocation, the same answer
//! every time, callable before anything is installed. Its `id` is stable (catalogue builds and `backends.json` name
//! the backend by it) and its `accelerators` and `requirements` hold whatever the model: a build's own needs, such as
//! its memory, are the catalogue's. A requirement fails with a stable [`Reason`](crate::Reason) code and its numbers,
//! never with a sentence (AGENTS.md); the engine ships [`MinMemoryMb`] and [`MinCores`], and a backend that needs
//! another check writes its own [`Requirement`] next to its file without changing the contract.
//!
//! ## `probe`: which accelerators work here
//!
//! The host reports what is present; the default keeps the declared accelerators it reports, in the declared order,
//! and is right for most backends. A backend overrides it only when trying is the only way to know whether it can
//! use one: a CUDA driver of the right version, Core ML for its operators. An override:
//!
//! - narrows: it returns a subset of the declared accelerators the host reported, best first, never one the host
//!   left out; an empty list means the backend does not run here, and the resolver rejects its builds with
//!   `no-accelerator`;
//! - must be quick and must not panic: no downloads, no model, no library of its own (it may not be installed yet);
//!   it may ask the system (load the CUDA driver, query a device) and answers "no" when that fails;
//! - is called at most once per backend and resolver: the answer is cached, so it must not depend on the model.
//!
//! ## `open` and `load`: from installed files to a running model
//!
//! Called only for the selected build, after the installer has put the build's files and this backend's
//! `backends.json` files for this platform in storage. `open` opens the backend's library (see *Binding the library*)
//! from its files, each found in `files` by its `name` in `backends.json`, and returns it as a [`Library`]; the
//! library's `load` loads one model. The engine keeps one open library per backend, counted by the models loaded from
//! it: it opens the library for the first of them and drops it once the last one has left memory (a model unused for a
//! while is unloaded; its files stay installed). So a library outlives every model it loaded, and a model may rely on
//! it. `load`:
//!
//! - finds each file it needs in `files` by its key in the catalogue;
//! - loads the model's files on the `accelerator` it is given, one that `probe` returned; it does not fall back to
//!   another one by itself (the engine decides that, with a new selection);
//! - returns a [`LoadedModel`] that is a speech-to-text model, a text-to-speech model, or both, and keeps nothing:
//!   the engine owns what it returns, and the backend object stays empty and stateless (what is open lives in the
//!   library).
//!
//! Neither may download, fetch or write anything, nor read files that are not in `files`: the engine does not reach
//! for the network or the file system by itself, and the host only hands over what it installed. It fails with a
//! stable [`Error`](crate::Error) code, never with a sentence: the codes say what failed (the library did not open,
//! the model did not load) and are shared by every backend, so a client translates them once; the cause goes to the
//! logs. The codes, with the engine's English text for each:
//!
//! - `file-not-installed`: a file the model needs is not installed.
//! - `library-open-failed`: the backend's library could not be opened.
//! - `model-load-failed`: the model's files could not be loaded.
//! - `unsupported-model`: this backend cannot run this model's files.
//! - `unsupported-accelerator`: this backend cannot run on this accelerator.
//! - `not-implemented`: this backend cannot load models yet (the stubs, which fail to `open` with it).
//!
//! What a loaded model fails with is shared the same way: `transcription-failed` (the speech could not be
//! transcribed), `speech-failed` (the text could not be spoken), `unknown-voice` (the model has no such voice) and
//! `invalid-text` (the text has a character the backend cannot take).
//!
//! ## Binding the library
//!
//! Nothing heavy is linked into the app, so a backend's engine library is never a build-time dependency:
//!
//! - native: the backend declares the library's C API in Rust (hand-written `extern "C"` signatures, or generated
//!   once by bindgen and checked in) and opens the downloaded library at run time with `libloading`, resolving those
//!   symbols into a table of function pointers; the first native backend that loads something adds `libloading` to
//!   the native dependencies. A `-sys` crate that links at build time, or that downloads at build time, is not used;
//! - web: the backend's engine is a JavaScript module, and the backend imports it at run time (a dynamic `import()`
//!   through `wasm-bindgen`), so a page that never loads a model never fetches it. Where the module comes from is
//!   the backend's `backends.json` entry's to say: files the host stored, or `[]` when it comes with the npm
//!   package; either way it is never compiled into the engine's WebAssembly.
//!
//! ## Speech out: a whole buffer
//!
//! A text-to-speech model returns the whole utterance as one buffer, with its sample rate. A caller that wants
//! speech sooner splits the text into sentences and speaks them one by one. Streaming within an utterance, if it is
//! ever needed, changes the loaded model's interface alone, not `spec`, `probe` or `load`.

use async_trait::async_trait;

use crate::host::{Accelerator, Capabilities};
use crate::install::Installed;
use crate::maybe_send::{MaybeSend, MaybeSync};
use crate::Result;

mod implementations;
mod library;
mod loaded_model;
mod registry;
mod requirement;
mod runtime;
#[cfg(test)]
mod tests;

pub(crate) use library::Library;
pub(crate) use loaded_model::LoadedModel;
pub(crate) use registry::{built_in, find, BackendFactory};
#[allow(
    unused_imports,
    reason = "a common requirement no backend declares yet"
)]
pub(crate) use requirement::MinCores;
pub(crate) use requirement::{MinMemoryMb, Requirement};
pub(crate) use runtime::{is_known, runtime_files};

/// A backend's stable id, as catalogue builds name it ([`Build::backend`](crate::Build::backend)): "sherpa-onnx",
/// "whisper-cpp", "mlx", ...
pub type BackendId = &'static str;

/// What a backend is and needs, as data: a `const`, no I/O (see *`spec`: the record* above). Adding a backend is
/// mostly filling this in.
pub(crate) struct BackendSpec {
    /// What catalogue builds and `backends.json` call it. Stable: renaming it orphans their entries.
    pub(crate) id: BackendId,
    /// The accelerators it can run on, best first: the default is the first one that works here.
    pub(crate) accelerators: &'static [Accelerator],
    /// What the machine must meet, whatever the model: each one a check on the capabilities.
    pub(crate) requirements: &'static [&'static dyn Requirement],
}

/// What runs models: its data ([`BackendSpec`]), which of its accelerators work here, and opening its library.
#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
pub(crate) trait Backend: MaybeSend + MaybeSync {
    /// What it is and needs: its `const` record, the same every time.
    fn spec(&self) -> &BackendSpec;

    /// Which of the declared accelerators work here, best first. By default, the ones the host reports. A backend
    /// overrides it only when trying is the only way to know (a CUDA driver, a WebGPU adapter, Core ML): quickly,
    /// without its library or a model, answering "no" rather than failing. The engine caches the answer, and keeps
    /// only what the host reports: a probe narrows, it never adds an accelerator the host did not see (see *`probe`*
    /// above).
    fn probe(&self, caps: &Capabilities) -> Vec<Accelerator> {
        self.spec()
            .accelerators
            .iter()
            .copied()
            .filter(|accelerator| caps.has(*accelerator))
            .collect()
    }

    /// Opens this backend's library from its installed files, each by name in `files`, for the engine to load models
    /// with. It downloads nothing, reads nothing outside `files`, keeps nothing, and fails with a stable code (see
    /// *`open` and `load`* above).
    async fn open(&self, files: &Installed) -> Result<Box<dyn Library>>;
}
