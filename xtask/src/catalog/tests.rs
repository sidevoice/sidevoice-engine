use std::collections::HashMap;

use serde_json::json;

use super::{estimate_mb, percent_decoded, repin, whisper_languages, Entry, Hub};
use crate::Result;

/// A Hugging Face repository whose `main` is commit `c…c`, where `model.onnx` is in LFS and `config.json` is not;
/// and a GitHub release `models` whose `model.tar.bz2` has a digest and `old.tar.bz2` none.
#[derive(Default)]
struct FakeHub {
    downloads: Vec<String>,
}

const COMMIT: &str = "cccccccccccccccccccccccccccccccccccccccc";

impl Hub for FakeHub {
    fn commit(&mut self, repo: &str, revision: &str) -> Result<String> {
        assert_eq!((repo, revision), ("org/repo", "main"));
        Ok(COMMIT.to_owned())
    }

    fn entry(&mut self, repo: &str, commit: &str, path: &str) -> Result<Entry> {
        assert_eq!((repo, commit), ("org/repo", COMMIT));
        let files = HashMap::from([
            ("onnx/model.onnx", (100 * 1024 * 1024, Some("lfs digest"))),
            ("config.json", (10, None)),
        ]);
        let (bytes, lfs) = files.get(path).ok_or(format!("no file {path}"))?;
        Ok(Entry {
            bytes: *bytes,
            sha256: lfs.map(str::to_owned),
        })
    }

    fn release_asset(&mut self, repo: &str, tag: &str, name: &str) -> Result<Entry> {
        assert_eq!((repo, tag), ("org/repo", "models"));
        let sha256 = match name {
            "model.tar.bz2" => Some("asset digest".to_owned()),
            "old.tar.bz2" => None,
            _ => return Err(format!("no asset {name}")),
        };
        Ok(Entry {
            bytes: 100 * 1024 * 1024,
            sha256,
        })
    }

    fn download_sha256(&mut self, url: &str) -> Result<String> {
        self.downloads.push(url.to_owned());
        Ok(format!("digest of {url}"))
    }

    fn download_text(&mut self, url: &str) -> Result<String> {
        self.downloads.push(url.to_owned());
        Ok(TOKENIZER.to_owned())
    }
}

fn family(memory: serde_json::Value) -> serde_json::Value {
    json!({"id": "f", "models": [{"id": "m", "builds": [{
        "id": "m/b",
        "memory": memory,
        "files": [
            {"key": "model", "url": "https://huggingface.co/org/repo/resolve/main/onnx/model.onnx"},
            {
                "key": "config",
                "url": "https://huggingface.co/org/repo/resolve/main/config.json",
                "sha256": "old",
                "bytes": 1
            }
        ]
    }]}]})
}

#[test]
fn files_are_pinned_to_the_commit_with_their_size_and_digest_and_memory_is_estimated() {
    let mut doc = family(json!(null));
    let mut hub = FakeHub::default();
    let stale = repin(&mut doc, &mut hub).unwrap();
    assert_eq!(stale, ["m/b model", "m/b config", "m/b memory"]);
    let build = &doc["models"][0]["builds"][0];
    let pinned = format!("https://huggingface.co/org/repo/resolve/{COMMIT}");
    assert_eq!(
        build["files"][0],
        json!({"key": "model", "url": format!("{pinned}/onnx/model.onnx"), "sha256": "lfs digest", "bytes": 104857600})
    );
    // Not in LFS: downloaded from the pinned URL and hashed.
    let config = format!("{pinned}/config.json");
    assert_eq!(hub.downloads, std::slice::from_ref(&config));
    assert_eq!(build["files"][1]["sha256"], format!("digest of {config}"));
    // 100 MiB of weights (the config is not one), plus 30%.
    assert_eq!(
        build["memory"],
        json!({"mb": 130, "source": "estimated", "basis": "weights size + 30%"})
    );
    // Pinned once, nothing is stale, and key order is kept.
    assert_eq!(
        repin(&mut doc, &mut FakeHub::default()).unwrap(),
        Vec::<String>::new()
    );
    let keys: Vec<_> = doc["models"][0]["builds"][0]["files"][1]
        .as_object()
        .unwrap()
        .keys()
        .collect();
    assert_eq!(keys, ["key", "url", "sha256", "bytes"]);
}

