use crate::host::Accelerator;
use crate::platform::vulkan;

/// Windows: the machine's memory from `sysinfo`; no control groups.
/// The GPU's memory is what ggml's Vulkan backend reports for the device whisper.cpp runs on ([`vulkan`]).
pub struct Os;

impl super::Observer for Os {
    /// Vulkan, with the own memory of the first GPU ggml lists (what whisper.cpp uses, not the largest adapter). An
    /// integrated GPU has none: it works in the system's memory, which the CPU needs too, so no model counts on it.
    fn accelerator_bytes() -> Vec<(Accelerator, u64)> {
        vulkan::model_bytes()
            .map(|bytes| (Accelerator::Vulkan, bytes))
            .into_iter()
            .collect()
    }
}
