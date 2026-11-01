//! `cargo xtask ios-build [DIR]`: the engine built for iOS, device and simulator, and the two xcframeworks a shell embeds.
//! Needs macOS and Xcode (the iOS SDK links), and the Rust targets `aarch64-apple-ios` and `aarch64-apple-ios-sim`.
//!
//! sherpa-onnx's iOS build is two shared xcframeworks, each holding a device slice (`ios-arm64`) and a simulator one
//! (`ios-arm64_x86_64-simulator`): `sherpa-onnx.xcframework` (`SherpaOnnxC.framework`), and `onnxruntime.xcframework`
//! (`onnxruntime.framework`), the ONNX Runtime that sherpa-onnx's library loads (`@rpath/onnxruntime.framework`). One
//! runtime for sherpa-onnx and for `ort` (the smart-turn backend), which resolves `OrtGetApiBase` from the same
//! framework. The archives `xtask/sherpa-onnx-libs.json` pins are checked against their digests and unpacked into `DIR`
//! (default `<target>/ios`), and the engine is built in release for each target against its own slices: sherpa-onnx
//! through `SHERPA_ONNX_LIB_DIR`, the ONNX Runtime through `ONNXRUNTIME_FRAMEWORK_DIR` (build.rs). Otherwise
//! `sherpa-onnx-sys`'s build script would download the archive unchecked and keep both slices' libraries in one cache
//! that building the second removes (fixed in sherpa-onnx 2.0, which the engine does not use yet). A cdylib is linked
//! too (`crate-type`), so a symbol the frameworks lack fails the build.
//!
//! `DIR/sherpa-onnx.xcframework` and `DIR/onnxruntime.xcframework` are what the app embeds (and signs) in its bundle: the
//! engine itself is linked into the app's own library.

use std::fs;
use std::io;
use std::path::Path;
use std::process::Command;

use crate::{empty_dir, metadata, repo, sherpa_libs, Result};

/// Each Rust target of an iOS build and the slice of the xcframeworks it links.
const TARGETS: [(&str, &str); 2] = [
    ("aarch64-apple-ios", "ios-arm64"),
    ("aarch64-apple-ios-sim", "ios-arm64_x86_64-simulator"),
];
/// The sherpa-onnx xcframework, its framework and the binary inside it.
const SHERPA_XCFRAMEWORK: &str = "sherpa-onnx.xcframework";
const SHERPA: &str = "SherpaOnnxC";
/// The ONNX Runtime xcframework, its framework and the binary inside it.
const ONNXRUNTIME_XCFRAMEWORK: &str = "onnxruntime.xcframework";
const ONNXRUNTIME: &str = "onnxruntime";

/// `cargo xtask ios-build [DIR]`.
pub(crate) fn build(dir: Option<&str>) -> Result<()> {
    let (sherpa, onnxruntime) = sherpa_libs::ios_archives(None)?;
    let (_, target) = metadata()?;
    let out = dir.map_or_else(|| target.join("ios"), std::path::PathBuf::from);
    empty_dir(&out)?;
    for archive in [sherpa, onnxruntime] {
        zip::ZipArchive::new(io::Cursor::new(archive))
            .and_then(|mut zip| zip.extract(&out))
            .map_err(|e| format!("unpacking into {}: {e}", out.display()))?;
    }
    for (triple, slice) in TARGETS {
        let binary = |xcframework: &str, name: &str| {
            out.join(xcframework)
                .join(slice)
                .join(format!("{name}.framework"))
                .join(name)
        };
        // sherpa-onnx's library under the name the linker is asked for (`-lsherpa-onnx-c-api`); the framework's own
        // install name (`@rpath/SherpaOnnxC.framework/SherpaOnnxC`) is what the app finds it by at run time.
        let lib = out.join(format!("lib-{triple}"));
        fs::create_dir_all(&lib).map_err(|e| format!("{}: {e}", lib.display()))?;
        copy(
            &binary(SHERPA_XCFRAMEWORK, SHERPA),
            &lib.join("libsherpa-onnx-c-api.dylib"),
        )?;
        // The ONNX Runtime as the linker finds a framework: `<dir>/onnxruntime.framework/onnxruntime`.
        let frameworks = out.join(format!("frameworks-{triple}"));
        let framework = frameworks.join(format!("{ONNXRUNTIME}.framework"));
        fs::create_dir_all(&framework).map_err(|e| format!("{}: {e}", framework.display()))?;
        copy(
            &binary(ONNXRUNTIME_XCFRAMEWORK, ONNXRUNTIME),
            &framework.join(ONNXRUNTIME),
        )?;
        link(triple, &lib, &frameworks)?;
    }
    for xcframework in [SHERPA_XCFRAMEWORK, ONNXRUNTIME_XCFRAMEWORK] {
        println!("{}", out.join(xcframework).display());
    }
    Ok(())
}

/// The engine built in release for `triple`, linked against the sherpa-onnx library in `lib` and the ONNX Runtime
/// framework in `frameworks`.
fn link(triple: &str, lib: &Path, frameworks: &Path) -> Result<()> {
    let status = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(["build", "--locked", "--release", "--target", triple])
        .current_dir(repo())
        .env("SHERPA_ONNX_LIB_DIR", lib)
        .env("ONNXRUNTIME_FRAMEWORK_DIR", frameworks)
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
