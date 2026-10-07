//! What a host reports about the place the engine runs in: the contract each platform's host fills in.
//!
//! The host reports what it can see without trying anything: the OS, the architecture, memory and cores, and which
//! accelerators are present (an NVIDIA GPU, Apple silicon, `navigator.gpu` in a page). Whether a backend can actually
//! use one (a CUDA driver that loads, a WebGPU adapter that is granted, a Core ML model that compiles) is not the
//! host's to say: each backend finds out in its `probe`, and the probe can only narrow what the host reported, never
//! add to it.

mod accelerator;
mod runs;

pub use accelerator::Accelerator;
pub use runs::Runs;

/// What the host knows about the place the engine runs in, gathered once when the host is built.
///
/// Unknown values: `memory_mb` and `cores` are `None` when the host cannot tell (a page cannot read memory reliably),
/// and a requirement on an unknown value passes: the engine does not reject a build on a guess, and loading is what
/// finds out. An accelerator the host is unsure of is left out: it is absent, and nothing runs on it. `os` and `arch`
/// are always given: natively they name the platform whose `backends.json` entry is read (`Platform`), and a pair the
/// engine does not know leaves every backend without a runtime (`no-runtime-for-platform`).
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
    /// Memory available to models, in MB, when the host can tell.
    pub memory_mb: Option<u32>,
    /// CPU cores, when the host can tell.
    pub cores: Option<u32>,
}

impl Capabilities {
    /// Whether the host reports `accelerator`.
    #[must_use]
    pub fn has(&self, accelerator: Accelerator) -> bool {
        self.accelerators.contains(&accelerator)
    }
}
