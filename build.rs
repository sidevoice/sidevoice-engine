//! The only platform conditions in the engine, named once. A backend file that uses a library which cannot compile
//! everywhere starts with `#![cfg(<alias>)]`; everything else (OS, architecture, memory, GPU, CUDA, WebGPU) is decided
//! at run time in that backend's `probe()`. A backend linked in only with a Cargo feature has an alias that says both.
//!
//! And the bundled catalogue's list of families (`src/catalog/bundled.rs`): every `catalog/families/<family>.json`, in
//! name order, so that adding a family is adding its file.

use std::path::Path;
use std::{env, fs};

use cfg_aliases::cfg_aliases;

fn main() {
    cfg_aliases! {
        web: { target_arch = "wasm32" },
        native: { not(target_arch = "wasm32") },
        apple_silicon: { all(target_os = "macos", target_arch = "aarch64") },
        sherpa_onnx: { all(not(target_arch = "wasm32"), feature = "sherpa-onnx") },
    }
    bundled_families();
}

/// Writes `$OUT_DIR/bundled_families.rs`: a slice of `(name, include_str!(file))`, the name being the file's without
/// `.json`. Cargo runs this again when a file is added, removed or changed.
fn bundled_families() {
    let dir = Path::new(&env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"))
        .join("catalog/families");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut files: Vec<_> = fs::read_dir(&dir)
        .expect("catalog/families")
        .map(|entry| entry.expect("catalog/families").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    files.sort();
    let mut list = String::from("&[\n");
    for path in &files {
        println!("cargo:rerun-if-changed={}", path.display());
        let name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .expect("a UTF-8 file name");
        let path = path.to_str().expect("a UTF-8 path");
        list.push_str(&format!("    ({name:?}, include_str!({path:?})),\n"));
    }
    list.push(']');
    let out = Path::new(&env::var("OUT_DIR").expect("OUT_DIR")).join("bundled_families.rs");
    fs::write(out, list).expect("bundled_families.rs");
}
