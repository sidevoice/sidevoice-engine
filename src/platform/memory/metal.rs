//! Apple's GPUs, on macOS and iOS: the memory Metal advises the system default GPU's working set to stay within
//! (`MTLDevice.recommendedMaxWorkingSetSize`), a part of the unified memory the CPU uses too.

use objc2_metal::{MTLCreateSystemDefaultDevice, MTLDevice};

use crate::host::Accelerator;

/// Metal's advised working set, in bytes, when the system has a default GPU.
pub fn working_set() -> Vec<(Accelerator, u64)> {
    MTLCreateSystemDefaultDevice()
        .map(|device| (Accelerator::Metal, device.recommendedMaxWorkingSetSize()))
        .into_iter()
        .collect()
}
