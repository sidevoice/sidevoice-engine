//! The engine on npm: `@sidevoice/engine`, the wasm32 build of the engine as `wasm-bindgen --target web`
//! emits it (`dist/`), with its `package.json`, a README and the licence. It carries the engine's version.
//!
//! - `npm`: build it and `npm pack` it into `target/npm/` (emptied first): one `.tgz`.
//! - `npm-smoke`: install that tarball into a temporary directory as a consumer does and run
//!   `xtask/npm/smoke.mjs` there in Node: the packaged build must load and keep the backends it registers
//!   ([`WEB_BACKENDS`]).
//! - `npm-publish TAG`: the tarball of the GitHub release TAG (read back from the release and checked against its
//!   `SHA256SUMS` and attestation), published by trusted publishing (OIDC) only, with provenance.
//!
//! npm is the pinned [`NPM_VERSION`], installed into `target/npm-cli`, run with a configuration of its own: no
//! `.npmrc` and no token from the environment can take part.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};

use crate::manifest::published_name;
use crate::publish::{gh, signer, verify_attestation};
use crate::util::*;
use crate::{wasm_bindgen, Result, WASM, WASM_STEM, WEB_CRATE};

pub(crate) const PACKAGE: &str = "@sidevoice/engine";
/// The backends the web build registers: what `npm-smoke` must find in the packaged build.
const WEB_BACKENDS: &[&str] = &["transformers-js"];
/// The npm CLI every npm step runs: trusted publishing needs 11.5.1 or later. Bumped by hand, like any pin.
const NPM_VERSION: &str = "11.21.0";
/// The oldest Node.js npm's trusted publishing supports.
const PUBLISH_NODE: [u64; 3] = [22, 14, 0];
const SMOKE_JS: &str = include_str!("../npm/smoke.mjs");
const REPOSITORY: &str = "sidevoice/sidevoice-engine";
const REGISTRY: &str = "https://registry.npmjs.org";

/// The file `npm pack` writes for the package.
pub(crate) fn tarball_name(version: &str) -> String {
    format!(
        "{}-{version}.tgz",
        PACKAGE.trim_start_matches('@').replace('/', "-")
    )
}

/// Where `npm` leaves the tarball.
fn out_dir() -> Result<PathBuf> {
    Ok(target_dir()?.join("npm"))
}

/// The dist-tag a version is published under: `next` for one with a `-` suffix (a release candidate), else
/// `latest`.
pub(crate) fn dist_tag(version: &str) -> &'static str {
    if version.contains('-') {
        "next"
    } else {
        "latest"
    }
}

/// `package.json`. `exports` is the one entry point; TypeScript finds its types beside it.
pub(crate) fn package_manifest(version: &str) -> Value {
    json!({
        "name": PACKAGE,
        "version": version,
        "description": "Sidevoice engine for the web: local voice models in the browser, as WebAssembly.",
        "license": "Apache-2.0",
        "type": "module",
        "exports": {".": format!("./dist/{WASM_STEM}.js")},
        "types": format!("./dist/{WASM_STEM}.d.ts"),
        "files": ["dist"],
        "keywords": ["sidevoice", "voice", "speech", "stt", "tts", "wasm"],
        "homepage": format!("https://github.com/{REPOSITORY}#readme"),
        "bugs": {"url": format!("https://github.com/{REPOSITORY}/issues")},
        // npm checks provenance against this: it must be the repository the workflow runs in.
        "repository": {"type": "git", "url": format!("git+https://github.com/{REPOSITORY}.git")},
    })
}

fn readme(version: &str) -> String {
    format!(
        "# {PACKAGE}\n\nThe [Sidevoice](https://github.com/sidevoice) engine for the web, version {version}: it knows \
         a catalogue of local voice models, works out which build of each fits the device, and takes the chosen one \
         from absent to ready. This package is its WebAssembly build (`wasm-bindgen --target web`).\n\n\
         ```js\nimport init, {{ WebEngine }} from \"{PACKAGE}\";\n\nawait init();\nconst engine = await \
         WebEngine.create(host); // host: {{ capabilities() }}, see the types\nengine.backends();\n```\n\n\
         Source, documentation and issues: https://github.com/{REPOSITORY}\n\nApache-2.0. Sidevoice is a trademark; \
         see TRADEMARKS.md in the repository.\n"
    )
}

