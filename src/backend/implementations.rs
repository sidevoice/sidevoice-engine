//! One file per backend. Each registers itself with `inventory::submit!`, so nothing else lists them: adding a backend
//! is its file and its line here (README, "How to add a backend"; the contract is in `backend.rs`). A file whose
//! library cannot compile everywhere starts with one `#![cfg]`, its own condition (a platform alias from build.rs, and the
//! Cargo feature that links its library, if one does), and is empty elsewhere. A file reaches the registry's
//! `BackendFactory` itself (`crate::backend::registry`), so nothing outside it changes when a backend comes or goes.
//! sherpa-onnx and whisper.cpp open their libraries and load models; MLX and transformers.js are stubs: they describe
//! themselves, keep the default `probe`, and open nothing yet.

mod mlx;
mod sherpa_onnx;
mod transformers_js;
mod whisper_cpp;
