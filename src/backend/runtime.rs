//! A backend's runtime: which library files it needs on each platform, from the data file `backends.json` at the
//! repository root, compiled in. No backend's code says what it fetches. The engine reads the entry for one backend and
//! the platform it runs on, and only that one; the installer fetches those files with the model's.
//!
//! Every backend lists every platform, always the same six keys: `macos-aarch64`, `macos-x86_64`, `linux-x86_64`,
//! `linux-aarch64`, `windows-x86_64` and `web`. A platform's value is the list of files to download there: `[]` when
//! the backend runs there and downloads nothing, and `null` when it does not run there (the funnel rejects it with
//! `no-runtime-for-platform`). A missing or unknown key is a parse error, so neither an omission nor a typo passes.
//!
//! Each backend has one `version`, from its `upstream`; each file's `url` may say `{version}`, and its `sha256` is
//! written by `cargo xtask pin-backends`, never by hand. The types below are the file's schema: anything they do not
//! name is an error, and the tests parse it on every target.

use std::sync::OnceLock;

use serde::Deserialize;

use crate::host::{Capabilities, Runs};
use crate::install::Artifact;

const DATA: &str = include_str!("../../backends.json");

/// A platform `backends.json` has a key for: an OS and an architecture, or the web build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Platform {
    MacosAarch64,
    MacosX86_64,
    LinuxX86_64,
    LinuxAarch64,
    WindowsX86_64,
    Web,
}

impl Platform {
    /// The platform the host reports, if `backends.json` knows it.
    pub(crate) fn of(caps: &Capabilities) -> Option<Self> {
        if caps.runs == Runs::Page {
            return Some(Self::Web);
        }
        match (caps.os.as_str(), caps.arch.as_str()) {
            ("macos", "aarch64") => Some(Self::MacosAarch64),
            ("macos", "x86_64") => Some(Self::MacosX86_64),
            ("linux", "x86_64") => Some(Self::LinuxX86_64),
            ("linux", "aarch64") => Some(Self::LinuxAarch64),
            ("windows", "x86_64") => Some(Self::WindowsX86_64),
            _ => None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Data {
    backends: Vec<Entry>,
}

/// One backend: what it is, where it comes from, its version and its files per platform.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    /// Its `BackendSpec` id.
    id: String,
    #[allow(
        dead_code,
        reason = "for people: the engine reads ids, versions and files"
    )]
    name: String,
    #[allow(
        dead_code,
        reason = "for people: the engine reads ids, versions and files"
    )]
    description: String,
    #[allow(dead_code, reason = "for people: the engine reads the urls")]
    upstream: String,
    version: String,
    platforms: Platforms,
}

/// Every platform's files: `null` where the backend does not run, `[]` where it runs and downloads nothing. Every key
/// is required (`deserialize_with` stops serde from reading a missing one as `null`), and no other is allowed.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Platforms {
    #[serde(rename = "macos-aarch64", deserialize_with = "Option::deserialize")]
    macos_aarch64: Option<Vec<File>>,
    #[serde(rename = "macos-x86_64", deserialize_with = "Option::deserialize")]
    macos_x86_64: Option<Vec<File>>,
    #[serde(rename = "linux-x86_64", deserialize_with = "Option::deserialize")]
    linux_x86_64: Option<Vec<File>>,
    #[serde(rename = "linux-aarch64", deserialize_with = "Option::deserialize")]
    linux_aarch64: Option<Vec<File>>,
    #[serde(rename = "windows-x86_64", deserialize_with = "Option::deserialize")]
    windows_x86_64: Option<Vec<File>>,
    #[serde(deserialize_with = "Option::deserialize")]
    web: Option<Vec<File>>,
}

impl Platforms {
    /// The files to download on `platform`, or `None` if the backend does not run there.
    fn get(&self, platform: Platform) -> Option<&[File]> {
        match platform {
            Platform::MacosAarch64 => &self.macos_aarch64,
            Platform::MacosX86_64 => &self.macos_x86_64,
            Platform::LinuxX86_64 => &self.linux_x86_64,
            Platform::LinuxAarch64 => &self.linux_aarch64,
            Platform::WindowsX86_64 => &self.windows_x86_64,
            Platform::Web => &self.web,
        }
        .as_deref()
    }
}

/// One file to download.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    /// What `load` finds it by in `Installed`.
    name: String,
    /// Where from; `{version}` is the backend's version.
    url: String,
    sha256: String,
}

fn entries() -> &'static [Entry] {
    static DATA_PARSED: OnceLock<Data> = OnceLock::new();
    &DATA_PARSED
        .get_or_init(|| serde_json::from_str(DATA).expect("backends.json is checked by the tests"))
        .backends
}

/// The library files `backend` needs on `platform`, each with the name `load` finds it by, or `None` when it does not
/// run there (`null`, or no entry for `backend` at all).
pub(crate) fn runtime_files(backend: &str, platform: Platform) -> Option<Vec<(String, Artifact)>> {
    let entry = entries().iter().find(|entry| entry.id == backend)?;
    let files = entry.platforms.get(platform)?;
    let artifact = |file: &File| Artifact {
        key: format!("backends/{}/{}/{}", entry.id, entry.version, file.name),
        url: file.url.replace("{version}", &entry.version),
        sha256: file.sha256.clone(),
    };
    Some(
        files
            .iter()
            .map(|file| (file.name.clone(), artifact(file)))
            .collect(),
    )
}

#[cfg(test)]
mod tests;