/// The pinned npm (`target/npm-cli`), with a configuration of its own (empty user and global config, its own cache)
/// and none of the environment's npm settings or tokens.
fn npm_command() -> Result<Command> {
    let dir = target_dir()?.join("npm-cli");
    let mut command = Command::new("node");
    command.arg(dir.join("node_modules/npm/bin/npm-cli.js"));
    for (key, _) in env::vars_os() {
        let key = key.to_string_lossy();
        if key.to_ascii_lowercase().starts_with("npm_config_")
            || key == "NODE_AUTH_TOKEN"
            || key == "NPM_TOKEN"
        {
            command.env_remove(key.as_ref());
        }
    }
    command
        .env("npm_config_userconfig", dir.join("user-npmrc"))
        .env("npm_config_globalconfig", dir.join("global-npmrc"))
        .env("npm_config_cache", target_dir()?.join("npm-cache"))
        .env("npm_config_registry", REGISTRY)
        .env("npm_config_update_notifier", "false")
        .env("npm_config_fund", "false")
        .env("npm_config_audit", "false");
    Ok(command)
}

/// Runs the pinned npm and returns its standard output.
fn npm(args: &[&str], dir: Option<&Path>) -> Result<String> {
    let mut command = npm_command()?;
    command.args(args);
    if let Some(dir) = dir {
        command.current_dir(dir);
    }
    run(command, &format!("npm {}", args.join(" ")))
}

