use super::Requirement;
use crate::host::Capabilities;
use crate::resolver::Reason;

/// At least this many CPU cores. Unknown cores pass.
#[allow(dead_code, reason = "a common requirement no backend declares yet")]
pub(crate) struct MinCores(pub(crate) u32);

impl Requirement for MinCores {
    fn check(&self, caps: &Capabilities) -> Result<(), Reason> {
        match caps.cores {
            Some(has) if has < self.0 => Err(Reason::with_numbers("cores", self.0, has)),
            _ => Ok(()),
        }
    }
}
