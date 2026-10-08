//! What the engine knows of memory, without holding it: weak references to each backend's open library and to each
//! build's model in memory. The app's [`LoadedModel`](crate::LoadedModel)s hold them; the engine only finds them again,
//! so that a backend's models share one library and a build loaded twice is one model in memory. There is no clock and
//! no unloading here: what nothing holds is gone.

use std::collections::BTreeMap;
use std::sync::{Arc, Weak};

use super::loaded::Resident;
use crate::backend::{BackendId, Library};

#[cfg(test)]
mod tests;

/// Each backend's library and each build's model, while something holds them.
#[derive(Default)]
pub(super) struct Memory {
    libraries: BTreeMap<BackendId, Weak<dyn Library>>,
    models: BTreeMap<String, Weak<Resident>>,
}

impl Memory {
    /// `backend`'s library, if a model loaded with it is still in memory.
    pub(super) fn library(&self, backend: BackendId) -> Option<Arc<dyn Library>> {
        self.libraries.get(backend).and_then(Weak::upgrade)
    }

    /// `build`'s model, if it is in memory.
    pub(super) fn model(&self, build: &str) -> Option<Arc<Resident>> {
        self.models.get(build).and_then(Weak::upgrade)
    }

    /// Remembers `library`, `backend`'s, and `model`, `build`'s, and forgets what nothing holds any more.
    pub(super) fn remember(
        &mut self,
        backend: BackendId,
        library: &Arc<dyn Library>,
        build: &str,
        model: &Arc<Resident>,
    ) {
        self.libraries
            .retain(|_, library| library.strong_count() > 0);
        self.models.retain(|_, model| model.strong_count() > 0);
        self.libraries.insert(backend, Arc::downgrade(library));
        self.models.insert(build.to_owned(), Arc::downgrade(model));
    }
}
