//! What an install needs, worked out before anything is downloaded: every artifact checked (a well-formed digest, a
//! plain relative archive path, archives only where they can be unpacked, no key naming two things), where each sits
//! in the build's folder, and the distinct digests it wants, whole, unpacked, or both.

use std::collections::BTreeMap;

use super::{digest, member_path, Artifact};
use crate::{Error, Result};

#[cfg(test)]
mod tests;

/// One distinct digest the build needs: whole, unpacked, or both.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Wanted<'a> {
    pub(super) sha256: &'a str,
    /// The first URL that serves it.
    pub(super) url: &'a str,
    pub(super) whole: bool,
    pub(super) unpacked: bool,
}

/// Every digest well formed, every archive path a plain relative path (and archives only where they can be unpacked),
/// and no key naming two different things.
/// Returns each artifact's archive path, normalised. Fails with `digest-invalid`, `archive-unsupported`,
/// `archive-path-invalid` or `artifact-key-conflict`.
pub(super) fn check(artifacts: &[Artifact]) -> Result<Vec<Option<String>>> {
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

/// Where each artifact sits in the build's folder: a member at its path inside the archive, a file where its URL puts it
/// ([`file_path`]). Fails with `file-name-invalid` for a URL that gives no usable path, and `file-path-conflict` when
/// two different files would sit at one place.
pub(super) fn paths(artifacts: &[Artifact], members: &[Option<String>]) -> Result<Vec<String>> {
    let mut placed = BTreeMap::new();
    let mut paths = Vec::new();
    for (artifact, member) in artifacts.iter().zip(members) {
        let path = match member {
            Some(member) => member.clone(),
            None => file_path(&artifact.url).ok_or(Error::new("file-name-invalid"))?,
        };
        let what = (artifact.sha256.as_str(), member.as_deref());
        if *placed.entry(path.clone()).or_insert(what) != what {
            return Err(Error::new("file-path-conflict"));
        }
        paths.push(path);
    }
    Ok(paths)
}

/// Where a downloaded file sits in its build's folder, from its URL (without its query or fragment): for a Hugging
/// Face file (`…/resolve/<revision>/<path>`), its path in the repository, as the hub's own snapshot folders keep it
/// (`onnx/model_q8.onnx`, which transformers.js and mlx-audio look for); for any other URL, a GitHub release asset
/// among them, its last segment. `None` unless that is a plain relative path ([`member_path`]).
pub(super) fn file_path(url: &str) -> Option<String> {
    let url = url.split(['?', '#']).next()?;
    if let Some((_, revision_and_path)) = url.split_once("/resolve/") {
        let (_, path) = revision_and_path.split_once('/')?;
        return member_path(path);
    }
    let (_, name) = url.rsplit_once('/')?;
    member_path(name).filter(|name| !name.contains('/'))
}

/// The distinct digests of `artifacts`, in order, each with the first URL that serves it and how it is wanted.
pub(super) fn wanted(artifacts: &[Artifact]) -> Vec<Wanted<'_>> {
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
pub(super) fn unpacked(sha256: &str) -> String {
    format!("{sha256}-unpacked")
}
