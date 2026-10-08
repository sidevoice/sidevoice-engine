//! The engine of one place: its host, its catalogue and the backends compiled into it, and what an app does with a
//! model: list them ([`Engine::models`]), install and uninstall one, and load one ([`Engine::load`]), which returns a
//! [`LoadedModel`] that transcribes or speaks and is unloaded when dropped.
//!
//! Inside: `model` (a model as [`Engine::models`] lists it), `loaded` (a model in memory: [`LoadedModel`], [`Stt`],
//! [`Tts`]), `audio` ([`Audio`], and resampling), `memory` (weak references: one library per backend, one model per
//! build) and `error` (why an engine cannot be built).

use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::backend::{self, Backend, BackendId};
use crate::catalog::{BuildEntry, Catalog, CatalogSource, ModelEntry, ModelFile};
use crate::host::{Accelerator, Host, Platform};
use crate::install::{Artifact, Cancel, Installer, ProgressSink};
use crate::resolver::{Reason, Rejection, Resolver, RuntimeFiles};
use crate::{Error, Result};

mod audio;
mod error;
pub(super) mod loaded;
mod memory;
mod model;
#[cfg(test)]
mod tests;

pub use audio::Audio;
pub use error::ConfigError;
pub use loaded::{LoadedModel, Stt, Tts};
pub use model::{Model, ModelBuild};

use loaded::Resident;
use memory::Memory;

/// The engine of one place: what can run here, what is installed, and what is loaded.
///
/// It holds no model itself: a [`LoadedModel`] does, and the model stays in memory while one of its build lives.
/// Loading a build that is already in memory returns it again, and a backend's models share its library.
pub struct Engine {
    host: Box<dyn Host>,
    catalog: Catalog,
    backends: Vec<Box<dyn Backend>>,
    runtime_files: RuntimeFiles,
    resolver: Resolver,
    installer: Installer,
    memory: Mutex<Memory>,
    /// Held while a model is loaded, so that two loads of one build make one model in memory.
    loading: async_lock::Mutex<()>,
}

impl fmt::Debug for Engine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Engine")
            .field("catalog", &self.catalog)
            .field("backends", &self.backends())
            .finish_non_exhaustive()
    }
}

impl Engine {
    /// Builds nothing heavy: the backends are empty objects until a model is loaded. The catalogue is the merge of
    /// `sources`, in order; the one this repository ships, [`BundledCatalog`](crate::BundledCatalog), is one of them
    /// only when passed.
    ///
    /// # Errors
    ///
    /// [`ConfigError::Source`] if a catalogue source fails to load, [`ConfigError::Catalog`] if the merged catalogue
    /// is inconsistent.
    pub fn new(
        host: Box<dyn Host>,
        sources: Vec<Box<dyn CatalogSource>>,
    ) -> Result<Self, ConfigError> {
        Self::with_backends(host, sources, backend::built_in(), backend::runtime_files)
    }

    /// An engine with these backends, whose library files are `runtime_files`'s to say.
    pub(crate) fn with_backends(
        host: Box<dyn Host>,
        sources: Vec<Box<dyn CatalogSource>>,
        backends: Vec<Box<dyn Backend>>,
        runtime_files: RuntimeFiles,
    ) -> Result<Self, ConfigError> {
        let catalog = Catalog::merge(&sources).map_err(ConfigError::Source)?;
        let compiled: Vec<_> = backends.iter().map(|backend| backend.spec().id).collect();
        let problems = catalog.check(&|id| backend::is_known(id) || compiled.contains(&id));
        if !problems.is_empty() {
            return Err(ConfigError::Catalog(problems));
        }
        Ok(Self {
            host,
            catalog,
            backends,
            runtime_files,
            resolver: Resolver::new(runtime_files),
            installer: Installer,
            memory: Mutex::default(),
            loading: async_lock::Mutex::new(()),
        })
    }

    /// The ids of the backends compiled into this build.
    #[must_use]
    pub fn backends(&self) -> Vec<BackendId> {
        self.backends
            .iter()
            .map(|backend| backend.spec().id)
            .collect()
    }

