//! Installing a build: one installer for every backend, which does not know what a file is. It gets one list of
//! [`Artifact`]s, the build's model files (catalogue) and its backend's files for this platform (`backends.json`)
//! alike, downloads each through the host's `Fetcher`, checks it against its SHA-256 as it arrives, and stores it
//! through the host's `Storage`.
//!
//! Storage is laid out as Hugging Face's hub cache. Each file is a blob stored under its digest, once, whichever builds
//! use it: a file shared by two builds, or unchanged across a version, is downloaded once, and a blob is only stored
//! once its bytes hash to its name. Each build has its folder, named after the build's id, where every file sits under
//! its original name (the last segment of its URL), as a link to its blob: so a backend whose engine expects a model
//! directory, or looks at file names and extensions, finds what it expects. [`Installed`] is where each key's file is
//! in that folder, and a build is installed when its folder is stored, which happens only once every file is in it.
//! Files stay on disk when their models leave memory; [`Installer::uninstall`] removes a build's folder, and each of
//! its blobs no other folder links.
//!
//! An artifact with an `archive_path` is a member of an archive: several keys may share one archive (the same `url`
//! and `sha256`), each naming its own member. Each distinct archive is downloaded once, checked against its digest,
//! and only then unpacked, once, into a tree stored as the blob `<sha256>-unpacked`; the archive itself is then
//! removed, unless an artifact also wants it whole. The member sits in the build's folder at its path inside the
//! archive: a file, or a directory with every file below it.
//!
//! The codes it fails with, and the engine's English text for each:
//!
//! - `digest-invalid`: a file's digest is not a SHA-256 in lowercase hex.
//! - `artifact-key-conflict`: two files of the build have the same name.
//! - `archive-path-invalid`: a file's path inside its archive is not a plain relative path.
//! - `file-name-invalid`: a file's URL does not end in a usable file name.
//! - `file-path-conflict`: two files of the build would sit at the same place in its folder.
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
//! Inside: `progress` (what the installer reports as it goes), `cancel` (how it is stopped), `digest` (SHA-256) and
//! `archive` (unpacking, safely, in native builds).

use std::collections::{BTreeMap, BTreeSet};

use crate::host::{Host, Storage};
use crate::{Error, Result};

#[cfg(native)]
mod archive;
mod cancel;
mod digest;
mod progress;
#[cfg(test)]
mod tests;

pub use cancel::Cancel;
use digest::Hasher;
pub use progress::{Progress, ProgressSink};

/// One file to download, or one member of an archive to download: where from, its digest, and the name `load` finds
/// it by.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Artifact {
    /// The name the backend's `load` finds it by: its key in the catalogue, or its `name` in `backends.json`. Not its
    /// name in [`Storage`](crate::Storage), which is its digest.
    pub key: String,
    /// Where it is downloaded from: the file, or the archive it is in.
    pub url: String,
    /// The SHA-256 digest of what `url` serves (the file, or the whole archive): 64 lowercase hex digits.
    pub sha256: String,
    /// `None` for a file. For a member of an archive (a tar, bzip2-compressed or not), its path inside it,
    /// `/`-separated (`kokoro-int8-en-v0_19/model.int8.onnx`): a file or a directory.
    pub archive_path: Option<String>,
}

/// A build whose files are in storage: each artifact's key, and where the host keeps it in the build's folder (a path, an
/// OPFS name, ...): the file, or the member of the archive. A backend file's key is its name in `backends.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Installed {
    pub(crate) files: BTreeMap<String, String>,
}

impl Installed {
    /// Where the host keeps the file or directory whose key is `name`, if it is installed.
    #[allow(
        dead_code,
        reason = "sherpa-onnx takes every file by its key, the stubs load nothing: only tests ask for one"
    )]
    pub(crate) fn file(&self, name: &str) -> Option<&str> {
        self.files.get(name).map(String::as_str)
    }
}

/// Puts a build's files in storage.
#[derive(Debug)]
pub(crate) struct Installer;

