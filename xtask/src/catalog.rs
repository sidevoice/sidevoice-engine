//! The catalogue's pins, which nobody types. `pin-catalog` reads every family in `catalog/families/` and pins each file
//! of each build from its source's API:
//!
//! - a Hugging Face `resolve` URL at any revision (a branch, a tag or a commit) is pinned to the commit that revision
//!   names, so that the file can never change under it; its `bytes` and `sha256` are the API's (the LFS digest), or,
//!   for a small file kept in git rather than LFS, those of the file downloaded (`curl`);
//! - a GitHub release asset (`https://github.com/<owner>/<repo>/releases/download/<tag>/<name>`) cannot be pinned by
//!   its URL, since an asset can be uploaded again: it is marked `mutable`, and its `bytes` and `sha256` are the
//!   release API's (or, for an asset the API has no digest for, those of the file downloaded). Its `sha256` is then
//!   all that pins it.
//!
//! A file's `archive_path` (the file or directory inside an archive that its key names) is left as it is.
//!
//! A build with no `memory`, or an `estimated` one, gets its estimate: its weights (the files whose name, or path
//! inside their archive, ends in `.onnx`, `.onnx_data` (external weights), `.bin`, `.npz`, `.safetensors` or `.gguf`;
//! an archive counted once, at its size) plus 30%, in MB rounded up to a multiple of 10. A `declared` or `measured` figure is left as it is.
//! A model with a `languages_source` gets its `languages` from it: today, Whisper's, from the `LANGUAGES` table of
//! openai/whisper's tokenizer at a pinned commit.
//!
//! `pin-catalog --check` derives all of it again and fails if anything differs, writing nothing.

use std::collections::{HashMap, HashSet};
use std::env;
use std::path::Path;

use serde_json::{json, Value};

use crate::{empty_dir, read, repo, run_in, sha256, write, Result};

const HUB: &str = "https://huggingface.co/";
const RELEASES: &str = "https://github.com/";
const WEIGHTS: [&str; 6] = [
    ".onnx",
    ".onnx_data",
    ".bin",
    ".npz",
    ".safetensors",
    ".gguf",
];
const BASIS: &str = "weights size + 30%";

/// What an API says about one file.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    bytes: u64,
    /// Its SHA-256, when the API reports it (Hugging Face for LFS files, GitHub for recent assets).
    sha256: Option<String>,
}

/// What pinning asks Hugging Face and GitHub, so that the tests can answer instead.
trait Hub {
    /// The commit `revision` of the Hugging Face `repo` names.
    fn commit(&mut self, repo: &str, revision: &str) -> Result<String>;
    /// The file `path` of the Hugging Face `repo` at `commit`.
    fn entry(&mut self, repo: &str, commit: &str, path: &str) -> Result<Entry>;
    /// The asset `name` of the GitHub `repo`'s release `tag`.
    fn release_asset(&mut self, repo: &str, tag: &str, name: &str) -> Result<Entry>;
    /// The SHA-256 of the file at `url`, downloaded.
    fn download_sha256(&mut self, url: &str) -> Result<String>;
    /// The text of the file at `url`, downloaded.
    fn download_text(&mut self, url: &str) -> Result<String>;
}

