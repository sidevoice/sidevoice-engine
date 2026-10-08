//! What an install needs, worked out before anything is downloaded: every artifact checked (a well-formed digest, a
//! plain relative archive path, no key naming two things), and the distinct digests it wants, whole, unpacked, or
//! both.

use std::collections::BTreeMap;

use super::{archive, digest, Artifact};
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

/// Every digest well formed, every archive path a plain relative path, and no key naming two different things.
/// Returns each artifact's archive path, normalised. Fails with `digest-invalid`, `archive-path-invalid` or
/// `artifact-key-conflict`.
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
                Some(archive::member_path(path).ok_or(Error::new("archive-path-invalid"))?)
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
