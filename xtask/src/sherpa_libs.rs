//! `cargo xtask sherpa-libs`: sherpa-onnx's prebuilt static libraries, the ones the `sherpa-onnx` crate links, fetched
//! once, checked against a pinned digest and kept outside Cargo's target directory, for the build to link through
//! `SHERPA_ONNX_LIB_DIR`.
//!
//! Left alone, `sherpa-onnx-sys`'s build script downloads its archive into `target/sherpa-onnx-prebuilt`, checks
//! nothing, and links from there. A build cache that prunes `target/` (CI's rust-cache keeps its directories and
//! drops their files) then leaves the link pointing at an empty directory, and the script, which returns that
//! directory as long as it exists, never fetches it again. With `SHERPA_ONNX_LIB_DIR` set, the script downloads
//! nothing and links the directory given; this command is what fills it.
//!
//! - `sherpa-libs [DIR]`: the archive for this machine in DIR (default `~/.cache/sidevoice-engine/sherpa-onnx`),
//!   downloaded unless it is there, checked against `xtask/sherpa-onnx-libs.json`, and its `lib/` unpacked beside it.
//!   Prints that directory, and appends `SHERPA_ONNX_LIB_DIR=<it>` to `$GITHUB_ENV` when that is set.
//! - `sherpa-libs --linked`: every build of `sherpa-onnx-sys` under the target directory linked from
//!   `$SHERPA_ONNX_LIB_DIR`, and nothing downloaded into `target/sherpa-onnx-prebuilt` (CI's proof).
//! - `sherpa-libs --pin` / `--check`: the file written (or checked) from the version Cargo.toml pins and the
//!   digests GitHub publishes for the release's assets.
//!
//! The archive names are the static ones of `sherpa-onnx-sys`'s build script (`archive_name`), for the version
//! pinned; the archive holds `<name without .tar.bz2>/lib`, as the script expects.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::{env, io};

use serde::{Deserialize, Serialize};

use crate::{metadata, read, repo, run_in, sha256, write, Result};

/// The pinned archives, relative to the repository.
const PINS: &str = "xtask/sherpa-onnx-libs.json";
const RELEASES: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download";

/// `xtask/sherpa-onnx-libs.json`.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Pins {
    /// sherpa-onnx's version: the one Cargo.toml pins the crate to.
    version: String,
    /// Per platform (`<os>-<arch>`, Rust's names), its static archive.
    archives: BTreeMap<String, Archive>,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Archive {
    name: String,
    sha256: String,
}

/// `cargo xtask sherpa-libs [DIR]`.
pub(crate) fn fetch(dir: Option<&str>) -> Result<()> {
    let pins = pins()?;
    let platform = format!("{}-{}", env::consts::OS, env::consts::ARCH);
    let archive = pins
        .archives
        .get(&platform)
        .ok_or(format!("{PINS}: no archive for {platform}"))?;
    let dir = match dir {
        Some(dir) => PathBuf::from(dir),
        None => default_dir()?,
    };
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = dir.join(&archive.name);
    if !path.is_file() || sha256(&read(&path)?) != archive.sha256 {
        let url = format!("{RELEASES}/v{}/{}", pins.version, archive.name);
        let partial = dir.join(format!("{}.partial", archive.name));
        let partial_text = partial.to_string_lossy().into_owned();
        run_in(&dir, "curl -fsSL --retry 3 -o", &[&partial_text, &url])?;
        let got = sha256(&read(&partial)?);
        if got != archive.sha256 {
            let _ = fs::remove_file(&partial);
            return Err(format!("{url}: sha256 {got}, pinned {}", archive.sha256));
        }
        fs::rename(&partial, &path).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    let stem = archive
        .name
        .strip_suffix(".tar.bz2")
        .ok_or(format!("{}: not a .tar.bz2", archive.name))?;
    let lib = dir.join(stem).join("lib");
    if !lib.is_dir() {
        unpack(&path, &dir, stem)?;
    }
    let lib = lib.to_string_lossy().into_owned();
    println!("{lib}");
    if let Some(github_env) = env::var_os("GITHUB_ENV") {
        let mut text =
            String::from_utf8_lossy(&read(Path::new(&github_env)).unwrap_or_default()).into_owned();
        text.push_str(&format!("SHERPA_ONNX_LIB_DIR={lib}\n"));
        write(Path::new(&github_env), text.as_bytes())?;
    }
    Ok(())
}

/// Unpacks `archive` into `dir`, through a scratch directory so that a half-unpacked `stem/` never stands for a whole
/// one.
fn unpack(archive: &Path, dir: &Path, stem: &str) -> Result<()> {
    let scratch = dir.join(format!("{stem}.unpacking"));
    let _ = fs::remove_dir_all(&scratch);
    let file = File::open(archive).map_err(|e| format!("{}: {e}", archive.display()))?;
    tar::Archive::new(bzip2::read::BzDecoder::new(io::BufReader::new(file)))
        .unpack(&scratch)
        .map_err(|e| format!("{}: {e}", archive.display()))?;
    if !scratch.join(stem).join("lib").is_dir() {
        return Err(format!("{}: no {stem}/lib inside", archive.display()));
    }
    let _ = fs::remove_dir_all(dir.join(stem));
    fs::rename(scratch.join(stem), dir.join(stem)).map_err(|e| format!("{stem}: {e}"))?;
    let _ = fs::remove_dir_all(&scratch);
    Ok(())
}

/// `~/.cache/sidevoice-engine/sherpa-onnx`.
fn default_dir() -> Result<PathBuf> {
    let home = env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .ok_or("no HOME: pass a directory")?;
    Ok(PathBuf::from(home).join(".cache/sidevoice-engine/sherpa-onnx"))
}

/// `cargo xtask sherpa-libs --linked`.
pub(crate) fn linked() -> Result<()> {
    let lib = env::var("SHERPA_ONNX_LIB_DIR").map_err(|_| "SHERPA_ONNX_LIB_DIR is not set")?;
    let (_, target) = metadata()?;
    let mut outputs = Vec::new();
    find_outputs(&target, 0, &mut outputs);
    if outputs.is_empty() {
        return Err(format!("{}: no build of sherpa-onnx-sys", target.display()));
    }
    let want = format!("cargo:rustc-link-search=native={lib}");
    for output in &outputs {
        let text = String::from_utf8_lossy(&read(output)?).into_owned();
        if !text.lines().any(|line| line == want) {
            return Err(format!("{}: not linked from {lib}", output.display()));
        }
        println!("{}: linked from {lib}", output.display());
    }
    let prebuilt = target.join("sherpa-onnx-prebuilt");
    if has_files(&prebuilt) {
        return Err(format!(
            "{}: the build script downloaded",
            prebuilt.display()
        ));
    }
    Ok(())
}

/// Every `build/sherpa-onnx-sys-*/output` below `dir`, link-size's nested target directory included.
fn find_outputs(dir: &Path, depth: usize, outputs: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if !path.is_dir() || matches!(name.as_str(), "deps" | ".fingerprint" | "incremental") {
            continue;
        }
        if name.starts_with("sherpa-onnx-sys-") && path.join("output").is_file() {
            outputs.push(path.join("output"));
        } else if depth < 6 {
            find_outputs(&path, depth + 1, outputs);
        }
    }
}

