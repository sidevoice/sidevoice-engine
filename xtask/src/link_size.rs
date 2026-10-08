//! `cargo xtask link-size`: what linking each backend's engine costs an app. The smallest program that uses the engine
//! (`link_size/probe.rs`) is built in release mode and stripped, as an app ships, once per set of the engine's
//! features: none, `sherpa-onnx` alone, `whisper-cpp` alone, and the default (both). Each build must report exactly the
//! linked backends it expects; the sizes and what each backend adds to the build without any are printed, and added to
//! `$GITHUB_STEP_SUMMARY` when it is set.

use std::env;
use std::path::Path;

use crate::{empty_dir, metadata, read, repo, run_in, write, Result};

const PROBE: &str = include_str!("link_size/probe.rs");

/// The backends whose engine is linked, by their feature, which is also their id.
const LINKED: [&str; 2] = ["sherpa-onnx", "whisper-cpp"];

/// `cargo xtask link-size`.
pub(crate) fn measure() -> Result<()> {
    let (_, target) = metadata()?;
    let root = target.join("link-size");
    let shared_target = root.join("target");
    let none = build(&root, &shared_target, "none", &[])?;
    let mut alone = Vec::new();
    for backend in LINKED {
        alone.push(build(&root, &shared_target, backend, &[backend])?);
    }
    let default = build(&root, &shared_target, "default", &LINKED)?;
    let mb = |bytes: u64| bytes as f64 / 1_048_576.0;
    let size = |bytes: u64| format!("{:.1} MB ({bytes} B)", mb(bytes));
    let costs = |bytes: u64| format!("+{:.1} MB", mb(bytes.saturating_sub(none)));
    let mut table =
        String::from("| Platform | Engine with | Size | Over none |\n|---|---|---|---|\n");
    let platform = format!("{} {}", env::consts::OS, env::consts::ARCH);
    table.push_str(&format!(
        "| {platform} | no linked backend | {} | |\n",
        size(none)
    ));
    for (backend, bytes) in LINKED.iter().zip(&alone) {
        table.push_str(&format!(
            "| {platform} | `{backend}` | {} | {} |\n",
            size(*bytes),
            costs(*bytes)
        ));
    }
    table.push_str(&format!(
        "| {platform} | default (`{}`) | {} | {} |\n",
        LINKED.join("`, `"),
        size(default),
        costs(default)
    ));
    println!("{table}");
    if let Some(summary) = env::var_os("GITHUB_STEP_SUMMARY") {
        let mut text =
            String::from_utf8_lossy(&read(Path::new(&summary)).unwrap_or_default()).into_owned();
        text.push_str("## Link size (release, stripped)\n\n");
        text.push_str(&table);
        write(Path::new(&summary), text.as_bytes())?;
    }
    Ok(())
}

/// The probe built as the package `probe-<name>`, with the engine's `features` and no others, run once to check that
/// the linked backends it has are exactly those, and its size in bytes.
fn build(root: &Path, shared_target: &Path, name: &str, features: &[&str]) -> Result<u64> {
    let dir = root.join(name);
    empty_dir(&dir.join("src"))?;
    let engine = repo()
        .canonicalize()
        .map_err(|e| format!("the repository: {e}"))?;
    let engine = engine.to_string_lossy().replace('\\', "/");
    let manifest = format!(
        "[package]\nname = \"probe-{name}\"\nversion = \"0.0.0\"\nedition = \"2021\"\npublish = false\n\n\
         [dependencies]\nsidevoice-engine = {{ path = \"{engine}\", default-features = false, features = {features:?} }}\n\n\
         [profile.release]\nstrip = true\n\n[workspace]\n"
    );
    write(&dir.join("Cargo.toml"), manifest.as_bytes())?;
    write(&dir.join("src/main.rs"), PROBE.as_bytes())?;
    // The engine's own lock, so that the probe builds the versions it is tested with.
    write(&dir.join("Cargo.lock"), &read(&repo().join("Cargo.lock"))?)?;
    let target = shared_target.to_string_lossy();
    run_in(&dir, "cargo build --release --target-dir", &[&target])?;
    let binary = shared_target
        .join("release")
        .join(format!("probe-{name}{}", env::consts::EXE_SUFFIX));
    let backends = run_in(&dir, &binary.to_string_lossy(), &[])?;
    let has: Vec<&str> = backends.trim().split(',').collect();
    if LINKED
        .iter()
        .any(|backend| has.contains(backend) != features.contains(backend))
    {
        return Err(format!("probe-{name} has the backends {backends:?}"));
    }
    let bytes = std::fs::metadata(&binary)
        .map_err(|e| format!("{}: {e}", binary.display()))?
        .len();
    Ok(bytes)
}
