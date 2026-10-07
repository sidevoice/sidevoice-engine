//! Helpers shared by every command: files, JSON, processes, temporary directories.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;
use sha2::{Digest, Sha256, Sha512};

use crate::{Result, WEB_CRATE};

pub(crate) fn repo() -> PathBuf {
    // The tooling is xtask/, a package of its own beside the engine's.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask/ has a parent")
        .to_path_buf()
}

pub(crate) fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// npm's `dist.integrity` of a tarball: `sha512-` and the base64 of its SHA-512.
pub(crate) fn npm_integrity(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let digest = Sha512::digest(bytes);
    let mut text = String::from("sha512-");
    for chunk in digest.chunks(3) {
        let bits = chunk.iter().enumerate().fold(0u32, |bits, (index, byte)| {
            bits | (u32::from(*byte) << (16 - 8 * index))
        });
        for index in 0..4 {
            if index <= chunk.len() {
                text.push(char::from(
                    ALPHABET[((bits >> (18 - 6 * index)) & 63) as usize],
                ));
            } else {
                text.push('=');
            }
        }
    }
    text
}

pub(crate) fn read(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).map_err(|error| format!("{}: {error}", path.display()))
}

pub(crate) fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    fs::write(path, bytes).map_err(|error| format!("{}: {error}", path.display()))
}

pub(crate) fn mkdir(path: &Path) -> Result<()> {
    fs::create_dir_all(path).map_err(|error| format!("{}: {error}", path.display()))
}

/// Removes `path` if it is there and creates it empty.
pub(crate) fn empty_dir(path: &Path) -> Result<()> {
    match fs::remove_dir_all(path) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
            Err(format!("{}: {error}", path.display()))
        }
        _ => mkdir(path),
    }
}

pub(crate) fn path_str(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| format!("{}: not UTF-8", path.display()))
}

pub(crate) fn parse_json(bytes: &[u8], what: &str) -> Result<Value> {
    serde_json::from_slice(bytes).map_err(|error| format!("{what}: {error}"))
}

/// Pretty JSON with a final newline, as `package.json` is written.
pub(crate) fn pretty(value: &Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec_pretty(value).expect("JSON values serialize");
    bytes.push(b'\n');
    bytes
}

/// Runs a program to completion and returns its standard output; a failure is an error with its standard error.
pub(crate) fn output(program: &str, args: &[&str], dir: Option<&Path>) -> Result<String> {
    let mut command = Command::new(program);
    command.args(args);
    if let Some(dir) = dir {
        command.current_dir(dir);
    }
    run(command, &format!("{program} {}", args.join(" ")))
}

/// Runs `command` to completion and returns its standard output; a failure is an error naming `what`.
pub(crate) fn run(mut command: Command, what: &str) -> Result<String> {
    let result = command
        .output()
        .map_err(|error| format!("{what}: {error}"))?;
    if !result.status.success() {
        return Err(format!(
            "{what}: {}{}",
            String::from_utf8_lossy(&result.stderr),
            String::from_utf8_lossy(&result.stdout)
        ));
    }
    String::from_utf8(result.stdout).map_err(|_| format!("{what}: output is not UTF-8"))
}

pub(crate) fn download(url: &str) -> Result<Vec<u8>> {
    let result = Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--retry",
            "3",
            url,
        ])
        .output()
        .map_err(|error| format!("curl: {error}"))?;
    if !result.status.success() {
        return Err(format!(
            "download {url}: {}",
            String::from_utf8_lossy(&result.stderr)
        ));
    }
    Ok(result.stdout)
}

pub(crate) fn cargo_program() -> String {
    env::var("CARGO").unwrap_or_else(|_| "cargo".into())
}

pub(crate) fn git(args: &[&str]) -> Result<String> {
    Ok(output("git", args, Some(&repo()))?.trim().to_string())
}

/// `cargo metadata --no-deps` of the workspace.
pub(crate) fn metadata() -> Result<Value> {
    parse_json(
        output(
            &cargo_program(),
            &["metadata", "--locked", "--no-deps", "--format-version", "1"],
            Some(&repo()),
        )?
        .as_bytes(),
        "cargo metadata",
    )
}

/// The released crate's version, from its Cargo.toml.
pub(crate) fn engine_version() -> Result<String> {
    metadata()?["packages"]
        .as_array()
        .and_then(|packages| packages.iter().find(|package| package["name"] == WEB_CRATE))
        .and_then(|package| package["version"].as_str())
        .map(str::to_string)
        .ok_or_else(|| format!("no {WEB_CRATE} in the workspace"))
}

/// Cargo's target directory (`CARGO_TARGET_DIR` included).
pub(crate) fn target_dir() -> Result<PathBuf> {
    metadata()?["target_directory"]
        .as_str()
        .map(PathBuf::from)
        .ok_or_else(|| "cargo metadata names no target directory".into())
}

/// Every file below `root`, as paths relative to it with `/` separators, sorted.
pub(crate) fn files_below(root: &Path) -> Result<Vec<String>> {
    fn visit(root: &Path, dir: &Path, found: &mut Vec<String>) -> Result<()> {
        for entry in fs::read_dir(dir).map_err(|error| format!("{}: {error}", dir.display()))? {
            let path = entry.map_err(|error| error.to_string())?.path();
            if path.is_dir() {
                visit(root, &path, found)?;
            } else {
                let relative = path.strip_prefix(root).expect("below root");
                found.push(path_str(relative)?.to_string());
            }
        }
        Ok(())
    }
    let mut found = Vec::new();
    visit(root, root, &mut found)?;
    found.sort();
    Ok(found)
}

/// Lines of a `SHA256SUMS` file as (digest, name).
pub(crate) fn parse_sums(bytes: &[u8]) -> Result<Vec<(String, String)>> {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(|line| {
            line.split_once("  ")
                .map(|(digest, name)| (digest.to_string(), name.to_string()))
                .ok_or_else(|| format!("malformed SHA256SUMS line: {line}"))
        })
        .collect()
}

pub(crate) struct TempDir(pub(crate) PathBuf);

impl TempDir {
    pub(crate) fn new(label: &str) -> Result<Self> {
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        let serial = SERIAL.fetch_add(1, Ordering::Relaxed);
        let path = env::temp_dir().join(format!("{label} {}-{serial}", std::process::id()));
        empty_dir(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sums_are_two_space_separated() {
        let sums = parse_sums(b"abc  one.tgz\ndef  two.json\n").unwrap();
        assert_eq!(
            sums,
            vec![
                ("abc".into(), "one.tgz".into()),
                ("def".into(), "two.json".into())
            ]
        );
        assert!(parse_sums(b"abc one\n").is_err());
    }

    #[test]
    fn the_integrity_is_npm_s() {
        // `printf abc | openssl dgst -sha512 -binary | base64`
        assert_eq!(
            npm_integrity(b"abc"),
            "sha512-3a81oZNherrMQXNJriBBMRLm+k6JqX6iCp7u5ktV05ohkpkqJ0/BqDa6PCOj/uu9RU1EI2Q86A4qmslPpUyknw=="
        );
        assert!(npm_integrity(b"").ends_with("=="));
    }

    #[test]
    fn files_below_lists_every_file_relative_and_sorted() {
        let dir = TempDir::new("xtask-files-below").unwrap();
        mkdir(&dir.0.join("dist")).unwrap();
        for file in ["package.json", "dist/b.js", "dist/a.wasm"] {
            write(&dir.0.join(file), b"").unwrap();
        }
        assert_eq!(
            files_below(&dir.0).unwrap(),
            ["dist/a.wasm", "dist/b.js", "package.json"]
        );
    }
}
