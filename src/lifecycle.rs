//! A build's lifecycle, from absent to ready. Crate-private: the engine exposes no build state yet; it becomes public
//! when an [`Engine`](crate::Engine) method reports one.

/// Where a build is on its way to running: `Absent → Installing → Installed → Loading → Ready`, or `Failed`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
#[allow(
    dead_code,
    reason = "the installer does not move builds through their lifecycle yet"
)]
pub(crate) enum BuildState {
    /// Nothing of it is in storage.
    Absent,
    /// Its files are being downloaded.
    Installing,
    /// Its files are in storage.
    Installed,
    /// Its backend is loading it.
    Loading,
    /// Loaded, ready to use.
    Ready,
    /// Installing or loading failed.
    Failed(crate::Error),
}
