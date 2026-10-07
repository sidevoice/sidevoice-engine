//! What each backend downloads, per platform: the data file `backends.json` at the repository root, compiled in. No
//! backend's code says what it fetches. The engine reads the entry for one backend and the platform it runs on, and
//! only that one; a backend with no entry for this platform cannot run here.
//!
//! Each backend has one `version`, which Renovate watches on its `upstream`'s GitHub releases; each file's `url` may
//! say `{version}`, and its `sha256` is written by `cargo xtask pin-backends`, never by hand. The types below are the
//! file's schema: anything they do not name is an error, and the tests parse it on every target.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::Deserialize;

use crate::host::{Capabilities, Runs};
use crate::install::Artifact;

const DATA: &str = include_str!("../../backends.json");

/// A platform `backends.json` can have downloads for: an OS and an architecture, or the web build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
pub(crate) enum Platform {
    #[serde(rename = "macos-aarch64")]
    MacosAarch64,
    #[serde(rename = "macos-x86_64")]
    MacosX86_64,
    #[serde(rename = "linux-x86_64")]
    LinuxX86_64,
    #[serde(rename = "linux-aarch64")]
    LinuxAarch64,
    #[serde(rename = "windows-x86_64")]
    WindowsX86_64,
    #[serde(rename = "web")]
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
    #[allow(
        dead_code,
        reason = "for Renovate and pin-backends: the engine reads the urls"
    )]
    upstream: String,
    version: String,
    platforms: BTreeMap<Platform, Vec<File>>,
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

/// The files `backend` downloads on `platform`, each with the name `load` finds it by, or `None` when its entry has
/// none for `platform`: it cannot run there.
pub(crate) fn downloads(backend: &str, platform: Platform) -> Option<Vec<(String, Artifact)>> {
    let entry = entries().iter().find(|entry| entry.id == backend)?;
    let files = entry.platforms.get(&platform)?;
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
mod tests {
    use std::collections::HashSet;

    use super::{entries, Platform};
    use crate::host::{Capabilities, Runs};

    #[cfg(web)]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    #[test]
    fn the_data_file_is_well_formed() {
        let mut ids = HashSet::new();
        for entry in entries() {
            assert!(ids.insert(&entry.id), "{} is there twice", entry.id);
            assert!(!entry.version.is_empty(), "{} has no version", entry.id);
            for (platform, files) in &entry.platforms {
                let mut names = HashSet::new();
                for file in files {
                    let what = format!("{} {platform:?} {}", entry.id, file.name);
                    assert!(names.insert(&file.name), "{what} is there twice");
                    assert!(file.url.starts_with("https://"), "{what}: not https");
                    let hex = |c: char| c.is_ascii_digit() || ('a'..='f').contains(&c);
                    assert!(
                        file.sha256.len() == 64 && file.sha256.chars().all(hex),
                        "{what}: no digest; run `cargo xtask pin-backends`"
                    );
                }
            }
        }
    }

    #[test]
    fn a_platform_is_the_os_and_architecture_or_the_web() {
        let caps = |runs, os: &str, arch: &str| Capabilities {
            runs,
            os: os.to_owned(),
            arch: arch.to_owned(),
            accelerators: Vec::new(),
            memory_mb: None,
            cores: None,
        };
        let of = |runs, os, arch| Platform::of(&caps(runs, os, arch));
        assert_eq!(
            of(Runs::Native, "macos", "aarch64"),
            Some(Platform::MacosAarch64)
        );
        assert_eq!(
            of(Runs::Native, "windows", "x86_64"),
            Some(Platform::WindowsX86_64)
        );
        assert_eq!(of(Runs::Page, "linux", "x86_64"), Some(Platform::Web));
        assert_eq!(of(Runs::Native, "freebsd", "x86_64"), None);
    }
}
