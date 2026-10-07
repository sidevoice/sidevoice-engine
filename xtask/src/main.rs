//! Build tooling for the engine, run as `cargo xtask <command>` (alias in `.cargo/config.toml`).
//!
//! - `test-wasm`: the engine's tests compiled to wasm32 and run in Node by `wasm-bindgen-test-runner`, the pinned
//!   wasm-bindgen CLI (from PATH at that version, else fetched and checked into `target/tools/`:
//!   xtask/src/wasm_bindgen.rs).
//! - `npm`: the npm package `@sidevoice/engine`: the release wasm32 build of the engine through
//!   `wasm-bindgen --target web`, its `package.json`, README and licence, packed by `npm pack` into `target/npm/`
//!   (xtask/src/npm.rs).
//! - `npm-smoke`: install that tarball into a temporary directory as a consumer does and, in Node, import the
//!   package, load its wasm from `node_modules` and create an engine on a plain-object host: it must have the web
//!   build's backends.
//! - `manifest DIR [--tag vX.Y.Z]`: give the npm tarball in DIR its published name (`sidevoice-engine-nightly.tgz`
//!   for the nightly) and write `SHA256SUMS` over the assets in DIR; with a tag, the crate version must be that
//!   release.
//! - `publish DIR TAG`: attach every file in DIR to the release TAG (for `nightly`, move the tag here first and drop
//!   older assets), download them back, check them against `SHA256SUMS` and the attestation, and publish.
//! - `npm-publish TAG`: the tarball of the GitHub release TAG (a `vX.Y.Z`, never the nightly), checked against the
//!   release's `SHA256SUMS` and attestation, published to npm by trusted publishing with provenance.

mod manifest;
mod npm;
mod publish;
mod util;
mod wasm_bindgen;

use std::env;
use std::ffi::OsStr;
use std::path::Path;
use std::process::{Command, ExitCode};

pub(crate) type Result<T> = std::result::Result<T, String>;

pub(crate) const WASM: &str = "wasm32-unknown-unknown";
/// The crate whose wasm32 build is the npm package; its version is the release's.
pub(crate) const WEB_CRATE: &str = "sidevoice-engine";
/// The stem of that build's `.wasm` (the crate's library name), after which wasm-bindgen names its output.
pub(crate) const WASM_STEM: &str = "sidevoice_engine";

const USAGE: &str = "usage: cargo xtask test-wasm | npm | npm-smoke | manifest DIR [--tag vX.Y.Z] | publish DIR TAG | npm-publish TAG";

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let result = match args.as_slice() {
        ["test-wasm"] => test_wasm(),
        ["npm"] => npm::npm_package(),
        ["npm-smoke"] => npm::smoke(),
        ["manifest", dir] => manifest::manifest(Path::new(dir), None),
        ["manifest", dir, "--tag", tag] => manifest::manifest(Path::new(dir), Some(tag)),
        ["publish", dir, tag] => publish::publish(Path::new(dir), tag),
        ["npm-publish", tag] => npm::publish(tag),
        _ => Err(USAGE.to_owned()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("xtask: {error}");
            ExitCode::FAILURE
        }
    }
}

fn test_wasm() -> Result<()> {
    let tools = wasm_bindgen::tools()?;
    // Doctests are left out: rustdoc cannot run them on wasm32.
    cargo(
        &["test", "--locked", "--target", WASM, "--lib"],
        &[(
            "CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER",
            tools.test_runner.as_os_str(),
        )],
    )
}

/// Runs cargo with its output on this terminal.
pub(crate) fn cargo(args: &[&str], envs: &[(&str, &OsStr)]) -> Result<()> {
    let cargo = util::cargo_program();
    let status = Command::new(&cargo)
        .args(args)
        .envs(envs.iter().copied())
        .status()
        .map_err(|error| format!("{cargo}: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("cargo {} failed: {status}", args.join(" ")))
    }
}
