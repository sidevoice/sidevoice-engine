//! `backends.json`'s digests, which nobody types. After a backend's `version` or a file changes, `pin-backends`
//! downloads every file of every backend at its pinned version (`curl`) and writes its `sha256`; `pin-backends --check`
//! downloads them too and fails if any digest is not the file's.

use std::env;

use serde_json::Value;

use crate::{empty_dir, read, repo, run_in, sha256, write, Result};

/// `cargo xtask pin-backends [--check]`.
pub(crate) fn pin(check: bool) -> Result<()> {
    let path = repo().join("backends.json");
    let text = String::from_utf8(read(&path)?).map_err(|_| "backends.json is not UTF-8")?;
    let mut doc: Value =
        serde_json::from_str(&text).map_err(|error| format!("backends.json: {error}"))?;
    let dir = env::temp_dir().join("xtask-pin-backends");
    let stale = repin(&mut doc, |url| {
        empty_dir(&dir)?;
        run_in(&dir, "curl -fsSL --retry 3 -o download", &[url])?;
        Ok(sha256(&read(&dir.join("download"))?))
    })?;
    let pinned = format!("{doc:#}\n");
    for file in &stale {
        println!("{} {file}", if check { "stale:" } else { "pinned:" });
    }
    if check {
        if !stale.is_empty() {
            return Err("backends.json has stale digests: run `cargo xtask pin-backends`".into());
        }
        if pinned != text {
            return Err("backends.json is not formatted: run `cargo xtask pin-backends`".into());
        }
    } else {
        write(&path, pinned.as_bytes())?;
    }
    Ok(())
}

/// Writes into `doc` the digest of every file, `digest` given its url with the backend's version in it, and returns the
/// files whose digest changed, as `backend platform name`.
fn repin(doc: &mut Value, mut digest: impl FnMut(&str) -> Result<String>) -> Result<Vec<String>> {
    let mut stale = Vec::new();
    let backends = doc["backends"].as_array_mut().ok_or("no backends")?;
    for backend in backends {
        let id = backend["id"]
            .as_str()
            .ok_or("a backend with no id")?
            .to_owned();
        let version = backend["version"].as_str();
        let version = version.ok_or(format!("{id}: no version"))?.to_owned();
        let platforms = backend["platforms"].as_object_mut();
        for (platform, files) in platforms.ok_or(format!("{id}: no platforms"))? {
            // `null`: the backend does not run there, so there is nothing to pin. Which keys must be there is the engine's
            // to check (src/backend/downloads.rs).
            if files.is_null() {
                continue;
            }
            for file in files
                .as_array_mut()
                .ok_or(format!("{id} {platform}: not a list"))?
            {
                let name = file["name"].as_str().unwrap_or_default();
                let what = format!("{id} {platform} {name}");
                let url = file["url"].as_str().ok_or(format!("{what}: no url"))?;
                let sha = digest(&url.replace("{version}", &version))?;
                if file["sha256"] != sha.as_str() {
                    stale.push(what);
                    file["sha256"] = sha.into();
                }
            }
        }
    }
    Ok(stale)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::repin;

    #[test]
    fn every_file_is_pinned_at_its_backends_version_and_the_changes_are_listed() {
        let mut doc = json!({"backends": [{
            "id": "lib",
            "version": "1.2.3",
            "platforms": {
                "linux-x86_64": [
                    {"name": "a", "url": "https://x/v{version}/a", "sha256": ""},
                    {"name": "b", "url": "https://x/b", "sha256": "digest of https://x/b"}
                ],
                "web": [],
                "windows-x86_64": null
            }
        }]});
        let stale = repin(&mut doc, |url| Ok(format!("digest of {url}"))).unwrap();
        assert_eq!(stale, ["lib linux-x86_64 a"]);
        let files = &doc["backends"][0]["platforms"]["linux-x86_64"];
        assert_eq!(files[0]["sha256"], "digest of https://x/v1.2.3/a");
        assert!(doc["backends"][0]["platforms"]["windows-x86_64"].is_null());
        // Key order is kept: the file is rewritten as it was, digests aside.
        let keys: Vec<_> = doc["backends"][0].as_object().unwrap().keys().collect();
        assert_eq!(keys, ["id", "version", "platforms"]);
    }
}
