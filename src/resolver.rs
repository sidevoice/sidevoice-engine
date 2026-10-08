//! The funnel, the same for every backend: for each build of the catalogue, is its backend compiled here, does it have
//! an entry for this platform in `backends.json`, which of its accelerators work here (what the host reports, narrowed
//! by `probe`, cached), is one of them an accelerator the build can take (its `requires`), does the machine meet the
//! build's and the backend's requirements (a value the host cannot tell passes). Then, per model, a build: the
//! catalogue's builds carry no order, and ranking them is still to come (sidevoice-engine#4); until then it is the
//! first that fits, on its backend's preferred accelerator. Every rejected build is kept with its reason.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use crate::backend::{self, Backend, BackendId, MinMemoryMb, Requirement};
use crate::catalog::{BuildEntry, Capability, Catalog, ModelEntry};
use crate::host::{Accelerator, Capabilities, Platform};

mod offer;
#[cfg(test)]
mod tests;

pub use offer::Reason;
pub(crate) use offer::{Offer, Rejection};

/// The library files a backend needs on a platform (`backends.json`, or a test's own); `None` where it does not run.
pub(crate) type RuntimeFiles = fn(&str, Platform) -> Option<Vec<crate::install::Artifact>>;

/// The funnel, remembering each backend's probe.
#[derive(Debug)]
pub(crate) struct Resolver {
    probes: Mutex<HashMap<BackendId, Vec<Accelerator>>>,
    runtime_files: RuntimeFiles,
}

impl Default for Resolver {
    /// The funnel over `backends.json`.
    fn default() -> Self {
        Self::new(backend::runtime_files)
    }
}

impl Resolver {
    /// The funnel, with each backend's files for a platform from `runtime_files`.
    pub(crate) fn new(runtime_files: RuntimeFiles) -> Self {
        Self {
            probes: Mutex::default(),
            runtime_files,
        }
    }

    /// Every build of `model`, ranked, each with the accelerator it runs on here or why it cannot run here: the builds
    /// that fit first, then the rest, each group in catalogue order. Ranking is still the first that fits
    /// (sidevoice-engine#4).
    pub(crate) fn builds<'a>(
        &self,
        model: &'a ModelEntry,
        backends: &[Box<dyn Backend>],
        caps: &Capabilities,
    ) -> Vec<(&'a BuildEntry, Result<Accelerator, Rejection>)> {
        let (fitting, rejected): (Vec<_>, Vec<_>) = model
            .builds
            .iter()
            .map(|build| {
                let fit = match backend::find(backends, &build.backend) {
                    None => Err(Rejection::BackendNotInThisBuild),
                    Some(backend) => self.fit(build, backend, caps),
                };
                (build, fit)
            })
            .partition(|(_, fit)| fit.is_ok());
        fitting.into_iter().chain(rejected).collect()
    }

    /// Every model that can do `capability`, offered with a build that fits, and every build that cannot run here,
    /// with why: what the web bridge lists.
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
            for (build, fit) in self.builds(model, backends, caps) {
                match fit {
                    Err(why) => out.push(rejected(model, build, why)),
                    Ok(accelerator) => fitting.push((build.clone(), accelerator)),
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
        build: &BuildEntry,
        backend: &dyn Backend,
        caps: &Capabilities,
    ) -> Result<Accelerator, Rejection> {
        // Data, not backend code: a backend with no entry for this platform in backends.json cannot run here.
        if Platform::of(caps)
            .and_then(|platform| (self.runtime_files)(backend.spec().id, platform))
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
        // The build's own needs (catalogue) are checked like the backend's. Its other `requires` are not checked yet:
        // the host does not report WebGPU features, and the WebAssembly cap is the ranking's (sidevoice-engine#4).
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

fn rejected(model: &ModelEntry, build: &BuildEntry, why: Rejection) -> Offer {
    Offer::Rejected {
        model: model.clone(),
        build: build.clone(),
        why,
    }
}
