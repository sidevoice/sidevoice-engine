//! What the operating system shows of memory, in one observation with one unit: the **mebibyte** (2^20 bytes), which
//! every `_mb` name in the engine (the catalogue's `memory.mb`, `MinMemoryMb`, `Capabilities::memory_mb`) means.
//!
//! - `total_mb`: the machine's physical memory, or its cgroup's limit where that is lower. `None` when the OS cannot
//!   tell, and when it says zero.
//! - `available_mb`: what this process may still allocate. Where the OS keeps a budget for the process (iOS) that
//!   figure; elsewhere what the machine has available now. Not the same thing as `total_mb`, and it changes from one
//!   call to the next. `None` when the OS cannot tell; `Some(0)` is an answer, not a gap.

#[cfg(test)]
mod tests;

/// What the engine asks of the OS about memory, in bytes as the OS gives them. An OS says what it observes;
/// [`MemoryObservation`] decides what is reported. The defaults are `sysinfo`'s readings, which Linux, Windows and
/// Android (`/proc/meminfo`) share and an OS overrides only where it has a better figure.
pub trait Observer {
    /// The machine's physical memory.
    fn physical_bytes() -> Option<u64> {
        let total = refreshed().total_memory();
        (total > 0).then_some(total)
    }

    /// The memory limit of the control group the process runs in, where the OS has such groups and sets one.
    fn limit_bytes() -> Option<u64> {
        refreshed()
            .cgroup_limits()
            .map(|limits| limits.total_memory)
    }

    /// What the machine has available to a new allocation now, whatever process makes it.
    fn free_bytes() -> Option<u64> {
        let system = refreshed();
        (system.total_memory() > 0).then(|| system.available_memory())
    }

    /// What this process may allocate before the OS ends it, where the OS keeps such a budget; no answer where it
    /// does not. Zero is read as no answer too: it is also what such a call says where it does not apply.
    fn process_bytes() -> Option<u64> {
        None
    }
}

fn refreshed() -> sysinfo::System {
    let mut system = sysinfo::System::new();
    system.refresh_memory();
    system
}

/// The machine's memory, as one observation of the OS this is built for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryObservation {
    /// Physical memory (or its cgroup's limit, if lower), in MiB; `None` when unknown.
    pub total_mb: Option<u32>,
    /// What this process may still allocate, in MiB; `None` when unknown.
    pub available_mb: Option<u32>,
}

impl MemoryObservation {
    /// What this OS shows now.
    #[must_use]
    pub fn now() -> Self {
        Self::observed::<Machine>()
    }

    /// What `O` shows, with the decisions made here: the lower of the physical memory and the group's limit is the
    /// total; the process's own budget, when the OS has one, is what is available, and the machine's figure when not.
    fn observed<O: Observer>() -> Self {
        let limit = O::limit_bytes().filter(|bytes| *bytes > 0);
        let total = [O::physical_bytes(), limit].into_iter().flatten().min();
        Self {
            total_mb: total.map(mebibytes).filter(|mb| *mb > 0),
            available_mb: O::process_bytes()
                .filter(|bytes| *bytes > 0)
                .or_else(O::free_bytes)
                .map(mebibytes),
        }
    }
}

/// `bytes` in whole MiB, saturating at what a `u32` holds.
fn mebibytes(bytes: u64) -> u32 {
    u32::try_from(bytes / (1024 * 1024)).unwrap_or(u32::MAX)
}

#[cfg(target_os = "android")]
mod android;
#[cfg(target_os = "android")]
pub use android::Os as Machine;

#[cfg(target_os = "ios")]
mod ios;
#[cfg(target_os = "ios")]
pub use ios::Os as Machine;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::Os as Machine;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::Os as Machine;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::Os as Machine;
