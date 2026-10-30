//! Whether C libraries that take a path as a narrow `char *` can open it. Windows reads those in the process's ANSI
//! code page, not as UTF-8, unless the executable declares `activeCodePage` UTF-8 in its manifest (Windows 10 1903
//! and later): espeak-ng, which sherpa-onnx links for Kokoro and Piper, cannot open a data directory under a profile
//! named `C:\Users\Iñaki` otherwise, and aborts the process when it cannot (sidevoice-engine#130).

use std::path::Path;

/// What the engine asks of the operating system about narrow paths.
pub trait NarrowPaths {
    /// Whether a library taking `path` as a narrow `char *` opens the very file it names. Callers fail the load when
    /// it does not, instead of handing the library a path it would abort on.
    fn opens(path: &Path) -> bool;
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