    /// Every model of the catalogue, in catalogue order, each with its builds ranked (those that run here first, with
    /// the accelerator each would use; the rest with why not), whether it is installed, and the build the engine
    /// recommends: the first that runs here.
    ///
    /// # Errors
    ///
    /// What the host's storage fails with, asked what is installed.
    pub async fn models(&self) -> Result<Vec<Model>> {
        let caps = self.host.capabilities();
        let mut models = Vec::new();
        let mut entries = Vec::new();
        for family in self.catalog.families() {
            entries.extend(family.models.iter().map(|entry| (&family.id, entry)));
        }
        for (family, entry) in entries {
            let mut builds = Vec::new();
            for (build, fit) in self.resolver.builds(entry, &self.backends, &caps) {
                let installed = self.is_installed(build).await?;
                builds.push(ModelBuild {
                    id: build.id.clone(),
                    backend: build.backend.clone(),
                    accelerator: fit.as_ref().ok().copied(),
                    precision: build.precision.clone(),
                    download_bytes: download_bytes(build),
                    memory_mb: build.memory.mb,
                    available: fit.is_ok(),
                    reasons: fit.err().map(reason).into_iter().collect(),
                    installed,
                });
            }
            models.push(Model {
                id: entry.id.clone(),
                family: family.clone(),
                capabilities: entry.capabilities.clone(),
                parameters_m: entry.parameters_m,
                languages: entry.languages.clone(),
                license: entry.license.clone(),
                voices: entry.voices.clone(),
                installed: builds.iter().any(|build| build.installed),
                recommended_build: builds
                    .iter()
                    .find(|build| build.available)
                    .map(|build| build.id.clone()),
                builds,
            });
        }
        Ok(models)
    }

    /// Installs `build` of the model `model`, or, with `None`, the build [`Engine::load`] would use: its files and its
    /// backend's for this platform, nothing else. It tells `progress` how far it has got, and stops once `cancel` is
    /// cancelled; cancelled, or with the future dropped, nothing half downloaded is stored.
    ///
    /// # Errors
    ///
    /// `model-not-found`, `build-not-found` (not a build of that model), `no-build-available` (none runs here), the
    /// reason a build asked for does not run here (`backend-not-in-this-build`, `memory`, ...), `cancelled`, and what
    /// installing fails with (`digest-mismatch`, `download-failed`, ...).
    pub async fn install(
        &self,
        model: &str,
        build: Option<&str>,
        progress: &dyn ProgressSink,
        cancel: &Cancel,
    ) -> Result<()> {
        let (_, build, _) = self.choose(model, build).await?;
        let (_, artifacts) = self.artifacts(build)?;
        self.installer
            .install(&build.id, &artifacts, self.host.as_ref(), progress, cancel)
            .await
            .map(drop)
    }

    /// Removes every build of the model `model` from storage: each build's folder, then each of its files no other
    /// build's folder links, so a file another model also uses stays.
    ///
    /// # Errors
    ///
    /// `model-not-found`, `model-in-use` while a [`LoadedModel`] of it lives, and what the host's storage fails with.
    pub async fn uninstall(&self, model: &str) -> Result<()> {
        let entry = self.entry(model)?;
        let in_use = {
            let memory = lock(&self.memory);
            entry
                .builds
                .iter()
                .any(|build| memory.model(&build.id).is_some())
        };
        if in_use {
            return Err(Error::new("model-in-use"));
        }
        for build in &entry.builds {
            let artifacts = match self.artifacts(build) {
                Ok((_, artifacts)) => artifacts,
                Err(_) => build.files.iter().map(ModelFile::artifact).collect(),
            };
            self.installer
                .uninstall(&build.id, &artifacts, self.host.storage())
                .await?;
        }
        Ok(())
    }

    /// Whether `build`'s folder is stored, which it only is with every file in it.
    async fn is_installed(&self, build: &BuildEntry) -> Result<bool> {
        Ok(self.host.storage().find_folder(&build.id).await?.is_some())
    }

