use std::collections::HashMap;

use serde_json::json;

use super::{estimate_mb, percent_decoded, repin, Entry, Hub};
use crate::Result;

/// A hub with one repository: `main` is commit `c…c`, `model.onnx` is in LFS and `config.json` is not.
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
            lfs_sha256: lfs.map(str::to_owned),
        })
    }

    fn download_sha256(&mut self, url: &str) -> Result<String> {
        self.downloads.push(url.to_owned());
        Ok(format!("digest of {url}"))
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
fn only_hugging_face_resolve_urls_can_be_pinned() {
    let mut doc = family(json!(null));
    doc["models"][0]["builds"][0]["files"][0]["url"] =
        "https://github.com/org/repo/releases/x.tar.bz2".into();
    let error = repin(&mut doc, &mut FakeHub::default()).unwrap_err();
    assert_eq!(error, "m/b model: not a Hugging Face resolve URL");
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
