//! One file per backend. Each registers itself with `inventory::submit!`, so nothing else lists them: adding a backend
//! is its file and its line here (README, "How to add a backend"; the contract is in `backend.rs`). A file whose
//! library cannot compile everywhere starts with one `#![cfg]`, its own condition (a platform alias from build.rs), and is
//! empty elsewhere. Where the OS decides (a phone has no whisper.cpp, an iPhone no ONNX Runtime), the declaration below is wrapped in the macro of
//! `platform::local_engines` that names that condition: the OS is `platform`'s to know. A file reaches the registry's
//! `BackendFactory` itself (`crate::backend::registry`), so nothing outside it changes when a backend comes or goes.

mod mlx;
crate::platform::local_engines::where_onnx_runtime_builds! {
    mod onnx_runtime;
}
mod sherpa_onnx;
mod transformers_js;
crate::platform::local_engines::where_whisper_cpp_builds! {
    mod whisper_cpp;
}
