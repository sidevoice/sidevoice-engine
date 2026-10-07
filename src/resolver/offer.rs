//! What the resolver returns: per model, an offer with its best build, or a build rejected with the step and the
//! reason.

use super::Rejection;
use crate::catalog::{Build, Model};
use crate::host::Accelerator;

/// What the resolver says about one model: offered with its best build and accelerator, or one build rejected and why.
/// Every rejection is kept, so a client can show why a model is not offered here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Offer {
    /// The model can run here.
    Offered {
        /// The model.
        model: Model,
        /// Its best build that fits here.
        build: Build,
        /// The best accelerator for that build here.
        accelerator: Accelerator,
        /// Its other builds that fit here, best first.
        alternatives: Vec<Build>,
    },
    /// One build of the model cannot run here.
    Rejected {
        /// The model.
        model: Model,
        /// The build that cannot run here.
        build: Build,
        /// Why not.
        why: Rejection,
    },
}
