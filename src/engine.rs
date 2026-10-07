//! The engine of one place: its host, its catalogue and the backends compiled into it, and the steps from a task to
//! a loaded model: offers (resolver.rs), the choice per stage, installing (install.rs) and loading.

use std::fmt;
use std::sync::{Mutex, PoisonError};

use crate::backend::{self, Backend, BackendId, LoadedModel, Platform};
use crate::catalog::{Build, Catalog, CatalogSource, Model, Problem, Task};
use crate::host::{Accelerator, Host};
use crate::install::Installer;
use crate::resolver::{Offer, Resolver};
use crate::{Error, Result};

mod lifecycle;
#[cfg(test)]
mod tests;

/// The engine of one place: what can run here, the choice per stage, and the models prepared.
pub struct Engine {
    host: Box<dyn Host>,
    catalog: Catalog,
    backends: Vec<Box<dyn Backend>>,
    resolver: Resolver,
    installer: Installer,
    loaded: Mutex<Vec<Box<dyn LoadedModel>>>,
}

impl fmt::Debug for Engine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Engine")
            .field("catalog", &self.catalog)
            .field("backends", &self.backends())
            .finish_non_exhaustive()
    }
}

/// Why an engine cannot be built.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConfigError {
    /// A catalogue source failed to load.
    Source(Error),
    /// The merged catalogue is inconsistent.
    Catalog(Vec<Problem>),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Stable codes, like `Error`'s: the source's own code is its `source()`.
        f.write_str(match self {
            Self::Source(_) => "catalog-source-failed",
            Self::Catalog(_) => "catalog-inconsistent",
        })
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Source(error) => Some(error),
            Self::Catalog(_) => None,
        }
    }
}

/// What the person asked for in advanced options; `None` leaves it to the engine.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Preferences {
    /// A model id.
    pub model: Option<String>,
    /// A backend id, as [`Engine::backends`] lists them.
    pub backend: Option<String>,
    /// An accelerator.
    pub accelerator: Option<Accelerator>,
}

/// The model, build and accelerator chosen for a stage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    /// The model chosen.
    pub model: Model,
    /// Its build to run.
    pub build: Build,
    /// The accelerator to run it on.
    pub accelerator: Accelerator,
}

/// A prepared model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
        let catalog = Catalog::merge(&sources).map_err(ConfigError::Source)?;
        let problems = catalog.check();
        if !problems.is_empty() {
            return Err(ConfigError::Catalog(problems));
        }
        Ok(Self {
            host,
            catalog,
            backends: backend::built_in(),
            resolver: Resolver::default(),
            installer: Installer,
            loaded: Mutex::default(),
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

    /// Installs the selected build and loads it: only that backend is ever activated. The installer gets the model's
    /// files and the backend's files for this platform (`backends.json`), and nothing else.
    ///
    /// # Errors
    ///
    /// `backend-not-in-this-build` if the selection's backend is not compiled in, `no-runtime-for-platform` if it has
    /// nothing to download for this platform, and whatever installing or loading fails with (today, `not-implemented`).
    pub async fn prepare(&self, selection: &Selection) -> Result<Handle> {
        let backend = backend::find(&self.backends, &selection.build.backend)
            .ok_or(Error::new("backend-not-in-this-build"))?;
        let runtime = Platform::of(&self.host.capabilities())
            .and_then(|platform| backend::downloads(backend.spec().id, platform))
            .ok_or(Error::new("no-runtime-for-platform"))?;
        // A model file is found by its key; a backend file, by its name in backends.json.
        let wanted: Vec<_> = selection
            .build
            .files
            .iter()
            .map(|artifact| (artifact.key.clone(), artifact.clone()))
            .chain(runtime)
            .collect();
        let files = self.installer.install(&wanted, self.host.as_ref()).await?;
        let model = backend
            .load(&selection.build, selection.accelerator, &files)
            .await?;
        // A poisoned list still holds whole models: a panic cannot leave a push half done.
        let mut loaded = self.loaded.lock().unwrap_or_else(PoisonError::into_inner);
        loaded.push(model);
        Ok(Handle(loaded.len() - 1))
    }
}
