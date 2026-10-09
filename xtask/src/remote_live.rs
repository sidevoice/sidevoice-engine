//! `cargo xtask remote-live`: CI's glue around the remote providers' real calls, the engine's ignored integration test
//! `tests/remote_live.rs`. It runs that test (which skips each provider whose key is not in the environment) with its
//! table kept in `target/remote-live`, then appends the table to `$GITHUB_STEP_SUMMARY` when it is set, whether the
//! test passed or not. The keys pass through the environment untouched: nothing here reads or prints them.

use std::env;
use std::fs::OpenOptions;
use std::io::Write;
use std::process::Command;

use crate::{read, repo, Result};

/// `cargo xtask remote-live`.
pub(crate) fn run() -> Result<()> {
    let dir = repo().join("target/remote-live");
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let status = Command::new(&cargo)
        .args([
            "test",
            "--locked",
            "--test",
            "remote_live",
            "--",
            "--ignored",
            "--nocapture",
        ])
        .env("SIDEVOICE_REMOTE_LIVE", &dir)
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
        return Err(format!("the remote providers' checks: {status}"));
    }
    Ok(())
}
