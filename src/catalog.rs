//! The catalogue of local models: the merge of every source's fragment. A model has several builds, one per backend
//! and format (whisper-small: ONNX for sherpa-onnx and transformers.js, GGUF for whisper.cpp, MLX for Apple).

use crate::install::Artifact;
use crate::maybe_send::{MaybeSend, MaybeSync};
use crate::Result;

/// Where catalogue entries come from: the catalogue bundled in the engine, a remote one pinned by digest, the
/// user's own models.
pub trait CatalogSource: MaybeSend + MaybeSync {
    /// This source's models.
    ///
    /// # Errors
    ///
    /// When the source cannot be read; [`Engine::new`](crate::Engine::new) then fails.
    fn load(&self) -> Result<CatalogFragment>;
}

/// What one source contributes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CatalogFragment {
    /// Its models, in the source's order.
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

/// The merged catalogue.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Catalog {
    models: Vec<Model>,
}

/// Something wrong with the merged catalogue.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Problem {
    /// Two sources, or one twice, define a model with this id.
    DuplicateModel {
        /// The repeated id.
        model: String,
    },
    /// A model with no build, which nothing could run.
    ModelWithoutBuilds {
        /// Its id.
        model: String,
    },
}

impl Catalog {
    /// Every source's models, in the order of `sources`.
    pub(crate) fn merge(sources: &[Box<dyn CatalogSource>]) -> Result<Self> {
        let mut models = Vec::new();
        for source in sources {
            models.extend(source.load()?.models);
        }
        Ok(Self { models })
    }

    /// The models of `task`, in catalogue order.
    pub(crate) fn models(&self, task: Task) -> impl Iterator<Item = &Model> {
        self.models.iter().filter(move |model| model.task == task)
    }

    /// What is wrong with the merged catalogue; empty if nothing is.
    #[must_use]
    pub(crate) fn check(&self) -> Vec<Problem> {
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
