//! Test doubles shared by the tests of every module: a host (CPU and Wasm everywhere, Metal on Apple silicon, 8 GB,
//! 8 cores) with nothing stored and no network, the same host with files in memory and a few to download, a small
//! catalogue with a build for each backend, one that needs too much memory and one for a backend no build has, the
//! builders it is made with, a provider's side of remote calls (keys, scripted answers, the requests made), and a way
//! to run a future to its end.

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use sha2::Digest;

#[cfg(native)]
use crate::TreeWriter;
use crate::{
    async_trait, Accelerator, Artifact, BuildEntry, Capabilities, Capability, CatalogFragment,
    CatalogSource, Credentials, Download, Error, Family, Fetcher, FolderWriter, Host, HttpClient,
    HttpRequest, HttpResponse, Memory, MemorySource, ModelEntry, ModelFile, NoCredentials,
    Requires, Result, Runs, Storage, StorageWriter,
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

    fn http(&self) -> &dyn HttpClient {
        self
    }

    fn credentials(&self) -> &dyn Credentials {
        &NoCredentials
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

    async fn read(&self, _name: &str) -> Result<Box<dyn Download>> {
        unreachable!("offers never read")
    }

    async fn remove(&self, _name: &str) -> Result<()> {
        unreachable!("offers never remove")
    }

    #[cfg(native)]
    fn open(&self, _name: &str) -> Result<Box<dyn std::io::Read + Send>> {
        unreachable!("offers never read")
    }

    #[cfg(native)]
    fn create_tree(&self, _name: &str) -> Result<Box<dyn TreeWriter>> {
        unreachable!("offers never store")
    }

    async fn find_folder(&self, _name: &str) -> Result<Option<String>> {
        Ok(None)
    }

    async fn find_in_folder(&self, _name: &str, _path: &str) -> Result<Option<String>> {
        Ok(None)
    }

    async fn create_folder(&self, _name: &str) -> Result<Box<dyn FolderWriter>> {
        unreachable!("offers never store")
    }

    async fn remove_folder(&self, _name: &str) -> Result<()> {
        unreachable!("offers never remove")
    }

    async fn is_linked(&self, _name: &str) -> Result<bool> {
        Ok(false)
    }
}

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl Fetcher for FakeHost {
    async fn fetch(&self, _url: &str) -> Result<Box<dyn Download>> {
        unreachable!("offers never download")
    }
}

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl HttpClient for FakeHost {
    async fn send(&self, _request: HttpRequest) -> Result<HttpResponse> {
        Err(Error::new("request-failed"))
    }
}

/// [`FakeHost`]'s capabilities, a storage in memory (files, trees, and build folders of links), and a fetcher that
/// serves some URLs, a few bytes at a time.
#[derive(Default)]
pub(crate) struct MemoryHost {
    served: BTreeMap<String, Vec<u8>>,
    stored: Arc<Mutex<BTreeMap<String, Vec<u8>>>>,
    trees: Arc<Mutex<BTreeMap<String, MemoryTree>>>,
    folders: Arc<Mutex<BTreeMap<String, MemoryFolder>>>,
    fetches: AtomicUsize,
    remote: Arc<FakeProvider>,
}

/// A build folder's links: each path, and the blob (and member of it) it links.
pub(crate) type MemoryFolder = BTreeMap<String, (String, Option<String>)>;

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
    #[cfg(native)]
    pub(crate) fn tree(&self, name: &str) -> Option<MemoryTree> {
        lock(&self.trees).get(name).cloned()
    }

    /// The build folder stored as `name`.
    pub(crate) fn folder(&self, name: &str) -> Option<MemoryFolder> {
        lock(&self.folders).get(name).cloned()
    }

    /// How many downloads were started.
    pub(crate) fn fetches(&self) -> usize {
        self.fetches.load(Ordering::Relaxed)
    }

    /// The providers' side of its API calls, and its keys: shared, so a test keeps it once the host is moved.
    pub(crate) fn remote(&self) -> Arc<FakeProvider> {
        Arc::clone(&self.remote)
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

    fn http(&self) -> &dyn HttpClient {
        self.remote.as_ref()
    }

    fn credentials(&self) -> &dyn Credentials {
        self.remote.as_ref()
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

    #[cfg(native)]
    fn open(&self, name: &str) -> Result<Box<dyn std::io::Read + Send>> {
        let bytes = lock(&self.stored).get(name).cloned();
        Ok(Box::new(MemoryReader(std::io::Cursor::new(
            bytes.ok_or(Error::new("storage-failed"))?,
        ))))
    }

    #[cfg(native)]
    fn create_tree(&self, name: &str) -> Result<Box<dyn TreeWriter>> {
        Ok(Box::new(MemoryTreeWriter {
            name: name.to_owned(),
            paths: MemoryTree::new(),
            current: None,
            trees: Arc::clone(&self.trees),
        }))
    }

    async fn find_folder(&self, name: &str) -> Result<Option<String>> {
        let stored = lock(&self.folders).contains_key(name);
        Ok(stored.then(|| format!("memory:models/{name}")))
    }

    async fn find_in_folder(&self, name: &str, path: &str) -> Result<Option<String>> {
        let below = format!("{path}/");
        let found = lock(&self.folders).get(name).is_some_and(|links| {
            links.contains_key(path) || links.keys().any(|other| other.starts_with(&below))
        });
        Ok(found.then(|| format!("memory:models/{name}/{path}")))
    }

    async fn create_folder(&self, name: &str) -> Result<Box<dyn FolderWriter>> {
        Ok(Box::new(MemoryFolderWriter {
            name: name.to_owned(),
            links: MemoryFolder::new(),
            stored: Arc::clone(&self.stored),
            trees: Arc::clone(&self.trees),
            folders: Arc::clone(&self.folders),
        }))
    }

    async fn remove_folder(&self, name: &str) -> Result<()> {
        lock(&self.folders).remove(name);
        Ok(())
    }

    async fn is_linked(&self, name: &str) -> Result<bool> {
        let folders = lock(&self.folders);
        let mut links = folders.values().flat_map(BTreeMap::values);
        Ok(links.any(|(blob, _)| blob == name))
    }
}

#[cfg(native)]
struct MemoryTreeWriter {
    name: String,
    paths: MemoryTree,
    current: Option<String>,
    trees: Arc<Mutex<BTreeMap<String, MemoryTree>>>,
}

#[cfg(native)]
impl TreeWriter for MemoryTreeWriter {
    fn directory(&mut self, path: &str) -> Result<()> {
        self.current = None;
        self.paths.insert(path.to_owned(), None);
        Ok(())
    }

    fn file(&mut self, path: &str) -> Result<()> {
        self.current = Some(path.to_owned());
        self.paths.insert(path.to_owned(), Some(Vec::new()));
        Ok(())
    }

    fn write(&mut self, bytes: &[u8]) -> Result<()> {
        let path = self.current.as_ref().ok_or(Error::new("storage-failed"))?;
        if let Some(Some(file)) = self.paths.get_mut(path) {
            file.extend_from_slice(bytes);
        }
        Ok(())
    }

    fn commit(self: Box<Self>) -> Result<String> {
        let location = format!("memory:{}", self.name);
        lock(&self.trees).insert(self.name, self.paths);
        Ok(location)
    }
}

struct MemoryFolderWriter {
    name: String,
    links: MemoryFolder,
    stored: Arc<Mutex<BTreeMap<String, Vec<u8>>>>,
    trees: Arc<Mutex<BTreeMap<String, MemoryTree>>>,
    folders: Arc<Mutex<BTreeMap<String, MemoryFolder>>>,
}

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl FolderWriter for MemoryFolderWriter {
    async fn link(&mut self, path: &str, blob: &str, member: Option<&str>) -> Result<()> {
        let exists = match member {
            None => lock(&self.stored).contains_key(blob),
            Some(member) => lock(&self.trees).get(blob).is_some_and(|paths| {
                let below = format!("{member}/");
                paths.contains_key(member) || paths.keys().any(|path| path.starts_with(&below))
            }),
        };
        if !exists {
            return Err(Error::new("storage-failed"));
        }
        self.links.insert(
            path.to_owned(),
            (blob.to_owned(), member.map(str::to_owned)),
        );
        Ok(())
    }

    async fn commit(self: Box<Self>) -> Result<String> {
        let location = format!("memory:models/{}", self.name);
        lock(&self.folders).entry(self.name).or_insert(self.links);
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

/// A stored file read synchronously, [`MemoryDownload::PART`] bytes at a time at most.
#[cfg(native)]
struct MemoryReader(std::io::Cursor<Vec<u8>>);

#[cfg(native)]
impl std::io::Read for MemoryReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let part = buf.len().min(MemoryDownload::PART);
        self.0.read(&mut buf[..part])
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
#[cfg(native)]
pub(crate) enum TarEntry<'a> {
    Directory(&'a str),
    File(&'a str, &'a [u8]),
    /// A file whose path is given by a GNU long name entry before it.
    LongNamed(&'a str, &'a [u8]),
    /// A file whose path is given by a pax header before it.
    PaxNamed(&'a str, &'a [u8]),
    /// A symbolic link to the second path.
    Symlink(&'a str, &'a str),
    /// An entry of another kind (a hard link, a device, a FIFO), by its type flag.
    Special(&'a str, u8),
}

/// A tar of `entries` (ustar headers), ended by two empty blocks.
#[cfg(native)]
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
            TarEntry::Special(path, kind) => out.extend(header(path, 0, *kind, "")),
        }
    }
    out.extend([0; 1024]);
    out
}

/// `bytes`, bzip2-compressed.
#[cfg(native)]
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

/// Runs `future` to its end on the tests' Tokio runtime, one for every test as an app has one: the native engine's
/// futures expect it, and a client's pooled connections outlive a single call.
#[cfg(native)]
pub(crate) fn block_on<F: Future>(future: F) -> F::Output {
    static RUNTIME: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("a Tokio runtime")
        })
        .block_on(future)
}

