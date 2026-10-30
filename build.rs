//! The engine's platform aliases, named once: `web`, `native`, `apple_silicon`, `whisper_cpp` (a native build that is
//! not a phone's: the one condition of Cargo.toml's `whisper-rs` table) and `onnx_runtime` (a native build that is not
//! iOS's: that of the `ort` table). They name platforms only; which of them a module needs is that module's own `#[cfg]`
//! to say.
//!
//! And the bundled catalogue's list of families (`src/catalog/bundled.rs`): every `catalog/families/<family>.json`, in
//! name order, so that adding a family is adding its file.
//!
//! And, when CI asks, on Windows, the manifest that makes UTF-8 the tests' ANSI code page.

use std::path::Path;
use std::{env, fs};

use cfg_aliases::cfg_aliases;

fn main() {
    cfg_aliases! {
        web: { target_arch = "wasm32" },
        native: { not(target_arch = "wasm32") },
        apple_silicon: { all(target_os = "macos", target_arch = "aarch64") },
        whisper_cpp: { all(native, not(target_os = "android"), not(target_os = "ios")) },
        onnx_runtime: { all(native, not(target_os = "ios")) },
    }
    bundled_families();
    test_manifest();
}

/// The engine's own tests, on Windows, run as an app must: UTF-8 is their ANSI code page (README, "Build and test"). The
/// manifest goes to the linker (`/MANIFEST:EMBED /MANIFESTINPUT`), as the desktop app does for every executable it
/// builds: through `rustc-link-arg`, not `-tests`, which leaves out the lib's own test binary, and with no resource
/// compiler. Only when `SIDEVOICE_TEST_MANIFEST` is set, which CI sets for the jobs that run tests: a consumer building
/// the engine as a dependency never has a manifest forced on its link. Nothing for any target but MSVC.
fn test_manifest() {
    println!("cargo:rerun-if-env-changed=SIDEVOICE_TEST_MANIFEST");
    if env::var_os("SIDEVOICE_TEST_MANIFEST").is_none()
        || env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc")
    {
        return;
    }
    let manifest = Path::new(&env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"))
        .join("tests/windows-utf8.manifest");
    println!("cargo:rerun-if-changed={}", manifest.display());
    println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
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
