//! The funnel, the same for every backend: for each build of the catalogue, is its backend compiled here, does it have
//! an entry for this platform in `backends.json`, which of its accelerators work here (what the host reports, narrowed
//! by `probe`, cached), does the machine meet the build's and the backend's requirements (a value the host cannot tell
//! passes). Then, per model, a build: the catalogue's builds carry no order, and ranking them is still to come
//! (sidevoice-engine#4); until then it is the first that fits, on its backend's preferred accelerator. Every rejected
//! build is kept with its reason.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use crate::backend::{self, Backend, BackendId, MinMemoryMb, Requirement};
use crate::catalog::{Build, Capability, Catalog, Model};
use crate::host::{Accelerator, Capabilities, Platform};

mod offer;
#[cfg(test)]
mod tests;

pub use offer::{Offer, Reason, Rejection};

/// Offers per capability, remembering each backend's probe.
#[derive(Debug, Default)]
pub(crate) struct Resolver {
    probes: Mutex<HashMap<BackendId, Vec<Accelerator>>>,
}

impl Resolver {
    /// Every model that can do `capability`, offered with a build that fits, and every build that cannot run here,
    /// with why.
    pub(crate) fn offers(
        &self,
        catalog: &Catalog,
        backends: &[Box<dyn Backend>],
        caps: &Capabilities,
        capability: Capability,
    ) -> Vec<Offer> {
        let mut out = Vec::new();
        for model in catalog.models(capability) {
            let mut fitting = Vec::new();
            for build in &model.builds {
                match backend::find(backends, &build.backend)
                    .map(|backend| self.fit(build, backend, caps))
                {
                    None => out.push(rejected(model, build, Rejection::BackendNotInThisBuild)),
                    Some(Err(why)) => out.push(rejected(model, build, why)),
                    Some(Ok(accelerator)) => fitting.push((build.clone(), accelerator)),
                }
            }
            let mut fitting = fitting.into_iter();
            if let Some((build, accelerator)) = fitting.next() {
                out.push(Offer::Offered {
                    model: model.clone(),
                    build,
                    accelerator,
                    alternatives: fitting.map(|(build, _)| build).collect(),
                });
            }
        }
        out
    }

    /// The best accelerator this build can run on here, or why it cannot: the backend's preference order, kept to
    /// what its probe confirms of what the host reports.
    fn fit(
        &self,
        build: &Build,
        backend: &dyn Backend,
        caps: &Capabilities,
    ) -> Result<Accelerator, Rejection> {
        // Data, not backend code: a backend with no entry for this platform in backends.json cannot run here.
        if Platform::of(caps)
            .and_then(|platform| backend::runtime_files(backend.spec().id, platform))
            .is_none()
        {
            return Err(Rejection::BackendUnavailable(Reason::new(
                "no-runtime-for-platform",
            )));
        }
        let spec = backend.spec();
        let probed = self.probe(backend, caps);
        // A probe narrows what the host reports, never widens it.
        let Some(accelerator) = spec
            .accelerators
            .iter()
            .copied()
            .find(|accelerator| caps.has(*accelerator) && probed.contains(accelerator))
        else {
            return Err(Rejection::BackendUnavailable(Reason::new("no-accelerator")));
        };
        // The build's own needs (catalogue) are checked like the backend's. Its `requires` are not checked yet: the
        // host does not report WebGPU features, and the WebAssembly cap is the ranking's (sidevoice-engine#4).
        let build_needs = MinMemoryMb(build.memory.mb);
        let requirements = std::iter::once(&build_needs as &dyn Requirement)
            .chain(spec.requirements.iter().copied());
        for requirement in requirements {
            requirement.check(caps).map_err(Rejection::DoesNotFit)?;
        }
        Ok(accelerator)
    }

    fn probe(&self, backend: &dyn Backend, caps: &Capabilities) -> Vec<Accelerator> {
        // A poisoned cache still holds whole answers: a panic cannot leave an entry half written.
        let mut probes = self.probes.lock().unwrap_or_else(PoisonError::into_inner);
        probes
            .entry(backend.spec().id)
            .or_insert_with(|| backend.probe(caps))
            .clone()
    }
}

fn rejected(model: &Model, build: &Build, why: Rejection) -> Offer {
    Offer::Rejected {
        model: model.clone(),
        build: build.clone(),
        why,
    }
}
