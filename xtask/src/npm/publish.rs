//! `cargo xtask npm-publish TAG`: the Release's tarball, verified, **staged** on npm by trusted publishing with
//! provenance. Nothing reaches npm without the operator's approval: a staged version waits until it is approved on
//! npmjs.com (`@sidevoice/engine` → staged versions) or with `npm stage approve <id>`, both with 2FA. As
//! sidevoice-connector stages its launcher (its `xtask/src/npm.rs`), with the same pinned npm.
//!
//! Every npm step runs the npm pinned here ([`NPM_VERSION`], installed into `target/npm-cli`; staged publishing needs
//! `npm stage`), with a configuration of its own: no `.npmrc`, no token or npm setting from the environment.
//!
//! A re-run carries on. A version already **published** with these bytes is skipped (compared with what `npm pack`
//! fetches from the registry); with other bytes it fails, since npm versions are immutable. A version already
//! **staged** with these bytes (its `shasum`) is skipped too, and its stage id printed again; one staged with other
//! bytes fails, to be rejected first. Whether a version is staged is asked of the registry with the token this run's
//! trusted publishing exchanges for; where the registry does not answer that token, staging goes ahead, and if npm
//! refuses because the version is staged already, the error says to approve or reject it.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::{env, fs};

use serde_json::{json, Value};
use sha1::{Digest, Sha1};

use crate::release::{download_verified, tarball_name};
use crate::{empty_dir, read, repo, run_in, write, Result};

use super::{parse, PACKAGE};

/// The npm CLI every npm step runs: trusted publishing needs 11.5.1 or later, staged publishing `npm stage`. The same
/// as sidevoice-connector's; bumped by hand, like any pin.
const NPM_VERSION: &str = "11.21.0";
const NPM_DIR: &str = "target/npm-cli";
/// The oldest Node.js npm's trusted publishing supports.
const PUBLISH_NODE: [u64; 3] = [22, 14, 0];
const REPOSITORY: &str = "sidevoice/sidevoice-engine";
const REGISTRY: &str = "https://registry.npmjs.org";

/// `cargo xtask npm-publish TAG`.
pub(crate) fn publish(tag: &str) -> Result<()> {
    let version = tag.strip_prefix('v').unwrap_or_default();
    if !version.starts_with(char::is_numeric) {
        return Err(format!("{tag}: not a vX.Y.Z; the nightly is never on npm"));
    }
    let node = node_version()?;
    if node < PUBLISH_NODE {
        return Err(format!(
            "trusted publishing needs Node.js {}.{}.{} or later; this is {}.{}.{}",
            PUBLISH_NODE[0], PUBLISH_NODE[1], PUBLISH_NODE[2], node[0], node[1], node[2]
        ));
    }
    ensure_npm()?;

    // Exactly the bytes the GitHub Release holds, checked against its SHA256SUMS and attestation.
    let dir = env::temp_dir().join("sidevoice-engine-npm-publish");
    empty_dir(&dir)?;
    let name = tarball_name(version);
    if !download_verified(tag, &dir)?.contains(&name) {
        return Err(format!("{name}: not in the Release's SHA256SUMS"));
    }
    let tarball = read(&dir.join(&name))?;
    let shasum = sha1_hex(&tarball);
    let spec = format!("{PACKAGE}@{version}");
    let dist_tag = dist_tag(version);

    // Published already: with these bytes it is done; with others nothing can fix it but a new version.
    let registry = dir.join("registry");
    empty_dir(&registry)?;
    match npm(&["pack", &spec], Some(&registry)) {
        Err(error) if error.contains("E404") || error.contains("ETARGET") => {}
        Err(error) => return Err(error),
        Ok(_) if read(&registry.join(&name))? == tarball => {
            return report(&format!(
                "{spec} is already published with these bytes; nothing to stage or approve"
            ));
        }
        Ok(_) => {
            return Err(format!(
                "{spec} is already published with other bytes; npm versions are immutable: release a new version"
            ))
        }
    }

    // Staged already: with these bytes, it only waits for the operator's approval.
    let token = trusted_publishing_token()?;
    match staged(&token, version)? {
        Some(item) if item.shasum == shasum => {
            return report(&awaiting(&spec, &item.id, dist_tag, "is already STAGED"));
        }
        Some(item) => {
            return Err(format!(
                "{spec} is already staged with other bytes (stage id {}, shasum {}; this build {shasum}): reject it \
                 on npmjs.com or with `npm stage reject {}`, then re-run",
                item.id, item.shasum, item.id
            ))
        }
        None => {}
    }

    let path = dir.join(&name);
    let path = path.to_str().ok_or("the tarball's path is not UTF-8")?;
    let args = [
        "stage",
        "publish",
        path,
        "--access",
        "public",
        "--provenance",
        "--tag",
        dist_tag,
        "--json",
    ];
    let staged_report = npm(&args, Some(&dir)).map_err(|error| publish_failed(&spec, &error))?;
    let id = stage_id(&staged_report)
        .unwrap_or_else(|| "(see `npm stage list @sidevoice/engine`)".into());
    report(&awaiting(&spec, &id, dist_tag, "is STAGED"))
}