/// One distinct digest the build needs: whole, unpacked, or both.
struct Wanted<'a> {
    sha256: &'a str,
    url: &'a str,
    whole: bool,
    unpacked: bool,
}

impl Installer {
    /// Puts `artifacts` in the build folder `folder` (the build's id), unless it is stored already: downloads, checks
    /// and stores whatever blob is not stored yet, one distinct file at a time, unpacking archives once checked, links
    /// each file into the folder under its name, and stores the folder. It tells `progress` as it goes, and stops
    /// between two parts of a file once `cancel` is cancelled. Dropping the future stops it too; either way, nothing
    /// half downloaded or half unpacked is stored, and no folder half made.
    ///
    /// # Errors
    ///
    /// The codes listed above. `digest-invalid`, `artifact-key-conflict`, `archive-path-invalid`,
    /// `file-name-invalid`, `file-path-conflict` and `archive-unsupported` are found before anything is downloaded.
    pub(crate) async fn install(
        &self,
        folder: &str,
        artifacts: &[Artifact],
        host: &dyn Host,
        progress: &dyn ProgressSink,
        cancel: &Cancel,
    ) -> Result<Installed> {
        let members = check(artifacts)?;
        let paths = paths(artifacts, &members)?;
        let storage = host.storage();
        if storage.find_folder(folder).await?.is_none() {
            let wanted = wanted(artifacts);
            let mut report = Progress {
                files: wanted.len(),
                done: 0,
                received: 0,
                size: None,
            };
            for wanted in &wanted {
                cancel.check()?;
                if wanted.whole && storage.find(wanted.sha256).await?.is_none() {
                    download(wanted, host, progress, cancel, report).await?;
                }
                if wanted.unpacked && storage.find(&unpacked(wanted.sha256)).await?.is_none() {
                    if storage.find(wanted.sha256).await?.is_none() {
                        download(wanted, host, progress, cancel, report).await?;
                    }
                    unpack(storage, wanted.sha256, cancel).await?;
                    if !wanted.whole {
                        storage.remove(wanted.sha256).await?;
                    }
                }
                report.done += 1;
                progress.progress(report);
            }
            let mut writer = storage.create_folder(folder).await?;
            let mut linked = BTreeSet::new();
            for ((artifact, member), path) in artifacts.iter().zip(&members).zip(&paths) {
                if !linked.insert(path) {
                    continue;
                }
                match member {
                    None => writer.link(path, &artifact.sha256, None).await?,
                    Some(member) => {
                        let tree = unpacked(&artifact.sha256);
                        if storage.find_member(&tree, member).await?.is_none() {
                            return Err(Error::new("archive-member-missing"));
                        }
                        writer.link(path, &tree, Some(member)).await?;
                    }
                }
            }
            writer.commit().await?;
        }
        let mut installed = Installed::default();
        for (artifact, path) in artifacts.iter().zip(&paths) {
            let location = storage.find_in_folder(folder, path).await?;
            let location = location.ok_or(Error::new("storage-failed"))?;
            installed.files.insert(artifact.key.clone(), location);
        }
        Ok(installed)
    }

    /// Removes the build folder `folder`, then each blob of `artifacts` that no other build folder links: a file
    /// shared by two builds stays as long as either is installed.
    ///
    /// # Errors
    ///
    /// What the host's storage fails with.
    #[cfg_attr(
        not(test),
        allow(dead_code, reason = "the engine does not remove builds yet")
    )]
    pub(crate) async fn uninstall(
        &self,
        folder: &str,
        artifacts: &[Artifact],
        storage: &dyn Storage,
    ) -> Result<()> {
        storage.remove_folder(folder).await?;
        let digests: BTreeSet<&str> = artifacts.iter().map(|a| a.sha256.as_str()).collect();
        for digest in digests {
            for blob in [digest.to_owned(), unpacked(digest)] {
                if !storage.is_linked(&blob).await? {
                    storage.remove(&blob).await?;
                }
            }
        }
        Ok(())
    }
}

