/// Android: the machine's memory from `sysinfo`, which reads `/proc/meminfo`: `MemAvailable` is the kernel's estimate
/// of what a new allocation can get, the figure `ActivityManager.getMemoryInfo` derives its `availMem` from. No JNI,
/// so no `Context` is needed. The system kills background processes (lmkd) as it runs low rather than refusing an
/// allocation; the process has no budget of its own to read.
pub struct Os;

impl super::Observer for Os {
    /// The native host lists no GPU or neural engine on Android (only the CPU, which takes the machine's memory), so
    /// there is nothing to give memory to.
    fn accelerator_bytes() -> Vec<(crate::host::Accelerator, u64)> {
        Vec::new()
    }
}
