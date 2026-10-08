//! Installing a build: one installer for every backend, which does not know what a file is. It gets the build's list
//! of [`Artifact`]s, its model files from the catalogue, downloads each through the host's `Fetcher`, checks it
//! against its SHA-256 as it arrives, and stores it through the host's `Storage`.
//!
//! Storage is content-addressed: each file is stored under its digest, and an artifact's `key` is only the name `load`
//! finds it by in [`Installed`]. So two backends, or two versions of one, never write the same name; a file shared by
//! two builds, or unchanged across a version, is downloaded once; and "already installed" is "its digest is stored",
//! because a file is only stored once its bytes hash to its name. Files stay on disk when their models leave memory;
//! removing them is a separate policy.
//!
//! An artifact with an `archive_path` is a member of an archive: several keys may share one archive (the same `url`
//! and `sha256`), each naming its own member. Each distinct archive is downloaded once, checked against its digest,
//! and only then unpacked, once, into a tree stored as `<sha256>-unpacked`; the archive itself is then removed, unless
//! an artifact also wants it whole. `Installed::file(key)` is then where that member is: a file or a directory.
//!
//! The codes it fails with, and the engine's English text for each:
//!
//! - `digest-invalid`: a file's digest is not a SHA-256 in lowercase hex.
//! - `artifact-key-conflict`: two files of the build have the same name.
//! - `archive-path-invalid`: a file's path inside its archive is not a plain relative path.
//! - `archive-unsupported`: this build cannot unpack archives (the web build: archives come only with native-only
//!   builds).
//! - `digest-mismatch`: a downloaded file is not the one expected.
//! - `archive-corrupt`: an archive could not be read.
//! - `archive-entry-unsupported`: an archive holds something that is not a plain file or directory inside it.
//! - `archive-too-large`: an archive unpacks to more than is allowed.
//! - `archive-member-missing`: an archive does not hold a file the build needs.
//! - `cancelled`: the install was cancelled.
//! - the host's own: `download-failed` (a file could not be downloaded), `storage-failed` (a file could not be
//!   stored).
//!
//! Inside, the steps (`plan`: what is wanted, checked before anything is downloaded; `download`: one file fetched,
//! verified and committed; `archive`: unpacking, safely, in native builds) and what they share (`progress`, what the
//! installer reports as it goes; `cancel`, how it is stopped; `digest`, SHA-256).

use std::collections::BTreeMap;

use crate::host::{Host, Storage};
use crate::{Error, Result};

#[cfg(native)]
mod archive;
mod cancel;
mod digest;
mod download;
mod plan;
mod progress;
#[cfg(test)]
mod tests;

pub use cancel::Cancel;
pub use progress::{Progress, ProgressSink};

/// One file to download, or one member of an archive to download: where from, its digest, and the name `load` finds
/// it by.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Artifact {
    /// The name the backend's `load` finds it by: its key in the catalogue. Not its name in
    /// [`Storage`](crate::Storage), which is its digest.
    pub key: String,
    /// Where it is downloaded from: the file, or the archive it is in.
    pub url: String,
    /// The SHA-256 digest of what `url` serves (the file, or the whole archive): 64 lowercase hex digits.
    pub sha256: String,
    /// `None` for a file. For a member of an archive (a tar, bzip2-compressed or not), its path inside it,
    /// `/`-separated (`kokoro-int8-en-v0_19/model.int8.onnx`): a file or a directory.
    pub archive_path: Option<String>,
}

/// A build whose files are in storage: each artifact's key, and where the host keeps it (a path, an OPFS name, ...):
/// the file, or the member of the unpacked archive.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Installed {
    pub(crate) files: BTreeMap<String, String>,
}

impl Installed {
    /// Where the host keeps the file or directory whose key is `name`, if it is installed.
    #[cfg_attr(
        not(sherpa_onnx),
        allow(dead_code, reason = "only sherpa-onnx loads a model yet")
    )]
    pub(crate) fn file(&self, name: &str) -> Option<&str> {
        self.files.get(name).map(String::as_str)
    }
}

/// Puts a build's files in storage.
#[derive(Debug)]
pub(crate) struct Installer;

