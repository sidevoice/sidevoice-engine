//! What a host reports about the place the engine runs in: the contract each platform's host fills in.
//!
//! The host reports what it can see without trying anything: the OS, the architecture, memory and cores, and which
//! accelerators are present (an NVIDIA GPU, Apple silicon, `navigator.gpu` in a page). Whether a backend can actually
//! use one (a CUDA driver that loads, a WebGPU adapter that is granted, a Core ML model that compiles) is not the
//! host's to say: each backend finds out in its `probe`, and the probe can only narrow what the host reported, never
//! add to it.

mod accelerator;

pub use accelerator::{Accelerator, AcceleratorClass, AcceleratorUse};

#[cfg(test)]
mod tests;

/// What the host knows about the place the engine runs in, gathered once when the host is built.
///
/// Unknown values: `memory_mb` and `cores` are `None` when the host cannot tell (a page cannot read memory reliably),
/// and a requirement on an unknown value passes: the engine does not reject a build on a guess, and loading is what
/// finds out. An accelerator the host is unsure of is left out: it is absent, and nothing runs on it. `os` and `arch`
/// are always given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capabilities {
    /// A native process or a page.
    pub runs: Runs,
    /// `std::env::consts::OS` values: "macos", "linux", "windows", ...; "web" in a page that does not know better.
    pub os: String,
    /// `std::env::consts::ARCH` values: "aarch64", "x86_64", ...; "wasm32" in a page that does not know better.
    pub arch: String,
    /// The accelerators present here, as far as the host can see; order does not matter. Each backend's `probe`
    /// confirms which of these it can actually use.
    pub accelerators: Vec<Accelerator>,
    /// Memory available to models, in MiB (2^20 bytes, what every `_mb` in the engine means), when the host can tell.
    pub memory_mb: Option<u32>,
    /// CPU cores, when the host can tell.
    pub cores: Option<u32>,
    /// The memory a GPU or a neural engine can give models, for each the host can tell; one it leaves out is checked
    /// against `memory_mb` ([`Capabilities::memory_for`]). Order does not matter.
    pub accelerator_memory: Vec<AcceleratorMemory>,
    /// Whether the system stops serving the GPU to the app while the app is in the background (iOS: a Metal command
    /// buffer committed then fails). A build that runs on a GPU here then works only while the app is in the
    /// foreground ([`ModelBuild::stops_in_background`](crate::ModelBuild::stops_in_background)). `false` when the host
    /// has no such limit, or cannot tell.
    pub gpu_stops_in_background: bool,
}

impl Capabilities {
    /// Whether the host reports `accelerator`.
    #[must_use]
    pub fn has(&self, accelerator: Accelerator) -> bool {
        self.accelerators.contains(&accelerator)
    }

    /// The memory a model running on `accelerator` can take here, in MiB: `memory_mb` for a CPU; for a GPU or a neural
    /// engine its own figure ([`Capabilities::accelerator_memory`]), else `memory_mb` too (Apple's and the phones'
    /// GPUs share it, and a discrete GPU no backend reports yet is not refused on a guess). `None` when unknown.
    #[must_use]
    pub fn memory_for(&self, accelerator: Accelerator) -> Option<u32> {
        if accelerator.class() == AcceleratorClass::Cpu {
            return self.memory_mb;
        }
        self.accelerator_memory
            .iter()
            .find(|memory| memory.accelerator == accelerator)
            .map(|memory| memory.mb)
            .or(self.memory_mb)
    }
}

/// The memory an accelerator can give models, in MiB: on Apple, the working set Metal advises a GPU to stay within
/// (part of the unified memory `memory_mb` is).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AcceleratorMemory {
    /// The accelerator.
    pub accelerator: Accelerator,
    /// Its memory for models, in MiB.
    pub mb: u32,
}

/// Whether the engine runs in a native process or in a page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Runs {
    /// A native process: desktop, headless, a server.
    Native,
    /// A web page (the wasm32 build).
    Page,
}
