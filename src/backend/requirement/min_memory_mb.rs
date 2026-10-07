use super::Requirement;
use crate::host::Capabilities;
use crate::resolver::Reason;

/// At least this much memory, in MB. Unknown memory passes.
pub(crate) struct MinMemoryMb(pub(crate) u32);

impl Requirement for MinMemoryMb {
    fn check(&self, caps: &Capabilities) -> Result<(), Reason> {
        match caps.memory_mb {
            Some(has) if has < self.0 => Err(Reason::with_numbers("memory", self.0, has)),
            _ => Ok(()),
        }
    }
}
