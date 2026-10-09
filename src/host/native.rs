//! The native host, built into every native build: the machine's facts from the standard library and `sysinfo`, files
//! in a directory the app passes in, downloads and API calls over HTTPS, and the keys the app hands it.
//!
//! Inside: `directory` (the storage) and `http` (the downloads and API calls).

use std::path::PathBuf;
use std::thread;

use crate::host::{
    Accelerator, Capabilities, Credentials, Fetcher, Host, HttpClient, NoCredentials, Runs, Storage,
};
use crate::Result;

mod directory;
mod http;
#[cfg(test)]
mod tests;

use directory::Directory;
use http::Http;

/// The host of a native process (the desktop app, a headless one): `NativeHost::new(data_dir)` and pass it to
/// [`Engine::new`](crate::Engine::new). Its only parameter is the directory its files live in, which the app chooses
/// (its own data directory, say) and should keep for the engine alone; everything else it finds out once, when it is
/// built:
///
/// - `os` and `arch` from `std::env::consts`, `cores` from `std::thread::available_parallelism`, and `memory_mb`, the
///   machine's memory (or its cgroup's limit, if lower), from `sysinfo`;
/// - accelerators: `Cpu` always; `Metal` and `CoreMl` on macOS; `Cuda` where an NVIDIA driver is installed (Linux's
///   `/proc/driver/nvidia/version`, Windows' `nvcuda.dll`). Each backend's probe confirms what it can actually use.
///
/// Downloads go through `reqwest`, with rustls: the HTTP client sidevoice-core and the desktop app use. **The engine's
/// futures expect a Tokio runtime** in a native build: `reqwest` needs one, and archives are unpacked on its blocking
/// threads. Run them on the app's own runtime; outside one, they panic. Remote backends' API calls go through the same
/// client.
///
/// It has no keys of its own: an app that uses remote models hands it where they are with
/// [`NativeHost::with_credentials`] (the OS keychain, say); without, it has none ([`NoCredentials`]).
pub struct NativeHost {
    capabilities: Capabilities,
    storage: Directory,
    http: Http,
    credentials: Box<dyn Credentials>,
}

impl std::fmt::Debug for NativeHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeHost")
            .field("capabilities", &self.capabilities)
            .field("storage", &self.storage)
            .finish_non_exhaustive()
    }
}

impl NativeHost {
    /// A host whose files live in `data_dir`, created if it does not exist.
    ///
    /// # Errors
    ///
    /// `storage-failed` if the directory cannot be created, and `storage-path-not-utf8` if its path is not UTF-8 (the
    /// engine hands file locations to backends as text).
    pub fn new(data_dir: impl Into<PathBuf>) -> Result<Self> {
        Ok(Self {
            capabilities: capabilities(),
            storage: Directory::new(data_dir.into())?,
            http: Http::default(),
            credentials: Box::new(NoCredentials),
        })
    }

    /// This host, with the keys of remote providers from `credentials`, which it asks each time a key is needed.
    #[must_use]
    pub fn with_credentials(mut self, credentials: impl Credentials + 'static) -> Self {
        self.credentials = Box::new(credentials);
        self
    }
}

impl Host for NativeHost {
    fn capabilities(&self) -> Capabilities {
        self.capabilities.clone()
    }

    fn storage(&self) -> &dyn Storage {
        &self.storage
    }

    fn fetcher(&self) -> &dyn Fetcher {
        &self.http
    }

    fn http(&self) -> &dyn HttpClient {
        &self.http
    }

    fn credentials(&self) -> &dyn Credentials {
        self.credentials.as_ref()
    }
}

/// What this machine has, as far as can be seen without trying anything.
fn capabilities() -> Capabilities {
    Capabilities {
        runs: Runs::Native,
        os: std::env::consts::OS.to_owned(),
        arch: std::env::consts::ARCH.to_owned(),
        accelerators: accelerators(),
        memory_mb: memory_mb(),
        cores: thread::available_parallelism()
            .ok()
            .map(|cores| u32::try_from(cores.get()).unwrap_or(u32::MAX)),
    }
}

fn accelerators() -> Vec<Accelerator> {
    let mut accelerators = vec![Accelerator::Cpu];
    if cfg!(target_os = "macos") {
        accelerators.extend([Accelerator::Metal, Accelerator::CoreMl]);
    }
    if nvidia_driver() {
        accelerators.push(Accelerator::Cuda);
    }
    accelerators
}

/// Whether an NVIDIA driver is installed: present, not proven to work (that is a backend's probe).
fn nvidia_driver() -> bool {
    if cfg!(target_os = "linux") {
        return std::path::Path::new("/proc/driver/nvidia/version").exists();
    }
    if cfg!(target_os = "windows") {
        return std::env::var_os("SystemRoot").is_some_and(|root| {
            PathBuf::from(root)
                .join("System32")
                .join("nvcuda.dll")
                .exists()
        });
    }
    false
}

/// The machine's memory in MB, or its cgroup's limit if lower; `None` if it cannot be read.
fn memory_mb() -> Option<u32> {
    let mut system = sysinfo::System::new();
    system.refresh_memory();
    let mut bytes = system.total_memory();
    if let Some(limits) = system.cgroup_limits() {
        if limits.total_memory > 0 {
            bytes = bytes.min(limits.total_memory);
        }
    }
    let mb = bytes / (1024 * 1024);
    (mb > 0).then(|| u32::try_from(mb).unwrap_or(u32::MAX))
}
