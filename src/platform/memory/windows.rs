/// Windows: the machine's memory from `sysinfo`; no control groups.
pub struct Os;

impl super::Observer for Os {
    /// No GPU is supported here yet, so none has memory; CUDA, Vulkan or DXGI each add their figure when a backend
    /// runs there (like the default arm of a switch).
    fn accelerator_bytes() -> Vec<(crate::host::Accelerator, u64)> {
        Vec::new()
    }
}
