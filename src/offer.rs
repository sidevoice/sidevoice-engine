//! What the resolver returns: per model, an offer with its best build, or a build rejected with the step and the
//! reason.

use crate::catalog::{Build, Model};
use crate::host::Accelerator;

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
