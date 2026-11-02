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
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{empty_dir, metadata, repo, sherpa_libs, Result};

/// Each Rust target of an iOS build and the slice of the xcframeworks it links.
const TARGETS: [(&str, &str); 2] = [
    ("aarch64-apple-ios", "ios-arm64"),
    ("aarch64-apple-ios-sim", "ios-arm64_x86_64-simulator"),
];
/// The oldest iOS the engine is built for here (`IPHONEOS_DEPLOYMENT_TARGET`), for Rust and for the C, C++ and
/// Objective-C the build scripts compile (whisper.cpp and ggml, aws-lc) alike. Without it the C compilers target the
/// SDK's own version and rustc links for its default (10.0), and the link fails (`___chkstk_darwin`). whisper.cpp needs
/// iOS 13 or later (`std::filesystem`); an app builds with its own minimum (README, "iOS").
const DEPLOYMENT_TARGET: &str = "15.0";
/// The sherpa-onnx xcframework, its framework and the binary inside it.
const SHERPA_XCFRAMEWORK: &str = "sherpa-onnx.xcframework";
const SHERPA: &str = "SherpaOnnxC";
/// The ONNX Runtime xcframework, its framework and the binary inside it.
const ONNXRUNTIME_XCFRAMEWORK: &str = "onnxruntime.xcframework";
const ONNXRUNTIME: &str = "onnxruntime";

/// `cargo xtask ios-build [DIR]`.
pub(crate) fn build(dir: Option<&str>) -> Result<()> {
    let out = prepare(dir)?;
    for (triple, _) in TARGETS {
        link(
            triple,
            &out.join(format!("lib-{triple}")),
            &out.join(format!("frameworks-{triple}")),
        )?;
    }
    print_frameworks(&out);
    Ok(())
}

/// `cargo xtask mobile-libs ios [DIR]`: the same xcframeworks in the same layout, without building the engine.
pub(crate) fn libs(dir: Option<&str>) -> Result<()> {
    print_frameworks(&prepare(dir)?);
    Ok(())
}

fn print_frameworks(out: &Path) {
    for xcframework in [SHERPA_XCFRAMEWORK, ONNXRUNTIME_XCFRAMEWORK] {
        println!("{}", out.join(xcframework).display());
    }
}

/// The pinned archives checked and unpacked into `DIR` (returned), with the per-target directories the linker is
/// pointed at.
fn prepare(dir: Option<&str>) -> Result<PathBuf> {
    let (sherpa, onnxruntime) = sherpa_libs::ios_archives(None)?;
    let (_, target) = metadata()?;
    let out = dir.map_or_else(|| target.join("ios"), PathBuf::from);
    empty_dir(&out)?;
    for archive in [sherpa, onnxruntime] {
        zip::ZipArchive::new(io::Cursor::new(archive))
            .and_then(|mut zip| zip.extract(&out))
            .map_err(|e| format!("unpacking into {}: {e}", out.display()))?;
    }
    layout(&out)?;
    Ok(out)
}

/// For each target, from the unpacked xcframeworks in `out`: `lib-<triple>/` and `frameworks-<triple>/`.
fn layout(out: &Path) -> Result<()> {
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
    }
    Ok(())
}

/// The engine built in release for `triple`, for iOS [`DEPLOYMENT_TARGET`], linked against the sherpa-onnx library in
/// `lib` and the ONNX Runtime framework in `frameworks`.
fn link(triple: &str, lib: &Path, frameworks: &Path) -> Result<()> {
    let status = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(["build", "--locked", "--release", "--target", triple])
        .current_dir(repo())
        .env("SHERPA_ONNX_LIB_DIR", lib)
        .env("ONNXRUNTIME_FRAMEWORK_DIR", frameworks)
        .env("IPHONEOS_DEPLOYMENT_TARGET", DEPLOYMENT_TARGET)
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

#[cfg(test)]
mod tests;