    /// Loads `build` of the model `model`, or, with `None`, an installed build that runs here, else the recommended
    /// one; it is installed first if it is not (telling `progress`, stopping once `cancel` is cancelled). A build
    /// already in memory is not loaded again: the [`LoadedModel`] returned shares it. Only the build's backend is ever
    /// activated, and its library is opened with the first of its models.
    ///
    /// # Errors
    ///
    /// What [`Engine::install`] fails with, and what loading fails with (`model-load-failed`, `file-not-installed`,
    /// `unsupported-model`, `not-implemented`, ...).
    pub async fn load(
        &self,
        model: &str,
        build: Option<&str>,
        progress: &dyn ProgressSink,
        cancel: &Cancel,
    ) -> Result<LoadedModel> {
        let (entry, build, accelerator) = self.choose(model, build).await?;
        if let Some(resident) = lock(&self.memory).model(&build.id) {
            return Ok(LoadedModel::new(&entry.id, &build.id, resident));
        }
        let (backend, artifacts) = self.artifacts(build)?;
        let files = self
            .installer
            .install(&build.id, &artifacts, self.host.as_ref(), progress, cancel)
            .await?;
        cancel.check()?;
        let _loading = self.loading.lock().await;
        // Another load of this build may have finished while this one waited.
        if let Some(resident) = lock(&self.memory).model(&build.id) {
            return Ok(LoadedModel::new(&entry.id, &build.id, resident));
        }
        let id = backend.spec().id;
        let open = lock(&self.memory).library(id);
        let library = match open {
            Some(library) => library,
            None => Arc::from(backend.open(&files).await?),
        };
        let model = library.load(build, accelerator, &files).await?;
        let languages = entry.languages.clone();
        let resident = Resident::new(model, Arc::clone(&library), languages, entry.voices.clone());
        lock(&self.memory).remember(id, &library, &build.id, &resident);
        Ok(LoadedModel::new(&entry.id, &build.id, resident))
    }

    /// Every model of the catalogue, in catalogue order.
    fn entries(&self) -> impl Iterator<Item = &ModelEntry> {
        self.catalog.entries()
    }

    /// The model `model` (`model-not-found` otherwise).
    fn entry(&self, model: &str) -> Result<&ModelEntry> {
        self.entries()
            .find(|entry| entry.id == model)
            .ok_or(Error::new("model-not-found"))
    }

    /// The build to install or load and the accelerator it runs on: `build` if it is one of `model`'s and runs here,
    /// else the first build that runs here and is installed, else the first that runs here.
    async fn choose(
        &self,
        model: &str,
        build: Option<&str>,
    ) -> Result<(&ModelEntry, &BuildEntry, Accelerator)> {
        let entry = self.entry(model)?;
        let ranked = self
            .resolver
            .builds(entry, &self.backends, &self.host.capabilities());
        if let Some(wanted) = build {
            let (build, fit) = ranked
                .into_iter()
                .find(|(build, _)| build.id == wanted)
                .ok_or(Error::new("build-not-found"))?;
            let accelerator = fit.map_err(|why| Error::new(reason(why).code))?;
            return Ok((entry, build, accelerator));
        }
        let available: Vec<_> = ranked
            .into_iter()
            .filter_map(|(build, fit)| fit.ok().map(|accelerator| (build, accelerator)))
            .collect();
        for &(build, accelerator) in &available {
            if self.is_installed(build).await? {
                return Ok((entry, build, accelerator));
            }
        }
        let (build, accelerator) = available
            .first()
            .copied()
            .ok_or(Error::new("no-build-available"))?;
        Ok((entry, build, accelerator))
    }

    /// `build`'s backend, and every file it needs here: the model's, then the backend's for this platform.
    fn artifacts(&self, build: &BuildEntry) -> Result<(&dyn Backend, Vec<Artifact>)> {
        let backend = backend::find(&self.backends, &build.backend)
            .ok_or(Error::new("backend-not-in-this-build"))?;
        let runtime = Platform::of(&self.host.capabilities())
            .and_then(|platform| (self.runtime_files)(backend.spec().id, platform))
            .ok_or(Error::new("no-runtime-for-platform"))?;
        let mut artifacts: Vec<_> = build.files.iter().map(ModelFile::artifact).collect();
        artifacts.extend(runtime);
        Ok((backend, artifacts))
    }
}

/// What installing `build` downloads, in bytes: each distinct download once.
fn download_bytes(build: &BuildEntry) -> u64 {
    let mut seen = Vec::new();
    build
        .files
        .iter()
        .filter(|file| {
            let new = !seen.contains(&&file.url);
            seen.push(&file.url);
            new
        })
        .map(|file| file.bytes)
        .sum()
}

/// A rejection as the reason a person reads: its stable code and numbers.
fn reason(why: Rejection) -> Reason {
    match why {
        Rejection::BackendNotInThisBuild => Reason::new("backend-not-in-this-build"),
        Rejection::BackendUnavailable(reason) | Rejection::DoesNotFit(reason) => reason,
    }
}

/// A poisoned lock still holds whole values: nothing here is left half done by a panic.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
