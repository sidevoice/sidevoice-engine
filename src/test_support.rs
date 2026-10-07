//! Test doubles shared by the tests of every module: a host (CPU and Wasm everywhere, Metal on Apple silicon, 8 GB,
//! 8 cores) with nothing stored and no network, the same host with files in memory and a few to download, a small
//! catalogue with a build for each backend, one that needs too much memory and one for a backend no build has, and
//! a way to run a future to its end.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll};

use sha2::Digest;

use crate::{
    async_trait, Accelerator, Artifact, Build, Capabilities, CatalogFragment, CatalogSource,
    Download, Error, Fetcher, Host, Model, Result, Runs, Storage, StorageWriter, Task,
};

pub(crate) struct FakeHost;

impl Host for FakeHost {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            runs: if cfg!(web) { Runs::Page } else { Runs::Native },
            os: std::env::consts::OS.to_owned(),
            arch: std::env::consts::ARCH.to_owned(),
            accelerators: if cfg!(apple_silicon) {
                vec![Accelerator::Metal, Accelerator::Cpu]
            } else {
                vec![Accelerator::Cpu, Accelerator::Wasm]
            },
            memory_mb: Some(8_192),
            cores: Some(8),
        }
    }

    fn storage(&self) -> &dyn Storage {
        self
    }

    fn fetcher(&self) -> &dyn Fetcher {
        self
    }
}

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl Storage for FakeHost {
    async fn find(&self, _name: &str) -> Result<Option<String>> {
        Ok(None)
    }

    async fn create(&self, _name: &str) -> Result<Box<dyn StorageWriter>> {
        unreachable!("offers never store")
    }
}

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl Fetcher for FakeHost {
    async fn fetch(&self, _url: &str) -> Result<Box<dyn Download>> {
        unreachable!("offers never download")
    }
}

/// [`FakeHost`]'s capabilities, a storage in memory, and a fetcher that serves some URLs, a few bytes at a time.
#[derive(Default)]
pub(crate) struct MemoryHost {
    served: BTreeMap<String, Vec<u8>>,
    stored: Arc<Mutex<BTreeMap<String, Vec<u8>>>>,
    fetches: AtomicUsize,
}

impl MemoryHost {
    /// Serving each `(url, bytes)`.
    pub(crate) fn serving(files: &[(&str, &[u8])]) -> Self {
        Self {
            served: files
                .iter()
                .map(|(url, bytes)| ((*url).to_owned(), bytes.to_vec()))
                .collect(),
            ..Self::default()
        }
    }

    /// Stores `bytes` as `name`, as if installed before.
    pub(crate) fn store(&self, name: &str, bytes: &[u8]) {
        lock(&self.stored).insert(name.to_owned(), bytes.to_vec());
    }

    /// What is stored, by name.
    pub(crate) fn stored(&self) -> BTreeMap<String, Vec<u8>> {
        lock(&self.stored).clone()
    }

    /// How many downloads were started.
    pub(crate) fn fetches(&self) -> usize {
        self.fetches.load(Ordering::Relaxed)
    }
}

impl Host for MemoryHost {
    fn capabilities(&self) -> Capabilities {
        FakeHost.capabilities()
    }

    fn storage(&self) -> &dyn Storage {
        self
    }

    fn fetcher(&self) -> &dyn Fetcher {
        self
    }
}

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl Storage for MemoryHost {
    async fn find(&self, name: &str) -> Result<Option<String>> {
        Ok(lock(&self.stored)
            .contains_key(name)
            .then(|| format!("memory:{name}")))
    }

    async fn create(&self, name: &str) -> Result<Box<dyn StorageWriter>> {
        Ok(Box::new(MemoryWriter {
            name: name.to_owned(),
            bytes: Vec::new(),
            stored: Arc::clone(&self.stored),
        }))
    }
}

