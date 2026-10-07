//! The shape of `backends.json`, and reading it. Anything these types do not name is an error, and so is any of the six
//! platform keys missing: neither a typo nor an omission passes. The tests parse the file on every target.

use std::sync::OnceLock;

use serde::Deserialize;

use crate::host::Platform;

const DATA: &str = include_str!("../../../backends.json");

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Data {
    backends: Vec<Entry>,
}

/// One backend: what it is, where it comes from, its version and its files per platform.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Entry {
    /// Its `BackendSpec` id.
    pub(super) id: String,
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
    pub(super) version: String,
    pub(super) platforms: Platforms,
}

/// Every platform's files: `null` where the backend does not run, `[]` where it runs and downloads nothing. Every key
/// is required (`deserialize_with` stops serde from reading a missing one as `null`), and no other is allowed.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Platforms {
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
    pub(super) fn get(&self, platform: Platform) -> Option<&[File]> {
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
pub(super) struct File {
    /// Its `Artifact` key: what `load` finds it by in `Installed`.
    pub(super) name: String,
    /// Where from; `{version}` is the backend's version.
    pub(super) url: String,
    pub(super) sha256: String,
    /// For a member of an archive, its path inside it; `{version}` is the backend's version. Optional: absent, the
    /// file is the one `url` serves (the catalogue's `archive_path` works the same way).
    #[serde(default)]
    pub(super) archive_path: Option<String>,
}

/// Every backend's entry, parsed once.
pub(super) fn entries() -> &'static [Entry] {
    static PARSED: OnceLock<Data> = OnceLock::new();
    &PARSED
        .get_or_init(|| serde_json::from_str(DATA).expect("backends.json is checked by the tests"))
        .backends
}

#[cfg(test)]
mod tests;
