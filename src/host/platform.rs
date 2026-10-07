//! Which platform the engine runs on, from what the host reports: an OS and an architecture, or the web build.

use crate::host::{Capabilities, Runs};

/// A platform the engine knows: an OS and an architecture, or the web build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Platform {
    MacosAarch64,
    MacosX86_64,
    LinuxX86_64,
    LinuxAarch64,
    WindowsX86_64,
    Web,
}

impl Platform {
    /// The platform `caps` describe, if it is one the engine knows.
    pub(crate) fn of(caps: &Capabilities) -> Option<Self> {
        if caps.runs == Runs::Page {
            return Some(Self::Web);
        }
        match (caps.os.as_str(), caps.arch.as_str()) {
            ("macos", "aarch64") => Some(Self::MacosAarch64),
            ("macos", "x86_64") => Some(Self::MacosX86_64),
            ("linux", "x86_64") => Some(Self::LinuxX86_64),
            ("linux", "aarch64") => Some(Self::LinuxAarch64),
            ("windows", "x86_64") => Some(Self::WindowsX86_64),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
