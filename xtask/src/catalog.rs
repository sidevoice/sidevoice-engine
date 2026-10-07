//! The catalogue's pins, which nobody types. `pin-catalog` reads every family in `catalog/families/`, and for each file
//! of each build, whose `url` is a Hugging Face `resolve` URL at any revision (a branch, a tag or a commit):
//!
//! - pins the URL to the commit that revision names, so that the file can never change under it;
//! - writes its `bytes` and `sha256` from the Hugging Face API: the LFS digest the API reports, or, for a small file
//!   kept in git rather than LFS, the digest of the file downloaded (`curl`).
//!
//! A build with no `memory`, or an `estimated` one, gets its estimate: its weights (its `.onnx`, `.bin`, `.npz`,
//! `.safetensors` and `.gguf` files) plus 30%, in MB rounded up to a multiple of 10. A `declared` or `measured` figure
//! is left as it is. `pin-catalog --check` derives all of it again and fails if anything differs, writing nothing.

use std::collections::HashMap;
use std::env;
use std::path::Path;

use serde_json::{json, Value};

use crate::{empty_dir, read, repo, run_in, sha256, write, Result};

const HUB: &str = "https://huggingface.co/";
const WEIGHTS: [&str; 5] = [".onnx", ".bin", ".npz", ".safetensors", ".gguf"];
const BASIS: &str = "weights size + 30%";

/// What the Hugging Face API says about one file at one commit.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    bytes: u64,
    /// Its SHA-256, when it is in LFS.
    lfs_sha256: Option<String>,
}

/// What pinning asks Hugging Face, so that the tests can answer instead.
trait Hub {
    /// The commit `revision` of `repo` names.
    fn commit(&mut self, repo: &str, revision: &str) -> Result<String>;
    /// The file `path` of `repo` at `commit`.
    fn entry(&mut self, repo: &str, commit: &str, path: &str) -> Result<Entry>;
    /// The SHA-256 of the file at `url`, downloaded.
    fn download_sha256(&mut self, url: &str) -> Result<String>;
}

