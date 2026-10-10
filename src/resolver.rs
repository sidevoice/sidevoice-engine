//! The funnel, the same for every backend: for each build of the catalogue, is its backend compiled here (which
//! backends a build of the engine has is decided when it is compiled), which of its accelerators work here (what the
//! host reports, narrowed by `probe`, cached), is one of them an accelerator the build can take (its `requires`), in a
//! page does the build fit in what WebAssembly hands it (`requires.wasm_max_mb`), does the machine meet the build's and
//! the backend's requirements (a value the host cannot tell passes). The builds of a model are listed with those that
//! run first, each on its backend's preferred accelerator, and every one that does not with its reason (`rejection`);
//! ranking the builds that run is still to come (sidevoice-engine#4).

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use crate::backend::{self, Backend, BackendId, MinMemoryMb, Requirement};
use crate::catalog::{BuildEntry, ModelEntry};
use crate::host::{Accelerator, Capabilities, Runs};

mod rejection;
#[cfg(test)]
mod tests;

pub use rejection::Reason;
pub(crate) use rejection::Rejection;

/// The funnel, remembering each backend's probe.
#[derive(Debug, Default)]
pub(crate) struct Resolver {
    probes: Mutex<HashMap<BackendId, Vec<Accelerator>>>,
}

impl Resolver {
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

    /// The best accelerator this build can run on here, or why it cannot: the backend's preference order, kept to
    /// what its probe confirms of what the host reports, then to what the build requires.
    fn fit(
        &self,
        build: &BuildEntry,
        backend: &dyn Backend,
        caps: &Capabilities,
    ) -> Result<Accelerator, Rejection> {
        let spec = backend.spec();
        if let Some(reason) = backend.unavailable() {
            return Err(Rejection::BackendUnavailable(reason));
        }
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
