//! What an install reports as it goes, and who it reports to.

use crate::maybe_send::{MaybeSend, MaybeSync};

/// How far an install has got: files done of all, and the bytes of the file being downloaded. Reported when a file
/// starts downloading, after each part of it, and when a file is done (downloaded, or found already stored).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Progress {
    /// The distinct files the build needs: an archive counts once, however many of its members it needs.
    pub files: usize,
    /// How many of them are stored (and unpacked, for an archive).
    pub done: usize,
    /// Bytes received of the file being downloaded; 0 between files.
    pub received: u64,
    /// The size of the file being downloaded, when the server says; `None` between files.
    pub size: Option<u64>,
}

/// Who an install reports its [`Progress`] to. Any `Fn(Progress)` is one (`&|_| {}` ignores it); it is called on the
/// install's own task, so it should return quickly, handing the progress on rather than acting on it.
pub trait ProgressSink: MaybeSend + MaybeSync {
    /// The install got this far.
    fn progress(&self, progress: Progress);
}

impl<F: Fn(Progress) + MaybeSend + MaybeSync> ProgressSink for F {
    fn progress(&self, progress: Progress) {
        self(progress);
    }
}
