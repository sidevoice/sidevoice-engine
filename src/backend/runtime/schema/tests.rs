use std::collections::HashSet;

use super::{entries, Platforms};
use crate::host::Platform;

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
}
