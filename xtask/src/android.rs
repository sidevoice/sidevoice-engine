//! `cargo xtask android-build [DIR]`: the engine built for Android, and the libraries a shell packages beside it.
//!
//! `cargo ndk` builds the engine in release for each ABI (`arm64-v8a` for phones, `x86_64` for the emulator), linked
//! against sherpa-onnx's Android libraries from the archive `xtask/sherpa-onnx-libs.json` pins: checked against its
//! digest here, then handed to `sherpa-onnx-sys`'s build script (`SHERPA_ONNX_ARCHIVE_DIR`), which would otherwise
//! download one it does not check. A cdylib is linked too (`crate-type`), so a symbol the libraries lack fails the build.
//!
//! `DIR/<abi>/` (default `<target>/android`) then holds what the engine needs at run time and a shell must package in
//! its `jniLibs/<abi>/`: `libsherpa-onnx-c-api.so` and `libonnxruntime.so` (the second also carries the ONNX Runtime the
//! `onnxruntime` backend's `ort` resolves). The archive's other libraries are for Java and C++ apps. Needs cargo-ndk
//! and the NDK (`ANDROID_NDK_HOME`).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

use crate::{empty_dir, metadata, repo, sherpa_libs, Result};

/// Each ABI Android names, and the Rust target that builds it.
const ABIS: [(&str, &str); 2] = [
    ("arm64-v8a", "aarch64-linux-android"),
    ("x86_64", "x86_64-linux-android"),
];
/// The Android API level the engine is linked for: the oldest `sidevoice-voice` runs on (cpal's AAudio needs 26).
const API_LEVEL: &str = "26";
/// What the engine links at run time, from sherpa-onnx's archive.
const RUNTIME_LIBS: [&str; 2] = ["libsherpa-onnx-c-api.so", "libonnxruntime.so"];

/// `cargo xtask android-build [DIR]`.
pub(crate) fn build(dir: Option<&str>) -> Result<()> {
    let archives = sherpa_libs::android_archive(None)?;
    let (_, target) = metadata()?;
    let out = dir.map_or_else(|| target.join("android"), PathBuf::from);
    let mut ndk = Command::new(env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    ndk.args(["ndk", "-P", API_LEVEL]);
    for (abi, _) in ABIS {
        ndk.args(["-t", abi]);
    }
    let status = ndk
        .args(["build", "--locked", "--release"])
        .current_dir(repo())
        .env("SHERPA_ONNX_ARCHIVE_DIR", &archives)
        // The libraries of the machine building (CI's set-up exports a directory of them) are not Android's.
        .env_remove("SHERPA_ONNX_LIB_DIR")
        .status()
        .map_err(|e| format!("cargo ndk: {e}"))?;
    if !status.success() {
        return Err(format!("cargo ndk build: {status}"));
    }
    empty_dir(&out)?;
    for (abi, triple) in ABIS {
        let built = target.join(triple).join("release");
        let packaged = out.join(abi);
        fs::create_dir_all(&packaged).map_err(|e| format!("{}: {e}", packaged.display()))?;
        for lib in RUNTIME_LIBS {
            copy(&built.join(lib), &packaged.join(lib))?;
        }
        println!("{}", packaged.display());
    }
    Ok(())
}

fn copy(from: &Path, to: &Path) -> Result<()> {
    fs::copy(from, to).map(drop).map_err(|e| {
        format!(
            "{} (the engine links it, the build copies it): {e}",
            from.display()
        )
    })
}
