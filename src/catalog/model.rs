//! The catalogue's data: models, what each is for, and their builds.

use crate::install::Artifact;

/// What a model is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Task {
    /// Speech to text.
    Stt,
    /// Text to speech.
    Tts,
}

/// A model, with every build of it the catalogue knows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Model {
    /// Its stable id: "whisper-small", "kokoro", ...
    pub id: String,
    /// The model family a backend knows how to run: "whisper", "kokoro", "piper", ...
    pub family: String,
    /// What it is for.
    pub task: Task,
    /// Best first: the ranking step keeps the first build that fits.
    pub builds: Vec<Build>,
}

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
    /// The model's files.
    pub files: Vec<Artifact>,
}
