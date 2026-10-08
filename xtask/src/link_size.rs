//! `cargo xtask link-size`: what linking sherpa-onnx costs an app. The smallest program that uses the engine
//! (`link_size/probe.rs`) is built in release mode and stripped, as an app ships, twice: with the engine's default
//! features (the sherpa-onnx backend, linked) and without them. Each build must report the backends it expects; the
//! two sizes and their difference are printed, and added to `$GITHUB_STEP_SUMMARY` when it is set.

use std::env;
use std::path::Path;

use crate::{empty_dir, metadata, read, repo, run_in, write, Result};

const PROBE: &str = include_str!("link_size/probe.rs");

/// `cargo xtask link-size`.
pub(crate) fn measure() -> Result<()> {
    let (_, target) = metadata()?;
    let root = target.join("link-size");
    let shared_target = root.join("target");
    let with = build(&root, &shared_target, "with", true)?;
    let without = build(&root, &shared_target, "without", false)?;
    let mb = |bytes: u64| bytes as f64 / 1_048_576.0;
    let table = format!(
        "| Platform | Engine with sherpa-onnx | Engine without | sherpa-onnx costs |\n|---|---|---|---|\n\
         | {} {} | {:.1} MB ({with} B) | {:.1} MB ({without} B) | {:.1} MB |\n",
        env::consts::OS,
        env::consts::ARCH,
        mb(with),
        mb(without),
        mb(with.saturating_sub(without)),
    );
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

/// The probe built as the package `probe-<name>`, with or without the engine's default features, run once to check
/// which backends it has, and its size in bytes.
fn build(root: &Path, shared_target: &Path, name: &str, sherpa: bool) -> Result<u64> {
    let dir = root.join(name);
    empty_dir(&dir.join("src"))?;
    let engine = repo()
        .canonicalize()
        .map_err(|e| format!("the repository: {e}"))?;
    let engine = engine.to_string_lossy().replace('\\', "/");
    let manifest = format!(
        "[package]\nname = \"probe-{name}\"\nversion = \"0.0.0\"\nedition = \"2021\"\npublish = false\n\n\
         [dependencies]\nsidevoice-engine = {{ path = \"{engine}\", default-features = {sherpa} }}\n\n\
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
    if backends.contains("sherpa-onnx") != sherpa {
        return Err(format!("probe-{name} has the backends {backends:?}"));
    }
    let bytes = std::fs::metadata(&binary)
        .map_err(|e| format!("{}: {e}", binary.display()))?
        .len();
    Ok(bytes)
}
