//! The engine of one place: its host, its catalogue and the backends compiled into it, and the steps from a task to
//! a loaded model: offers (resolver.rs), the choice per stage, installing (install.rs), loading, and unloading what
//! goes unused.
//!
//! Inside: `selection` (what the person asks for and what is chosen), `error` (why an engine cannot be built),
//! `lifecycle` (a build's state) and `memory` (what is loaded, and when it is unloaded).

use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use crate::backend::{self, Backend, BackendId};
use crate::catalog::{Build, Catalog, CatalogSource, Task};
use crate::host::{Host, Platform};
use crate::install::{Artifact, Cancel, Installer, ProgressSink};
use crate::resolver::{Offer, Resolver};
use crate::{Error, Result};

mod error;
mod lifecycle;
mod memory;
mod selection;
#[cfg(test)]
mod tests;

pub use error::ConfigError;
pub use lifecycle::BuildState;
pub use selection::{Preferences, Selection};

use memory::Memory;

/// How long a model may go unused before it is unloaded from memory, unless [`Engine::with_idle_unload`] says
/// otherwise.
pub const DEFAULT_IDLE_UNLOAD: Duration = Duration::from_secs(10 * 60);

/// The library files a backend needs on a platform (`backends.json`, or a test's own).
type RuntimeFiles = fn(&str, Platform) -> Option<Vec<Artifact>>;

/// The engine of one place: what can run here, the choice per stage, and the models prepared.
///
/// A prepared model stays in memory while it is used; one unused for the idle time ([`DEFAULT_IDLE_UNLOAD`], or
/// [`Engine::with_idle_unload`]) is unloaded, and its build is [`BuildState::Installed`] again. The engine has no
/// timer of its own: it unloads idle models on each [`Engine::prepare`], and when the app calls
/// [`Engine::unload_idle`], which it should do on a schedule of its own (once a minute is plenty).
pub struct Engine {
    host: Box<dyn Host>,
    catalog: Catalog,
    backends: Vec<Box<dyn Backend>>,
    runtime_files: RuntimeFiles,
    resolver: Resolver,
    installer: Installer,
    /// Builds being installed or loaded, and those whose last attempt failed; any other build's state is read from
    /// memory and storage.
    states: Mutex<BTreeMap<String, BuildState>>,
    memory: Mutex<Memory>,
}

impl fmt::Debug for Engine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Engine")
            .field("catalog", &self.catalog)
            .field("backends", &self.backends())
            .finish_non_exhaustive()
    }
}

/// A prepared model. Once the model is unloaded for going unused, preparing its build again gives a new handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Handle(usize);

