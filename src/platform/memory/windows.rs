/// Windows: the machine's memory from `sysinfo`; no control groups.
pub struct Os;

impl super::Observer for Os {
    /// No GPU here that a backend runs on natively; a CUDA, Vulkan or DXGI figure comes with the backend that does.
    fn accelerator_bytes() -> Vec<(crate::host::Accelerator, u64)> {
        Vec::new()
    }
}
