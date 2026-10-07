//! What the engine holds in memory: each loaded model with the library of its backend it was loaded with, and when it
//! was last used. A model unused for the idle time is unloaded; a backend's library is shared by its models (counted
//! by `Arc`) and closed when the last of them is unloaded. Unloading frees memory only: the files stay installed.
//!
//! There is no timer here (the engine has no runtime of its own): idle models are unloaded when [`Memory::unload_idle`]
//! is called, which the engine does on each `prepare` and the app on its own schedule (`Engine::unload_idle`).

use std::collections::BTreeMap;
use std::sync::{Arc, Weak};
use std::time::Duration;

use crate::backend::{Library, LoadedModel};
use crate::engine::Handle;
use crate::{Error, Result};

#[cfg(test)]
mod tests;

/// The loaded models and their backends' libraries.
pub(super) struct Memory {
    models: BTreeMap<Handle, Resident>,
    /// Each backend's open library, while some model holds it.
    libraries: BTreeMap<String, Weak<dyn Library>>,
    next: usize,
    idle: Duration,
    clock: fn() -> Duration,
}

/// A loaded model. Fields drop in order: the model before its library.
struct Resident {
    build: String,
    /// `None` while it is taken out to be used.
    model: Option<Box<dyn LoadedModel>>,
    #[allow(
        dead_code,
        reason = "held: it keeps the library open while the model is loaded"
    )]
    library: Arc<dyn Library>,
    used: Duration,
}

impl Memory {
    /// Nothing loaded; models unused for `idle` are unloaded.
    pub(super) fn new(idle: Duration) -> Self {
        Self::with_clock(idle, now)
    }

    /// Nothing loaded, on `clock` (a monotonic time from any origin).
    pub(super) fn with_clock(idle: Duration, clock: fn() -> Duration) -> Self {
        Self {
            models: BTreeMap::new(),
            libraries: BTreeMap::new(),
            next: 0,
            idle,
            clock,
        }
    }

    /// How long a model may go unused before it is unloaded.
    pub(super) fn set_idle(&mut self, idle: Duration) {
        self.idle = idle;
    }

    /// The handle of `build`, if it is loaded, which counts as using it.
    pub(super) fn handle(&mut self, build: &str) -> Option<Handle> {
        let now = (self.clock)();
        let (handle, resident) = self
            .models
            .iter_mut()
            .find(|(_, resident)| resident.build == build)?;
        resident.used = now;
        Some(*handle)
    }

    /// Whether `build` is loaded.
    pub(super) fn is_loaded(&self, build: &str) -> bool {
        self.models.values().any(|resident| resident.build == build)
    }

    /// `backend`'s library, if it is open: some model loaded with it is still in memory.
    pub(super) fn library(&self, backend: &str) -> Option<Arc<dyn Library>> {
        self.libraries.get(backend).and_then(Weak::upgrade)
    }

    /// Keeps `model`, of `build`, loaded with `library`, `backend`'s; it holds the library open until it is unloaded.
    pub(super) fn insert(
        &mut self,
        build: &str,
        backend: &str,
        library: Arc<dyn Library>,
        model: Box<dyn LoadedModel>,
    ) -> Handle {
        let handle = Handle(self.next);
        self.next += 1;
        self.libraries
            .insert(backend.to_owned(), Arc::downgrade(&library));
        let resident = Resident {
            build: build.to_owned(),
            model: Some(model),
            library,
            used: (self.clock)(),
        };
        self.models.insert(handle, resident);
        handle
    }

    /// Takes `handle`'s model out to be used, which counts as using it, until [`Memory::put_back`]: `model-not-loaded`
    /// if it is not in memory (it was unloaded, or never loaded here), `model-busy` if it is out already.
    pub(super) fn take(&mut self, handle: Handle) -> Result<Box<dyn LoadedModel>> {
        let now = (self.clock)();
        let resident = self
            .models
            .get_mut(&handle)
            .ok_or(Error::new("model-not-loaded"))?;
        resident.used = now;
        resident.model.take().ok_or(Error::new("model-busy"))
    }

    /// Returns `handle`'s model, taken out by [`Memory::take`]: it was in use until now.
    pub(super) fn put_back(&mut self, handle: Handle, model: Box<dyn LoadedModel>) {
        let now = (self.clock)();
        if let Some(resident) = self.models.get_mut(&handle) {
            resident.used = now;
            resident.model = Some(model);
        }
    }

    /// Unloads every model unused for the idle time, and closes the libraries no model holds any more. A model taken
    /// out is in use, and stays.
    pub(super) fn unload_idle(&mut self) {
        let now = (self.clock)();
        let idle = self.idle;
        self.models.retain(|_, resident| {
            resident.model.is_none() || now.saturating_sub(resident.used) < idle
        });
        self.libraries
            .retain(|_, library| library.strong_count() > 0);
    }

    /// How many libraries are open.
    #[cfg(test)]
    pub(super) fn open_libraries(&self) -> usize {
        self.libraries
            .values()
            .filter(|library| library.strong_count() > 0)
            .count()
    }
}

/// Monotonic time since the first time it was asked.
#[cfg(native)]
fn now() -> Duration {
    use std::sync::OnceLock;
    use std::time::Instant;

    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed()
}

/// The page's clock (`Instant` panics on wasm32). It is wall time: when the clock is set, an idle model may be
/// unloaded a little early or late.
#[cfg(web)]
fn now() -> Duration {
    Duration::from_secs_f64((js_sys::Date::now() / 1000.0).max(0.0))
}
