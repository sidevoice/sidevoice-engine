//! The funnel, the same for every backend: for each build of the catalogue, is its backend compiled here (which
//! backends a build of the engine has is decided when it is compiled), which of its accelerators work here (what the
//! host reports, narrowed by `probe`, cached), is one of them an accelerator the build can take (its `requires`), in a
//! page does the build fit in what WebAssembly hands it (`requires.wasm_max_mb`), does the machine meet the build's and
//! the backend's requirements (a value the host cannot tell passes). The builds of a model are listed with those that
//! run first, each on its backend's preferred accelerator, and every one that does not with its reason (`rejection`).
//!
//! The builds that run are ranked by the class of the accelerator each would run on here ([`Accelerator::class`],
//! sidevoice-engine#4): a GPU (Metal, CUDA, WebGPU) first, then a neural engine (Core ML), then a CPU (natively or in
//! WebAssembly). Within a class they keep catalogue order: that is the documented fallback until measured speed and
//! quality rank them (sidevoice-engine#16). So on Apple silicon whisper.cpp on Metal comes before sherpa-onnx on the
//! CPU, and where both run on the CPU the catalogue's order stands.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use crate::backend::{self, Backend, BackendId, MinMemoryMb, Requirement};
use crate::catalog::{BuildEntry, ModelEntry};
use crate::host::{Accelerator, AcceleratorClass, Capabilities, Runs};

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
    /// that fit first, by the class of their accelerator ([`Accelerator::class`]), then the rest; each class, and the
    /// rest, in catalogue order.
    pub(crate) fn builds<'a>(
        &self,
        model: &'a ModelEntry,
        backends: &[Box<dyn Backend>],
        caps: &Capabilities,
    ) -> Vec<(&'a BuildEntry, Result<Accelerator, Rejection>)> {
        let (mut fitting, rejected): (Vec<_>, Vec<_>) = model
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
        // A stable sort: within a class, catalogue order stands.
        fitting.sort_by_key(|(_, fit)| fit.as_ref().ok().map(|accelerator| accelerator.class()));
        fitting.into_iter().chain(rejected).collect()
    }

    /// The accelerator to load `build` on instead of `failed`, which did not load it: the best CPU it can run on here,
    /// if `failed` is a GPU or a neural engine. The engine falls back to it once, and says so (`AcceleratorUse`); a CPU
    /// that fails has nothing below it.
    pub(crate) fn fallback(
        &self,
        build: &BuildEntry,
        backend: &dyn Backend,
        caps: &Capabilities,
        failed: Accelerator,
    ) -> Option<Accelerator> {
        if failed.class() == AcceleratorClass::Cpu {
            return None;
        }
        self.accelerators(build, backend, caps)
            .ok()?
            .into_iter()
            .find(|accelerator| accelerator.class() == AcceleratorClass::Cpu)
    }

    /// The accelerators this build can run on here, best first, or why none: the backend's preference order, kept to
    /// what its probe confirms of what the host reports, then to what the build requires. Never empty when it is `Ok`.
    fn accelerators(
        &self,
        build: &BuildEntry,
        backend: &dyn Backend,
        caps: &Capabilities,
    ) -> Result<Vec<Accelerator>, Rejection> {
        let spec = backend.spec();
        if let Some(reason) = backend.unavailable() {
            return Err(Rejection::BackendUnavailable(reason));
        }
        let probed = self.probe(backend, caps);
        // A probe narrows what the host reports, never widens it.
        let working: Vec<_> = spec
            .accelerators
            .iter()
            .copied()
            .filter(|accelerator| caps.has(*accelerator) && probed.contains(accelerator))
            .collect();
        if working.is_empty() {
            return Err(Rejection::BackendUnavailable(Reason::new("no-accelerator")));
        }
        // A build that runs on fewer accelerators than its backend says so in `requires`.
        let accepted = &build.requires.accelerators;
        let accepted: Vec<_> = working
            .into_iter()
            .filter(|accelerator| accepted.is_empty() || accepted.contains(accelerator))
            .collect();
        if accepted.is_empty() {
            return Err(Rejection::DoesNotFit(Reason::new("build-accelerator")));
        }
        Ok(accepted)
    }

    /// The best accelerator this build can run on here, or why it cannot ([`Resolver::accelerators`]'s first), then
    /// whether the machine meets what the build and the backend need.
    fn fit(
        &self,
        build: &BuildEntry,
        backend: &dyn Backend,
        caps: &Capabilities,
    ) -> Result<Accelerator, Rejection> {
        let spec = backend.spec();
        let accelerator = self
            .accelerators(build, backend, caps)?
            .into_iter()
            .next()
            .ok_or(Rejection::DoesNotFit(Reason::new("build-accelerator")))?;
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
