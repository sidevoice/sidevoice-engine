//! What each backend downloads, per platform: the data file `backends.json` at the repository root, compiled in. No
//! backend's code says what it fetches. The engine reads the entry for one backend and the platform it runs on, and
//! only that one.
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

/// The files `backend` downloads on `platform`, each with the name `load` finds it by, or `None` when it does not run
/// there (`null`, or no entry for `backend` at all).
pub(crate) fn downloads(backend: &str, platform: Platform) -> Option<Vec<(String, Artifact)>> {
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
mod tests {
    use std::collections::HashSet;

    use super::{downloads, entries, Platform, Platforms};
    use crate::host::{Capabilities, Runs};

    #[cfg(web)]
    use wasm_bindgen_test::wasm_bindgen_test as test;

    const ALL: [Platform; 6] = [
        Platform::MacosAarch64,
        Platform::MacosX86_64,
        Platform::LinuxX86_64,
        Platform::LinuxAarch64,
        Platform::WindowsX86_64,
        Platform::Web,
    ];

    #[test]
    fn the_data_file_is_well_formed() {
        let mut ids = HashSet::new();
        for entry in entries() {
            assert!(ids.insert(&entry.id), "{} is there twice", entry.id);
            assert!(!entry.version.is_empty(), "{} has no version", entry.id);
            let platforms = ALL.map(|platform| (platform, entry.platforms.get(platform)));
            for (platform, files) in platforms {
                let files = files.unwrap_or_default();
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

    /// A `platforms` object with every key set to `null` but `without`, and `extra` (if any) set to `[]`.
    fn platforms(without: &str, extra: &str) -> String {
        let keys = [
            "macos-aarch64",
            "macos-x86_64",
            "linux-x86_64",
            "linux-aarch64",
            "windows-x86_64",
            "web",
        ];
        let mut fields: Vec<_> = keys
            .iter()
            .filter(|key| **key != without)
            .map(|key| format!("\"{key}\": null"))
            .collect();
        if !extra.is_empty() {
            fields.push(format!("\"{extra}\": []"));
        }
        format!("{{{}}}", fields.join(", "))
    }

    #[test]
    fn every_platform_key_is_required_and_no_other_is_allowed() {
        let parse = |json: &str| serde_json::from_str::<Platforms>(json);
        assert!(parse(&platforms("", "")).is_ok());
        let missing = parse(&platforms("web", "")).expect_err("a missing platform");
        assert!(
            missing.to_string().contains("missing field `web`"),
            "{missing}"
        );
        let unknown = parse(&platforms("", "freebsd-x86_64")).expect_err("an unknown platform");
        assert!(unknown.to_string().contains("unknown field"), "{unknown}");
    }

    #[test]
    fn null_is_not_running_there_and_an_empty_list_is_running_with_nothing_to_download() {
        let parsed: Platforms = serde_json::from_str(&platforms("web", "web")).expect("platforms");
        assert!(parsed.get(Platform::LinuxX86_64).is_none());
        assert_eq!(parsed.get(Platform::Web).map(<[_]>::len), Some(0));
        // And in the data file: sherpa-onnx is native only, and the placeholders download nothing where they run.
        assert!(downloads("sherpa-onnx", Platform::Web).is_none());
        assert_eq!(downloads("mlx", Platform::MacosAarch64), Some(Vec::new()));
        assert!(downloads("mlx", Platform::LinuxX86_64).is_none());
    }
}
