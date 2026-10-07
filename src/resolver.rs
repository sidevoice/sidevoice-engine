//! The funnel, the same for every backend: for each build of the catalogue, is its backend compiled here, which of its
//! accelerators work here (`probe`, cached), does the machine meet the build's and the backend's requirements. Then,
//! per model, the best build: the catalogue's order, and the backend's accelerator preference. Every rejected build is
//! kept with its reason.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::backend::{Backend, BackendId, MinMemoryMb, Requirement};
use crate::catalog::{Build, Catalog, Model, Task};
use crate::host::{Accelerator, Capabilities};

/// What the resolver says about one model: offered with its best build and accelerator, or one build rejected and why.
/// Every rejection is kept, so a client can show why a model is not offered here.
#[derive(Debug, Clone, PartialEq)]
pub enum Offer {
    Offered {
        model: Model,
        build: Build,
        accelerator: Accelerator,
        alternatives: Vec<Build>,
    },
    Rejected {
        model: Model,
        build: Build,
        why: Rejection,
    },
}

/// The step of the funnel that rejected a build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rejection {
    /// 1. The build's backend is not compiled into this build of the engine.
    BackendNotInThisBuild,
    /// 2. None of the backend's accelerators works here (`Backend::probe`).
    BackendUnavailable(Reason),
    /// 3. The machine does not meet a requirement of the build or of its backend (memory, cores, ...).
    DoesNotFit(Reason),
}

/// Why a backend or a build does not fit: a stable code that clients translate (never text in a language), with the
/// numbers the message needs (what is needed, what there is).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reason {
    pub code: &'static str,
    pub needs: Option<u32>,
    pub has: Option<u32>,
}

impl Reason {
    pub const fn new(code: &'static str) -> Self {
        Self {
            code,
            needs: None,
            has: None,
        }
    }

    pub const fn numbers(code: &'static str, needs: u32, has: u32) -> Self {
        Self {
            code,
            needs: Some(needs),
            has: Some(has),
        }
    }
}

#[derive(Default)]
pub(crate) struct Resolver {
    probes: Mutex<HashMap<BackendId, Vec<Accelerator>>>,
}

impl Resolver {
    pub(crate) fn offers(
        &self,
        catalog: &Catalog,
        backends: &[Box<dyn Backend>],
        caps: &Capabilities,
        task: Task,
    ) -> Vec<Offer> {
        let mut out = Vec::new();
        for model in catalog.models(task) {
            let mut fitting = Vec::new();
            for build in &model.builds {
                let backend = backends
                    .iter()
                    .find(|backend| backend.spec().id == build.backend);
                match backend.map(|backend| self.fit(build, backend.as_ref(), caps)) {
                    None => out.push(rejected(model, build, Rejection::BackendNotInThisBuild)),
                    Some(Err(why)) => out.push(rejected(model, build, why)),
                    Some(Ok(accelerator)) => fitting.push((build.clone(), accelerator)),
                }
            }
            if !fitting.is_empty() {
                let (build, accelerator) = fitting.remove(0);
                let alternatives = fitting.into_iter().map(|(build, _)| build).collect();
                out.push(Offer::Offered {
                    model: model.clone(),
                    build,
                    accelerator,
                    alternatives,
                });
            }
        }
        out
    }

    /// The best accelerator this build can run on here, or why it cannot.
    pub(crate) fn fit(
        &self,
        build: &Build,
        backend: &dyn Backend,
        caps: &Capabilities,
    ) -> Result<Accelerator, Rejection> {
        let spec = backend.spec();
        let working = self.probe(backend, caps);
        let Some(accelerator) = spec
            .accelerators
            .iter()
            .copied()
            .find(|accelerator| working.contains(accelerator))
        else {
            return Err(Rejection::BackendUnavailable(Reason::new("no-accelerator")));
        };
        // The build's own needs (catalogue) are checked like the backend's.
        let build_needs = MinMemoryMb(build.memory_mb);
        let requirements = std::iter::once(&build_needs as &dyn Requirement)
            .chain(spec.requirements.iter().copied());
        for requirement in requirements {
            requirement.check(caps).map_err(Rejection::DoesNotFit)?;
        }
        Ok(accelerator)
    }

    fn probe(&self, backend: &dyn Backend, caps: &Capabilities) -> Vec<Accelerator> {
        let mut probes = self.probes.lock().expect("probe cache lock");
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
