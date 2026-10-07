//! A model of the catalogue, with every build of it; inside, `task` (what a model is for) and `build` (one way to run
//! it).

mod build;
mod task;

pub use build::Build;
pub use task::Task;

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