fn has_files(dir: &Path) -> bool {
    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .any(|entry| {
            let path = entry.path();
            if path.is_dir() {
                has_files(&path)
            } else {
                true
            }
        })
}

/// `cargo xtask sherpa-libs --pin` (or `--check`).
pub(crate) fn pin(check: bool) -> Result<()> {
    let version = crate_version(&String::from_utf8_lossy(&read(&repo().join("Cargo.toml"))?))?;
    let release = release(&version)?;
    let mut archives = BTreeMap::new();
    for (platform, name) in archive_names(&version) {
        let sha256 = release
            .get(&name)
            .ok_or(format!("sherpa-onnx v{version}: no asset {name}"))?
            .clone();
        archives.insert(platform, Archive { name, sha256 });
    }
    let pinned = Pins { version, archives };
    if check {
        if pins()? != pinned {
            return Err(format!(
                "{PINS} is not what --pin writes: run cargo xtask sherpa-libs --pin"
            ));
        }
        return Ok(());
    }
    let mut text = serde_json::to_string_pretty(&pinned).map_err(|e| e.to_string())?;
    text.push('\n');
    write(&repo().join(PINS), text.as_bytes())
}

fn pins() -> Result<Pins> {
    serde_json::from_slice(&read(&repo().join(PINS))?).map_err(|e| format!("{PINS}: {e}"))
}

/// The version Cargo.toml pins the `sherpa-onnx` crate to (`version = "=X.Y.Z"`).
fn crate_version(manifest: &str) -> Result<String> {
    manifest
        .lines()
        .filter(|line| line.starts_with("sherpa-onnx = "))
        .find_map(|line| line.split("version = \"=").nth(1)?.split('"').next())
        .map(str::to_owned)
        .ok_or("Cargo.toml: no sherpa-onnx = { version = \"=X.Y.Z\", ... }".into())
}

/// The static archive of each platform, as `sherpa-onnx-sys`'s build script names it (`archive_name`, static arms).
fn archive_names(version: &str) -> Vec<(String, String)> {
    [
        ("linux-x86_64", "linux-x64-static-lib"),
        ("linux-aarch64", "linux-aarch64-static-lib"),
        ("macos-x86_64", "osx-x64-static-lib"),
        ("macos-aarch64", "osx-arm64-static-lib"),
        ("windows-x86_64", "win-x64-static-MT-Release-lib"),
        ("windows-aarch64", "win-arm64-static-MT-Release-lib"),
    ]
    .into_iter()
    .map(|(platform, kind)| {
        (
            platform.to_owned(),
            format!("sherpa-onnx-v{version}-{kind}.tar.bz2"),
        )
    })
    .collect()
}

/// Each asset of the release `v<version>` and its SHA-256, as GitHub publishes it. Asked with `GITHUB_TOKEN` when it
/// is set.
fn release(version: &str) -> Result<BTreeMap<String, String>> {
    let url = format!("https://api.github.com/repos/k2-fsa/sherpa-onnx/releases/tags/v{version}");
    let auth = env::var("GITHUB_TOKEN").map(|token| format!("Authorization: Bearer {token}"));
    let mut args: Vec<&str> = auth
        .as_deref()
        .into_iter()
        .flat_map(|h| ["-H", h])
        .collect();
    args.push(&url);
    let body = run_in(Path::new("."), "curl -fsSL --retry 3", &args)?;
    let release: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| format!("{url}: {e}"))?;
    Ok(release["assets"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|asset| {
            let name = asset["name"].as_str()?;
            let sha256 = asset["digest"].as_str()?.strip_prefix("sha256:")?;
            Some((name.to_owned(), sha256.to_owned()))
        })
        .collect())
}

#[cfg(test)]
mod tests;
