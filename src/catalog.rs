//! The catalogue of local models: the merge of every source's fragment. A model has several builds, one per backend
//! and format (whisper-small: ONNX for sherpa-onnx and transformers.js, GGUF for whisper.cpp, MLX for Apple).

use crate::backend::BackendId;
use crate::install::Artifact;
use crate::Result;

/// Where catalogue entries come from: the catalogue bundled in the engine, a remote one pinned by digest, the
/// user's own models.
pub trait CatalogSource: Send + Sync {
    fn load(&self) -> Result<CatalogFragment>;
}

/// What one source contributes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CatalogFragment {
    pub models: Vec<Model>,
}

/// What a model is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Task {
    /// Speech to text.
    Stt,
    /// Text to speech.
    Tts,
}

/// A model family a backend knows how to run: "whisper", "kokoro", "piper", ...
pub type Family = &'static str;

#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    pub id: String,
    pub family: String,
    pub task: Task,
    /// Best first: the ranking step keeps the first build that fits.
    pub builds: Vec<Build>,
}

/// One way to run a model: a backend, a format, what it needs, and the model's files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Build {
    pub id: String,
    pub backend: BackendId,
    pub format: String,
    pub memory_mb: u32,
    pub files: Vec<Artifact>,
}

/// The merged catalogue.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Catalog {
    models: Vec<Model>,
}

/// Something wrong with the merged catalogue, found by [`Catalog::check`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    DuplicateModel { model: String },
    ModelWithoutBuilds { model: String },
}

impl Catalog {
    pub fn merge(sources: &[Box<dyn CatalogSource>]) -> Result<Self> {
        let mut models = Vec::new();
        for source in sources {
            models.extend(source.load()?.models);
        }
        Ok(Self { models })
    }

    pub fn models(&self, task: Task) -> impl Iterator<Item = &Model> {
        self.models.iter().filter(move |model| model.task == task)
    }

    pub fn check(&self) -> Vec<Problem> {
        let mut problems = Vec::new();
        for (i, model) in self.models.iter().enumerate() {
            if self.models[..i].iter().any(|other| other.id == model.id) {
                problems.push(Problem::DuplicateModel {
                    model: model.id.clone(),
                });
            }
            if model.builds.is_empty() {
                problems.push(Problem::ModelWithoutBuilds {
                    model: model.id.clone(),
                });
            }
        }
        problems
    }
}