/// Runs `future`, which must not wait: on the web a test cannot block, and the fakes never wait.
#[cfg(web)]
pub(crate) fn block_on<F: Future>(future: F) -> F::Output {
    use std::pin::pin;
    use std::task::{Context, Poll};

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
pub(crate) fn build(id: &str, backend: &str, memory_mb: u32) -> BuildEntry {
    BuildEntry {
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
        call_params: Default::default(),
        config: Default::default(),
    }
}

/// A model that can do `capability`, with `builds`.
pub(crate) fn model(id: &str, capability: Capability, builds: Vec<BuildEntry>) -> ModelEntry {
    ModelEntry {
        id: id.to_owned(),
        capabilities: vec![capability],
        parameters_m: 1,
        languages: vec!["en".to_owned()],
        languages_source: None,
        license: "MIT".to_owned(),
        voices: Vec::new(),
        builds,
    }
}

/// A family of `models`.
pub(crate) fn family(id: &str, models: Vec<ModelEntry>) -> Family {
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
                                build("whisper-small-onnx", "sherpa-onnx", 1_024),
                                build("whisper-small-gguf", "whisper-cpp", 1_024),
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

/// The output of `future`, which must be ready on its first poll: the engine's futures that do not wait on a host
/// (a backend's `load`, a loaded model's work) are, and the tests have no executor.
#[cfg(native)]
pub(crate) fn ready<F: std::future::Future>(future: F) -> F::Output {
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    match std::pin::pin!(future).poll(&mut context) {
        std::task::Poll::Ready(output) => output,
        std::task::Poll::Pending => panic!("the future waits on something"),
    }
}

/// Waits `ms` milliseconds, letting the page run what it queued (a task spawned with `spawn_local`).
#[cfg(web)]
pub(crate) async fn pause(ms: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        let set_timeout =
            js_sys::Reflect::get(&js_sys::global(), &"setTimeout".into()).expect("setTimeout");
        let set_timeout: js_sys::Function = wasm_bindgen::JsCast::unchecked_into(set_timeout);
        set_timeout
            .call2(&wasm_bindgen::JsValue::UNDEFINED, &resolve, &ms.into())
            .expect("a timeout");
    });
    wasm_bindgen_futures::JsFuture::from(promise)
        .await
        .expect("resolved");
}

