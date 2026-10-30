//! `cargo xtask consumer-check`: the engine built the way a downstream app builds it. A minimal crate
//! (`consumer_check/consumer.rs`) that depends on the engine is generated outside the repository, so it inherits none
//! of this repository's `.cargo/config.toml`, and carries only the configuration the README ("Build and test") tells
//! a consumer to set: `GGML_NATIVE=OFF`, the Windows static C runtime (`+crt-static`, `CMAKE_POLICY_DEFAULT_CMP0091`
//! `NEW`, `CMAKE_MSVC_RUNTIME_LIBRARY` `MultiThreaded`) and, on Linux x86_64, libstdc++'s old string ABI. It is built and
//! run, and must report both linked backends. `SHERPA_ONNX_LIB_DIR` comes from the environment, as in the build's setup.

use std::env;

use crate::{empty_dir, read, repo, run_in, write, Result};

const CONSUMER: &str = include_str!("consumer_check/consumer.rs");

/// The consumer's own `.cargo/config.toml`: the README's list, nothing more.
const CONFIG: &str = r#"[env]
GGML_NATIVE = "OFF"
CMAKE_POLICY_DEFAULT_CMP0091 = "NEW"
CMAKE_MSVC_RUNTIME_LIBRARY = "MultiThreaded"
CXXFLAGS_x86_64_unknown_linux_gnu = "-D_GLIBCXX_USE_CXX11_ABI=0"

[target.x86_64-pc-windows-msvc]
rustflags = ["-C", "target-feature=+crt-static"]
[target.aarch64-pc-windows-msvc]
rustflags = ["-C", "target-feature=+crt-static"]
"#;

/// `cargo xtask consumer-check`.
pub(crate) fn run() -> Result<()> {
    let root = env::temp_dir().join("sidevoice-consumer-check");
    let dir = root.join("consumer");
    empty_dir(&dir)?;
    empty_dir(&dir.join("src"))?;
    empty_dir(&dir.join(".cargo"))?;
    // Not `canonicalize`: on Windows that gives a verbatim (`\\?\`) path, which Cargo does not take as a dependency path.
    let engine = std::path::absolute(repo()).map_err(|e| format!("the repository: {e}"))?;
    let engine = engine.to_string_lossy().replace('\\', "/");
    let manifest = format!(
        "[package]\nname = \"consumer\"\nversion = \"0.0.0\"\nedition = \"2021\"\npublish = false\n\n\
         [dependencies]\nsidevoice-engine = {{ path = \"{engine}\" }}\n\n[workspace]\n"
    );
    write(&dir.join("Cargo.toml"), manifest.as_bytes())?;
    write(&dir.join("src/main.rs"), CONSUMER.as_bytes())?;
    write(&dir.join(".cargo/config.toml"), CONFIG.as_bytes())?;
    // The engine's own lock, so that the consumer builds the versions the engine is tested with.
    write(&dir.join("Cargo.lock"), &read(&repo().join("Cargo.lock"))?)?;
    let target = root.join("target");
    run_in(
        &dir,
        "cargo build --target-dir",
        &[&target.to_string_lossy()],
    )?;
    let binary = target
        .join("debug")
        .join(format!("consumer{}", env::consts::EXE_SUFFIX));
    let said = run_in(&dir, &binary.to_string_lossy(), &[])?;
    println!("{}", said.trim());
    for backend in ["sherpa-onnx", "whisper-cpp"] {
        if !said.split(',').any(|id| id.trim() == backend) {
            return Err(format!(
                "the consumer's backends are `{}`, not {backend}",
                said.trim()
            ));
        }
    }
    Ok(())
}
