use super::Requirement;
use crate::host::Accelerator;

/// A backend's stable id, as catalogue builds name it ([`Build::backend`](crate::Build::backend)): "sherpa-onnx",
/// "whisper-cpp", "mlx", ...
pub type BackendId = &'static str;

/// What a backend is and needs, as data. Adding a backend is mostly filling this in.
pub(crate) struct BackendSpec {
    /// What catalogue builds call it.
    pub(crate) id: BackendId,
    /// The accelerators it can run on, best first: the default is the first one that works here.
    pub(crate) accelerators: &'static [Accelerator],
    /// What the machine must meet, whatever the model: each one a check on the capabilities.
    pub(crate) requirements: &'static [&'static dyn Requirement],
}
