//! Why an engine cannot be built.

use std::fmt;

use crate::catalog::Problem;
use crate::Error;

/// Why an engine cannot be built.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConfigError {
    /// A catalogue source failed to load.
    Source(Error),
    /// The merged catalogue is inconsistent.
    Catalog(Vec<Problem>),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Stable codes, like `Error`'s: the source's own code is its `source()`.
        f.write_str(match self {
            Self::Source(_) => "catalog-source-failed",
            Self::Catalog(_) => "catalog-inconsistent",
        })
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Source(error) => Some(error),
            Self::Catalog(_) => None,
        }
    }
}
