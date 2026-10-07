//! The catalogue of local models: the merge of every source's fragment. Three levels: a family (one loader: whisper,
//! kokoro, ...), its models, and each model's builds, one per way to run it (whisper-small: ONNX for sherpa-onnx and
//! transformers.js, GGML for whisper.cpp, MLX for Apple).
//!
//! Inside: `family` and `model` (the shape, read strictly: an unknown or a missing key is an error) and `bundled`
//! (the families this repository ships, `catalog/families/<family>.json`, compiled in).

use std::collections::HashSet;

use crate::backend;
use crate::maybe_send::{MaybeSend, MaybeSync};
use crate::Result;

mod bundled;
mod family;
mod model;
#[cfg(test)]
mod tests;

pub use bundled::BundledCatalog;
pub use family::Family;
pub use model::{Build, Capability, Memory, MemorySource, Model, ModelFile, Precision, Requires};

/// Where catalogue entries come from: the catalogue bundled in the engine, a remote one pinned by digest, the
/// user's own models.
pub trait CatalogSource: MaybeSend + MaybeSync {
    /// This source's families.
    ///
    /// # Errors
    ///
    /// When the source cannot be read; [`Engine::new`](crate::Engine::new) then fails.
    fn load(&self) -> Result<CatalogFragment>;
}

/// What one source contributes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CatalogFragment {
    /// Its families, with their models.
    pub families: Vec<Family>,
}

/// The merged catalogue.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Catalog {
    families: Vec<Family>,
}

/// Something wrong with the merged catalogue.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Problem {
    /// Two sources, or one twice, define a family with this id.
    DuplicateFamily {
        /// The repeated id.
        family: String,
    },
    /// A family with no model.
    FamilyWithoutModels {
        /// Its id.
        family: String,
    },
    /// Two models with this id, in one family or in two.
    DuplicateModel {
        /// The repeated id.
        model: String,
    },
    /// A model that says it can do nothing.
    ModelWithoutCapabilities {
        /// Its id.
        model: String,
    },
    /// A model with no build, which nothing could run.
    ModelWithoutBuilds {
        /// Its id.
        model: String,
    },
    /// Two builds with this id, anywhere in the catalogue.
    DuplicateBuild {
        /// The repeated id.
        build: String,
    },
    /// A build whose backend no build of the engine has (it is not in `backends.json`).
    UnknownBackend {
        /// The build.
        build: String,
        /// The backend it names.
        backend: String,
    },
    /// A build with no file, which nothing could load.
    BuildWithoutFiles {
        /// Its id.
        build: String,
    },
    /// Two files of one build with this key.
    DuplicateFile {
        /// The build.
        build: String,
        /// The repeated key.
        key: String,
    },
    /// A file whose `sha256` is not a SHA-256 digest in lowercase hex: it could not be checked once downloaded.
    FileWithoutDigest {
        /// The build.
        build: String,
        /// The file's key.
        key: String,
    },
}

impl Catalog {
    /// Every source's families, in the order of `sources`.
    pub(crate) fn merge(sources: &[Box<dyn CatalogSource>]) -> Result<Self> {
        let mut families = Vec::new();
        for source in sources {
            families.extend(source.load()?.families);
        }
        Ok(Self { families })
    }

    /// The models that can do `capability`, in catalogue order.
    pub(crate) fn models(&self, capability: Capability) -> impl Iterator<Item = &Model> {
        self.families
            .iter()
            .flat_map(|family| &family.models)
            .filter(move |model| model.capabilities.contains(&capability))
    }

    /// What is wrong with the merged catalogue; empty if nothing is.
    #[must_use]
    pub(crate) fn check(&self) -> Vec<Problem> {
        let mut problems = Vec::new();
        let (mut families, mut models, mut builds) =
            (HashSet::new(), HashSet::new(), HashSet::new());
        for family in &self.families {
            if !families.insert(&family.id) {
                problems.push(Problem::DuplicateFamily {
                    family: family.id.clone(),
                });
            }
            if family.models.is_empty() {
                problems.push(Problem::FamilyWithoutModels {
                    family: family.id.clone(),
                });
            }
            for model in &family.models {
                if !models.insert(&model.id) {
                    problems.push(Problem::DuplicateModel {
                        model: model.id.clone(),
                    });
                }
                if model.capabilities.is_empty() {
                    problems.push(Problem::ModelWithoutCapabilities {
                        model: model.id.clone(),
                    });
                }
                if model.builds.is_empty() {
                    problems.push(Problem::ModelWithoutBuilds {
                        model: model.id.clone(),
                    });
                }
                for build in &model.builds {
                    if !builds.insert(&build.id) {
                        problems.push(Problem::DuplicateBuild {
                            build: build.id.clone(),
                        });
                    }
                    check_build(build, &mut problems);
                }
            }
        }
        problems
    }
}

/// A build's own problems: its backend and its files.
fn check_build(build: &Build, problems: &mut Vec<Problem>) {
    if !backend::is_known(&build.backend) {
        problems.push(Problem::UnknownBackend {
            build: build.id.clone(),
            backend: build.backend.clone(),
        });
    }
    if build.files.is_empty() {
        problems.push(Problem::BuildWithoutFiles {
            build: build.id.clone(),
        });
    }
    let mut keys = HashSet::new();
    for file in &build.files {
        if !keys.insert(&file.key) {
            problems.push(Problem::DuplicateFile {
                build: build.id.clone(),
                key: file.key.clone(),
            });
        }
        let hex = |c: u8| c.is_ascii_digit() || (b'a'..=b'f').contains(&c);
        if file.sha256.len() != 64 || !file.sha256.bytes().all(hex) {
            problems.push(Problem::FileWithoutDigest {
                build: build.id.clone(),
                key: file.key.clone(),
            });
        }
    }
}
