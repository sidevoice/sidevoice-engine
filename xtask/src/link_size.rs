//! `cargo xtask link-size`: what linking the engine costs an app. The smallest program that uses the engine
//! (`link_size/probe.rs`) is built in release mode and stripped, as an app ships; its size and the backends it was
//! built with are printed, and added to `$GITHUB_STEP_SUMMARY` when it is set.

use std::env;
use std::path::Path;

use crate::{empty_dir, metadata, read, repo, run_in, write, Result};

const PROBE: &str = include_str!("link_size/probe.rs");

/// `cargo xtask link-size`.
pub(crate) fn measure() -> Result<()> {
    let (_, target) = metadata()?;
    let root = target.join("link-size");
    let (bytes, backends) = build(&root)?;
    let platform = format!("{} {}", env::consts::OS, env::consts::ARCH);
    let table = format!(
        "| Platform | Size | Backends |\n|---|---|---|\n| {platform} | {:.1} MB ({bytes} B) | {} |\n",
        bytes as f64 / 1_048_576.0,
        backends.trim()
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

/// The probe, built in `root` and run once: its size in bytes, and the backends it says it was built with.
fn build(root: &Path) -> Result<(u64, String)> {
    let dir = root.join("probe");
    empty_dir(&dir.join("src"))?;
    let engine = repo()
        .canonicalize()
        .map_err(|e| format!("the repository: {e}"))?;
    let engine = engine.to_string_lossy().replace('\\', "/");
    let manifest = format!(
        "[package]\nname = \"probe\"\nversion = \"0.0.0\"\nedition = \"2021\"\npublish = false\n\n\
         [dependencies]\nsidevoice-engine = {{ path = \"{engine}\" }}\n\n\
         [profile.release]\nstrip = true\n\n[workspace]\n"
    );
    write(&dir.join("Cargo.toml"), manifest.as_bytes())?;
    write(&dir.join("src/main.rs"), PROBE.as_bytes())?;
    // The engine's own lock, so that the probe builds the versions it is tested with.
    write(&dir.join("Cargo.lock"), &read(&repo().join("Cargo.lock"))?)?;
    let target = root.join("target");
    run_in(
        &dir,
        "cargo build --release --target-dir",
        &[&target.to_string_lossy()],
    )?;
    let binary = target
        .join("release")
        .join(format!("probe{}", env::consts::EXE_SUFFIX));
    let backends = run_in(&dir, &binary.to_string_lossy(), &[])?;
    let bytes = std::fs::metadata(&binary)
        .map_err(|e| format!("{}: {e}", binary.display()))?
        .len();
    Ok((bytes, backends))
}
