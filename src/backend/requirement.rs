//! What the machine must meet for a backend (or a build) to run here: each requirement is a check on the host's
//! capabilities, answering yes or no with a reason. This file is the interface; the common requirements the engine
//! ships are in requirement/, one file each, and a backend can write its own without changing the contract.

use crate::host::Capabilities;
use crate::offer::Reason;

mod min_cores;
mod min_memory_mb;

pub use min_cores::MinCores;
pub use min_memory_mb::MinMemoryMb;

/// One condition the machine must meet.
pub trait Requirement: Send + Sync {
    fn check(&self, caps: &Capabilities) -> Result<(), Reason>;
}
