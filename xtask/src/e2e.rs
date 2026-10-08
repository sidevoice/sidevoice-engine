//! `cargo xtask e2e [DIR]`: CI's glue around the voice loop, which is the engine's own integration test
//! (`tests/voice_loop.rs`, plan in `tests/voice_loop.json`). It runs that ignored test with everything kept in DIR
//! (`target/voice-loop` unless given: the models, the clips, what each model said), then appends the table the test
//! wrote (`DIR/summary.md`) to `$GITHUB_STEP_SUMMARY` when it is set, whether the test passed or not.

use std::env;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

use crate::{read, repo, Result};

/// `cargo xtask e2e [DIR]`.
pub(crate) fn run(dir: Option<&str>) -> Result<()> {
    let dir = dir.map_or_else(|| repo().join("target/voice-loop"), PathBuf::from);
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let status = Command::new(&cargo)
        .args([
            "test",
            "--locked",
            "--test",
            "voice_loop",
            "--",
            "--ignored",
            "--nocapture",
        ])
        .env("SIDEVOICE_VOICE_LOOP", &dir)
        .current_dir(repo())
        .status()
        .map_err(|e| format!("{cargo} test: {e}"))?;
    if let Some(summary) = env::var_os("GITHUB_STEP_SUMMARY") {
        if let Ok(table) = read(&dir.join("summary.md")) {
            let mut file = OpenOptions::new()
                .append(true)
                .create(true)
                .open(&summary)
                .map_err(|e| format!("$GITHUB_STEP_SUMMARY: {e}"))?;
            file.write_all(&table)
                .map_err(|e| format!("$GITHUB_STEP_SUMMARY: {e}"))?;
        }
    }
    if !status.success() {
        return Err(format!("the voice loop: {status}"));
    }
    Ok(())
}
