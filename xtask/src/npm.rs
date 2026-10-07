//! The engine on npm: `@sidevoice/engine`, the wasm32 build as `wasm-bindgen --target web` emits it (`dist/`), with
//! the checked-in `npm/package.json` (its version stamped from the crate's) and `npm/README.md`, and the licence.

use std::{env, fs};

use serde_json::{json, Value};

use crate::release::{download_verified, tarball_name};
use crate::{empty_dir, metadata, read, repo, run_in, sh, write, Result};

const PACKAGE: &str = "@sidevoice/engine";
/// The library name, after which wasm-bindgen names its output.
const STEM: &str = "sidevoice_engine";
/// The backends the web build registers: what `npm-smoke` must find in the packaged build.
const WEB_BACKENDS: &[&str] = &["transformers-js"];
/// The oldest npm that publishes by trusted publishing.
const MIN_NPM: [u64; 3] = [11, 5, 1];
const SMOKE_JS: &str = include_str!("../npm/smoke.mjs");

fn parse(bytes: &[u8], what: &str) -> Result<Value> {
    serde_json::from_slice(bytes).map_err(|error| format!("{what}: {error}"))
}

/// `cargo xtask npm`: build, bind and pack into `target/npm/sidevoice-engine-X.Y.Z.tgz`.
pub(crate) fn package() -> Result<()> {
    let (version, target) = metadata()?;
    sh("cargo build --locked --release --lib --target wasm32-unknown-unknown")?;
    let pkg = target.join("npm-package");
    empty_dir(&pkg)?;
    let wasm = format!("wasm32-unknown-unknown/release/{STEM}.wasm");
    let bindgen = format!("wasm-bindgen --target web --out-dir npm-package/dist {wasm}");
    run_in(&target, &bindgen, &[])?;

    let mut manifest = parse(&read(&repo().join("npm/package.json"))?, "npm/package.json")?;
    manifest["version"] = version.clone().into();
    let manifest = format!("{manifest:#}\n");
    write(&pkg.join("package.json"), manifest.as_bytes())?;
    for (from, to) in [("npm/README.md", "README.md"), ("LICENSE", "LICENSE")] {
        fs::copy(repo().join(from), pkg.join(to)).map_err(|error| format!("{from}: {error}"))?;
    }

    // npm packs all of `dist/` (wasm-bindgen's `snippets/` too); the entry point, types and wasm must be there.
    empty_dir(&target.join("npm"))?;
    let report = run_in(&pkg, "npm pack --json --pack-destination ../npm", &[])?;
    let report = &parse(report.as_bytes(), "npm pack")?[0];
    let packed: Vec<_> = report["files"].as_array().into_iter().flatten().collect();
    for file in [".js", ".d.ts", "_bg.wasm"].map(|suffix| format!("dist/{STEM}{suffix}")) {
        if !packed.iter().any(|packed| packed["path"] == file.as_str()) {
            return Err(format!("npm pack left out {file}"));
        }
    }
    let tarball = tarball_name(&version);
    if report["filename"] != tarball.as_str() {
        let wrote = &report["filename"];
        return Err(format!("npm pack wrote {wrote}, not {tarball}"));
    }
    println!("{}", target.join("npm").join(tarball).display());
    Ok(())
}

/// `cargo xtask npm-smoke`: install the tarball into a consumer's project and, in Node, create an engine with it.
pub(crate) fn smoke() -> Result<()> {
    let (version, target) = metadata()?;
    // Beside target/npm, in a path with a space in it, as a home directory may have.
    let dir = target.join("npm smoke");
    empty_dir(&dir)?;
    write(&dir.join("package.json"), br#"{"type": "module"}"#)?;
    let tarball = format!("../npm/{}", tarball_name(&version));
    run_in(&dir, "npm install --no-audit --no-fund", &[&tarball])?;
    write(&dir.join("smoke.mjs"), SMOKE_JS.as_bytes())?;
    let report = run_in(&dir, &format!("node smoke.mjs {STEM}_bg.wasm"), &[])?;
    let report = parse(report.as_bytes(), "smoke.mjs")?;
    let (found, want) = (&report["backends"], json!(WEB_BACKENDS));
    if *found != want {
        return Err(format!(
            "the packaged engine has the backends {found}, not {want}"
        ));
    }
    println!("{PACKAGE}@{version} installs and runs: {report}");
    Ok(())
}

/// The dist-tag of a version: `next` for one with a `-` suffix (a release candidate), else `latest`.
fn dist_tag(version: &str) -> &'static str {
    if version.contains('-') {
        "next"
    } else {
        "latest"
    }
}

/// Whether `npm --version` printed `MIN_NPM` or later.
fn npm_can_publish(version: &str) -> bool {
    let numbers = version.split('.').map(|part| part.parse().unwrap_or(0));
    numbers.take(3).collect::<Vec<u64>>() >= MIN_NPM.to_vec()
}

/// `cargo xtask npm-publish TAG`: the Release's tarball, verified, published by trusted publishing with provenance.
pub(crate) fn publish(tag: &str) -> Result<()> {
    let version = tag.strip_prefix('v').unwrap_or_default();
    if !version.starts_with(char::is_numeric) {
        return Err(format!("{tag}: not a vX.Y.Z; the nightly is never on npm"));
    }
    let npm = sh("npm --version")?;
    let npm = npm.trim();
    if !npm_can_publish(npm) {
        return Err(format!(
            "npm {npm}: trusted publishing needs 11.5.1 or later"
        ));
    }

    // Exactly the bytes the GitHub Release holds, checked against its SHA256SUMS and attestation.
    let dir = env::temp_dir().join("sidevoice-engine-npm-publish");
    empty_dir(&dir)?;
    let name = tarball_name(version);
    if !download_verified(tag, &dir)?.contains(&name) {
        return Err(format!("{name}: not in the Release's SHA256SUMS"));
    }

    // A re-run carries on: a version already published with these very bytes is left as it is.
    let spec = format!("{PACKAGE}@{version}");
    let registry = dir.join("registry");
    empty_dir(&registry)?;
    match run_in(&registry, &format!("npm pack {spec}"), &[]) {
        Err(error) if error.contains("E404") || error.contains("ETARGET") => {}
        Err(error) => return Err(error),
        Ok(_) if read(&registry.join(&name))? == read(&dir.join(&name))? => {
            println!("{spec} is already published with these bytes");
            return Ok(());
        }
        Ok(_) => return Err(format!("{spec} is on npm with other bytes: release anew")),
    }
    let dist_tag = dist_tag(version);
    let publish = format!("npm publish {name} --access public --provenance --tag {dist_tag}");
    run_in(&dir, &publish, &[]).map_err(|error| {
        let workflow = env::var("GITHUB_WORKFLOW_REF").unwrap_or_default();
        format!(
            "{error}\nnpm takes trusted publishing only: on npmjs.com, {PACKAGE} → Settings → Trusted publisher \
             must name this repository and the workflow that started this run: {workflow}"
        )
    })?;
    println!("published {spec} (dist-tag {dist_tag})");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_release_goes_to_latest_and_a_candidate_to_next() {
        assert_eq!(dist_tag("0.2.0"), "latest");
        assert_eq!(dist_tag("0.2.0-rc.1"), "next");
    }

    #[test]
    fn trusted_publishing_needs_npm_11_5_1() {
        assert!(npm_can_publish("11.5.1") && npm_can_publish("11.10.0"));
        assert!(!npm_can_publish("11.5.0") && !npm_can_publish("10.9.2"));
    }
}
