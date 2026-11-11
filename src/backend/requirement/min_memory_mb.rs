use super::Requirement;
use crate::host::Capabilities;
use crate::resolver::Reason;

/// At least this much memory, in MB. Unknown memory passes. As a backend's requirement it is checked against the
/// machine's memory; the resolver checks a build's own against the memory of the accelerator it would run on
/// ([`MinMemoryMb::met_by`], with `Capabilities::memory_for`).
pub(crate) struct MinMemoryMb(pub(crate) u32);

impl MinMemoryMb {
    /// Whether `has` MB meet it (unknown memory does), or why not, with the numbers.
    pub(crate) fn met_by(&self, has: Option<u32>) -> Result<(), Reason> {
        match has {
            Some(has) if has < self.0 => Err(Reason::with_numbers("memory", self.0, has)),
            _ => Ok(()),
        }
    }
}

impl Requirement for MinMemoryMb {
    fn check(&self, caps: &Capabilities) -> Result<(), Reason> {
        self.met_by(caps.memory_mb)
    }
}
