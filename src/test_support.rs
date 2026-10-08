//! Test doubles shared by the tests of every module: a host (CPU and Wasm everywhere, Metal on Apple silicon, 8 GB,
//! 8 cores) with nothing stored and no network, the same host with files in memory and a few to download, a small
//! catalogue with a build for each backend, one that needs too much memory and one for a backend no build has, the
//! builders it is made with, and a way to run a future to its end.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll};

use sha2::Digest;

use crate::{
    async_trait, Accelerator, Artifact, Build, Capabilities, Capability, CatalogFragment,
    CatalogSource, Download, Error, Family, Fetcher, Host, Memory, MemorySource, Model, ModelFile,
    Requires, Result, Runs, Storage, StorageWriter, TreeWriter,
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

    async fn find_member(&self, _tree: &str, _path: &str) -> Result<Option<String>> {
        Ok(None)
    }

    async fn create(&self, _name: &str) -> Result<Box<dyn StorageWriter>> {
        unreachable!("offers never store")
    }

    async fn create_tree(&self, _name: &str) -> Result<Box<dyn TreeWriter>> {
        unreachable!("offers never store")
    }

    async fn read(&self, _name: &str) -> Result<Box<dyn Download>> {
        unreachable!("offers never read")
    }

    async fn remove(&self, _name: &str) -> Result<()> {
        unreachable!("offers never remove")
    }
}

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl Fetcher for FakeHost {
    async fn fetch(&self, _url: &str) -> Result<Box<dyn Download>> {
        unreachable!("offers never download")
    }
}

/// [`FakeHost`]'s capabilities, a storage in memory (files, and trees), and a fetcher that serves some URLs, a few
/// bytes at a time.
#[derive(Default)]
pub(crate) struct MemoryHost {
    served: BTreeMap<String, Vec<u8>>,
    stored: Arc<Mutex<BTreeMap<String, Vec<u8>>>>,
    trees: Arc<Mutex<BTreeMap<String, MemoryTree>>>,
    fetches: AtomicUsize,
}

/// A tree's paths: a file's bytes, or `None` for a directory.
pub(crate) type MemoryTree = BTreeMap<String, Option<Vec<u8>>>;

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

    /// The tree stored as `name`.
    pub(crate) fn tree(&self, name: &str) -> Option<MemoryTree> {
        lock(&self.trees).get(name).cloned()
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
        let stored = lock(&self.stored).contains_key(name) || lock(&self.trees).contains_key(name);
        Ok(stored.then(|| format!("memory:{name}")))
    }

    async fn find_member(&self, tree: &str, path: &str) -> Result<Option<String>> {
        let below = format!("{path}/");
        let found = lock(&self.trees).get(tree).is_some_and(|paths| {
            paths.contains_key(path) || paths.keys().any(|other| other.starts_with(&below))
        });
        Ok(found.then(|| format!("memory:{tree}/{path}")))
    }

    async fn create(&self, name: &str) -> Result<Box<dyn StorageWriter>> {
        Ok(Box::new(MemoryWriter {
            name: name.to_owned(),
            bytes: Vec::new(),
            stored: Arc::clone(&self.stored),
        }))
    }

    async fn create_tree(&self, name: &str) -> Result<Box<dyn TreeWriter>> {
        Ok(Box::new(MemoryTreeWriter {
            name: name.to_owned(),
            paths: MemoryTree::new(),
            current: None,
            trees: Arc::clone(&self.trees),
        }))
    }

    async fn read(&self, name: &str) -> Result<Box<dyn Download>> {
        let bytes = lock(&self.stored).get(name).cloned();
        Ok(Box::new(MemoryDownload(
            bytes.ok_or(Error::new("storage-failed"))?,
        )))
    }

    async fn remove(&self, name: &str) -> Result<()> {
        lock(&self.stored).remove(name);
        lock(&self.trees).remove(name);
        Ok(())
    }
}