/// `cargo xtask pin-catalog [--check]`.
pub(crate) fn pin(check: bool) -> Result<()> {
    let dir = repo().join("catalog/families");
    let mut paths: Vec<_> = std::fs::read_dir(&dir)
        .map_err(|error| format!("{}: {error}", dir.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    paths.sort();
    let mut hub = HuggingFace::default();
    let mut failed = Vec::new();
    for path in paths {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let text = String::from_utf8(read(&path)?).map_err(|_| format!("{name}: not UTF-8"))?;
        let mut doc: Value =
            serde_json::from_str(&text).map_err(|error| format!("{name}: {error}"))?;
        let stale = repin(&mut doc, &mut hub)?;
        let pinned = format!("{doc:#}\n");
        for what in &stale {
            println!("{} {name} {what}", if check { "stale:" } else { "pinned:" });
        }
        if check {
            if !stale.is_empty() || pinned != text {
                failed.push(name.into_owned());
            }
        } else {
            write(&path, pinned.as_bytes())?;
        }
    }
    if !failed.is_empty() {
        let failed = failed.join(", ");
        return Err(format!(
            "stale or unformatted: {failed}; run `cargo xtask pin-catalog`"
        ));
    }
    Ok(())
}

/// Pins every file of every build of the family `doc`, estimates memory where it is estimated, and returns what
/// changed, as `build key` or `build memory`.
fn repin(doc: &mut Value, hub: &mut impl Hub) -> Result<Vec<String>> {
    let mut stale = Vec::new();
    let models = doc["models"].as_array_mut().ok_or("no models")?;
    for model in models {
        let builds = model["builds"]
            .as_array_mut()
            .ok_or("a model with no builds")?;
        for build in builds {
            let id = build["id"].as_str().ok_or("a build with no id")?.to_owned();
            let mut weights = 0;
            let files = build["files"].as_array_mut();
            for file in files.ok_or(format!("{id}: no files"))? {
                let what = format!("{id} {}", file["key"].as_str().unwrap_or_default());
                let url = file["url"].as_str().ok_or(format!("{what}: no url"))?;
                let (url, entry, sha256) =
                    pin_file(url, hub).map_err(|e| format!("{what}: {e}"))?;
                if WEIGHTS.iter().any(|ext| url.ends_with(ext)) {
                    weights += entry.bytes;
                }
                let before = file.clone();
                file["url"] = url.into();
                file["sha256"] = sha256.into();
                file["bytes"] = entry.bytes.into();
                if *file != before {
                    stale.push(what);
                }
            }
            let source = build["memory"]["source"].as_str();
            if source.is_none_or(|source| source == "estimated") {
                let memory =
                    json!({"mb": estimate_mb(weights), "source": "estimated", "basis": BASIS});
                if build["memory"] != memory {
                    stale.push(format!("{id} memory"));
                    build["memory"] = memory;
                }
            }
        }
    }
    Ok(stale)
}

/// The URL pinned to a commit, what the API says of the file, and its digest.
fn pin_file(url: &str, hub: &mut impl Hub) -> Result<(String, Entry, String)> {
    let (repo, rest) = url
        .strip_prefix(HUB)
        .and_then(|rest| rest.split_once("/resolve/"))
        .ok_or("not a Hugging Face resolve URL")?;
    let (revision, path) = rest.split_once('/').ok_or("no path after the revision")?;
    let commit = if revision.len() == 40 && revision.bytes().all(|c| c.is_ascii_hexdigit()) {
        revision.to_owned()
    } else {
        hub.commit(repo, revision)?
    };
    let url = format!("{HUB}{repo}/resolve/{commit}/{path}");
    let entry = hub.entry(repo, &commit, &percent_decoded(path)?)?;
    let sha256 = match &entry.lfs_sha256 {
        Some(sha256) => sha256.clone(),
        None => hub.download_sha256(&url)?,
    };
    Ok((url, entry, sha256))
}

/// A URL's path as the API names the file: `espeak-ng-data/voices/%21v/Mr%20serious` is `…/!v/Mr serious`.
fn percent_decoded(path: &str) -> Result<String> {
    let mut bytes = Vec::with_capacity(path.len());
    let mut rest = path.as_bytes();
    while let Some((&byte, tail)) = rest.split_first() {
        rest = tail;
        if byte != b'%' {
            bytes.push(byte);
            continue;
        }
        let hex = tail.get(..2).and_then(|hex| std::str::from_utf8(hex).ok());
        let decoded = hex.and_then(|hex| u8::from_str_radix(hex, 16).ok());
        bytes.push(decoded.ok_or(format!("{path}: a bad escape"))?);
        rest = &tail[2..];
    }
    String::from_utf8(bytes).map_err(|_| format!("{path}: not UTF-8"))
}

/// `weights` bytes plus 30%, in MB (MiB), rounded up to a multiple of 10.
fn estimate_mb(weights: u64) -> u64 {
    let mb = (weights * 13).div_ceil(10 * 1024 * 1024);
    mb.div_ceil(10) * 10
}

/// The Hugging Face API through `curl`, each answer asked once.
#[derive(Default)]
struct HuggingFace {
    commits: HashMap<(String, String), String>,
    trees: HashMap<(String, String), HashMap<String, Entry>>,
    digests: HashMap<String, String>,
}

impl HuggingFace {
    fn get(url: &str) -> Result<Value> {
        let body = run_in(Path::new("."), "curl -fsSL --retry 3", &[url])?;
        serde_json::from_str(&body).map_err(|error| format!("{url}: {error}"))
    }
}

impl Hub for HuggingFace {
    fn commit(&mut self, repo: &str, revision: &str) -> Result<String> {
        let key = (repo.to_owned(), revision.to_owned());
        if let Some(commit) = self.commits.get(&key) {
            return Ok(commit.clone());
        }
        let info = Self::get(&format!("{HUB}api/models/{repo}/revision/{revision}"))?;
        let commit = info["sha"]
            .as_str()
            .ok_or(format!("{repo}@{revision}: no commit"))?;
        self.commits.insert(key, commit.to_owned());
        Ok(commit.to_owned())
    }

    fn entry(&mut self, repo: &str, commit: &str, path: &str) -> Result<Entry> {
        let key = (repo.to_owned(), commit.to_owned());
        if !self.trees.contains_key(&key) {
            let url = format!("{HUB}api/models/{repo}/tree/{commit}?recursive=true");
            let tree = Self::get(&url)?;
            let tree = tree.as_array().ok_or(format!("{url}: not a list"))?;
            // The API pages long listings; none of ours is that long, and a partial one must not pass for whole.
            if tree.len() >= 1000 {
                return Err(format!("{url}: paged listing, not supported"));
            }
            let files = tree
                .iter()
                .filter(|item| item["type"] == "file")
                .map(|item| {
                    let entry = Entry {
                        bytes: item["size"].as_u64().unwrap_or_default(),
                        lfs_sha256: item["lfs"]["oid"].as_str().map(str::to_owned),
                    };
                    (item["path"].as_str().unwrap_or_default().to_owned(), entry)
                });
            self.trees.insert(key.clone(), files.collect());
        }
        self.trees[&key]
            .get(path)
            .cloned()
            .ok_or(format!("{repo}@{commit}: no file {path}"))
    }

    fn download_sha256(&mut self, url: &str) -> Result<String> {
        if let Some(sha256) = self.digests.get(url) {
            return Ok(sha256.clone());
        }
        let dir = env::temp_dir().join("xtask-pin-catalog");
        empty_dir(&dir)?;
        run_in(&dir, "curl -fsSL --retry 3 -o download", &[url])?;
        let sha256 = sha256(&read(&dir.join("download"))?);
        self.digests.insert(url.to_owned(), sha256.clone());
        Ok(sha256)
    }
}

#[cfg(test)]
mod tests;
