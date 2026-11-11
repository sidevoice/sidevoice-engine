/// macOS: `available` is the kernel's memory-pressure level (`kern.memorystatus_level`, the percentage of memory the
/// system still counts as free) of the total, an estimate and not an allocation limit; the machine's other figures
/// are `sysinfo`'s, its GPU's working set Metal's, and its neural engine shares the machine's memory.
pub struct Os;

impl super::Observer for Os {
    /// Metal's working set, and for Core ML the machine's memory: the neural engine shares it.
    fn accelerator_bytes() -> Vec<(crate::host::Accelerator, u64)> {
        let mut accelerators = super::metal::working_set();
        accelerators
            .extend(Self::physical_bytes().map(|bytes| (crate::host::Accelerator::CoreMl, bytes)));
        accelerators
    }

    fn free_bytes() -> Option<u64> {
        let mut level: u32 = 0;
        let mut size = std::mem::size_of::<u32>();
        // SAFETY: `kern.memorystatus_level` is a 32-bit integer; `level` and `size` outlive the call, and `size` is its
        // length.
        let status = unsafe {
            libc::sysctlbyname(
                c"kern.memorystatus_level".as_ptr(),
                (&raw mut level).cast(),
                &raw mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        if status != 0 || level > 100 {
            return None;
        }
        Self::physical_bytes().map(|total| total / 100 * u64::from(level))
    }
}
