//! Backends: what runs models (sherpa-onnx, whisper.cpp, MLX, transformers.js, ...). This file is the contract every
//! backend implements ([`Backend`], with its data in [`BackendSpec`]); the engine does the matching, ranking,
//! selection and installing for every backend alike (`crate::resolver`, `crate::install`). Backends belong to the
//! engine: none of this is public, except a backend's id and what [`BackendInfo`] says of it.
//!
//! Inside: `requirement` (what the machine must meet, and the common requirements), `registry` (how the backends of
//! this build are found), `library` (what `open` returns, which loads models), `loaded_model` (what `load` returns)
//! and `implementations` (one file per backend).
//!
//! # The contract
//!
//! A backend is its record, an optional `probe`, its `open` and its library's `load`. Everything else is somebody
//! else's:
//!
//! - which models it runs: each catalogue build names its backend ([`BuildEntry::backend`](crate::BuildEntry::backend));
//! - what it downloads: the build's files, from the catalogue; a backend's own library comes with the engine for now
//!   (see *Binding the library*), so it downloads nothing of its own;
//! - whether a build fits here, which build and accelerator win, and installing them: the resolver and the installer;
//! - when its library is opened and closed, and when a model leaves memory: the engine (see *`open` and `load`*).
//!
//! ## `spec`: the record
//!
//! A `const` [`BackendSpec`], returned by reference. It must be plain data: no I/O, no allocation, the same answer
//! every time, callable before anything is installed. Its `id` is stable (catalogue builds name the backend by it,
//! and it is one of [`KNOWN`]); its `name`, `description` and `upstream` say what it is, as
//! [`Engine::backends`](crate::Engine::backends) lists it; and its `accelerators` and `requirements` hold whatever
//! the model: a build's own needs, such as its memory, are the catalogue's. A requirement fails with a stable
//! [`Reason`](crate::Reason) code and its numbers, never with a sentence (AGENTS.md); the engine ships
//! [`MinMemoryMb`] and [`MinCores`], and a backend that needs another check writes its own [`Requirement`] next to
//! its file without changing the contract.
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
//! Called only for the chosen build, after the installer has put the build's files in storage. `open` opens the
//! backend's library (see *Binding the library*) and returns it as a [`Library`]; it is handed the build's `files`,
//! which a library that comes with the engine does not need. The library's `load` loads one model. The engine keeps
//! one open library per backend, held by the models loaded from it: it opens the library for the first of them and
//! drops it once the last one has left memory (dropped by the app; its files stay installed). So a library outlives
//! every model it loaded, and a model may rely on it. `load`:
//!
//! - finds each file it needs in `files` by its key in the catalogue;
//! - loads the model's files on the `accelerator` it is given, one that `probe` returned; it does not fall back to
//!   another one by itself (the engine decides that, with a new selection);
//! - returns a [`BackendModel`] that is a speech-to-text model, a text-to-speech model, a voice activity detector, or
//!   several, and keeps nothing:
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
//! - `unsupported-language`: this model does not know the language it was asked for.
//! - `not-implemented`: this backend cannot load models yet (the stubs, which fail to `open` with it).
//!
//! What a loaded model fails with is shared the same way: `transcription-failed` (the speech could not be
//! transcribed), `speech-failed` (the text could not be spoken), `unknown-voice` (the model has no such voice),
//! `invalid-text` (the text has a character the backend cannot take) and `detection-failed` (the audio could not be
//! run through the voice activity detector).
//!
//! ## Remote backends
//!
//! A remote backend runs models on a provider's servers (OpenAI, ElevenLabs), and says so in its record
//! ([`BackendSpec::provider`]). Its builds have no files, only the provider's id of the model (`api_model`): installing
//! one only checks that the host has the provider's key, and nothing is stored. Its accelerator is
//! [`Accelerator::Remote`], which every host has. Its `load` makes no call; its model calls the provider through the
//! host ([`Host::http`](crate::Host::http)), with the key the host hands it for that call
//! ([`Host::credentials`](crate::Host::credentials)), and keeps no key. A remote model fails with the shared codes, and
//! these: `credential-missing` (the host has no key for the provider), `credential-rejected` (the provider refused it),
//! `rate-limited` (the provider asks to slow down), and the host's `request-failed` (no answer) and
//! `credentials-failed` (the keys could not be read).
//!
//! ## Binding the library
//!
//! The design is that nothing heavy is linked into the app: a backend's engine library is downloaded when a model
//! needs it, like the model.
//!
//! - native: the backend opens the downloaded library at run time and calls its C API through a table of function
//!   pointers resolved then. **Phase 1 exceptions:** sherpa-onnx is linked, through the official `sherpa-onnx` crate
//!   (static, pinned exactly in Cargo.toml, native builds only), and so is whisper.cpp, through
//!   `whisper-rs` (compiled from the sources it bundles, the same way), so neither downloads anything. Loading them on demand is sidevoice-engine#33, which also
//!   decides where a downloaded library's files are declared. Any other native backend follows the design;
//! - web: the backend's engine is a JavaScript module, and the backend imports it at run time (a dynamic `import()`
//!   through `wasm-bindgen`), so a page that never loads a model never fetches it. The module comes with the npm
//!   package; it is never compiled into the engine's WebAssembly.
//!
//! ## Voice activity: a stream
//!
//! A voice activity detector is the one streaming model: its model makes streams, each with its own state, fed one
//! window of samples at a time at the model's rate. A backend whose library segments speech itself (sherpa-onnx)
//! reports its segments; one that only computes a probability per window segments it with `segmenter`, by the same
//! rules, so the events mean the same on every backend.
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
pub(crate) mod remote;
mod requirement;
mod segmenter;
#[cfg(test)]
mod tests;

