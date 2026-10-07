//! Installing a build: one installer for every backend, which does not know what a file is. It gets one list of
//! [`Artifact`]s, the build's model files (catalogue) and its backend's files for this platform (`backends.json`)
//! alike, downloads each through the host's `Fetcher`, checks it against its SHA-256 as it arrives, and stores it
//! through the host's `Storage`.
//!
//! Storage is content-addressed: each file is stored under its digest, and an artifact's `key` is only the name `load`
//! finds it by in [`Installed`]. So two backends, or two versions of one, never write the same name; a file shared by
//! two builds, or unchanged across a version, is downloaded once; and "already installed" is "its digest is stored",
//! because a file is only stored once its bytes hash to its name. Files stay on disk when their models leave memory;
//! removing them is a separate policy.
//!
//! Inside: `progress` (what the installer reports as it goes), `cancel` (how it is stopped) and `digest` (SHA-256).

use std::collections::BTreeMap;

use crate::host::Host;
use crate::{Error, Result};

mod cancel;
mod digest;
mod progress;
#[cfg(test)]
mod tests;

pub use cancel::Cancel;
use digest::Hasher;
pub use progress::{Progress, ProgressSink};

/// One file to download: where from, its digest, and the name `load` finds it by.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Artifact {
    /// The name the backend's `load` finds it by: its key in the catalogue, or its `name` in `backends.json`. Not its
    /// name in [`Storage`](crate::Storage), which is its digest.
    pub key: String,
    /// Where it is downloaded from.
    pub url: String,
    /// Its SHA-256 digest: 64 lowercase hex digits.
    pub sha256: String,
}

/// A build whose files are in storage: each artifact's key, and where the host keeps it (a path, an OPFS name, ...).
/// A backend file's key is its name in `backends.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Installed {
    pub(crate) files: BTreeMap<String, String>,
}

impl Installed {
    /// Where the host keeps the file whose key is `name`, if it is installed.
    #[allow(dead_code, reason = "the stub backends load nothing yet")]
    pub(crate) fn file(&self, name: &str) -> Option<&str> {
        self.files.get(name).map(String::as_str)
    }
}

/// Puts a build's files in storage.
#[derive(Debug)]
pub(crate) struct Installer;

impl Installer {
    /// Downloads, checks and stores whatever of `artifacts` is not stored yet, one file at a time, telling `progress`
    /// as it goes, and stops between two parts of a file once `cancel` is cancelled. Dropping the future stops it too;
    /// either way, a file only half downloaded is never stored.
    ///
    /// # Errors
    ///
    /// `digest-invalid` if an artifact's digest is not 64 lowercase hex digits, `artifact-key-conflict` if two
    /// artifacts share a key with different digests (both checked before anything is downloaded), `digest-mismatch` if
    /// a file's bytes do not match its digest, `cancelled`, and whatever the host's fetcher or storage fails with
    /// (`download-failed`, `storage-failed`).
    pub(crate) async fn install(
        &self,
        artifacts: &[Artifact],
        host: &dyn Host,
        progress: &dyn ProgressSink,
        cancel: &Cancel,
    ) -> Result<Installed> {
        check(artifacts)?;
        let mut installed = Installed::default();
        let mut report = Progress {
            files: artifacts.len(),
            done: 0,
            received: 0,
            size: None,
        };
        for artifact in artifacts {
            cancel.check()?;
            let location = match host.storage().find(&artifact.sha256).await? {
                Some(location) => location,
                None => download(artifact, host, progress, cancel, report).await?,
            };
            installed.files.insert(artifact.key.clone(), location);
            report.done += 1;
            progress.progress(report);
        }
        Ok(installed)
    }
}

/// Every digest well formed, and no key naming two different files.
fn check(artifacts: &[Artifact]) -> Result<()> {
    let mut digests = BTreeMap::new();
    for artifact in artifacts {
        if !digest::is_valid(&artifact.sha256) {
            return Err(Error::new("digest-invalid"));
        }
        let digest = digests.entry(&artifact.key).or_insert(&artifact.sha256);
        if *digest != &artifact.sha256 {
            return Err(Error::new("artifact-key-conflict"));
        }
    }
    Ok(())
}

/// Downloads `artifact` into storage under its digest, and returns where the host keeps it. `report` is the progress
/// so far, before this file.
async fn download(
    artifact: &Artifact,
    host: &dyn Host,
    progress: &dyn ProgressSink,
    cancel: &Cancel,
    mut report: Progress,
) -> Result<String> {
    let mut download = host.fetcher().fetch(&artifact.url).await?;
    let mut file = host.storage().create(&artifact.sha256).await?;
    let mut sha256 = Hasher::default();
    report.size = download.size();
    progress.progress(report);
    while let Some(bytes) = download.chunk().await? {
        // Returning drops `file` uncommitted: nothing is stored.
        cancel.check()?;
        sha256.update(&bytes);
        file.write(&bytes).await?;
        report.received += bytes.len() as u64;
        progress.progress(report);
    }
    if sha256.finish() != artifact.sha256 {
        return Err(Error::new("digest-mismatch"));
    }
    file.commit().await
}
