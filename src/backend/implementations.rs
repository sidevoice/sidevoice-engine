//! One file per backend. Each registers itself with `inventory::submit!`, so nothing else lists them: adding a backend
//! is its file and its line here (README, "How to add a backend"; the contract is in `backend.rs`). A file whose
//! library cannot compile everywhere starts with one `#![cfg]`, its own condition (a platform alias from build.rs), and is
//! empty elsewhere. A file reaches the registry's
//! `BackendFactory` itself (`crate::backend::registry`), so nothing outside it changes when a backend comes or goes.

mod mlx;
mod sherpa_onnx;
mod transformers_js;
mod whisper_cpp;