/// What the job says once a version waits for approval.
fn awaiting(spec: &str, id: &str, dist_tag: &str, state: &str) -> String {
    format!(
        "{spec} {state} (stage id {id}, dist-tag {dist_tag}), awaiting the operator's approval: npmjs.com → \
         {PACKAGE} → staged versions, or `npm stage approve {id}` (2FA). Until it is approved, nothing new is on npm."
    )
}

/// Prints `line` as the job's last word, and adds it to the job's summary when there is one.
fn report(line: &str) -> Result<()> {
    println!("{line}");
    if let Some(summary) = env::var_os("GITHUB_STEP_SUMMARY") {
        let mut text =
            String::from_utf8_lossy(&read(Path::new(&summary)).unwrap_or_default()).into_owned();
        text.push_str(&format!("## npm\n\n{line}\n"));
        write(Path::new(&summary), text.as_bytes())?;
    }
    Ok(())
}

fn publish_failed(spec: &str, error: &str) -> String {
    format!(
        "{spec}: npm refused it. Publishing uses trusted publishing only (repository {REPOSITORY}, workflow {}), \
         with staged publishing allowed for {PACKAGE}; if the version is already staged, approve or reject it on \
         npmjs.com. {error}",
        calling_workflow()
    )
}

/// The dist-tag of a version: `next` for one with a `-` suffix (a release candidate), else `latest`.
pub(super) fn dist_tag(version: &str) -> &'static str {
    if version.contains('-') {
        "next"
    } else {
        "latest"
    }
}

/// npm's `shasum` of a tarball: its SHA-1, in lowercase hex.
fn sha1_hex(bytes: &[u8]) -> String {
    Sha1::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// A staged version, as the registry lists it.
#[derive(Debug, PartialEq, Eq)]
struct Staged {
    id: String,
    shasum: String,
}

/// The version `version` of the package among the registry's staged versions, if it is there; `None` too when the
/// registry does not answer `token` (then staging goes ahead and npm says whether it is staged already).
fn staged(token: &str, version: &str) -> Result<Option<Staged>> {
    let url = format!(
        "{REGISTRY}/-/stage?package={}&page=0&perPage=100",
        PACKAGE.replace('/', "%2f")
    );
    let (status, body) = curl_secret(&url, "GET", token)?;
    if status != 200 {
        println!("the registry does not list staged versions to this run (HTTP {status}); staging");
        return Ok(None);
    }
    Ok(staged_version(
        &parse(&body, "the staged versions")?,
        version,
    ))
}

/// The pending staged `version` in the registry's list of staged versions.
fn staged_version(list: &Value, version: &str) -> Option<Staged> {
    let items = list["items"].as_array()?;
    items
        .iter()
        .filter(|item| item["packageName"] == PACKAGE && item["version"] == version)
        .filter(|item| {
            let status = item["status"].as_str().unwrap_or("pending");
            !matches!(status, "rejected" | "approved" | "published" | "expired")
        })
        .find_map(|item| {
            Some(Staged {
                id: item["id"].as_str()?.to_owned(),
                shasum: item["shasum"].as_str().unwrap_or_default().to_owned(),
            })
        })
}

/// The stage id in `npm stage publish --json`'s report.
fn stage_id(report: &str) -> Option<String> {
    fn find(value: &Value) -> Option<String> {
        match value {
            Value::Object(map) => map
                .get("stageId")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| map.values().find_map(find)),
            Value::Array(items) => items.iter().find_map(find),
            _ => None,
        }
    }
    find(&serde_json::from_str(report).ok()?)
}

