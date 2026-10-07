use crate::host::Accelerator;
use crate::install::Artifact;

/// One way to run a model: a backend, a format, what it needs, and the model's files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Build {
    /// Its stable id: "whisper-small-onnx", ...
    pub id: String,
    /// The id of the backend that runs it ([`Engine::backends`](crate::Engine::backends)).
    pub backend: String,
    /// The format of its files: "onnx", "gguf", "mlx", ...
    pub format: String,
    /// The memory it needs to run, in MB.
    pub memory_mb: u32,
    /// The accelerators it accepts (a model exported for the CPU only, say); empty accepts any its backend runs on.
    /// Order does not matter: the backend's preference ranks them.
    pub accelerators: Vec<Accelerator>,
    /// The model's files.
    pub files: Vec<Artifact>,
}
