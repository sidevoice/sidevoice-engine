//! `cargo xtask android-build [DIR]`: the engine built for Android, and the libraries a shell packages beside it.
//!
//! The archive `xtask/sherpa-onnx-libs.json` pins is checked against its digest, then unpacked here, afresh, into
//! `<target>/android-sherpa/`. `cargo ndk` builds the engine in release for each ABI (`arm64-v8a` for phones, `x86_64`
//! for the emulator), one ABI at a time, each linked against that ABI's directory of the unpacked archive through
//! `SHERPA_ONNX_LIB_DIR`. The variable is the one `sherpa-onnx-sys`'s build script reads before its own cache
//! (`target/sherpa-onnx-prebuilt`), which a stale or edited copy would otherwise stand in for what was verified. A
//! cdylib is linked too (`crate-type`), so a symbol the libraries lack fails the build.
//!
//! `DIR/<abi>/` (default `<target>/android`) then holds what the engine needs at run time and a shell must package in
//! its `jniLibs/<abi>/`, copied from the same unpacked directory it was linked against: `libsherpa-onnx-c-api.so` and
//! `libonnxruntime.so` (the second also carries the ONNX Runtime the `onnxruntime` backend's `ort` resolves). The
//! archive's other libraries are for Java and C++ apps. Needs cargo-ndk and the NDK (`ANDROID_NDK_HOME`).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

use crate::{empty_dir, metadata, repo, sherpa_libs, Result};

/// Each ABI Android names.
const ABIS: [&str; 2] = ["arm64-v8a", "x86_64"];
/// The Android API level the engine is linked for: the oldest `sidevoice-voice` runs on (cpal's AAudio needs 26).
const API_LEVEL: &str = "26";
/// What the engine links at run time, from sherpa-onnx's archive.
const RUNTIME_LIBS: [&str; 2] = ["libsherpa-onnx-c-api.so", "libonnxruntime.so"];

/// `cargo xtask android-build [DIR]`.
pub(crate) fn build(dir: Option<&str>) -> Result<()> {
    let archive = sherpa_libs::android_archive(None)?;
    let (_, target) = metadata()?;
    let libs = sherpa_libs::unpack_android(&archive, &target.join("android-sherpa"))?;
    let out = dir.map_or_else(|| target.join("android"), PathBuf::from);
    empty_dir(&out)?;
    for abi in ABIS {
        let linked = libs.join(abi);
        ndk(abi, &linked)?;
        let packaged = out.join(abi);
        fs::create_dir_all(&packaged).map_err(|e| format!("{}: {e}", packaged.display()))?;
        for lib in RUNTIME_LIBS {
            copy(&linked.join(lib), &packaged.join(lib))?;
        }
        println!("{}", packaged.display());
    }
    Ok(())
}

/// The engine built in release for `abi`, linked against the libraries in `linked`.
fn ndk(abi: &str, linked: &Path) -> Result<()> {
    let status = Command::new(env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args([
            "ndk",
            "-P",
            API_LEVEL,
            "-t",
            abi,
            "build",
            "--locked",
            "--release",
        ])
        .current_dir(repo())
        .env("SHERPA_ONNX_LIB_DIR", linked)
        .env_remove("SHERPA_ONNX_ARCHIVE_DIR")
        .status()
        .map_err(|e| format!("cargo ndk -t {abi}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("cargo ndk -t {abi} build: {status}"))
    }
}

fn copy(from: &Path, to: &Path) -> Result<()> {
    fs::copy(from, to).map(drop).map_err(|e| {
        format!(
            "{} (the engine links it, the archive holds it): {e}",
            from.display()
        )
    })
}
