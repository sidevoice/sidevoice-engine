//! What a host reports about the place the engine runs in.

/// What the host knows about the place the engine runs in. What can only be known by trying (a CUDA driver, a WebGPU
/// adapter) is not here: each backend finds out when it probes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capabilities {
    /// A native process or a page.
    pub runs: Runs,
    /// `std::env::consts::OS` values: "macos", "linux", "windows", ...
    pub os: String,
    /// `std::env::consts::ARCH` values: "aarch64", "x86_64", ...
    pub arch: String,
    /// The accelerators the host knows work here.
    pub accelerators: Vec<Accelerator>,
    /// Memory available to models, when the host can tell.
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

/// Whether the engine runs in a native process or in a page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Runs {
    /// A native process: desktop, headless, a server.
    Native,
    /// A web page (the wasm32 build).
    Page,
}

/// Hardware or a runtime a model can run on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Accelerator {
    /// The CPU, natively.
    Cpu,
    /// An NVIDIA GPU through CUDA.
    Cuda,
    /// Apple's Core ML.
    CoreMl,
    /// An Apple GPU through Metal.
    Metal,
    /// The GPU from a page, through WebGPU.
    WebGpu,
    /// The CPU from a page, through WebAssembly.
    Wasm,
}