/// What remote calls meet in tests: the keys the host has, an answer for each URL prefix, and every request made.
#[derive(Default)]
pub(crate) struct FakeProvider {
    keys: Mutex<BTreeMap<String, String>>,
    answers: Mutex<Vec<(String, HttpResponse)>>,
    requests: Mutex<Vec<HttpRequest>>,
    /// Cancelled when a request whose URL starts with its prefix is answered: what a person cancelling while that request
    /// is out does.
    cancels: Mutex<Vec<(String, crate::Cancel)>>,
}

impl FakeProvider {
    /// `cancel` is cancelled while a request whose URL starts with `prefix` is out, before it is answered.
    pub(crate) fn cancel_during(&self, prefix: &str, cancel: &crate::Cancel) {
        lock(&self.cancels).push((prefix.to_owned(), cancel.clone()));
    }

    /// The host has `key` for `provider`.
    pub(crate) fn key(&self, provider: &str, key: &str) {
        lock(&self.keys).insert(provider.to_owned(), key.to_owned());
    }

    /// The host has no key for `provider` any more.
    pub(crate) fn forget(&self, provider: &str) {
        lock(&self.keys).remove(provider);
    }

    /// A request whose URL starts with `prefix` is answered with `status` and `body` (the last such answer given: a later one
    /// replaces it).
    pub(crate) fn answer(&self, prefix: &str, status: u16, body: &[u8]) {
        let response = HttpResponse {
            status,
            body: body.to_vec(),
        };
        lock(&self.answers).insert(0, (prefix.to_owned(), response));
    }

