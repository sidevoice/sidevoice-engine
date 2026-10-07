//! One file per backend: its data (`BackendSpec`), its own accelerator check when the default is not enough (`probe`)
//! and how it loads a model. A file whose library cannot compile everywhere starts with one `#![cfg(<alias>)]` (aliases
//! in build.rs) and is empty elsewhere. Each registers itself with `inventory::submit!`.
//!
//! These are stubs: they describe themselves, and load nothing yet. `mlx.rs` shows a `probe` of its own.

mod mlx;
mod sherpa_onnx;
mod transformers_js;
