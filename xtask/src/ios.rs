//! `cargo xtask ios-build [DIR]`: the engine built for iOS, device and simulator, and the xcframework a shell embeds.
//! Needs macOS and Xcode (the iOS SDK links), and the Rust targets `aarch64-apple-ios` and `aarch64-apple-ios-sim`.
//!
//! sherpa-onnx's iOS build is one shared xcframework, `SherpaOnnxC.xcframework`, which holds both the device slice
//! (`ios-arm64`) and the simulator one (`ios-arm64_x86_64-simulator`), with the ONNX Runtime linked inside it. The
//! archive `xtask/sherpa-onnx-libs.json` pins is checked against its digest and unpacked into `DIR` (default
//! `<target>/ios`), and the engine is built in release for each target against its own slice, linked through
//! `SHERPA_ONNX_LIB_DIR`: `sherpa-onnx-sys`'s build script would otherwise download the archive unchecked and keep both
//! slices' libraries in one cache that building the second removes (fixed in sherpa-onnx 2.0, which the engine does
//! not use yet). A cdylib is linked too (`crate-type`), so a symbol the framework lacks fails the build.
//!
//! `DIR/SherpaOnnxC.xcframework` is what the app embeds (and signs) in its bundle: the engine itself is linked into
//! the app's own library.

use std::fs::{self, File};
use std::path::Path;
use std::process::Command;

use crate::{empty_dir, metadata, repo, sherpa_libs, Result};

/// Each Rust target of an iOS build and the slice of the xcframework it links.
const TARGETS: [(&str, &str); 2] = [
    ("aarch64-apple-ios", "ios-arm64"),
    ("aarch64-apple-ios-sim", "ios-arm64_x86_64-simulator"),
];
/// The framework and its binary inside a slice.
const FRAMEWORK: &str = "SherpaOnnxC";

/// `cargo xtask ios-build [DIR]`.
pub(crate) fn build(dir: Option<&str>) -> Result<()> {
    let archive = sherpa_libs::ios_archive(None)?;
    let (_, target) = metadata()?;
    let out = dir.map_or_else(|| target.join("ios"), std::path::PathBuf::from);
    empty_dir(&out)?;
    let file = File::open(&archive).map_err(|e| format!("{}: {e}", archive.display()))?;
    zip::ZipArchive::new(file)
        .and_then(|mut zip| zip.extract(&out))
        .map_err(|e| format!("{}: {e}", archive.display()))?;
    let xcframework = out.join(format!("{FRAMEWORK}.xcframework"));
    for (triple, slice) in TARGETS {
        let binary = xcframework
            .join(slice)
            .join(format!("{FRAMEWORK}.framework"))
            .join(FRAMEWORK);
        // The name the linker is asked for (`-lsherpa-onnx-c-api`); the framework's own install name
        // (`@rpath/SherpaOnnxC.framework/SherpaOnnxC`) is what the app finds it by at run time.
        let lib = out.join(format!("lib-{triple}"));
        fs::create_dir_all(&lib).map_err(|e| format!("{}: {e}", lib.display()))?;
        copy(&binary, &lib.join("libsherpa-onnx-c-api.dylib"))?;
        link(triple, &lib)?;
    }
    println!("{}", xcframework.display());
    Ok(())
}

/// The engine built in release for `triple`, linked against the libraries in `lib`.
fn link(triple: &str, lib: &Path) -> Result<()> {
    let status = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(["build", "--locked", "--release", "--target", triple])
        .current_dir(repo())
        .env("SHERPA_ONNX_LIB_DIR", lib)
        .status()
        .map_err(|e| format!("cargo build --target {triple}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("cargo build --target {triple}: {status}"))
    }
}

fn copy(from: &Path, to: &Path) -> Result<()> {
    fs::copy(from, to)
        .map(drop)
        .map_err(|e| format!("{} to {}: {e}", from.display(), to.display()))
}
