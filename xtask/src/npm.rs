//! The engine on npm: `@sidevoice/engine`, the wasm32 build as `wasm-bindgen --target web` emits it (`dist/`), with
//! the checked-in `npm/package.json` (its version stamped from the crate's) and `npm/README.md`, and the licence.

use std::fs;

use serde_json::{json, Value};

use crate::release::tarball_name;
use crate::{empty_dir, metadata, read, repo, run_in, sh, write, Result};

mod publish;

pub(crate) use publish::publish;

const PACKAGE: &str = "@sidevoice/engine";
/// The library name, after which wasm-bindgen names its output.
const STEM: &str = "sidevoice_engine";
/// The backends the web build registers: what `npm-smoke` must find in the packaged build.
const WEB_BACKENDS: &[&str] = &["transformers-js"];
const SMOKE_JS: &str = include_str!("../npm/smoke.mjs");

fn parse(bytes: &[u8], what: &str) -> Result<Value> {
    serde_json::from_slice(bytes).map_err(|error| format!("{what}: {error}"))
}

/// `cargo xtask npm`: build, bind and pack into `target/npm/sidevoice-engine-X.Y.Z.tgz`.
pub(crate) fn package() -> Result<()> {
    println!("{}", packed()?.display());
    Ok(())
}

/// The npm package, built, bound and packed: the tarball's path.
pub(crate) fn packed() -> Result<std::path::PathBuf> {
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
    let copied = [
        ("npm/README.md", "README.md"),
        ("npm/THIRD_PARTY_NOTICES.md", "THIRD_PARTY_NOTICES.md"),
        ("LICENSE", "LICENSE"),
    ];
    for (from, to) in copied {
        fs::copy(repo().join(from), pkg.join(to)).map_err(|error| format!("{from}: {error}"))?;
    }

    // npm packs all of `dist/` (wasm-bindgen's `snippets/` too, where the dynamic imports of transformers.js and
    // eSpeak NG are); the entry point, types and wasm must be there, and the third-party notices.
    empty_dir(&target.join("npm"))?;
    let report = run_in(&pkg, "npm pack --json --pack-destination ../npm", &[])?;
    let report = &parse(report.as_bytes(), "npm pack")?[0];
    let packed: Vec<_> = report["files"].as_array().into_iter().flatten().collect();
    let wanted = [".js", ".d.ts", "_bg.wasm"].map(|suffix| format!("dist/{STEM}{suffix}"));
    for file in wanted
        .into_iter()
        .chain(["THIRD_PARTY_NOTICES.md".to_owned()])
    {
        if !packed.iter().any(|packed| packed["path"] == file.as_str()) {
            return Err(format!("npm pack left out {file}"));
        }
    }
    let tarball = tarball_name(&version);
    if report["filename"] != tarball.as_str() {
        let wrote = &report["filename"];
        return Err(format!("npm pack wrote {wrote}, not {tarball}"));
    }
    Ok(target.join("npm").join(tarball))
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