    /// Every request made, in order.
    pub(crate) fn requests(&self) -> Vec<HttpRequest> {
        lock(&self.requests).clone()
    }
}

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl HttpClient for FakeProvider {
    /// `request-failed` for a URL nothing answers.
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse> {
        let answer = lock(&self.answers)
            .iter()
            .find(|(prefix, _)| request.url.starts_with(prefix.as_str()))
            .map(|(_, response)| response.clone());
        for (prefix, cancel) in lock(&self.cancels).iter() {
            if request.url.starts_with(prefix.as_str()) {
                cancel.cancel();
            }
        }
        lock(&self.requests).push(request);
        answer.ok_or(Error::new("request-failed"))
    }
}

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl Credentials for FakeProvider {
    async fn credential(&self, provider: &str) -> Result<Option<String>> {
        Ok(lock(&self.keys).get(provider).cloned())
    }
}

/// The API of `provider` through a host with no keys yet, and that host's provider side.
pub(crate) fn remote_api(provider: &'static str) -> (crate::provider::Api, Arc<FakeProvider>) {
    let host = MemoryHost::default();
    let remote = host.remote();
    let host: Arc<dyn Host> = Arc::new(host);
    (crate::provider::Api::new(&host, provider), remote)
}

/// The value of `request`'s header `name`, whatever its case.
pub(crate) fn header<'a>(request: &'a HttpRequest, name: &str) -> Option<&'a str> {
    let found = request
        .headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name));
    found.map(|(_, value)| value.as_str())
}

/// Whether `body` holds `part`.
pub(crate) fn contains(body: &[u8], part: &str) -> bool {
    body.windows(part.len())
        .any(|window| window == part.as_bytes())
}

/// The parts of OpenAI's spec its facts come from, shaped as the spec shapes them: what the provider's tests read,
/// and what a fake provider serves at its URL.
pub(crate) fn openai_spec() -> serde_json::Value {
    use serde_json::json;
    json!({ "components": { "schemas": {
        "CreateTranscriptionRequest": { "properties": {
            "model": { "anyOf": [{ "type": "string" }, { "type": "string", "enum": ["whisper-1", "gpt-4o-transcribe", "gpt-4o-transcribe-diarize"] }] },
            "language": { "type": "string" },
            "chunking_strategy": { "anyOf": [
                { "description": "Controls how the audio is cut into chunks. Required when using `gpt-4o-transcribe-diarize` for inputs longer than 30 seconds.", "anyOf": [{ "type": "string", "enum": ["auto"] }, { "$ref": "#/components/schemas/VadConfig" }] },
                { "type": "null" },
            ]},
        }},
        "CreateSpeechRequest": { "properties": {
            "model": { "anyOf": [{ "type": "string" }, { "type": "string", "enum": ["tts-1", "gpt-4o-mini-tts"] }] },
            "voice": { "anyOf": [
                { "anyOf": [{ "$ref": "#/components/schemas/VoiceIdsShared" }, { "type": "string", "enum": ["fable", "nova"] }] },
                { "type": "object", "properties": { "id": { "type": "string" } } },
            ]},
            "speed": { "type": "number", "minimum": 0.25, "maximum": 4 },
            "response_format": { "type": "string", "enum": ["mp3", "pcm"] },
        }},
        "VoiceIdsShared": { "anyOf": [{ "type": "string" }, { "type": "string", "enum": ["alloy", "ash"] }] },
    }}})
}
