use super::Reason;

/// The step of the funnel that rejected a build.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Rejection {
    /// 1. The build's backend is not compiled into this build of the engine.
    BackendNotInThisBuild,
    /// 2. The backend cannot run here: it has nothing to download for this platform (`no-runtime-for-platform`), or none
    ///    of its accelerators works here (`no-accelerator`).
    BackendUnavailable(Reason),
    /// 3. The build does not fit here: none of the accelerators that work here is one it accepts
    ///    (`build-accelerator`), or the machine does not meet a requirement of the build or of its backend (`memory`,
    ///    `cores`, ...).
    DoesNotFit(Reason),
}