struct MemoryWriter {
    name: String,
    bytes: Vec<u8>,
    stored: Arc<Mutex<BTreeMap<String, Vec<u8>>>>,
}

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl StorageWriter for MemoryWriter {
    async fn write(&mut self, bytes: &[u8]) -> Result<()> {
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }

    async fn commit(self: Box<Self>) -> Result<String> {
        let location = format!("memory:{}", self.name);
        lock(&self.stored).insert(self.name, self.bytes);
        Ok(location)
    }
}

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl Fetcher for MemoryHost {
    async fn fetch(&self, url: &str) -> Result<Box<dyn Download>> {
        self.fetches.fetch_add(1, Ordering::Relaxed);
        let bytes = self.served.get(url).ok_or(Error::new("download-failed"))?;
        Ok(Box::new(MemoryDownload(bytes.clone())))
    }
}

/// The bytes left to hand over, [`MemoryDownload::PART`] at a time.
struct MemoryDownload(Vec<u8>);

impl MemoryDownload {
    const PART: usize = 4;
}

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl Download for MemoryDownload {
    fn size(&self) -> Option<u64> {
        Some(self.0.len() as u64)
    }

    async fn chunk(&mut self) -> Result<Option<Vec<u8>>> {
        if self.0.is_empty() {
            return Ok(None);
        }
        let rest = self.0.split_off(self.0.len().min(Self::PART));
        Ok(Some(std::mem::replace(&mut self.0, rest)))
    }
}

/// An artifact keyed `key`, at `url`, whose content is `bytes`.
pub(crate) fn artifact(key: &str, url: &str, bytes: &[u8]) -> Artifact {
    Artifact {
        key: key.to_owned(),
        url: url.to_owned(),
        sha256: sha256(bytes),
    }
}

/// The SHA-256 of `bytes`, in lowercase hex.
pub(crate) fn sha256(bytes: &[u8]) -> String {
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Runs `future` to its end on this thread. On the web, where a test cannot wait, it must not wait.
pub(crate) fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    #[cfg(native)]
    let waker = {
        struct Unpark(std::thread::Thread);
        impl std::task::Wake for Unpark {
            fn wake(self: Arc<Self>) {
                self.0.unpark();
            }
        }
        std::task::Waker::from(Arc::new(Unpark(std::thread::current())))
    };
    #[cfg(web)]
    let waker = std::task::Waker::noop().clone();
    let mut context = Context::from_waker(&waker);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => return output,
            #[cfg(native)]
            Poll::Pending => std::thread::park(),
            #[cfg(web)]
            Poll::Pending => panic!("a test future on the web must not wait"),
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) struct FakeCatalog;

impl CatalogSource for FakeCatalog {
    fn load(&self) -> Result<CatalogFragment> {
        let build = |id: &str, backend: &str, format: &str, memory_mb| Build {
            id: id.to_owned(),
            backend: backend.to_owned(),
            format: format.to_owned(),
            memory_mb,
            accelerators: Vec::new(),
            files: Vec::new(),
        };
        Ok(CatalogFragment {
            models: vec![
                Model {
                    id: "whisper-small".to_owned(),
                    family: "whisper".to_owned(),
                    task: Task::Stt,
                    builds: vec![
                        build("whisper-small-mlx", "mlx", "mlx", 1_024),
                        build("whisper-small-gguf", "whisper-cpp", "gguf", 1_024),
                        build("whisper-small-onnx", "sherpa-onnx", "onnx", 1_024),
                        build("whisper-small-web", "transformers-js", "onnx", 1_024),
                    ],
                },
                Model {
                    id: "whisper-large".to_owned(),
                    family: "whisper".to_owned(),
                    task: Task::Stt,
                    builds: vec![build("whisper-large-onnx", "sherpa-onnx", "onnx", 16_384)],
                },
                Model {
                    id: "kokoro".to_owned(),
                    family: "kokoro".to_owned(),
                    task: Task::Tts,
                    builds: vec![build("kokoro-onnx", "sherpa-onnx", "onnx", 512)],
                },
            ],
        })
    }
}