/// Where each artifact sits in the build's folder: a member at its path inside the archive, a file under the last
/// segment of its URL. Fails with `file-name-invalid` for a URL that ends in no usable name, and `file-path-conflict`
/// when two different files would sit at one place.
fn paths(artifacts: &[Artifact], members: &[Option<String>]) -> Result<Vec<String>> {
    let mut placed = BTreeMap::new();
    let mut paths = Vec::new();
    for (artifact, member) in artifacts.iter().zip(members) {
        let path = match member {
            Some(member) => member.clone(),
            None => file_name(&artifact.url).ok_or(Error::new("file-name-invalid"))?,
        };
        let what = (artifact.sha256.as_str(), member.as_deref());
        if *placed.entry(path.clone()).or_insert(what) != what {
            return Err(Error::new("file-path-conflict"));
        }
        paths.push(path);
    }
    Ok(paths)
}

/// The file name a URL ends in, without its query or fragment, if it is a plain one.
fn file_name(url: &str) -> Option<String> {
    let url = url.split(['?', '#']).next()?;
    let (_, name) = url.rsplit_once('/')?;
    member_path(name).filter(|name| !name.contains('/'))
}

/// Every digest well formed, every archive path a plain relative path (and archives only where they can be unpacked),
/// and no key naming two different things.
/// Returns each artifact's archive path, normalised.
fn check(artifacts: &[Artifact]) -> Result<Vec<Option<String>>> {
    let mut keys = BTreeMap::new();
    let mut members = Vec::new();
    for artifact in artifacts {
        if !digest::is_valid(&artifact.sha256) {
            return Err(Error::new("digest-invalid"));
        }
        let member = match &artifact.archive_path {
            None => None,
            Some(path) => {
                if cfg!(web) {
                    return Err(Error::new("archive-unsupported"));
                }
                Some(member_path(path).ok_or(Error::new("archive-path-invalid"))?)
            }
        };
        let named = (artifact.sha256.as_str(), member.clone());
        if *keys
            .entry(artifact.key.as_str())
            .or_insert_with(|| named.clone())
            != named
        {
            return Err(Error::new("artifact-key-conflict"));
        }
        members.push(member);
    }
    Ok(members)
}

/// The distinct digests of `artifacts`, in order, each with the first URL that serves it and how it is wanted.
fn wanted(artifacts: &[Artifact]) -> Vec<Wanted<'_>> {
    let mut wanted: Vec<Wanted<'_>> = Vec::new();
    for artifact in artifacts {
        let entry = match wanted
            .iter_mut()
            .position(|wanted| wanted.sha256 == artifact.sha256)
        {
            Some(i) => &mut wanted[i],
            None => {
                wanted.push(Wanted {
                    sha256: &artifact.sha256,
                    url: &artifact.url,
                    whole: false,
                    unpacked: false,
                });
                wanted.last_mut().expect("just pushed")
            }
        };
        if artifact.archive_path.is_some() {
            entry.unpacked = true;
        } else {
            entry.whole = true;
        }
    }
    wanted
}

/// The storage name of the tree the archive `sha256` unpacks to.
fn unpacked(sha256: &str) -> String {
    format!("{sha256}-unpacked")
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
    let mut tree = storage.create_tree(&unpacked(sha256))?;
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

/// Downloads `wanted` into storage under its digest. `report` is the progress so far, before this file.
async fn download(
    wanted: &Wanted<'_>,
    host: &dyn Host,
    progress: &dyn ProgressSink,
    cancel: &Cancel,
    mut report: Progress,
) -> Result<()> {
    let mut download = host.fetcher().fetch(wanted.url).await?;
    let mut file = host.storage().create(wanted.sha256).await?;
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
    if sha256.finish() != wanted.sha256 {
        return Err(Error::new("digest-mismatch"));
    }
    file.commit().await.map(drop)
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
