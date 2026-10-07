//! Installing a build: one installer for every backend. It downloads the build's model files (catalogue) and the
//! backend's library files (the backends' data file) through the host's `Fetcher` into its `Storage`, checks each
//! against its digest, and moves the build through its lifecycle (`BuildState`). Not implemented yet: the interface is
//! the skeleton's.

use std::collections::BTreeMap;

use crate::host::Host;
use crate::{Error, Result};

/// One file to download: where from, its digest, and its key in storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    pub key: String,
    pub url: String,
    pub sha256: String,
}

/// A build whose files are in storage: each artifact's key, and where the host keeps it (a path, an OPFS name, ...).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Installed {
    pub files: BTreeMap<String, String>,
}

pub struct Installer;

impl Installer {
    /// Downloads and checks whatever of `artifacts` is not in storage yet.
    pub async fn install(&self, _artifacts: &[Artifact], _host: &dyn Host) -> Result<Installed> {
        Err(Error::new("not-implemented"))
    }
}
