/// iOS: the system ends a process that goes over its memory limit, and tells it what is left of it. `sysinfo`'s figures
/// are the machine's, which a process cannot use.
pub struct Os;

extern "C" {
    /// `<os/proc.h>`, iOS 13 and later: the bytes this process may still allocate before it hits its limit. Nothing
    /// maintained binds it (the `libc` crate does not). Zero means the process is at its limit or over it: nothing is
    /// left, and the machine's free memory is not a substitute.
    fn os_proc_available_memory() -> usize;
}

impl super::Observer for Os {
    fn process_bytes() -> Option<u64> {
        // SAFETY: a libSystem function that takes no arguments and has no preconditions.
        let bytes = unsafe { os_proc_available_memory() };
        u64::try_from(bytes).ok()
    }
}