pub(crate) use library::{Library, Load};
pub(crate) use loaded_model::{BackendModel, VadModel, VadStreamModel, Window};
#[cfg(test)]
pub(crate) use loaded_model::{SttModel, TtsModel};
pub(crate) use registry::{built_in, find};
#[allow(
    unused_imports,
    reason = "a common requirement no backend declares yet"
)]
pub(crate) use requirement::MinCores;
pub(crate) use requirement::{MinMemoryMb, Requirement};
#[cfg_attr(
    native,
    allow(
        unused_imports,
        reason = "only the web build's backend segments speech itself"
    )
)]
pub(crate) use segmenter::Segmenter;

/// A backend's stable id, as catalogue builds name it ([`BuildEntry::backend`](crate::BuildEntry::backend)): "sherpa-onnx",
/// "whisper-cpp", "mlx", ...
pub type BackendId = &'static str;

/// What a backend is and needs, as data: a `const`, no I/O (see *`spec`: the record* above). Adding a backend is
/// mostly filling this in.
pub(crate) struct BackendSpec {
    /// What catalogue builds call it. Stable: renaming it orphans them. It must be one of [`KNOWN`].
    pub(crate) id: BackendId,
    /// Its name, as a person reads it: "sherpa-onnx", "MLX", "Transformers.js".
    pub(crate) name: &'static str,
    /// What it is, in one sentence, in English (it is not UI: the app has its own text for each id).
    pub(crate) description: &'static str,
    /// Where it comes from: its upstream repository.
    pub(crate) upstream: &'static str,
    /// The accelerators it can run on, best first: the default is the first one that works here.
    pub(crate) accelerators: &'static [Accelerator],
    /// What the machine must meet, whatever the model: each one a check on the capabilities.
    pub(crate) requirements: &'static [&'static dyn Requirement],
    /// For a remote backend, the provider whose API it calls, by the id the host's [`Credentials`](crate::Credentials)
    /// know its key by (`"openai"`); `None` for a backend that runs models here. A remote backend's builds have no
    /// files: installing one only checks that the host has the key, and its models make their calls through the host.
    pub(crate) provider: Option<&'static str>,
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

    /// Opens this backend's library, for the engine to load models with; `files` are the build's installed files. It
    /// downloads nothing, reads nothing outside `files`, keeps nothing, and fails with a stable code (see
    /// *`open` and `load`* above).
    async fn open(&self, files: &Installed) -> Result<Box<dyn Library>>;
}

/// Every backend id a catalogue may name: the [`BackendSpec::id`] of each backend, in whichever build of the engine it
/// is compiled, and the ids of backends whose code is still to come, which the catalogue may already name. A build
/// naming any other is a catalogue problem (`UnknownBackend`).
pub(crate) const KNOWN: &[BackendId] = &[
    "sherpa-onnx",
    "mlx",
    "transformers-js",
    "whisper-cpp",
    "openai",
    "elevenlabs",
];

/// Whether `backend` is one of [`KNOWN`].
pub(crate) fn is_known(backend: &str) -> bool {
    KNOWN.contains(&backend)
}

/// Whether `backend` is a remote backend of this build ([`BackendSpec::provider`]). Remote backends compile into every
/// build: they run nothing here.
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "the engine asks its own backends; the catalogue's tests ask the registry"
    )
)]
pub(crate) fn is_remote(backend: &str) -> bool {
    built_in()
        .iter()
        .any(|built| built.spec().id == backend && built.spec().provider.is_some())
}

/// A backend compiled into this build, as [`Engine::backends`](crate::Engine::backends) lists it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BackendInfo {
    /// Its stable id: what catalogue builds call it.
    pub id: BackendId,
    /// Its name, as a person reads it.
    pub name: &'static str,
    /// What it is, in one sentence, in English: a developer's description, not UI.
    pub description: &'static str,
    /// Its upstream repository.
    pub upstream: &'static str,
}

impl BackendInfo {
    /// What `spec` says of its backend.
    pub(crate) fn of(spec: &BackendSpec) -> Self {
        Self {
            id: spec.id,
            name: spec.name,
            description: spec.description,
            upstream: spec.upstream,
        }
    }
}
