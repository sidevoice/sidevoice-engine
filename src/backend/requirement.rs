//! What the machine must meet for a backend (or a build) to run here: each requirement is a check on the host's
//! capabilities, answering yes or no with a reason. The engine has the common ones; a backend can write its own.

use crate::host::Capabilities;
use crate::offer::Reason;

/// One condition the machine must meet. The engine has the common ones ([`MinMemoryMb`], [`MinCores`]); a backend can
/// write its own without changing the contract.
pub trait Requirement: Send + Sync {
    fn check(&self, caps: &Capabilities) -> Result<(), Reason>;
}

/// At least this much memory, in MB. Unknown memory passes.
pub struct MinMemoryMb(pub u32);

impl Requirement for MinMemoryMb {
    fn check(&self, caps: &Capabilities) -> Result<(), Reason> {
        match caps.memory_mb {
            Some(has) if has < self.0 => Err(Reason::numbers("memory", self.0, has)),
            _ => Ok(()),
        }
    }
}

/// At least this many CPU cores. Unknown cores pass.
pub struct MinCores(pub u32);

impl Requirement for MinCores {
    fn check(&self, caps: &Capabilities) -> Result<(), Reason> {
        match caps.cores {
            Some(has) if has < self.0 => Err(Reason::numbers("cores", self.0, has)),
            _ => Ok(()),
        }
    }
}
