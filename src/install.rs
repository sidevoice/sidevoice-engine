//! Installing a build: one installer for every backend. It downloads the build's model files (catalogue) and the
//! backend's files for this platform (`backends.json`) through the host's `Fetcher` into its `Storage`, checks each
//! against its digest, and moves the build through its lifecycle (`BuildState`). Not implemented yet: the interface is
//! the skeleton's.

use std::collections::BTreeMap;

use crate::host::Host;
use crate::{Error, Result};

/// One file to download: where from, its digest, and its key in storage.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Artifact {
    /// Its key in [`Storage`](crate::Storage).
    pub key: String,
    /// Where it is downloaded from.
    pub url: String,
    /// Its SHA-256 digest, in hex.
    pub sha256: String,
}

/// A build whose files are in storage: each file's name, and where the host keeps it (a path, an OPFS name, ...).
/// A model file's name is its key; a backend file's, its name in `backends.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Installed {
    pub(crate) files: BTreeMap<String, String>,
}

impl Installed {
    /// Where the host keeps the file called `name`, if it is installed.
    #[allow(dead_code, reason = "the stub backends load nothing yet")]
    pub(crate) fn file(&self, name: &str) -> Option<&str> {
        self.files.get(name).map(String::as_str)
    }
}

/// Puts a build's files in storage.
#[derive(Debug)]
pub(crate) struct Installer;

impl Installer {
    /// Downloads and checks whatever of `files` (each with the name it gets in [`Installed`]) is not in storage yet.
    pub(crate) async fn install(
        &self,
        _files: &[(String, Artifact)],
        _host: &dyn Host,
    ) -> Result<Installed> {
        Err(Error::new("not-implemented"))
    }
}

#[cfg(test)]
mod tests;