impl Engine {
    /// Builds nothing heavy: the backends are empty objects until [`Engine::prepare`].
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
        let problems = catalog.check();
        if !problems.is_empty() {
            return Err(ConfigError::Catalog(problems));
        }
        Ok(Self {
            host,
            catalog,
            backends,
            runtime_files,
            resolver: Resolver::default(),
            installer: Installer,
            states: Mutex::default(),
            memory: Mutex::new(Memory::new(DEFAULT_IDLE_UNLOAD)),
        })
    }

    /// The same engine, unloading models unused for `idle` instead of [`DEFAULT_IDLE_UNLOAD`].
    #[must_use]
    pub fn with_idle_unload(self, idle: Duration) -> Self {
        lock(&self.memory).set_idle(idle);
        self
    }

    /// The ids of the backends compiled into this build.
    #[must_use]
    pub fn backends(&self) -> Vec<BackendId> {
        self.backends
            .iter()
            .map(|backend| backend.spec().id)
            .collect()
    }

    /// Every model of `task` that can run here with its best build, and every build that cannot, with why.
    #[must_use]
    pub fn offers(&self, task: Task) -> Vec<Offer> {
        self.resolver.offers(
            &self.catalog,
            &self.backends,
            &self.host.capabilities(),
            task,
        )
    }

    /// The choice for `task`: the best offer, or the model asked for. Backend and accelerator preferences are not
    /// applied yet.
    #[must_use]
    pub fn select(&self, task: Task, preferences: &Preferences) -> Option<Selection> {
        self.offers(task).into_iter().find_map(|offer| match offer {
            Offer::Offered {
                model,
                build,
                accelerator,
                ..
            } if preferences
                .model
                .as_ref()
                .is_none_or(|wanted| *wanted == model.id) =>
            {
                Some(Selection {
                    model,
                    build,
                    accelerator,
                })
            }
            _ => None,
        })
    }

    /// Where `build` is now: being installed or loaded, failed, loaded, or else whether all its files are stored.
    ///
    /// # Errors
    ///
    /// `backend-not-in-this-build`, `no-runtime-for-platform` (as [`Engine::prepare`]), and whatever the host's storage
    /// fails with.
    pub async fn state(&self, build: &Build) -> Result<BuildState> {
        if lock(&self.memory).is_loaded(&build.id) {
            return Ok(BuildState::Ready);
        }
        if let Some(state) = lock(&self.states).get(&build.id) {
            return Ok(state.clone());
        }
        let (_, artifacts) = self.artifacts(build)?;
        for artifact in &artifacts {
            if self.host.storage().find(&artifact.sha256).await?.is_none() {
                return Ok(BuildState::Absent);
            }
        }
        Ok(BuildState::Installed)
    }

    /// Installs the selected build and loads it: only that backend is ever activated. The installer gets the model's
    /// files and the backend's files for this platform (`backends.json`), and nothing else; it tells `progress` how
    /// far it has got, and stops once `cancel` is cancelled. The backend's library is opened with the first model of
    /// that backend in memory. A build already loaded is not loaded again: its handle is returned.
    ///
    /// The build goes [`BuildState::Installing`], then [`BuildState::Loading`], then [`BuildState::Ready`]; if that
    /// fails, [`BuildState::Failed`] with the error. Cancelled, or with the future dropped, it is left as storage
    /// has it.
    ///
    /// # Errors
    ///
    /// `backend-not-in-this-build` if the selection's backend is not compiled in, `no-runtime-for-platform` if it has
    /// nothing to download for this platform, `already-preparing` if the build is being prepared already, `cancelled`,
    /// whatever installing fails with (`digest-mismatch`, `download-failed`, ...; see [`Cancel`] and
    /// [`ProgressSink`]), and whatever loading fails with (today, `not-implemented`).
    pub async fn prepare(
        &self,
        selection: &Selection,
        progress: &dyn ProgressSink,
        cancel: &Cancel,
    ) -> Result<Handle> {
        self.unload_idle();
        let build = &selection.build;
        let (backend, artifacts) = self.artifacts(build)?;
        if let Some(handle) = lock(&self.memory).handle(&build.id) {
            return Ok(handle);
        }
        let preparing = Preparing::start(&self.states, &build.id)?;
        let result: Result<Handle> = async {
            let files = self
                .installer
                .install(&artifacts, self.host.as_ref(), progress, cancel)
                .await?;
            cancel.check()?;
            preparing.set(BuildState::Loading);
            let open = lock(&self.memory).library(backend.spec().id);
            let library = match open {
                Some(library) => library,
                None => Arc::from(backend.open(&files).await?),
            };
            let model = library.load(build, selection.accelerator, &files).await?;
            Ok(lock(&self.memory).insert(&build.id, backend.spec().id, library, model))
        }
        .await;
        drop(preparing);
        if let Err(error) = result {
            if error.code != "cancelled" {
                lock(&self.states).insert(build.id.clone(), BuildState::Failed(error));
            }
        }
        result
    }

    /// Unloads from memory every model unused for the idle time, and closes the libraries of backends with no model
    /// left. Their files stay installed.
    pub fn unload_idle(&self) {
        lock(&self.memory).unload_idle();
    }

    /// `build`'s backend, and every file it needs here: the model's, then the backend's for this platform.
    fn artifacts(&self, build: &Build) -> Result<(&dyn Backend, Vec<Artifact>)> {
        let backend = backend::find(&self.backends, &build.backend)
            .ok_or(Error::new("backend-not-in-this-build"))?;
        let runtime = Platform::of(&self.host.capabilities())
            .and_then(|platform| (self.runtime_files)(backend.spec().id, platform))
            .ok_or(Error::new("no-runtime-for-platform"))?;
        let mut artifacts = build.files.clone();
        artifacts.extend(runtime);
        Ok((backend, artifacts))
    }
}

/// A build being prepared: while it lives, the build's state is `Installing` or `Loading`; dropped, by finishing or
/// by the future being dropped, the build is left to memory and storage again.
struct Preparing<'a> {
    states: &'a Mutex<BTreeMap<String, BuildState>>,
    build: &'a str,
}

impl<'a> Preparing<'a> {
    /// Marks `build` as `Installing`, unless it is being prepared already (`already-preparing`).
    fn start(states: &'a Mutex<BTreeMap<String, BuildState>>, build: &'a str) -> Result<Self> {
        let mut known = lock(states);
        if matches!(
            known.get(build),
            Some(BuildState::Installing | BuildState::Loading)
        ) {
            return Err(Error::new("already-preparing"));
        }
        known.insert(build.to_owned(), BuildState::Installing);
        Ok(Self { states, build })
    }

    fn set(&self, state: BuildState) {
        lock(self.states).insert(self.build.to_owned(), state);
    }
}

impl Drop for Preparing<'_> {
    fn drop(&mut self) {
        lock(self.states).remove(self.build);
    }
}

/// A poisoned lock still holds whole values: nothing here is left half done by a panic.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
