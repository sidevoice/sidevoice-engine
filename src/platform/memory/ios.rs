/// iOS: the system ends a process that goes over its memory limit, and tells it what is left of it. `sysinfo`'s figures
/// are the machine's, which a process cannot use. Its GPU's working set is Metal's, and its neural engine's is the
/// process budget.
pub struct Os;

extern "C" {
    /// `<os/proc.h>`, iOS 13 and later: the bytes this process may still allocate before it hits its limit. Nothing
    /// maintained binds it (the `libc` crate does not). Zero means the process is at its limit or over it: nothing is
    /// left, and the machine's free memory is not a substitute.
    fn os_proc_available_memory() -> usize;
}

impl super::Observer for Os {
    /// Metal's working set, but never more than the process may still allocate, and Core ML's share: the process
    /// budget. The GPU's and the neural engine's memory is the app's on iOS, and the system ends the app past its
    /// budget whatever Metal advises. With no budget (the Simulator, Mac Catalyst, where the function answers
    /// nothing) Metal is not capped and Core ML takes the machine's memory.
    fn accelerator_bytes() -> Vec<(crate::host::Accelerator, u64)> {
        let budget = Self::process_bytes().filter(|bytes| *bytes > 0);
        let mut accelerators: Vec<_> = super::metal::working_set()
            .into_iter()
            .map(|(accelerator, bytes)| {
                (
                    accelerator,
                    budget.map_or(bytes, |budget| bytes.min(budget)),
                )
            })
            .collect();
        accelerators.extend(
            budget
                .or_else(Self::physical_bytes)
                .map(|bytes| (crate::host::Accelerator::CoreMl, bytes)),
        );
        accelerators
    }

    fn process_bytes() -> Option<u64> {
        // SAFETY: a libSystem function that takes no arguments and has no preconditions.
        let bytes = unsafe { os_proc_available_memory() };
        u64::try_from(bytes).ok()
    }
}