/// The job's OIDC token exchanged for an npm token for the package, as `npm stage publish` exchanges it. It fails here,
/// naming what to configure, when trusted publishing does not accept this run, and nothing is staged.
fn trusted_publishing_token() -> Result<String> {
    let (Ok(request_url), Ok(request_token)) = (
        env::var("ACTIONS_ID_TOKEN_REQUEST_URL"),
        env::var("ACTIONS_ID_TOKEN_REQUEST_TOKEN"),
    ) else {
        return Err("no OIDC token: the engine is staged on npm only by trusted publishing, from GitHub Actions with \
                    `permissions: id-token: write` (on the calling workflow too); never with a token"
            .into());
    };
    let audience = format!("npm:{}", REGISTRY.trim_start_matches("https://"));
    let (status, body) = curl_secret(
        &format!("{request_url}&audience={audience}"),
        "GET",
        &request_token,
    )?;
    let id_token = parse(&body, "the OIDC token response")?["value"]
        .as_str()
        .filter(|_| status == 200)
        .map(str::to_string)
        .ok_or_else(|| format!("GitHub gave no OIDC token (HTTP {status})"))?;
    let url = format!(
        "{REGISTRY}/-/npm/v1/oidc/token/exchange/package/{}",
        PACKAGE.replace('/', "%2f")
    );
    let (status, body) = curl_secret(&url, "POST", &id_token)?;
    let answer = serde_json::from_slice::<Value>(&body).unwrap_or(Value::Null);
    match answer["token"].as_str().filter(|_| (200..300).contains(&status)) {
        Some(token) => {
            println!(
                "trusted publishing: {PACKAGE} accepts {REPOSITORY} {}",
                calling_workflow()
            );
            Ok(token.to_owned())
        }
        None => Err(format!(
            "trusted publishing is not configured for {PACKAGE} (HTTP {status}: {}). On npmjs.com, {PACKAGE} → \
             Settings → Trusted publisher: GitHub Actions, repository {REPOSITORY}, workflow filename {} (the \
             workflow that started this run; npm checks it, not a workflow it calls), no environment, staged \
             publishing allowed. Nothing was staged.",
            answer["message"].as_str().unwrap_or("no message"),
            calling_workflow()
        )),
    }
}

/// The workflow npm's trusted publishing sees: the top-level one (`release-please.yml` when it calls `release.yml`),
/// from `GITHUB_WORKFLOW_REF`.
fn calling_workflow() -> String {
    env::var("GITHUB_WORKFLOW_REF")
        .ok()
        .and_then(|reference| {
            let path = reference.split('@').next()?.to_string();
            path.rsplit('/').next().map(str::to_string)
        })
        .unwrap_or_else(|| "(unknown)".into())
}

