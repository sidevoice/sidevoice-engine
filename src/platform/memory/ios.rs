/// iOS: the system ends a process that goes over its memory limit, and tells it what is left of it. `sysinfo`'s figures
/// are the machine's, which a process cannot use. Its GPU's working set is Metal's.
pub struct Os;

extern "C" {
    /// `<os/proc.h>`, iOS 13 and later: the bytes this process may still allocate before it hits its limit. Nothing
    /// maintained binds it (the `libc` crate does not). Zero means the process is at its limit or over it: nothing is
    /// left, and the machine's free memory is not a substitute.
    fn os_proc_available_memory() -> usize;
}

impl super::Observer for Os {
    /// Metal's working set, but never more than the process may still allocate: the GPU's memory is the app's on
    /// iOS, and the system ends the app past its budget whatever Metal advises. A budget of zero (the Simulator,
    /// Mac Catalyst, where the function answers nothing) caps nothing.
    fn accelerator_bytes() -> Vec<(crate::host::Accelerator, u64)> {
        let budget = Self::process_bytes().filter(|bytes| *bytes > 0);
        super::metal::working_set()
            .into_iter()
            .map(|(accelerator, bytes)| {
                (
                    accelerator,
                    budget.map_or(bytes, |budget| bytes.min(budget)),
                )
            })
            .collect()
    }

    fn process_bytes() -> Option<u64> {
        // SAFETY: a libSystem function that takes no arguments and has no preconditions.
        let bytes = unsafe { os_proc_available_memory() };
        u64::try_from(bytes).ok()
    }
}
