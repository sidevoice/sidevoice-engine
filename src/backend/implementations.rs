//! One file per backend. Each registers itself with `inventory::submit!`, so nothing else lists them: adding a backend
//! is its file and its line here (README, "How to add a backend"; the contract is in `backend.rs`). A file whose
//! library cannot compile everywhere starts with one `#![cfg(<alias>)]` (aliases in build.rs) and is empty elsewhere.
//! sherpa-onnx and transformers.js open their libraries and load models; MLX is a stub: it describes itself, keeps
//! the default `probe`, and opens nothing yet.

mod mlx;
mod sherpa_onnx;
mod transformers_js;
