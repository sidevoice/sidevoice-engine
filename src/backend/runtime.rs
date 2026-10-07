//! A backend's runtime: which library files it needs on each platform, from the data file `backends.json` at the
//! repository root, compiled in. No backend's code says what it fetches. The engine reads the entry for one backend and
//! the platform it runs on, and only that one; the installer fetches those files with the model's.
//!
//! Every backend lists every platform, always the same six keys: `macos-aarch64`, `macos-x86_64`, `linux-x86_64`,
//! `linux-aarch64`, `windows-x86_64` and `web`. A platform's value is the list of files to download there: `[]` when
//! the backend runs there and downloads nothing, and `null` when it does not run there (the funnel rejects it with
//! `no-runtime-for-platform`).
//!
//! Each backend has one `version`, from its `upstream`; each file's `url` may say `{version}`, and its `sha256` is
//! written by `cargo xtask pin-backends`, never by hand. Inside: `schema`, the file's shape and reading it.

use crate::host::Platform;
use crate::install::Artifact;

mod schema;
#[cfg(test)]
mod tests;

use schema::{entries, File};

/// The library files `backend` needs on `platform`, each keyed by its name (what `load` finds it by in `Installed`),
/// or `None` when it does not run there (`null`, or no entry for `backend` at all).
pub(crate) fn runtime_files(backend: &str, platform: Platform) -> Option<Vec<Artifact>> {
    let entry = entries().iter().find(|entry| entry.id == backend)?;
    let files = entry.platforms.get(platform)?;
    let artifact = |file: &File| Artifact {
        key: file.name.clone(),
        url: file.url.replace("{version}", &entry.version),
        sha256: file.sha256.clone(),
    };
    Some(files.iter().map(artifact).collect())
}
