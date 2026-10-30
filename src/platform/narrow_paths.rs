//! Paths for C libraries that take them as a narrow `char *`, which Windows reads in the ANSI code page, not as UTF-8:
//! espeak-ng, which sherpa-onnx links for Kokoro and Piper, cannot open a data directory under a profile named
//! `C:\Users\Iñaki` (sidevoice-engine#130), and aborts the process when it cannot.

use std::path::{Path, PathBuf};

/// What the engine asks of the operating system about narrow paths.
pub trait NarrowPaths {
    /// A name for `path` that a library taking a narrow `char *` path opens as well as `path` itself, or `None` when
    /// there is none (a Windows path with characters outside ASCII, when no junction can be made for it). Callers fail
    /// the load with it, not hand the library a path it cannot open.
    fn narrow(path: &Path) -> Option<PathBuf>;
}

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::Fs as Paths;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::Fs as Paths;

#[cfg(test)]
mod tests;
