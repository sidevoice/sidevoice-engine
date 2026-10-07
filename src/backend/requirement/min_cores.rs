use super::Requirement;
use crate::host::Capabilities;
use crate::offer::Reason;

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