/// Installs the pinned npm into `target/npm-cli` unless it is there.
fn ensure_npm() -> Result<()> {
    let dir = target_dir()?.join("npm-cli");
    let installed = fs::read(dir.join("node_modules/npm/package.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
    if installed.as_ref().map(|manifest| &manifest["version"]) != Some(&json!(NPM_VERSION)) {
        mkdir(&dir)?;
        output(
            "npm",
            &[
                "install",
                "--prefix",
                path_str(&dir)?,
                "--cache",
                path_str(&target_dir()?.join("npm-cache"))?,
                "--no-audit",
                "--no-fund",
                "--no-save",
                &format!("npm@{NPM_VERSION}"),
            ],
            None,
        )?;
    }
    for config in ["user-npmrc", "global-npmrc"] {
        write(&dir.join(config), b"")?;
    }
    let version = npm(&["--version"], None)?;
    if version.trim() != NPM_VERSION {
        return Err(format!(
            "the pinned npm reports {version:?}, not {NPM_VERSION}"
        ));
    }
    Ok(())
}

/// The `name` and `version` in a package tarball's `package.json`.
pub(crate) fn package_identity(tarball: &Path) -> Result<(String, String)> {
    let manifest = parse_json(
        output(
            "tar",
            &["-xzOf", path_str(tarball)?, "package/package.json"],
            None,
        )?
        .as_bytes(),
        &format!("{}: package.json", tarball.display()),
    )?;
    let field = |key: &str| manifest[key].as_str().unwrap_or("").to_string();
    Ok((field("name"), field("version")))
}

/// `cargo xtask npm`
pub(crate) fn npm_package() -> Result<()> {
    let tools = wasm_bindgen::tools()?;
    ensure_npm()?;
    let version = engine_version()?;
    crate::cargo(
        &[
            "build",
            "--locked",
            "--release",
            "--target",
            WASM,
            "-p",
            WEB_CRATE,
        ],
        &[],
    )?;
    let target = target_dir()?;
    let wasm = target
        .join(WASM)
        .join("release")
        .join(format!("{WASM_STEM}.wasm"));
    let package = target.join("npm-package");
    empty_dir(&package)?;
    output(
        path_str(&tools.bindgen)?,
        &[
            "--target",
            "web",
            "--out-dir",
            path_str(&package.join("dist"))?,
            path_str(&wasm)?,
        ],
        None,
    )?;
    for file in [
        format!("{WASM_STEM}.js"),
        format!("{WASM_STEM}.d.ts"),
        format!("{WASM_STEM}_bg.wasm"),
    ] {
        if !package.join("dist").join(&file).is_file() {
            return Err(format!("wasm-bindgen wrote no dist/{file}"));
        }
    }
    write(
        &package.join("package.json"),
        &pretty(&package_manifest(&version)),
    )?;
    write(&package.join("README.md"), readme(&version).as_bytes())?;
    write(&package.join("LICENSE"), &read(&repo().join("LICENSE"))?)?;

    // npm must pack exactly what is in the directory: nothing left out by `files`, nothing added.
    let expected = files_below(&package)?;
    let out = out_dir()?;
    empty_dir(&out)?;
    let report = parse_json(
        npm(
            &["pack", "--json", "--pack-destination", path_str(&out)?],
            Some(&package),
        )?
        .as_bytes(),
        "npm pack --json",
    )?;
    let report = &report[0];
    let mut packed: Vec<String> = report["files"]
        .as_array()
        .ok_or("npm pack --json lists no files")?
        .iter()
        .filter_map(|file| file["path"].as_str().map(str::to_string))
        .collect();
    packed.sort();
    if packed != expected {
        return Err(format!(
            "npm packed {packed:?}, but the package is {expected:?}"
        ));
    }
    let tarball = out.join(tarball_name(&version));
    if report["filename"] != tarball_name(&version).as_str() || !tarball.is_file() {
        return Err(format!(
            "npm pack wrote {}, not {}",
            report["filename"],
            tarball_name(&version)
        ));
    }
    println!(
        "{}",
        json!({"name": PACKAGE, "version": version, "tarball": path_str(&tarball)?,
               "integrity": npm_integrity(&read(&tarball)?)})
    );
    Ok(())
}

/// `cargo xtask npm-smoke`
pub(crate) fn smoke() -> Result<()> {
    ensure_npm()?;
    let version = engine_version()?;
    let tarball = out_dir()?.join(tarball_name(&version));
    if !tarball.is_file() {
        return Err(format!(
            "{}: no such tarball; run `cargo xtask npm` first",
            tarball.display()
        ));
    }
    // A consumer's project, in a path with a space as a home directory may have.
    let consumer = TempDir::new("sidevoice engine npm smoke")?;
    write(
        &consumer.0.join("package.json"),
        &pretty(&json!({"name": "smoke", "private": true, "type": "module"})),
    )?;
    npm(&["install", path_str(&tarball)?], Some(&consumer.0))?;
    let installed = parse_json(
        &read(
            &consumer
                .0
                .join("node_modules/@sidevoice/engine/package.json"),
        )?,
        "the installed package.json",
    )?;
    if installed["version"] != version.as_str() {
        return Err(format!(
            "installed {PACKAGE}@{}, not {version}",
            installed["version"]
        ));
    }
    write(&consumer.0.join("smoke.mjs"), SMOKE_JS.as_bytes())?;
    let wasm = format!("{WASM_STEM}_bg.wasm");
    let report = output("node", &["smoke.mjs", &wasm], Some(&consumer.0))?;
    let report = parse_json(report.as_bytes(), "smoke.mjs")?;
    if report["backends"] != json!(WEB_BACKENDS) {
        return Err(format!(
            "the packaged engine has the backends {}, not {WEB_BACKENDS:?}",
            report["backends"]
        ));
    }
    println!(
        "{}",
        json!({"installed": PACKAGE, "version": version, "backends": report["backends"]})
    );
    Ok(())
}

/// The workflow npm's trusted publishing sees: the top-level one (`release-please.yml` when it calls
/// `release.yml`), from `GITHUB_WORKFLOW_REF`.
fn calling_workflow() -> String {
    env::var("GITHUB_WORKFLOW_REF")
        .ok()
        .and_then(|reference| {
            let path = reference.split('@').next()?.to_string();
            path.rsplit('/').next().map(str::to_string)
        })
        .unwrap_or_else(|| "(unknown)".into())
}

fn node_version() -> Result<[u64; 3]> {
    let text = output("node", &["--version"], None)?;
    let parts: Vec<u64> = text
        .trim()
        .trim_start_matches('v')
        .split('.')
        .map(|part| part.parse().unwrap_or(0))
        .collect();
    match parts.as_slice() {
        [major, minor, patch] => Ok([*major, *minor, *patch]),
        _ => Err(format!("node --version: {text:?}")),
    }
}

/// What the registry holds for `PACKAGE@version`: its `dist.integrity`, or `None` when no such version is published.
fn published_integrity(version: &str) -> Result<Option<String>> {
    match npm(
        &[
            "view",
            &format!("{PACKAGE}@{version}"),
            "dist.integrity",
            "--json",
        ],
        None,
    ) {
        Ok(text) if text.trim().is_empty() => Ok(None),
        Ok(text) => Ok(parse_json(text.as_bytes(), "npm view")?
            .as_str()
            .map(str::to_string)),
        Err(error) if error.contains("E404") => Ok(None),
        Err(error) => Err(error),
    }
}

/// `cargo xtask npm-publish TAG`
pub(crate) fn publish(tag: &str) -> Result<()> {
    let version = tag
        .strip_prefix('v')
        .filter(|version| version.starts_with(|first: char| first.is_ascii_digit()))
        .ok_or_else(|| {
            format!("{tag} is not a vX.Y.Z release; the nightly is never published to npm")
        })?;
    if engine_version()? != version {
        return Err(format!(
            "Cargo.toml says {}, the release is {tag}",
            engine_version()?
        ));
    }
    let node = node_version()?;
    if node < PUBLISH_NODE {
        return Err(format!(
            "trusted publishing needs Node.js {}.{}.{} or later; this is {}.{}.{}",
            PUBLISH_NODE[0], PUBLISH_NODE[1], PUBLISH_NODE[2], node[0], node[1], node[2]
        ));
    }
    if env::var("ACTIONS_ID_TOKEN_REQUEST_URL").is_err() {
        return Err("no OIDC token: @sidevoice/engine is published only by trusted publishing, from GitHub \
                    Actions with `permissions: id-token: write` (on the calling workflow too); never with a token"
            .into());
    }
    let repository = env::var("GH_REPO").map_err(|_| "GH_REPO is not set")?;

    // Exactly what the GitHub release published, read back and checked against its SHA256SUMS and attestation.
    let name = published_name(version);
    let assets = TempDir::new("sidevoice-engine-npm-assets")?;
    gh(&[
        "release",
        "download",
        tag,
        "--dir",
        path_str(&assets.0)?,
        "--pattern",
        &name,
        "--pattern",
        "SHA256SUMS",
        "--pattern",
        "attestation.sigstore.json",
    ])?;
    let tarball = assets.0.join(&name);
    let sums = parse_sums(&read(&assets.0.join("SHA256SUMS"))?)?;
    let listed = sums
        .iter()
        .find(|(_, listed)| *listed == name)
        .ok_or_else(|| format!("{name} is not in the release's SHA256SUMS"))?;
    let bytes = read(&tarball)?;
    if sha256(&bytes) != listed.0 {
        return Err(format!("{name}: not the bytes SHA256SUMS lists"));
    }
    verify_attestation(
        &tarball,
        &repository,
        &assets.0.join("attestation.sigstore.json"),
        &signer(&repository),
    )?;
    let identity = package_identity(&tarball)?;
    if identity.0 != PACKAGE || identity.1 != version {
        return Err(format!(
            "{name} is {}@{}, not {PACKAGE}@{version}",
            identity.0, identity.1
        ));
    }

    ensure_npm()?;
    let spec = format!("{PACKAGE}@{version}");
    // A re-run carries on: a version already published with these very bytes is left as it is.
    let integrity = npm_integrity(&bytes);
    match published_integrity(version)? {
        Some(published) if published == integrity => {
            println!("{spec} is already published with these bytes");
            return Ok(());
        }
        Some(published) => {
            return Err(format!(
                "{spec} is already published with other bytes ({published}, this build {integrity}); npm versions \
                 are immutable: release a new version"
            ))
        }
        None => {}
    }
    let dist_tag = dist_tag(version);
    npm(
        &[
            "publish",
            path_str(&tarball)?,
            "--access",
            "public",
            "--provenance",
            "--tag",
            dist_tag,
            "--json",
        ],
        None,
    )
    .map_err(|error| {
        format!(
            "{spec}: npm refused it. Publishing uses trusted publishing only: on npmjs.com, {PACKAGE} → Settings → \
             Trusted publisher must name repository {REPOSITORY} and workflow filename {} (the workflow that \
             started this run; npm checks it, not a workflow it calls), no environment. {error}",
            calling_workflow()
        )
    })?;
    println!("published {spec} (dist-tag {dist_tag})");
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A minimal package tarball of `name` at `version` in `dir`, named as `npm pack` names it.
    pub(crate) fn tarball(dir: &Path, name: &str, version: &str) {
        let work = TempDir::new("xtask-npm-tarball").unwrap();
        mkdir(&work.0.join("package")).unwrap();
        write(
            &work.0.join("package/package.json"),
            &pretty(&json!({"name": name, "version": version})),
        )
        .unwrap();
        output(
            "tar",
            &[
                "-czf",
                path_str(&dir.join(tarball_name(version))).unwrap(),
                "-C",
                path_str(&work.0).unwrap(),
                "package",
            ],
            None,
        )
        .unwrap();
    }

    #[test]
    fn the_tarball_is_named_as_npm_pack_names_it() {
        assert_eq!(tarball_name("0.2.0"), "sidevoice-engine-0.2.0.tgz");
        assert_eq!(
            tarball_name("0.2.0-rc.1"),
            "sidevoice-engine-0.2.0-rc.1.tgz"
        );
    }

    #[test]
    fn a_release_goes_to_latest_and_a_candidate_to_next() {
        assert_eq!(dist_tag("0.2.0"), "latest");
        assert_eq!(dist_tag("0.2.0-rc.1"), "next");
    }

    #[test]
    fn the_package_is_an_es_module_of_the_wasm_bindgen_output() {
        let manifest = package_manifest("0.2.0");
        assert_eq!(manifest["name"], "@sidevoice/engine");
        assert_eq!(manifest["version"], "0.2.0");
        assert_eq!(manifest["type"], "module");
        assert_eq!(
            manifest["exports"],
            json!({".": "./dist/sidevoice_engine.js"})
        );
        assert_eq!(manifest["types"], "./dist/sidevoice_engine.d.ts");
        assert_eq!(manifest["license"], "Apache-2.0");
        assert_eq!(
            manifest["repository"]["url"],
            "git+https://github.com/sidevoice/sidevoice-engine.git"
        );
        assert!(readme("0.2.0").contains("version 0.2.0"));
    }

    #[test]
    fn a_tarball_says_what_it_is() {
        let dir = TempDir::new("xtask-npm-identity").unwrap();
        tarball(&dir.0, PACKAGE, "0.2.0");
        assert_eq!(
            package_identity(&dir.0.join(tarball_name("0.2.0"))).unwrap(),
            (PACKAGE.to_string(), "0.2.0".to_string())
        );
    }

    #[test]
    fn the_smoke_script_uses_the_package_by_its_name() {
        assert!(SMOKE_JS.contains(&format!("from \"{PACKAGE}\"")));
        // The wasm file is named by xtask (WASM_STEM), never by the script.
        assert!(SMOKE_JS
            .contains("new URL(process.argv[2], import.meta.resolve(\"@sidevoice/engine\"))"));
    }

    #[test]
    fn the_nightly_is_never_published() {
        let error = publish("nightly").unwrap_err();
        assert!(error.contains("never published to npm"), "{error}");
    }
}
