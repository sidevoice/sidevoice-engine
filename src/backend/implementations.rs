//! One file per backend. Each registers itself with `inventory::submit!`, so nothing else lists them: adding a backend
//! is its file and its line here. A file whose library cannot compile everywhere starts with one `#![cfg(<alias>)]`
//! (aliases in build.rs) and is empty elsewhere. These are stubs: they describe themselves and load nothing yet.

mod mlx;
mod sherpa_onnx;
mod transformers_js;
