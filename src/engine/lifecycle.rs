//! A build's lifecycle, from absent to ready and back: what [`Engine::state`](crate::Engine::state)
//! reports.

/// Where a build is on its way to running: `Absent → Installing → Installed → Loading → Ready`, or `Failed`. A ready
/// model left unused is unloaded from memory, and its build is `Installed` again: its files stay on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum BuildState {
    /// Some of its files are not in storage.
    Absent,
    /// Its files are being downloaded.
    Installing,
    /// Its files are in storage, and it is not in memory.
    Installed,
    /// Its backend is loading it.
    Loading,
    /// Loaded, ready to use.
    Ready,
    /// The last attempt to install or load it failed, with this code. Preparing it again starts over.
    Failed(crate::Error),
}
