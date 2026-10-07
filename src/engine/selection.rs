//! What is chosen for a stage: what the person asked for ([`Preferences`]) and what the engine picked ([`Selection`]).

use crate::catalog::{Build, Model};
use crate::host::Accelerator;

/// What the person asked for in advanced options; `None` leaves it to the engine.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Preferences {
    /// A model id.
    pub model: Option<String>,
    /// A backend id, as [`Engine::backends`](crate::Engine::backends) lists them.
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
