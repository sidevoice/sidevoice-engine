// Native only: it runs on the ONNX Runtime that sherpa-onnx links into native builds.
#![cfg(native)]
//! ONNX Runtime: ONNX models that no other backend's library knows how to configure, run directly. Today smart-turn
//! v3, end of turn.
//!
//! # Binding
//!
//! Native builds already link one ONNX Runtime, statically, inside sherpa-onnx's libraries (1.28 for sherpa-onnx
//! 1.13.8). This backend runs on that one rather than link a second: the `ort` crate's safe API, built with
//! `alternative-backend`, so it downloads and links no runtime of its own, and `open` hands it the linked runtime's C
//! API (`OrtGetApiBase`, resolved when the app is linked, asked for API 17). What it adds to an app is `ort`'s glue.
//! Where a runtime comes from once runtimes load on demand is sidevoice-engine#33 (and a generic ONNX Runtime backend,
//! #25): this backend then takes it from there.
//!
//! # Files
//!
//! - smart-turn: `smart_turn`, its ONNX model, given the features `crate::backend::smart_turn` makes (the same as on
//!   the web) as `input_features`; its one output is the probability that the turn is complete.
//!
//! # Accelerators
//!
//! The CPU: the linked runtime has no other execution provider (see sherpa-onnx's *Accelerators*).

use std::sync::OnceLock;

use async_trait::async_trait;
use ort::session::Session;
use ort::value::Tensor;

use crate::backend::registry::BackendFactory;
use crate::backend::smart_turn::{self, FRAMES, INPUT, MELS, SECONDS};
use crate::backend::{Backend, BackendModel, BackendSpec, EndOfTurnModel, Library, Load};
use crate::host::Accelerator;
use crate::install::Installed;
use crate::{Error, Result};

#[cfg(test)]
mod tests;

/// The key of smart-turn's file.
const SMART_TURN: &str = "smart_turn";

struct OnnxRuntime;

const SPEC: BackendSpec = BackendSpec {
    id: "onnxruntime",
    name: "ONNX Runtime",
    description: "ONNX models run directly (smart-turn) on the ONNX Runtime linked into native builds with sherpa-onnx.",
    upstream: "https://github.com/microsoft/onnxruntime",
    accelerators: &[Accelerator::Cpu],
    requirements: &[],
    provider: None,
};

inventory::submit! { BackendFactory(|| Box::new(OnnxRuntime)) }

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl Backend for OnnxRuntime {
    fn spec(&self) -> &BackendSpec {
        &SPEC
    }

    /// Hands `ort` the linked runtime's C API, once for the process. Nothing installed is read. Fails with
    /// `library-open-failed` when the runtime does not offer API 17.
    async fn open(&self, _files: &Installed) -> Result<Box<dyn Library>> {
        linked()?;
        Ok(Box::new(Linked))
    }
}

/// `ort` on the runtime sherpa-onnx links: set up the first time, remembered after.
fn linked() -> Result<()> {
    static API: OnceLock<bool> = OnceLock::new();
    let ready = *API.get_or_init(|| {
        // SAFETY: `OrtGetApiBase` is the linked runtime's entry point, which returns a pointer to a static table, and
        // `GetApi` a pointer to a static table of the version asked for, or null when it is too old.
        unsafe {
            let base = ort::sys::OrtGetApiBase();
            if base.is_null() {
                return false;
            }
            let api = ((*base).GetApi)(ort::sys::ORT_API_VERSION);
            if api.is_null() {
                return false;
            }
            // `false` when it was set already: then it is this same runtime, set by an earlier call.
            ort::set_api((*api).clone());
        }
        true
    });
    ready.then_some(()).ok_or(Error::new("library-open-failed"))
}

/// The linked runtime, which loads models.
struct Linked;

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl Library for Linked {
    /// `unsupported-accelerator` off the CPU, `unsupported-model` for files it does not know, `file-not-installed`,
    /// and `model-load-failed`.
    async fn load(&self, load: Load<'_>) -> Result<Box<dyn BackendModel>> {
        if load.accelerator != Accelerator::Cpu {
            return Err(Error::new("unsupported-accelerator"));
        }
        if load.files.files.keys().any(|key| key != SMART_TURN) {
            return Err(Error::new("unsupported-model"));
        }
        let path = load
            .files
            .file(SMART_TURN)
            .ok_or(Error::new("file-not-installed"))?;
        let session = Session::builder()
            .map_err(load_failed)?
            .with_intra_threads(threads())
            .map_err(load_failed)?
            .commit_from_file(path)
            .map_err(load_failed)?;
        Ok(Box::new(SmartTurn(session)))
    }
}

/// The threads a model runs on: the machine's, up to 4, as sherpa-onnx's models do.
fn threads() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get().min(4))
}

/// smart-turn in memory: its session.
struct SmartTurn(Session);

impl BackendModel for SmartTurn {
    fn as_end_of_turn(&mut self) -> Option<&mut dyn EndOfTurnModel> {
        Some(self)
    }

    fn memory_mb(&self) -> Option<u32> {
        None
    }
}

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl EndOfTurnModel for SmartTurn {
    fn seconds(&self) -> u32 {
        SECONDS
    }

    /// Runs on the calling thread. Fails with `end-of-turn-failed`.
    async fn probability(&mut self, pcm: &[f32]) -> Result<f32> {
        let input = Tensor::from_array(([1usize, MELS, FRAMES], smart_turn::features(pcm)))
            .map_err(run_failed)?;
        let outputs = self
            .0
            .run(ort::inputs![INPUT => input])
            .map_err(run_failed)?;
        let (_, answer) = outputs[0].try_extract_tensor::<f32>().map_err(run_failed)?;
        answer
            .first()
            .copied()
            .ok_or(Error::new("end-of-turn-failed"))
    }
}

/// `model-load-failed`, whatever `ort` said.
fn load_failed<E>(_: E) -> Error {
    Error::new("model-load-failed")
}

/// `end-of-turn-failed`, whatever `ort` said.
fn run_failed<E>(_: E) -> Error {
    Error::new("end-of-turn-failed")
}