#[test]
fn a_declared_or_measured_memory_is_left_as_it_is() {
    for source in ["declared", "measured"] {
        let memory = json!({"mb": 1, "source": source, "basis": "a test"});
        let mut doc = family(memory.clone());
        repin(&mut doc, &mut FakeHub::default()).unwrap();
        assert_eq!(doc["models"][0]["builds"][0]["memory"], memory);
    }
}

#[test]
fn only_hugging_face_files_and_github_release_assets_can_be_pinned() {
    let mut doc = family(json!(null));
    doc["models"][0]["builds"][0]["files"][0]["url"] =
        "https://github.com/org/repo/releases/x.tar.bz2".into();
    let error = repin(&mut doc, &mut FakeHub::default()).unwrap_err();
    assert_eq!(
        error,
        "m/b model: not a Hugging Face resolve URL or a GitHub release asset"
    );
}

#[test]
fn the_estimate_rounds_up_to_ten_mb() {
    assert_eq!(estimate_mb(0), 0);
    assert_eq!(estimate_mb(1), 10);
    // whisper-small's whisper.cpp q5_1 weights: 181.3 MiB, plus 30% is 235.7.
    assert_eq!(estimate_mb(190_085_487), 240);
}

#[test]
fn a_url_path_is_looked_up_decoded() {
    assert_eq!(
        percent_decoded("voices/%21v/Mr%20serious").unwrap(),
        "voices/!v/Mr serious"
    );
    assert!(percent_decoded("bad%2").is_err());
    assert!(percent_decoded("bad%zz").is_err());
}

#[test]
fn a_release_asset_is_pinned_by_its_digest_and_marked_mutable_and_its_archive_counts_once() {
    let asset = "https://github.com/org/repo/releases/download/models/model.tar.bz2";
    let old = "https://github.com/org/repo/releases/download/models/old.tar.bz2";
    let mut doc = json!({"id": "f", "models": [{"id": "m", "builds": [{
        "id": "m/b",
        "files": [
            {"key": "model", "url": asset, "archive_path": "m/model.onnx"},
            {"key": "voices", "url": asset, "archive_path": "m/voices.bin"},
            {"key": "data", "url": asset, "archive_path": "m/data", "mutable": false},
            {"key": "old", "url": old}
        ]
    }]}]});
    let mut hub = FakeHub::default();
    repin(&mut doc, &mut hub).unwrap();
    let files = &doc["models"][0]["builds"][0]["files"];
    assert_eq!(
        files[0],
        json!({
            "key": "model",
            "url": asset,
            "archive_path": "m/model.onnx",
            "sha256": "asset digest",
            "bytes": 104857600,
            "mutable": true
        })
    );
    assert_eq!(files[2]["mutable"], true);
    // An asset the API has no digest for is downloaded and hashed.
    assert_eq!(hub.downloads, [old]);
    assert_eq!(files[3]["sha256"], format!("digest of {old}"));
    // Two weights in one 100 MiB archive: 100 MiB, plus 30%.
    assert_eq!(doc["models"][0]["builds"][0]["memory"]["mb"], 130);
}

/// The shape of openai/whisper's `whisper/tokenizer.py` around its table, shortened.
const TOKENIZER: &str = r#"
LANGUAGES = {
    "en": "english",
    "es": "spanish",
    "jw": "javanese",
    "yue": "cantonese",
}

# language code lookup by name
TO_LANGUAGE_CODE = {
    **{language: code for code, language in LANGUAGES.items()},
}
"#;

#[test]
fn a_models_languages_are_written_from_its_source_with_whispers_javanese_as_bcp_47() {
    let source = "https://raw.githubusercontent.com/openai/whisper/abc/whisper/tokenizer.py";
    let mut doc = family(json!({"mb": 1, "source": "measured", "basis": "a test"}));
    doc["models"][0]["languages_source"] = source.into();
    doc["models"][0]["languages"] = json!(["multi"]);
    let mut hub = FakeHub::default();
    let stale = repin(&mut doc, &mut hub).unwrap();
    assert_eq!(stale[0], "m languages");
    assert_eq!(
        doc["models"][0]["languages"],
        json!(["en", "es", "jv", "yue"])
    );
    assert!(hub.downloads.contains(&source.to_owned()));
    assert!(whisper_languages("no table").is_err());

    doc["models"][0]["languages_source"] = "https://example.com/languages.txt".into();
    let error = repin(&mut doc, &mut FakeHub::default()).unwrap_err();
    assert!(error.contains("no reader"), "{error}");
}
