//! The platform, implemented by each platform outside this repository: a host in the browser, one in each native
//! process (desktop, headless, the core's machine), one on mobile later.

use async_trait::async_trait;

use crate::Result;

/// The facts, storage and downloads of the place the engine runs in. There is no default: without a host there is
/// no engine.
pub trait Host: Send + Sync {
    /// Known when the host is built: gathering it is the host's job, so asking is cheap and synchronous.
    fn capabilities(&self) -> Capabilities;
    /// Where engine packages and models live (a directory, OPFS, ...).
    fn storage(&self) -> &dyn Storage;
    /// Downloads, with progress and cancellation.
    fn fetcher(&self) -> &dyn Fetcher;
}

/// Where engine packages and models are kept.
#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
pub trait Storage: Send + Sync {
    /// Whether `key` is already stored, complete.
    async fn contains(&self, key: &str) -> Result<bool>;
}

/// Downloads into [`Storage`].
#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
pub trait Fetcher: Send + Sync {
    /// Downloads `url` into `key`, checked against `sha256`.
    async fn fetch(&self, url: &str, sha256: &str, key: &str) -> Result<()>;
}

/// What the host knows about the place the engine runs in. What can only be known by trying (a CUDA driver, a WebGPU
/// adapter) is not here: each backend finds out in its `probe()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capabilities {
    pub runs: Runs,
    /// `std::env::consts::OS` values: "macos", "linux", "windows", ...
    pub os: String,
    /// `std::env::consts::ARCH` values: "aarch64", "x86_64", ...
    pub arch: String,
    pub accelerators: Vec<Accelerator>,
    /// Memory available to models, when the host can tell.
    pub memory_mb: Option<u32>,
    /// CPU cores, when the host can tell.
    pub cores: Option<u32>,
}

impl Capabilities {
    pub fn has(&self, accelerator: Accelerator) -> bool {
        self.accelerators.contains(&accelerator)
    }
}

/// Whether the engine runs in a native process or in a page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runs {
    Native,
    Page,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Accelerator {
    Cpu,
    Cuda,
    CoreMl,
    Metal,
    WebGpu,
    Wasm,
}
