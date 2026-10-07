//! The engine of one place: its host, its catalogue and the backends compiled into it, and the steps from a task to
//! a loaded model: offers (resolver.rs), the choice per stage, installing (install.rs) and loading.

use std::sync::Mutex;

use crate::backend::{Backend, BackendId, LoadedModel};
use crate::catalog::{Build, Catalog, CatalogSource, Model, Problem, Task};
use crate::host::{Accelerator, Host};
use crate::install::Installer;
use crate::registry::built_in;
use crate::resolver::{Offer, Resolver};
use crate::{Error, Result};

pub struct Engine {
    host: Box<dyn Host>,
    catalog: Catalog,
    backends: Vec<Box<dyn Backend>>,
    resolver: Resolver,
    installer: Installer,
    loaded: Mutex<Vec<Box<dyn LoadedModel>>>,
}

/// Why an engine cannot be built.
#[derive(Debug, Clone, PartialEq)]
pub enum ConfigError {
    /// A catalogue source failed to load.
    Source(Error),
    /// The merged catalogue is inconsistent.
    Catalog(Vec<Problem>),
}

/// What the person asked for in advanced options; `None` leaves it to the engine.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Preferences {
    pub model: Option<String>,
    pub backend: Option<BackendId>,
    pub accelerator: Option<Accelerator>,
}

/// The model, build and accelerator chosen for a stage.
#[derive(Debug, Clone, PartialEq)]
pub struct Selection {
    pub model: Model,
    pub build: Build,
    pub accelerator: Accelerator,
}

/// A prepared model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Handle(usize);

impl Engine {
    /// Builds nothing heavy: the backends are empty objects until [`Engine::prepare`].
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
            backends: built_in(),
            resolver: Resolver::default(),
            installer: Installer,
            loaded: Mutex::default(),
        })
    }

    /// The ids of the backends compiled into this build.
    pub fn backends(&self) -> Vec<BackendId> {
        self.backends
            .iter()
            .map(|backend| backend.spec().id)
            .collect()
    }

    /// Every model of `task` that can run here with its best build, and every build that cannot, with why.
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

    /// Installs the selected build and loads it: only that backend is ever activated. Today the installer gets the
    /// model's files; the backend's own library files come from the backends' data file once it exists (#3).
    pub async fn prepare(&self, selection: &Selection) -> Result<Handle> {
        let backend = self
            .backends
            .iter()
            .find(|backend| backend.spec().id == selection.build.backend)
            .ok_or(Error::new("backend-not-in-this-build"))?;
        let files = self
            .installer
            .install(&selection.build.files, self.host.as_ref())
            .await?;
        let model = backend
            .load(&selection.build, selection.accelerator, &files)
            .await?;
        let mut loaded = self.loaded.lock().expect("loaded models lock");
        loaded.push(model);
        Ok(Handle(loaded.len() - 1))
    }
}
