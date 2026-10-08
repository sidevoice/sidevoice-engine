//! What the resolver returns: per model, an offer with its best build, or a build rejected with the step and the
//! reason.

use crate::catalog::{BuildEntry, ModelEntry};
use crate::host::Accelerator;

/// What the resolver says about one model: offered with its best build and accelerator, or one build rejected and why.
/// Every rejection is kept, so a client can show why a model is not offered here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Offer {
    /// The model can run here.
    Offered {
        /// The model.
        model: ModelEntry,
        /// Its best build that fits here.
        build: BuildEntry,
        /// The best accelerator for that build here.
        accelerator: Accelerator,
        /// Its other builds that fit here, best first.
        alternatives: Vec<BuildEntry>,
    },
    /// One build of the model cannot run here.
    Rejected {
        /// The model.
        model: ModelEntry,
        /// The build that cannot run here.
        build: BuildEntry,
        /// Why not.
        why: Rejection,
    },
}

/// The step of the funnel that rejected a build.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Rejection {
    /// 1. The build's backend is not compiled into this build of the engine.
    BackendNotInThisBuild,
    /// 2. The backend cannot run here: it has nothing to download for this platform (`no-runtime-for-platform`), or none
    ///    of its accelerators works here (`no-accelerator`).
    BackendUnavailable(Reason),
    /// 3. The build does not fit here: none of the accelerators that work here is one its `requires` allows
    ///    (`build-accelerator`), in a page it needs more memory than WebAssembly hands it (`wasm-memory`, with the
    ///    build's memory and its `requires.wasm_max_mb`), or the machine does not meet a requirement of the build or of
    ///    its backend (`memory`, `cores`, ...).
    DoesNotFit(Reason),
}

/// Why a backend or a build does not fit: a stable code that clients translate (never text in a language), with the
/// numbers the message needs (what is needed, what there is).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Reason {
    /// The stable code, such as `"memory"`.
    pub code: &'static str,
    /// What is needed, when it is a number.
    pub needs: Option<u32>,
    /// What there is, when it is a number.
    pub has: Option<u32>,
}

impl Reason {
    /// A reason with no numbers.
    #[must_use]
    pub const fn new(code: &'static str) -> Self {
        Self {
            code,
            needs: None,
            has: None,
        }
    }

    /// A reason with what is needed and what there is.
    #[must_use]
    pub const fn with_numbers(code: &'static str, needs: u32, has: u32) -> Self {
        Self {
            code,
            needs: Some(needs),
            has: Some(has),
        }
    }
}
