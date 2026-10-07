//! The engine of one place: its host, its catalogue and the backends compiled into it, and the steps from a task to
//! a loaded model: offers (resolver.rs), the choice per stage, installing (install.rs) and loading.
//!
//! Inside: `selection` (what the person asks for and what is chosen), `error` (why an engine cannot be built) and
//! `lifecycle` (a build's state).

use std::fmt;
use std::sync::{Mutex, PoisonError};

use crate::backend::{self, Backend, BackendId, LoadedModel};
use crate::catalog::{Catalog, CatalogSource, Task};
use crate::host::{Host, Platform};
use crate::install::Installer;
use crate::resolver::{Offer, Resolver};
use crate::{Error, Result};

mod error;
mod lifecycle;
mod selection;
#[cfg(test)]
mod tests;

pub use error::ConfigError;
pub use selection::{Preferences, Selection};

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
            .and_then(|platform| backend::runtime_files(backend.spec().id, platform))
            .ok_or(Error::new("no-runtime-for-platform"))?;
        let mut artifacts = selection.build.files.clone();
        artifacts.extend(runtime);
        let files = self
            .installer
            .install(&artifacts, self.host.as_ref())
            .await?;
        let model = backend
            .load(&selection.build, selection.accelerator, &files)
            .await?;
        // A poisoned list still holds whole models: a panic cannot leave a push half done.
        let mut loaded = self.loaded.lock().unwrap_or_else(PoisonError::into_inner);
        loaded.push(model);
        Ok(Handle(loaded.len() - 1))
    }
}
