//! The only platform conditions in the engine, named once. A backend file that uses a library which cannot compile
//! everywhere starts with `#![cfg(<alias>)]`; everything else (OS, architecture, memory, GPU, CUDA, WebGPU) is decided
//! at run time in that backend's `probe()`. A backend linked in only with a Cargo feature has an alias that says both.

use cfg_aliases::cfg_aliases;

fn main() {
    cfg_aliases! {
        web: { target_arch = "wasm32" },
        native: { not(target_arch = "wasm32") },
        apple_silicon: { all(target_os = "macos", target_arch = "aarch64") },
        sherpa_onnx: { all(not(target_arch = "wasm32"), feature = "sherpa-onnx") },
    }
}