impl Installer {
    /// Downloads, checks and stores whatever of `artifacts` is not stored yet, one distinct file at a time, unpacking
    /// archives once checked, telling `progress` as it goes, and stops between two parts of a file once `cancel` is
    /// cancelled. Dropping the future stops it too; either way, nothing half downloaded or half unpacked is stored.
    ///
    /// The steps are the plan (`plan.rs`: everything checked, then what is wanted), each file's download
    /// (`download.rs`), and each archive's unpacking (`archive.rs`); this only runs them in order.
    ///
    /// # Errors
    ///
    /// The codes listed above. `digest-invalid`, `artifact-key-conflict`, `archive-path-invalid` and
    /// `archive-unsupported` are found before anything is downloaded.
    pub(crate) async fn install(
        &self,
        artifacts: &[Artifact],
        host: &dyn Host,
        progress: &dyn ProgressSink,
        cancel: &Cancel,
    ) -> Result<Installed> {
        let members = plan::check(artifacts)?;
        let wanted = plan::wanted(artifacts);
        let storage = host.storage();
        let mut report = Progress {
            files: wanted.len(),
            done: 0,
            received: 0,
            size: None,
        };
        for wanted in &wanted {
            cancel.check()?;
            let (url, sha256) = (wanted.url, wanted.sha256);
            if wanted.whole && storage.find(sha256).await?.is_none() {
                download::download(url, sha256, host, progress, cancel, report).await?;
            }
            if wanted.unpacked && storage.find(&plan::unpacked(sha256)).await?.is_none() {
                if storage.find(sha256).await?.is_none() {
                    download::download(url, sha256, host, progress, cancel, report).await?;
                }
                unpack(storage, sha256, cancel).await?;
                if !wanted.whole {
                    storage.remove(sha256).await?;
                }
            }
            report.done += 1;
            progress.progress(report);
        }
        let mut installed = Installed::default();
        for (artifact, member) in artifacts.iter().zip(members) {
            let location = match member {
                None => storage.find(&artifact.sha256).await?,
                Some(path) => {
                    let tree = plan::unpacked(&artifact.sha256);
                    let member = storage.find_member(&tree, &path).await?;
                    Some(member.ok_or(Error::new("archive-member-missing"))?)
                }
            };
            let location = location.ok_or(Error::new("storage-failed"))?;
            installed.files.insert(artifact.key.clone(), location);
        }
        Ok(installed)
    }
    /// Whether every one of `artifacts` is in storage: the file under its digest, or, for a member of an archive, the
    /// tree that archive unpacks to (the archive itself is removed once unpacked).
    ///
    /// # Errors
    ///
    /// What the host's storage fails with.
    pub(crate) async fn is_installed(
        &self,
        artifacts: &[Artifact],
        storage: &dyn Storage,
    ) -> Result<bool> {
        for artifact in artifacts {
            let name = match artifact.archive_path {
                None => artifact.sha256.clone(),
                Some(_) => plan::unpacked(&artifact.sha256),
            };
            if storage.find(&name).await?.is_none() {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Removes from storage what `artifacts` installed, whole files and unpacked trees alike, except what `kept` (other
    /// models' files) also needs: a file shared by two builds stays as long as either wants it.
    ///
    /// # Errors
    ///
    /// What the host's storage fails with.
    pub(crate) async fn uninstall(
        &self,
        artifacts: &[Artifact],
        kept: &[Artifact],
        storage: &dyn Storage,
    ) -> Result<()> {
        for artifact in artifacts {
            if kept.iter().any(|other| other.sha256 == artifact.sha256) {
                continue;
            }
            storage.remove(&artifact.sha256).await?;
            storage.remove(&plan::unpacked(&artifact.sha256)).await?;
        }
        Ok(())
    }
}

/// Unpacks the stored archive `sha256` into its tree, on one of Tokio's blocking threads: `tar` reads synchronously.
/// Dropping the future stops the unpacking at its next part, and the tree is discarded.
#[cfg(native)]
async fn unpack(storage: &dyn Storage, sha256: &str, cancel: &Cancel) -> Result<()> {
    /// Stops the unpacking when the future that waits for it is dropped.
    struct StopOnDrop(Cancel);
    impl Drop for StopOnDrop {
        fn drop(&mut self) {
            self.0.cancel();
        }
    }

    let archive = storage.open(sha256)?;
    let mut tree = storage.create_tree(&plan::unpacked(sha256))?;
    let cancel = cancel.clone();
    let stop = StopOnDrop(Cancel::new());
    let stopped = stop.0.clone();
    let unpacking = tokio::task::spawn_blocking(move || {
        let check = || cancel.check().and_then(|()| stopped.check());
        archive::unpack(archive, tree.as_mut(), archive::Limits::DEFAULT, &check)?;
        tree.commit().map(drop)
    });
    let unpacked = match unpacking.await {
        Ok(unpacked) => unpacked,
        Err(error) if error.is_panic() => std::panic::resume_unwind(error.into_panic()),
        // The runtime is shutting down.
        Err(_) => Err(Error::new("cancelled")),
    };
    drop(stop);
    unpacked
}

/// Archives are refused before anything is downloaded ([`check`]).
#[cfg(web)]
async fn unpack(_storage: &dyn Storage, _sha256: &str, _cancel: &Cancel) -> Result<()> {
    Err(Error::new("archive-unsupported"))
}

/// `path` as a path inside a tree, if it is one: relative, `/`-separated, with `.` and empty segments dropped and no
/// `..`, backslash, colon or control character; `None` otherwise, or if nothing is left. What an artifact's
/// `archive_path` must be, and what an archive's entries must be named (`archive`).
fn member_path(path: &str) -> Option<String> {
    if path.starts_with('/') || path.len() > 4096 {
        return None;
    }
    let mut segments = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => return None,
            _ if segment
                .chars()
                .any(|c| c == '\\' || c == ':' || c.is_control()) =>
            {
                return None
            }
            _ => segments.push(segment),
        }
    }
    (!segments.is_empty() && segments.len() <= 64).then(|| segments.join("/"))
}
