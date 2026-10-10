//! The catalogue of local models: the merge of every source's fragment. Three levels: a family (one loader: whisper,
//! kokoro, ...), its models, and each model's builds, one per way to run it (whisper-small: ONNX for sherpa-onnx and
//! transformers.js, GGML for whisper.cpp, MLX for Apple).
//!
//! Inside: `family` and `model` (the shape, read strictly: an unknown or a missing key is an error) and `bundled`
//! (the families this repository ships, `catalog/families/<family>.json`, compiled in).

use std::collections::{HashMap, HashSet};

use crate::maybe_send::{MaybeSend, MaybeSync};
use crate::Result;
use model::CALL_ARGUMENTS;

mod bundled;
mod family;
mod model;
mod speed;
#[cfg(test)]
mod tests;

pub use bundled::BundledCatalog;
pub use family::Family;
pub use model::{
    BuildEntry, Capability, Gender, Memory, MemorySource, ModelEntry, ModelFile, Requires, Voice,
};
pub use speed::{FamilySpeed, SpeedRange};

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
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CatalogFragment {
    /// Its families, with their models.
    pub families: Vec<Family>,
}

/// The merged catalogue.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct MergedCatalog {
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
    /// A family's speed range has a bound that is not positive, a slowest speed above its fastest, no source, or an empty
    /// `decided_by`.
    InvalidSpeed {
        /// The family.
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
    /// A build whose backend no build of the engine has (its id is not one of the backends the engine knows).
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
    /// Two files of one build with this `url` but not the same `sha256`, `bytes` or `mutable`: keys that name parts of
    /// one archive must repeat what it is, since it is downloaded and unpacked once.
    InconsistentDownload {
        /// The build.
        build: String,
        /// The key of the file that disagrees with an earlier one.
        key: String,
    },
    /// A build's `call_params` naming an argument the interface's calls do not have, or one with no config path.
    UnknownCallArgument {
        /// The build.
        build: String,
        /// The argument as written.
        argument: String,
    },
    /// A language of a model or of one of its voices that is not a BCP 47 tag ("multi", "spanish", "es_ES").
    InvalidLanguage {
        /// The model.
        model: String,
        /// The tag as written.
        tag: String,
    },
    /// Two voices of one model with this id.
    DuplicateVoice {
        /// The model.
        model: String,
        /// The repeated id.
        voice: String,
    },
}

impl MergedCatalog {
    /// Every source's families, in the order of `sources`.
    pub(crate) fn merge(sources: &[Box<dyn CatalogSource>]) -> Result<Self> {
        let mut families = Vec::new();
        for source in sources {
            families.extend(source.load()?.families);
        }
        Ok(Self { families })
    }

    /// Every family, with its models, in catalogue order.
    pub(crate) fn families(&self) -> &[Family] {
        &self.families
    }

    /// Every model, in catalogue order.
    pub(crate) fn entries(&self) -> impl Iterator<Item = &ModelEntry> {
        self.families.iter().flat_map(|family| &family.models)
    }

    /// The models that can do `capability`, in catalogue order.
    pub(crate) fn models(&self, capability: Capability) -> impl Iterator<Item = &ModelEntry> {
        self.families
            .iter()
            .flat_map(|family| &family.models)
            .filter(move |model| model.capabilities.contains(&capability))
    }

    /// What is wrong with the merged catalogue, given which backend ids are `known`; empty if nothing is.
    #[must_use]
    pub(crate) fn check(&self, known: &dyn Fn(&str) -> bool) -> Vec<Problem> {
        let mut problems = Vec::new();
        let (mut families, mut models, mut builds) =
            (HashSet::new(), HashSet::new(), HashSet::new());
        for family in &self.families {
            if !families.insert(&family.id) {
                problems.push(Problem::DuplicateFamily {
                    family: family.id.clone(),
                });
            }
            if family.speed.as_ref().is_some_and(|speed| !speed.is_valid()) {
                problems.push(Problem::InvalidSpeed {
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
                check_languages(model, &mut problems);
                for build in &model.builds {
                    if !builds.insert(&build.id) {
                        problems.push(Problem::DuplicateBuild {
                            build: build.id.clone(),
                        });
                    }
                    check_build(build, known, &mut problems);
                }
            }
        }
        problems
    }
}

/// A build's own problems: its backend and its files.
fn check_build(build: &BuildEntry, known: &dyn Fn(&str) -> bool, problems: &mut Vec<Problem>) {
    if !known(&build.backend) {
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
    for (argument, paths) in &build.call_params {
        if !CALL_ARGUMENTS.contains(&argument.as_str()) || paths.is_empty() {
            problems.push(Problem::UnknownCallArgument {
                build: build.id.clone(),
                argument: argument.clone(),
            });
        }
    }
    let mut keys = HashSet::new();
    let mut downloads = HashMap::new();
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
        let download = (&file.sha256, file.bytes, file.mutable);
        if *downloads.entry(&file.url).or_insert(download) != download {
            problems.push(Problem::InconsistentDownload {
                build: build.id.clone(),
                key: file.key.clone(),
            });
        }
    }
}

/// A model's languages, and its voices' ids and languages.
fn check_languages(model: &ModelEntry, problems: &mut Vec<Problem>) {
    let voices = model.voices.iter().flat_map(|voice| &voice.languages);
    for tag in model.languages.iter().chain(voices) {
        if !is_bcp47(tag) {
            problems.push(Problem::InvalidLanguage {
                model: model.id.clone(),
                tag: tag.clone(),
            });
        }
    }
    let mut ids = HashSet::new();
    for voice in &model.voices {
        if !ids.insert(&voice.id) {
            problems.push(Problem::DuplicateVoice {
                model: model.id.clone(),
                voice: voice.id.clone(),
            });
        }
    }
}

/// Whether `tag` has the shape of a BCP 47 language tag: a primary language subtag of two or three lowercase letters,
/// then subtags of two to eight letters or digits ("es", "en-US", "pt-BR", "yue", "zh-Hant").
fn is_bcp47(tag: &str) -> bool {
    let mut subtags = tag.split('-');
    let primary = subtags.next().unwrap_or_default();
    (2..=3).contains(&primary.len())
        && primary.bytes().all(|byte| byte.is_ascii_lowercase())
        && subtags.all(|subtag| {
            (2..=8).contains(&subtag.len())
                && subtag.bytes().all(|byte| byte.is_ascii_alphanumeric())
        })
}
