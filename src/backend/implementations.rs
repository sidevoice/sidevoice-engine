//! One file per backend. Each registers itself with `inventory::submit!`, so nothing else lists them: adding a backend
//! is its file and its line here (README, "How to add a backend"; the contract is in `backend.rs`). Where a backend's
//! library cannot compile everywhere (MLX only on Apple silicon, whisper.cpp not on Android), its declaration below is
//! wrapped in the macro of `platform::local_engines` that names that condition: the OS is `platform`'s to know. A file
//! reaches the registry's `BackendFactory` itself (`crate::backend::registry`), so nothing outside it changes when a
//! backend comes or goes.

crate::platform::local_engines::where_mlx_builds! {
    mod mlx;
}
mod onnx_runtime;
mod sherpa_onnx;
mod transformers_js;
crate::platform::local_engines::where_whisper_cpp_builds! {
    mod whisper_cpp;
}