struct MemoryTreeWriter {
    name: String,
    paths: MemoryTree,
    current: Option<String>,
    trees: Arc<Mutex<BTreeMap<String, MemoryTree>>>,
}

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl TreeWriter for MemoryTreeWriter {
    async fn directory(&mut self, path: &str) -> Result<()> {
        self.current = None;
        self.paths.insert(path.to_owned(), None);
        Ok(())
    }

    async fn file(&mut self, path: &str) -> Result<()> {
        self.current = Some(path.to_owned());
        self.paths.insert(path.to_owned(), Some(Vec::new()));
        Ok(())
    }

    async fn write(&mut self, bytes: &[u8]) -> Result<()> {
        let path = self.current.as_ref().ok_or(Error::new("storage-failed"))?;
        if let Some(Some(file)) = self.paths.get_mut(path) {
            file.extend_from_slice(bytes);
        }
        Ok(())
    }

    async fn commit(self: Box<Self>) -> Result<String> {
        let location = format!("memory:{}", self.name);
        lock(&self.trees).insert(self.name, self.paths);
        Ok(location)
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
        archive_path: None,
    }
}

/// An artifact keyed `key`: the member `path` of the archive at `url`, whose content is `archive`.
pub(crate) fn member(key: &str, url: &str, archive: &[u8], path: &str) -> Artifact {
    Artifact {
        archive_path: Some(path.to_owned()),
        ..artifact(key, url, archive)
    }
}

/// One entry of a tar built by [`tar`].
pub(crate) enum TarEntry<'a> {
    Directory(&'a str),
    File(&'a str, &'a [u8]),
    /// A file whose path is given by a GNU long name entry before it.
    LongNamed(&'a str, &'a [u8]),
    /// A file whose path is given by a pax header before it.
    PaxNamed(&'a str, &'a [u8]),
    /// A symbolic link to the second path.
    Symlink(&'a str, &'a str),
}