/// A file pinned: where from, what it is, and whether only its digest pins it.
struct Pinned {
    url: String,
    bytes: u64,
    sha256: String,
    mutable: bool,
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
    let mut hub = Sources::default();
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
        if let Some(what) = repin_languages(model, hub)? {
            stale.push(what);
        }
        let builds = model["builds"]
            .as_array_mut()
            .ok_or("a model with no builds")?;
        for build in builds {
            let id = build["id"].as_str().ok_or("a build with no id")?.to_owned();
            let mut weights = 0;
            // An archive several keys name is one download: its size counts once.
            let mut counted = HashSet::new();
            let files = build["files"].as_array_mut();
            for file in files.ok_or(format!("{id}: no files"))? {
                let what = format!("{id} {}", file["key"].as_str().unwrap_or_default());
                let url = file["url"].as_str().ok_or(format!("{what}: no url"))?;
                let pinned = pin_file(url, hub).map_err(|e| format!("{what}: {e}"))?;
                let name = file["archive_path"].as_str().unwrap_or(&pinned.url);
                if WEIGHTS.iter().any(|ext| name.ends_with(ext))
                    && counted.insert(pinned.url.clone())
                {
                    weights += pinned.bytes;
                }
                let before = file.clone();
                file["url"] = pinned.url.into();
                file["sha256"] = pinned.sha256.into();
                file["bytes"] = pinned.bytes.into();
                let object = file
                    .as_object_mut()
                    .ok_or(format!("{what}: not an object"))?;
                if pinned.mutable {
                    object.insert("mutable".into(), true.into());
                } else {
                    object.remove("mutable");
                }
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

/// The file at `url`, pinned: a Hugging Face file to its commit, a GitHub release asset by its digest alone.
fn pin_file(url: &str, hub: &mut impl Hub) -> Result<Pinned> {
    if let Some(rest) = url.strip_prefix(RELEASES) {
        let (repo, asset) = rest
            .split_once("/releases/download/")
            .ok_or("not a Hugging Face resolve URL or a GitHub release asset")?;
        let (tag, name) = asset.split_once('/').ok_or("no asset after the tag")?;
        let entry = hub.release_asset(repo, &percent_decoded(tag)?, &percent_decoded(name)?)?;
        let sha256 = match entry.sha256 {
            Some(sha256) => sha256,
            None => hub.download_sha256(url)?,
        };
        return Ok(Pinned {
            url: url.to_owned(),
            bytes: entry.bytes,
            sha256,
            mutable: true,
        });
    }
    let (repo, rest) = url
        .strip_prefix(HUB)
        .and_then(|rest| rest.split_once("/resolve/"))
        .ok_or("not a Hugging Face resolve URL or a GitHub release asset")?;
    let (revision, path) = rest.split_once('/').ok_or("no path after the revision")?;
    let commit = if revision.len() == 40 && revision.bytes().all(|c| c.is_ascii_hexdigit()) {
        revision.to_owned()
    } else {
        hub.commit(repo, revision)?
    };
    let url = format!("{HUB}{repo}/resolve/{commit}/{path}");
    let entry = hub.entry(repo, &commit, &percent_decoded(path)?)?;
    let sha256 = match entry.sha256 {
        Some(sha256) => sha256,
        None => hub.download_sha256(&url)?,
    };
    Ok(Pinned {
        url,
        bytes: entry.bytes,
        sha256,
        mutable: false,
    })
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

/// The Hugging Face and GitHub APIs through `curl`, each answer asked once. GitHub is asked with `GITHUB_TOKEN` when
/// it is set (CI's runners share the anonymous rate limit).
#[derive(Default)]
struct Sources {
    commits: HashMap<(String, String), String>,
    trees: HashMap<(String, String), HashMap<String, Entry>>,
    releases: HashMap<(String, String), Value>,
    digests: HashMap<String, String>,
}

impl Sources {
    fn get(url: &str, headers: &[&str]) -> Result<Value> {
        let mut args: Vec<&str> = headers.iter().flat_map(|header| ["-H", header]).collect();
        args.push(url);
        let body = run_in(Path::new("."), "curl -fsSL --retry 3", &args)?;
        serde_json::from_str(&body).map_err(|error| format!("{url}: {error}"))
    }
}

impl Hub for Sources {
    fn commit(&mut self, repo: &str, revision: &str) -> Result<String> {
        let key = (repo.to_owned(), revision.to_owned());
        if let Some(commit) = self.commits.get(&key) {
            return Ok(commit.clone());
        }
        let info = Self::get(&format!("{HUB}api/models/{repo}/revision/{revision}"), &[])?;
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
            let tree = Self::get(&url, &[])?;
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
                        sha256: item["lfs"]["oid"].as_str().map(str::to_owned),
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

    fn release_asset(&mut self, repo: &str, tag: &str, name: &str) -> Result<Entry> {
        let key = (repo.to_owned(), tag.to_owned());
        if !self.releases.contains_key(&key) {
            let url = format!("https://api.github.com/repos/{repo}/releases/tags/{tag}");
            let auth =
                env::var("GITHUB_TOKEN").map(|token| format!("Authorization: Bearer {token}"));
            let headers: Vec<&str> = auth.as_deref().into_iter().collect();
            self.releases
                .insert(key.clone(), Self::get(&url, &headers)?);
        }
        let assets = self.releases[&key]["assets"].as_array();
        let asset = assets
            .into_iter()
            .flatten()
            .find(|asset| asset["name"] == name)
            .ok_or(format!("{repo} {tag}: no asset {name}"))?;
        let sha256 = asset["digest"]
            .as_str()
            .and_then(|digest| digest.strip_prefix("sha256:"));
        Ok(Entry {
            bytes: asset["size"]
                .as_u64()
                .ok_or(format!("{repo} {tag} {name}: no size"))?,
            sha256: sha256.map(str::to_owned),
        })
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

    fn download_text(&mut self, url: &str) -> Result<String> {
        let dir = env::temp_dir().join("xtask-pin-catalog");
        empty_dir(&dir)?;
        run_in(&dir, "curl -fsSL --retry 3 -o download", &[url])?;
        String::from_utf8(read(&dir.join("download"))?).map_err(|_| format!("{url}: not UTF-8"))
    }
}

/// Writes a model's `languages` from its `languages_source`, if it has one, and says so when they were not already
/// what the source lists. The only source read today is openai/whisper's tokenizer (`whisper/tokenizer.py`, at a
/// pinned commit): its `LANGUAGES` table's codes, in its order, with Whisper's one code that is not BCP 47 (`jw`)
/// written as its BCP 47 tag (`jv`).
fn repin_languages(model: &mut Value, hub: &mut impl Hub) -> Result<Option<String>> {
    let Some(source) = model["languages_source"].as_str() else {
        return Ok(None);
    };
    let id = model["id"].as_str().unwrap_or_default().to_owned();
    if !(source.starts_with("https://raw.githubusercontent.com/openai/whisper/")
        && source.ends_with("/whisper/tokenizer.py"))
    {
        return Err(format!("{id}: no reader for the languages source {source}"));
    }
    let languages = json!(whisper_languages(&hub.download_text(source)?)
        .map_err(|e| format!("{id}: {source}: {e}"))?);
    if model["languages"] == languages {
        return Ok(None);
    }
    model["languages"] = languages;
    Ok(Some(format!("{id} languages")))
}

/// The language codes of Whisper's `LANGUAGES = { "en": "english", ... }`, in order, as BCP 47 tags.
fn whisper_languages(tokenizer: &str) -> Result<Vec<String>> {
    let table = tokenizer
        .split_once("LANGUAGES = {")
        .and_then(|(_, rest)| rest.split_once('}'))
        .map(|(table, _)| table)
        .ok_or("no LANGUAGES table")?;
    let codes: Vec<String> = table
        .lines()
        .filter_map(|line| line.trim().strip_prefix('"')?.split_once('"'))
        .map(|(code, _)| if code == "jw" { "jv" } else { code }.to_owned())
        .collect();
    if codes.is_empty() {
        return Err("an empty LANGUAGES table".into());
    }
    Ok(codes)
}

#[cfg(test)]
mod tests;