/// A request whose secret header goes to curl on its standard input, never on its command line. Returns the HTTP
/// status and the body.
fn curl_secret(url: &str, method: &str, bearer: &str) -> Result<(u16, Vec<u8>)> {
    let mut child = Command::new("curl")
        .args(["--silent", "--show-error", "--retry", "3", "--config", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("curl: {error}"))?;
    let config = format!(
        "url = \"{url}\"\nrequest = \"{method}\"\nheader = \"Accept: application/json\"\n\
         header = \"Authorization: Bearer {bearer}\"\nwrite-out = \"\\n%{{http_code}}\"\n"
    );
    child
        .stdin
        .take()
        .ok_or("curl: no standard input")?
        .write_all(config.as_bytes())
        .map_err(|error| format!("curl: {error}"))?;
    let result = child
        .wait_with_output()
        .map_err(|error| format!("curl: {error}"))?;
    if !result.status.success() {
        return Err(format!(
            "{method} {url}: {}",
            String::from_utf8_lossy(&result.stderr)
        ));
    }
    let text = result.stdout;
    let split = text.iter().rposition(|byte| *byte == b'\n').unwrap_or(0);
    let status = String::from_utf8_lossy(&text[split..])
        .trim()
        .parse()
        .map_err(|_| format!("{method} {url}: no HTTP status"))?;
    Ok((status, text[..split].to_vec()))
}

fn node_version() -> Result<[u64; 3]> {
    let text = run_in(&repo(), "node --version", &[])?;
    let parts: Vec<u64> = text
        .trim()
        .trim_start_matches('v')
        .split('.')
        .map(|part| part.parse().unwrap_or(0))
        .collect();
    match parts.as_slice() {
        [major, minor, patch] => Ok([*major, *minor, *patch]),
        _ => Err(format!("node --version: {text:?}")),
    }
}

/// The pinned npm, run with a configuration of its own (empty user and global config, its own cache) and none of the
/// environment's npm settings or tokens; its standard output.
fn npm(args: &[&str], dir: Option<&Path>) -> Result<String> {
    let cli = repo().join(NPM_DIR).join("node_modules/npm/bin/npm-cli.js");
    let mut command = Command::new("node");
    command.arg(&cli).args(args);
    for (key, _) in env::vars_os() {
        let key = key.to_string_lossy();
        if key.to_ascii_lowercase().starts_with("npm_config_")
            || key == "NODE_AUTH_TOKEN"
            || key == "NPM_TOKEN"
        {
            command.env_remove(key.as_ref());
        }
    }
    let config = repo().join(NPM_DIR);
    command
        .env("npm_config_userconfig", config.join("user-npmrc"))
        .env("npm_config_globalconfig", config.join("global-npmrc"))
        .env("npm_config_cache", repo().join("target/npm-cache"))
        .env("npm_config_registry", REGISTRY)
        .env("npm_config_update_notifier", "false")
        .env("npm_config_fund", "false")
        .env("npm_config_audit", "false");
    if let Some(dir) = dir {
        command.current_dir(dir);
    }
    let result = command.output().map_err(|error| format!("node: {error}"))?;
    if !result.status.success() {
        return Err(format!(
            "npm {}: {}{}",
            args.join(" "),
            String::from_utf8_lossy(&result.stderr),
            String::from_utf8_lossy(&result.stdout)
        ));
    }
    String::from_utf8(result.stdout).map_err(|_| "npm: output is not UTF-8".into())
}

/// Installs the pinned npm into `target/npm-cli` unless it is there.
fn ensure_npm() -> Result<()> {
    let dir = repo().join(NPM_DIR);
    let installed = fs::read(dir.join("node_modules/npm/package.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
    if installed.as_ref().map(|manifest| &manifest["version"]) != Some(&json!(NPM_VERSION)) {
        fs::create_dir_all(&dir).map_err(|error| format!("{}: {error}", dir.display()))?;
        let prefix = dir.to_str().ok_or("target/npm-cli: not UTF-8")?;
        let cache = repo().join("target/npm-cache");
        let cache = cache.to_str().ok_or("target/npm-cache: not UTF-8")?;
        let package = format!("npm@{NPM_VERSION}");
        run_in(
            &repo(),
            "npm install --no-audit --no-fund --no-save --prefix",
            &[prefix, "--cache", cache, &package],
        )?;
    }
    for config in ["user-npmrc", "global-npmrc"] {
        write(&dir.join(config), b"")?;
    }
    let version = npm(&["--version"], None)?;
    if version.trim() != NPM_VERSION {
        return Err(format!(
            "the pinned npm reports {version:?}, not {NPM_VERSION}"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