/// A tar of `entries` (ustar headers), ended by two empty blocks.
pub(crate) fn tar(entries: &[TarEntry<'_>]) -> Vec<u8> {
    fn header(path: &str, size: usize, kind: u8, link: &str) -> Vec<u8> {
        let mut header = vec![0; 512];
        let name = &path.as_bytes()[..path.len().min(100)];
        header[..name.len()].copy_from_slice(name);
        header[100..108].copy_from_slice(b"0000644\0");
        header[124..136].copy_from_slice(format!("{size:011o}\0").as_bytes());
        header[136..148].copy_from_slice(b"00000000000\0");
        header[156] = kind;
        header[157..157 + link.len()].copy_from_slice(link.as_bytes());
        header[257..263].copy_from_slice(b"ustar\0");
        header[263..265].copy_from_slice(b"00");
        header[148..156].copy_from_slice(b"        ");
        let sum: u32 = header.iter().map(|&byte| u32::from(byte)).sum();
        header[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
        header
    }
    fn content(out: &mut Vec<u8>, bytes: &[u8]) {
        out.extend_from_slice(bytes);
        out.resize(out.len().div_ceil(512) * 512, 0);
    }
    let mut out = Vec::new();
    for entry in entries {
        match entry {
            TarEntry::Directory(path) => out.extend(header(path, 0, b'5', "")),
            TarEntry::File(path, bytes) => {
                out.extend(header(path, bytes.len(), b'0', ""));
                content(&mut out, bytes);
            }
            TarEntry::LongNamed(path, bytes) => {
                let name = format!("{path}\0");
                out.extend(header("././@LongLink", name.len(), b'L', ""));
                content(&mut out, name.as_bytes());
                out.extend(header("truncated", bytes.len(), b'0', ""));
                content(&mut out, bytes);
            }
            TarEntry::PaxNamed(path, bytes) => {
                let body = format!(" path={path}\n");
                let mut length = body.len() + 1;
                while format!("{length}{body}").len() != length {
                    length += 1;
                }
                let record = format!("{length}{body}");
                out.extend(header("PaxHeader", record.len(), b'x', ""));
                content(&mut out, record.as_bytes());
                out.extend(header("truncated", bytes.len(), b'0', ""));
                content(&mut out, bytes);
            }
            TarEntry::Symlink(path, target) => out.extend(header(path, 0, b'2', target)),
        }
    }
    out.extend([0; 1024]);
    out
}

/// `bytes`, bzip2-compressed.
pub(crate) fn bzip2(bytes: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut encoder = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::fast());
    encoder.write_all(bytes).expect("in memory");
    encoder.finish().expect("in memory")
}

/// The SHA-256 of `bytes`, in lowercase hex.
pub(crate) fn sha256(bytes: &[u8]) -> String {
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Runs `future` to its end on this thread, parking it while the future waits.
#[cfg(native)]
pub(crate) fn block_on<F: Future>(future: F) -> F::Output {
    struct Unpark(std::thread::Thread);
    impl std::task::Wake for Unpark {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let mut future = pin!(future);
    let waker = std::task::Waker::from(Arc::new(Unpark(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return output;
        }
        std::thread::park();
    }
}

/// Runs `future`, which must not wait: on the web a test cannot block, and the fakes never wait.
#[cfg(web)]
pub(crate) fn block_on<F: Future>(future: F) -> F::Output {
    let mut context = Context::from_waker(std::task::Waker::noop());
    match pin!(future).poll(&mut context) {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("a test future on the web must not wait"),
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) struct FakeCatalog;

/// A build of `backend` that needs `memory_mb`, with one file.
pub(crate) fn build(id: &str, backend: &str, memory_mb: u32) -> Build {
    Build {
        id: id.to_owned(),
        backend: backend.to_owned(),
        precision: "int8".to_owned(),
        requires: Requires::default(),
        memory: Memory {
            mb: memory_mb,
            source: MemorySource::Estimated,
            basis: "a test".to_owned(),
        },
        files: vec![ModelFile {
            key: "model".to_owned(),
            url: format!("https://example.com/{id}"),
            sha256: "0".repeat(64),
            bytes: 1,
            archive_path: None,
            mutable: false,
        }],
    }
}

/// A model that can do `capability`, with `builds`.
pub(crate) fn model(id: &str, capability: Capability, builds: Vec<Build>) -> Model {
    Model {
        id: id.to_owned(),
        capabilities: vec![capability],
        parameters_m: 1,
        languages: vec!["en".to_owned()],
        license: "MIT".to_owned(),
        builds,
    }
}

/// A family of `models`.
pub(crate) fn family(id: &str, models: Vec<Model>) -> Family {
    Family {
        id: id.to_owned(),
        architecture: id.to_owned(),
        source: format!("https://example.com/{id}"),
        models,
    }
}

impl CatalogSource for FakeCatalog {
    fn load(&self) -> Result<CatalogFragment> {
        Ok(CatalogFragment {
            families: vec![
                family(
                    "whisper",
                    vec![
                        model(
                            "whisper-small",
                            Capability::Stt,
                            vec![
                                build("whisper-small-mlx", "mlx", 1_024),
                                build("whisper-small-gguf", "whisper-cpp", 1_024),
                                build("whisper-small-onnx", "sherpa-onnx", 1_024),
                                build("whisper-small-web", "transformers-js", 1_024),
                            ],
                        ),
                        model(
                            "whisper-large",
                            Capability::Stt,
                            vec![build("whisper-large-onnx", "sherpa-onnx", 16_384)],
                        ),
                    ],
                ),
                family(
                    "kokoro",
                    vec![model(
                        "kokoro",
                        Capability::Tts,
                        vec![build("kokoro-onnx", "sherpa-onnx", 512)],
                    )],
                ),
            ],
        })
    }
}
