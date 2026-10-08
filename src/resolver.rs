//! The funnel, the same for every backend: for each build of the catalogue, is its backend compiled here, does it have
//! an entry for this platform in `backends.json`, which of its accelerators work here (what the host reports, narrowed
//! by `probe`, cached), is one of them an accelerator the build can take (its `requires`), in a page does the build fit
//! in what WebAssembly hands it (`requires.wasm_max_mb`), does the machine meet the build's and the backend's
//! requirements (a value the host cannot tell passes). Then, per model, a build: the
//! catalogue's builds carry no order, and ranking them is still to come (sidevoice-engine#4); until then it is the
//! first that fits, on its backend's preferred accelerator. Every rejected build is kept with its reason.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use crate::backend::{self, Backend, BackendId, MinMemoryMb, Requirement};
use crate::catalog::{Build, Capability, Catalog, Model};
use crate::host::{Accelerator, Capabilities, Platform, Runs};

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
    /// what its probe confirms of what the host reports, then to what the build requires.
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
        let mut working = spec
            .accelerators
            .iter()
            .copied()
            .filter(|accelerator| caps.has(*accelerator) && probed.contains(accelerator))
            .peekable();
        if working.peek().is_none() {
            return Err(Rejection::BackendUnavailable(Reason::new("no-accelerator")));
        }
        // A build that runs on fewer accelerators than its backend says so in `requires`.
        let accepted = &build.requires.accelerators;
        let Some(accelerator) =
            working.find(|accelerator| accepted.is_empty() || accepted.contains(accelerator))
        else {
            return Err(Rejection::DoesNotFit(Reason::new("build-accelerator")));
        };
        // In a page the build runs in WebAssembly, under the cap its catalogue entry declares (`requires.wasm_max_mb`, not
        // measured from the page): a build that needs more does not fit there, whatever the machine has.
        if let (Runs::Page, Some(cap)) = (caps.runs, build.requires.wasm_max_mb) {
            if build.memory.mb > cap {
                return Err(Rejection::DoesNotFit(Reason::with_numbers(
                    "wasm-memory",
                    build.memory.mb,
                    cap,
                )));
            }
        }
        // The build's own needs (catalogue) are checked like the backend's. Its WebGPU features are not checked yet: the
        // host does not report them.
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
