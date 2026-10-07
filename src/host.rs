//! The platform, implemented by each platform outside this repository: a host in the browser, one in each native
//! process (desktop, headless, the core's machine), one on mobile later.

use async_trait::async_trait;

use crate::maybe_send::{MaybeSend, MaybeSync};
use crate::Result;

/// The facts, storage and downloads of the place the engine runs in. There is no default: without a host there is
/// no engine.
///
/// A host must be `Send + Sync` in a native build and need not be in the web build ([`MaybeSend`], [`MaybeSync`]).
/// [`Storage`] and [`Fetcher`] are async traits: implement them with the re-exported
/// [`async_trait`](crate::async_trait) attribute, which must match the engine's on each target (futures are `Send` in a
/// native build, not on the web):
///
/// ```
/// use sidevoice_engine::{async_trait, Result, Storage};
///
/// struct Directory;
///
/// #[cfg_attr(not(target_arch = "wasm32"), async_trait)]
/// #[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
/// impl Storage for Directory {
///     async fn contains(&self, _key: &str) -> Result<bool> {
///         Ok(false)
///     }
/// }
/// ```
pub trait Host: MaybeSend + MaybeSync {
    /// Known when the host is built: gathering it is the host's job, so asking is cheap and synchronous.
    fn capabilities(&self) -> Capabilities;
    /// Where engine packages and models live (a directory, OPFS, ...).
    fn storage(&self) -> &dyn Storage;
    /// Downloads, with progress and cancellation.
    fn fetcher(&self) -> &dyn Fetcher;
}

/// Where engine packages and models are kept. Implemented with [`async_trait`](crate::async_trait), as [`Host`] shows.
#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
pub trait Storage: MaybeSend + MaybeSync {
    /// Whether `key` is already stored, complete.
    async fn contains(&self, key: &str) -> Result<bool>;
}

/// Downloads into [`Storage`]. Implemented with [`async_trait`](crate::async_trait), as [`Host`] shows.
#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
pub trait Fetcher: MaybeSend + MaybeSync {
    /// Downloads `url` into `key`, checked against `sha256`.
    async fn fetch(&self, url: &str, sha256: &str, key: &str) -> Result<()>;
}

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
