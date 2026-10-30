//! The engine over fake hosts, catalogues and a fake backend. What the local catalogue's models say: builds ranked, those that
//! run here first, the rest with why not, and what is installed. Then the interface: install, load (installing first,
//! one model in memory per build, one library per backend, closed with its last model), the build `load` chooses,
//! what fails and why, uninstalling, when each model was last loaded (noted on every load, kept in the host's storage,
//! forgotten with its last build), and a loaded model's calls: resampled audio, voices described by the catalogue,
//! and calls on one model waiting for one another.

#[cfg(native)]
use std::future::Future;
#[cfg(native)]
use std::pin::pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
#[cfg(native)]
use std::task::{Context, Poll};

use crate::backend::{Backend, BackendModel, BackendSpec, Library, SttModel, TtsModel};
use crate::catalog::{CatalogFragment, CatalogSource};
use crate::install::Installed;
use crate::test_support::{
    block_on, build, family, model, sha256, FakeCatalog, FakeHost, MemoryHost,
};
use crate::{
    async_trait, Accelerator, AcceleratorClass, BuildEntry, BundledCatalog, Cancel, Capability,
    Engine, Error, Gender, LocalModel, ModelFile, Reason, Result, Voice,
};
#[cfg(native)]
use crate::{VadOptions, VadStream};

#[cfg(web)]
use wasm_bindgen_test::wasm_bindgen_test as test;

#[test]
fn models_rank_the_builds_that_run_here_first_and_say_why_the_rest_do_not() {
    let engine = Engine::new(Box::new(FakeHost), vec![Box::new(FakeCatalog)]).expect("engine");
    let models = block_on(engine.models()).expect("models");
    let small = models
        .iter()
        .find(|m| m.id == "whisper-small")
        .expect("small");
    assert_eq!(small.family, "whisper", "its family, from the catalogue");

    // Not MLX on Apple silicon: the stub runs nothing yet (ENG-08). whisper.cpp on Metal there, a GPU, before
    // sherpa-onnx on the CPU (sidevoice-engine#4); elsewhere both are on the CPU, and catalogue order stands.
    let recommended = if cfg!(web) {
        "whisper-small-web"
    } else if cfg!(apple_silicon) {
        "whisper-small-gguf"
    } else {
        "whisper-small-onnx"
    };
    assert_eq!(small.recommended_build.as_deref(), Some(recommended));
    assert_eq!(small.builds[0].id, recommended);
    assert!(small.builds[0].available && small.builds[0].accelerator.is_some());
    // Each build that runs says its accelerator's class; one that does not, none.
    let class = if cfg!(apple_silicon) {
        AcceleratorClass::Gpu
    } else {
        AcceleratorClass::Cpu
    };
    assert_eq!(small.builds[0].accelerator_class(), Some(class));
    for build in &small.builds {
        assert_eq!(
            build.accelerator_class(),
            build.accelerator.map(Accelerator::class),
            "{}",
            build.id
        );
    }
    let last = small.builds.last().expect("builds");
    assert!(!last.available && last.accelerator_class().is_none());
    assert!(small.builds[0].reasons.is_empty());
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        let mlx = small
            .builds
            .iter()
            .find(|b| b.id == "whisper-small-mlx")
            .expect("mlx");
        assert!(!mlx.available && mlx.accelerator.is_none());
        assert_eq!(mlx.reasons, [Reason::new("not-implemented")]);
    }
    let gguf = small
        .builds
        .iter()
        .find(|b| b.id == "whisper-small-gguf")
        .expect("gguf");
    if cfg!(apple_silicon) {
        assert_eq!(gguf.accelerator, Some(Accelerator::Metal));
    } else if cfg!(not(target_arch = "wasm32")) {
        assert_eq!(gguf.accelerator, Some(Accelerator::Cpu));
    } else {
        assert!(!gguf.available && gguf.accelerator.is_none());
        assert_eq!(gguf.reasons, [Reason::new("backend-not-in-this-build")]);
    }
    let available: Vec<_> = small.builds.iter().map(|build| build.available).collect();
    assert!(
        available.windows(2).all(|pair| pair[0] >= pair[1]),
        "runs first"
    );
    assert!(!small.installed && small.builds.iter().all(|build| !build.installed));
    assert_eq!((small.parameters_m, small.download_bytes()), (1, 1));

    let large = models
        .iter()
        .find(|m| m.id == "whisper-large")
        .expect("large");
    assert_eq!(large.recommended_build, None);
    let why = if cfg!(target_arch = "wasm32") {
        // Its only build is native: on the web, its backend does not exist.
        Reason::new("backend-not-in-this-build")
    } else {
        Reason::with_numbers("memory", 16_384, 8_192)
    };
    assert_eq!(large.builds[0].reasons, [why]);
}

/// A model's builds' download sizes, for the test above: each build of the fake catalogue downloads one byte.
trait DownloadBytes {
    fn download_bytes(&self) -> u64;
}

impl DownloadBytes for crate::LocalModelInfo {
    fn download_bytes(&self) -> u64 {
        self.builds[0].download_bytes
    }
}

#[cfg(native)]
#[test]
fn a_native_engine_its_futures_and_its_loaded_models_can_cross_threads() {
    fn shared<T: Send + Sync>(_: &T) {}
    fn sent<T: Send>(_: T) {}
    fn shared_type<T: Send + Sync>() {}

    let engine = Engine::new(Box::new(FakeHost), vec![Box::new(FakeCatalog)]).expect("engine");
    shared(&engine);
    sent(engine.models());
    sent(engine.load("whisper-small", None, &|_| {}, &Cancel::new()));
    sent(engine.install("whisper-small", None, &|_| {}, &Cancel::new()));
    shared_type::<LocalModel>();
}

/// What a loaded model's calls return can cross threads natively: an app awaits them on its own runtime's tasks. Checked
/// when this compiles; never called.
#[cfg(native)]
#[allow(dead_code, reason = "a check at compile time")]
fn a_loaded_models_futures_are_send(loaded: &LocalModel, stream: &mut VadStream) {
    fn sent<T: Send>(_: T) {}
    if let Some(stt) = loaded.as_stt() {
        sent(stt.transcribe(&[], 16_000, None));
    }
    if let Some(tts) = loaded.as_tts() {
        sent(tts.voices());
        sent(tts.speak("", "", None, None));
    }
    if let Some(vad) = loaded.as_vad() {
        sent(vad.stream(VadOptions::default()));
    }
    if let Some(end_of_turn) = loaded.as_end_of_turn() {
        sent(end_of_turn.probability(&[], 16_000));
    }
    sent(stream.accept(&[]));
}

/// What each CI platform offers with the catalogue this repository ships: every bundled model of a capability, each
/// on a backend this platform compiles, and which of a model's builds the ranking makes win (sidevoice-engine#4).
#[test]
fn the_bundled_catalogue_offers_every_model_on_this_platforms_backends() {
    let engine = Engine::new(Box::new(FakeHost), vec![Box::new(BundledCatalog)]).expect("engine");
    let models = block_on(engine.models()).expect("models");
    let offered = |capability| {
        let mut offered: Vec<_> = models
            .iter()
            .filter(|model| model.capabilities.contains(&capability))
            .filter_map(|model| {
                let recommended = model.recommended_build.as_ref()?;
                let build = model.builds.iter().find(|build| &build.id == recommended)?;
                Some((model.id.clone(), build.backend.clone()))
            })
            .collect();
        offered.sort();
        offered
    };
    let backends: &[&str] = if cfg!(target_arch = "wasm32") {
        &["transformers-js"]
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        &["mlx", "sherpa-onnx", "whisper-cpp", "onnxruntime"]
    } else {
        &["sherpa-onnx", "whisper-cpp", "onnxruntime"]
    };

    let stt = offered(Capability::Stt);
    let models: Vec<_> = stt.iter().map(|(model, _)| model.as_str()).collect();
    if cfg!(target_arch = "wasm32") {
        // Not large-v3: its q8 build is over its WebAssembly cap (`wasm-memory`), and its fp16 build runs on WebGPU
        // only, which this host does not report.
        assert_eq!(
            models,
            [
                "whisper-base",
                "whisper-large-v3-turbo",
                "whisper-small",
                "whisper-tiny"
            ]
        );
    } else {
        assert_eq!(
            models,
            [
                "canary-180m-flash",
                "fastconformer-es-large",
                "parakeet-tdt-0.6b-v3",
                "qwen3-asr-0.6b",
                "whisper-base",
                "whisper-large-v3",
                "whisper-large-v3-turbo",
                "whisper-small",
                "whisper-tiny"
            ]
        );
    }
    for (model, backend) in &stt {
        assert!(backends.contains(&backend.as_str()), "{model} on {backend}");
    }
    // Whisper: on Apple silicon whisper.cpp runs on Metal, a GPU, so it wins every model over sherpa-onnx on the CPU
    // (sidevoice-engine#4); natively elsewhere both run on the CPU and catalogue order keeps sherpa-onnx.
    let whisper = if cfg!(web) {
        "transformers-js"
    } else if cfg!(apple_silicon) {
        "whisper-cpp"
    } else {
        "sherpa-onnx"
    };
    for (model, backend) in stt
        .iter()
        .filter(|(model, _)| model.starts_with("whisper-"))
    {
        assert_eq!(backend, whisper, "{model}");
    }

    let tts = offered(Capability::Tts);
    if cfg!(target_arch = "wasm32") {
        let web = |model: &str| (model.to_owned(), "transformers-js".to_owned());
        assert_eq!(
            tts,
            [
                web("kokoro-82m-v0.19"),
                web("kokoro-82m-v1.0"),
                web("supertonic-2")
            ]
        );
    } else {
        let models: Vec<_> = tts.iter().map(|(model, _)| model.as_str()).collect();
        assert_eq!(
            models,
            [
                "kokoro-82m-v0.19",
                "kokoro-82m-v1.0",
                "piper-en_US-ljspeech-medium",
                "piper-es_ES-carlfm-x_low",
                "supertonic-3"
            ]
        );
        assert!(tts.iter().all(|(_, backend)| backend == "sherpa-onnx"));
    }

    let vad = offered(Capability::Vad);
    let backend = if cfg!(target_arch = "wasm32") {
        "transformers-js"
    } else {
        "sherpa-onnx"
    };
    assert_eq!(vad, [("silero-vad".to_owned(), backend.to_owned())]);

    let end_of_turn = offered(Capability::EndOfTurn);
    let backend = if cfg!(target_arch = "wasm32") {
        "transformers-js"
    } else {
        "onnxruntime"
    };
    assert_eq!(
        end_of_turn,
        [("smart-turn-v3.2".to_owned(), backend.to_owned())]
    );
    // Every build of it runs on the CPU, or every one in a page on the same accelerator: catalogue order, fp32 first.
    let all = block_on(engine.models()).expect("models");
    let smart_turn = all
        .iter()
        .find(|model| model.id == "smart-turn-v3.2")
        .expect("smart-turn");
    let recommended = smart_turn.recommended_build.as_deref().unwrap_or_default();
    assert!(recommended.ends_with("-fp32"), "{recommended}");
}

/// A file of a fake build: `https://models/<id>`, holding `<id>`'s bytes.
fn file(id: &str) -> ModelFile {
    ModelFile {
        key: "model.onnx".to_owned(),
        url: format!("https://models/{id}"),
        sha256: sha256(id.as_bytes()),
        bytes: id.len() as u64,
        archive_path: None,
        mutable: false,
    }
}

/// A build of the fake backend, `id`, with its file.
fn fake(id: &str) -> BuildEntry {
    BuildEntry {
        files: vec![file(id)],
        ..build(id, "fake", 0)
    }
}

/// The fake catalogue: `ear` (speech to text, two builds), `voice` (text to speech, voices "a" declared and "b" not),
/// `broken` (fails to load), and `other`, which shares `ear`'s first file.
struct FakeModels;

impl CatalogSource for FakeModels {
    fn load(&self) -> Result<CatalogFragment> {
        let mut voice = model("voice", Capability::Tts, vec![fake("voice-1")]);
        voice.languages = vec!["es".to_owned(), "en".to_owned()];
        voice.voices = vec![Voice {
            id: "a".to_owned(),
            name: None,
            languages: vec!["es".to_owned()],
            gender: Some(Gender::Female),
        }];
        let mut other = model("other", Capability::Stt, vec![fake("other-1")]);
        other.builds[0].files.push(ModelFile {
            key: "shared".to_owned(),
            ..file("ear-1")
        });
        Ok(CatalogFragment {
            families: vec![family(
                "fake",
                vec![
                    model("ear", Capability::Stt, vec![fake("ear-1"), fake("ear-2")]),
                    voice,
                    model("broken", Capability::Stt, vec![fake("broken-1")]),
                    other,
                ],
            )],
        })
    }
}

/// A backend whose library counts how many times it was opened and how many are open, and whose models count how
/// many were loaded and how many of their calls run at once.
#[derive(Default, Clone)]
struct Counters {
    opened: Arc<AtomicUsize>,
    open: Arc<AtomicUsize>,
    loaded: Arc<AtomicUsize>,
    /// Loads that reached the library, finished or not.
    loading: Arc<AtomicUsize>,
    running: Arc<AtomicUsize>,
    most_running: Arc<AtomicUsize>,
}

struct FakeBackend(Counters);

const FAKE: BackendSpec = BackendSpec {
    id: "fake",
    name: "Fake",
    description: "A test double that counts what it opens and loads.",
    upstream: "https://example.com",
    accelerators: &[Accelerator::Cpu],
    requirements: &[],
};

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl Backend for FakeBackend {
    fn spec(&self) -> &BackendSpec {
        &FAKE
    }

    async fn open(&self, _files: &Installed) -> Result<Box<dyn Library>> {
        self.0.opened.fetch_add(1, Ordering::Relaxed);
        self.0.open.fetch_add(1, Ordering::Relaxed);
        Ok(Box::new(FakeLibrary(self.0.clone())))
    }
}

struct FakeLibrary(Counters);

impl Drop for FakeLibrary {
    fn drop(&mut self) {
        self.0.open.fetch_sub(1, Ordering::Relaxed);
    }
}

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl Library for FakeLibrary {
    async fn load(
        &self,
        build: &BuildEntry,
        _accelerator: Accelerator,
        files: &Installed,
    ) -> Result<Box<dyn BackendModel>> {
        if build.id.starts_with("broken") {
            return Err(Error::new("model-load-failed"));
        }
        assert!(files.file("model.onnx").is_some());
        self.0.loading.fetch_add(1, Ordering::Relaxed);
        // Natively, loading waits once, so that something else can run while a load is past its install and its model
        // is not yet in memory (the web's test executor cannot wait).
        #[cfg(native)]
        YieldOnce(false).await;
        assert!(
            files.file("model.onnx").is_some(),
            "its files outlive the load"
        );
        self.0.loaded.fetch_add(1, Ordering::Relaxed);
        Ok(Box::new(FakeModel(self.0.clone())))
    }
}

/// Speech to text that says how many samples it heard and in which language, yielding once on the way; and text to
/// speech with voices "a" and "b" at 8 kHz, which says as many samples as the text has bytes, times the speed.
struct FakeModel(Counters);

impl BackendModel for FakeModel {
    fn as_stt(&mut self) -> Option<&mut dyn SttModel> {
        Some(self)
    }

    fn as_tts(&mut self) -> Option<&mut dyn TtsModel> {
        Some(self)
    }

    fn memory_mb(&self) -> Option<u32> {
        None
    }
}

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl SttModel for FakeModel {
    async fn transcribe(&mut self, pcm: &[f32], language: Option<&str>) -> Result<String> {
        let running = self.0.running.fetch_add(1, Ordering::Relaxed) + 1;
        self.0.most_running.fetch_max(running, Ordering::Relaxed);
        // Natively, the call waits once, so that two calls overlap if nothing stops them (the web's test executor
        // cannot wait).
        #[cfg(native)]
        YieldOnce(false).await;
        self.0.running.fetch_sub(1, Ordering::Relaxed);
        Ok(format!("{} samples in {language:?}", pcm.len()))
    }
}

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl TtsModel for FakeModel {
    fn voices(&self) -> Vec<String> {
        vec!["a".to_owned(), "b".to_owned()]
    }

    fn sample_rate(&self) -> u32 {
        8_000
    }

    async fn speak(
        &mut self,
        text: &str,
        voice: &Voice,
        _language: Option<&str>,
        speed: f32,
    ) -> Result<Vec<f32>> {
        if voice.id != "a" && voice.id != "b" {
            return Err(Error::new("unknown-voice"));
        }
        Ok(vec![0.0; (text.len() as f32 * speed) as usize])
    }
}

/// A future that waits once: pending the first time it is polled (waking itself), ready the second.
#[cfg(native)]
struct YieldOnce(bool);

#[cfg(native)]
impl Future for YieldOnce {
    type Output = ();

    fn poll(mut self: std::pin::Pin<&mut Self>, context: &mut Context<'_>) -> Poll<()> {
        if self.0 {
            Poll::Ready(())
        } else {
            self.0 = true;
            context.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

fn served() -> MemoryHost {
    let ids = ["ear-1", "ear-2", "voice-1", "broken-1", "other-1"];
    let urls: Vec<_> = ids
        .iter()
        .map(|id| format!("https://models/{id}"))
        .collect();
    let files: Vec<(&str, &[u8])> = urls
        .iter()
        .zip(ids)
        .map(|(url, id)| (url.as_str(), id.as_bytes()))
        .collect();
    MemoryHost::serving(&files)
}

struct Fixture {
    engine: Engine,
    counters: Counters,
    /// The engine's host, shared: its storage and its clock.
    host: MemoryHost,
}

impl Fixture {
    fn new() -> Self {
        Self::over(served())
    }

    /// An engine over `host`.
    fn over(host: MemoryHost) -> Self {
        let counters = Counters::default();
        let backend = FakeBackend(counters.clone());
        let shared = host.sharing();
        let engine = Engine::with_backends(
            Box::new(host),
            vec![Box::new(FakeModels)],
            vec![Box::new(backend)],
        )
        .expect("engine");
        Self {
            engine,
            counters,
            host: shared,
        }
    }

    /// When the listing says `model` was last loaded.
    fn last_used(&self, model: &str) -> Option<u64> {
        let models = block_on(self.engine.models()).expect("models");
        models
            .iter()
            .find(|m| m.id == model)
            .expect("listed")
            .last_used_at
    }

    fn load(&self, model: &str, build: Option<&str>) -> Result<LocalModel> {
        block_on(self.engine.load(model, build, &|_| {}, &Cancel::new()))
    }

    fn install(&self, model: &str, build: Option<&str>) -> Result<()> {
        block_on(self.engine.install(model, build, &|_| {}, &Cancel::new()))
    }

    fn installed(&self, model: &str) -> Vec<(String, bool)> {
        let models = block_on(self.engine.models()).expect("models");
        let model = models.iter().find(|m| m.id == model).expect("listed");
        model
            .builds
            .iter()
            .map(|build| (build.id.clone(), build.installed))
            .collect()
    }

    fn count(counter: &AtomicUsize) -> usize {
        counter.load(Ordering::Relaxed)
    }
}

fn pair(build: &str, installed: bool) -> (String, bool) {
    (build.to_owned(), installed)
}

#[test]
fn installing_stores_a_build_and_models_say_it_is_installed() {
    let fixture = Fixture::new();
    assert_eq!(
        fixture.installed("ear"),
        [pair("ear-1", false), pair("ear-2", false)]
    );
    fixture.install("ear", Some("ear-2")).expect("installed");
    assert_eq!(
        fixture.installed("ear"),
        [pair("ear-1", false), pair("ear-2", true)]
    );
    let models = block_on(fixture.engine.models()).expect("models");
    assert!(
        models
            .iter()
            .find(|m| m.id == "ear")
            .expect("ear")
            .installed
    );
    assert_eq!(
        Fixture::count(&fixture.counters.opened),
        0,
        "installing loads nothing"
    );
}

#[test]
fn loading_installs_first_and_a_build_loaded_twice_is_one_model_with_one_library() {
    let fixture = Fixture::new();
    let first = fixture.load("ear", None).expect("loaded");
    assert_eq!((first.id(), first.build()), ("ear", "ear-1"));
    assert_eq!(first.capabilities(), [Capability::Stt, Capability::Tts]);
    assert_eq!(fixture.installed("ear")[0], pair("ear-1", true));

    let again = fixture.load("ear", Some("ear-1")).expect("loaded again");
    let voice = fixture.load("voice", None).expect("another model");
    let counters = &fixture.counters;
    assert_eq!(Fixture::count(&counters.loaded), 2, "ear once, voice once");
    assert_eq!(
        (
            Fixture::count(&counters.opened),
            Fixture::count(&counters.open)
        ),
        (1, 1),
        "one library"
    );

    drop((first, voice));
    assert_eq!(Fixture::count(&counters.open), 1, "`again` still holds ear");
    drop(again);
    assert_eq!(
        Fixture::count(&counters.open),
        0,
        "closed with its last model"
    );

    fixture.load("ear", None).expect("loaded once more");
    assert_eq!(Fixture::count(&counters.opened), 2);
    assert_eq!(Fixture::count(&counters.loaded), 3);
}

#[test]
fn load_without_a_build_takes_an_installed_one_that_runs_here_else_the_recommended() {
    let fixture = Fixture::new();
    fixture.install("ear", Some("ear-2")).expect("installed");
    assert_eq!(fixture.load("ear", None).expect("loaded").build(), "ear-2");
}

#[test]
fn what_cannot_be_loaded_says_why_with_a_code() {
    let fixture = Fixture::new();
    let code = |result: Result<LocalModel>| result.map(drop).expect_err("refused").code;
    assert_eq!(code(fixture.load("nobody", None)), "model-not-found");
    assert_eq!(
        code(fixture.load("ear", Some("voice-1"))),
        "build-not-found"
    );
    assert_eq!(code(fixture.load("broken", None)), "model-load-failed");
    assert_eq!(
        code(fixture.load("broken", None)),
        "model-load-failed",
        "not remembered"
    );

    let cancel = Cancel::new();
    cancel.cancel();
    let cancelled = block_on(fixture.engine.load("ear", None, &|_| {}, &cancel));
    assert_eq!(code(cancelled), "cancelled");
    assert_eq!(fixture.installed("ear")[0], pair("ear-1", false));
}

#[test]
fn uninstalling_waits_for_the_model_to_be_dropped_and_keeps_what_another_model_uses() {
    let fixture = Fixture::new();
    let ear = fixture.load("ear", None).expect("loaded");
    fixture.install("ear", Some("ear-2")).expect("installed");
    fixture.install("other", None).expect("installed");
    let uninstall = |model| block_on(fixture.engine.uninstall(model, None));
    assert_eq!(uninstall("ear"), Err(Error::new("model-in-use")));
    drop(ear);
    uninstall("ear").expect("uninstalled");
    // Both of ear's build folders go, and ear-2's file with them; ear-1's only file is one `other`'s folder links too,
    // so it stays, and `other` still loads.
    assert_eq!(
        fixture.installed("ear"),
        [pair("ear-1", false), pair("ear-2", false)]
    );
    assert_eq!(fixture.installed("other"), [pair("other-1", true)]);
    fixture.load("other", None).expect("still whole");
    assert_eq!(uninstall("nobody"), Err(Error::new("model-not-found")));
}

#[test]
fn one_build_is_uninstalled_alone_and_a_file_another_build_links_stays() {
    let fixture = Fixture::new();
    fixture.install("ear", Some("ear-1")).expect("installed");
    fixture.install("ear", Some("ear-2")).expect("installed");
    fixture.install("other", None).expect("installed");
    let uninstall = |model, build| block_on(fixture.engine.uninstall(model, Some(build)));
    uninstall("ear", "ear-2").expect("uninstalled");
    assert_eq!(
        fixture.installed("ear"),
        [pair("ear-1", true), pair("ear-2", false)],
        "ear-1 stays"
    );
    uninstall("ear", "ear-1").expect("uninstalled");
    // ear-1's only file is one `other`'s folder links too: it stays, and `other` still loads.
    assert_eq!(fixture.installed("other"), [pair("other-1", true)]);
    fixture.load("other", None).expect("still whole");
    uninstall("ear", "ear-1").expect("not installed: nothing to do");
    assert_eq!(
        uninstall("ear", "other-1"),
        Err(Error::new("build-not-found"))
    );
    assert_eq!(
        uninstall("nobody", "ear-1"),
        Err(Error::new("model-not-found"))
    );
}

#[test]
fn a_loaded_build_is_not_uninstalled_until_dropped() {
    let fixture = Fixture::new();
    let ear = fixture.load("ear", Some("ear-1")).expect("loaded");
    fixture.install("ear", Some("ear-2")).expect("installed");
    let uninstall = |build| block_on(fixture.engine.uninstall("ear", Some(build)));
    assert_eq!(uninstall("ear-1"), Err(Error::new("model-in-use")));
    uninstall("ear-2").expect("not loaded: uninstalled");
    drop(ear);
    uninstall("ear-1").expect("dropped: uninstalled");
    assert_eq!(
        fixture.installed("ear"),
        [pair("ear-1", false), pair("ear-2", false)]
    );
    fixture
        .install("ear", Some("ear-1"))
        .expect("installed again");
}

#[test]
fn each_load_notes_when_by_model_shared_or_not_and_installing_does_not() {
    let fixture = Fixture::new();
    assert_eq!(fixture.last_used("ear"), None, "never loaded");
    fixture.host.set_now(1_000);
    let first = fixture.load("ear", Some("ear-1")).expect("loaded");
    assert_eq!(fixture.last_used("ear"), Some(1_000));

    fixture.host.set_now(2_000);
    let shared = fixture.load("ear", Some("ear-1")).expect("loaded again");
    assert_eq!(Fixture::count(&fixture.counters.loaded), 1, "one model");
    assert_eq!(
        fixture.last_used("ear"),
        Some(2_000),
        "a shared load counts"
    );

    fixture.host.set_now(3_000);
    fixture.load("ear", Some("ear-2")).expect("another build");
    assert_eq!(fixture.last_used("ear"), Some(3_000), "by model, any build");

    fixture.install("voice", None).expect("installed");
    assert_eq!(fixture.last_used("voice"), None, "installed, never loaded");
    assert!(
        fixture.load("broken", None).is_err(),
        "a load that fails notes nothing"
    );
    assert_eq!(fixture.last_used("broken"), None);
    drop((first, shared));
}

#[test]
fn when_a_model_was_last_loaded_is_kept_in_the_hosts_storage_for_the_next_engine() {
    let fixture = Fixture::new();
    fixture.host.set_now(1_760_000_000_123);
    drop(fixture.load("ear", None).expect("loaded"));
    let stored = fixture.host.stored();
    assert_eq!(
        stored.get("last-used").map(Vec::as_slice),
        Some(br#"{"ear":1760000000123}"#.as_slice())
    );

    let next = Fixture::over(fixture.host.sharing());
    drop(fixture);
    assert_eq!(next.last_used("ear"), Some(1_760_000_000_123));
}

#[test]
fn the_listing_says_when_only_for_installed_models() {
    let host = served();
    host.store("last-used", br#"{"ear": 9, "voice": 7, "nobody": 5}"#);
    let fixture = Fixture::over(host);
    fixture.install("ear", Some("ear-2")).expect("installed");
    assert_eq!(fixture.last_used("ear"), Some(9));
    assert_eq!(fixture.last_used("voice"), None, "not installed");
}

#[test]
fn uninstalling_forgets_when_a_model_was_last_loaded_once_no_build_of_it_is_left() {
    let fixture = Fixture::new();
    fixture.host.set_now(1_000);
    fixture.install("ear", Some("ear-2")).expect("installed");
    drop(fixture.load("ear", Some("ear-1")).expect("loaded"));
    drop(fixture.load("voice", None).expect("loaded"));
    let uninstall = |model, build| block_on(fixture.engine.uninstall(model, build));

    uninstall("ear", Some("ear-2")).expect("uninstalled");
    assert_eq!(
        fixture.last_used("ear"),
        Some(1_000),
        "ear-1 is still installed"
    );
    uninstall("ear", Some("ear-1")).expect("uninstalled");
    fixture
        .install("ear", Some("ear-1"))
        .expect("installed again");
    assert_eq!(
        fixture.last_used("ear"),
        None,
        "forgotten with its last build"
    );

    uninstall("voice", None).expect("uninstalled");
    fixture.install("voice", None).expect("installed again");
    assert_eq!(fixture.last_used("voice"), None, "forgotten with the model");
}

#[test]
fn a_record_that_cannot_be_read_is_no_data_and_the_next_load_replaces_it() {
    let host = served();
    host.store("last-used", b"{ not json");
    let fixture = Fixture::over(host);
    fixture.install("ear", None).expect("installed");
    assert_eq!(fixture.last_used("ear"), None);
    fixture.host.set_now(5);
    drop(fixture.load("ear", None).expect("loads all the same"));
    assert_eq!(fixture.last_used("ear"), Some(5));
}

#[test]
fn transcribing_brings_the_audio_to_the_models_rate() {
    let fixture = Fixture::new();
    let ear = fixture.load("ear", None).expect("loaded");
    let stt = ear.as_stt().expect("speech to text");
    let heard = block_on(stt.transcribe(&[0.0; 8_000], 8_000, Some("es")));
    assert_eq!(heard.as_deref(), Ok("16000 samples in Some(\"es\")"));
    let as_is = block_on(stt.transcribe(&[0.0; 10], 16_000, None));
    assert_eq!(as_is.as_deref(), Ok("10 samples in None"));
}

#[test]
fn a_models_voices_are_described_by_the_catalogue_and_speech_comes_at_its_rate() {
    let fixture = Fixture::new();
    let voice = fixture.load("voice", None).expect("loaded");
    let tts = voice.as_tts().expect("text to speech");
    let voices = block_on(tts.voices());
    assert_eq!(
        voices,
        [
            Voice {
                id: "a".to_owned(),
                name: None,
                languages: vec!["es".to_owned()],
                gender: Some(Gender::Female),
            },
            // Not in the catalogue: the model's languages, and no gender.
            Voice {
                id: "b".to_owned(),
                name: None,
                languages: vec!["es".to_owned(), "en".to_owned()],
                gender: None,
            },
        ]
    );
    let audio = block_on(tts.speak("hola", "b", Some("es"), None)).expect("spoken");
    assert_eq!((audio.samples.len(), audio.sample_rate), (4, 8_000));
    let faster = block_on(tts.speak("hola", "a", None, Some(2.0))).expect("spoken");
    assert_eq!(faster.samples.len(), 8);
    let unknown = block_on(tts.speak("hola", "z", None, None));
    assert_eq!(unknown.map(drop), Err(Error::new("unknown-voice")));
}

/// Polls both futures, in turn, until both are done; this executor never sleeps, so it is only for futures that wake
/// themselves.
#[cfg(native)]
fn both<A: Future, B: Future>(a: A, b: B) -> (A::Output, B::Output) {
    let (mut a, mut b) = (pin!(a), pin!(b));
    let (mut done_a, mut done_b) = (None, None);
    let mut context = Context::from_waker(std::task::Waker::noop());
    while done_a.is_none() || done_b.is_none() {
        if done_a.is_none() {
            if let Poll::Ready(out) = a.as_mut().poll(&mut context) {
                done_a = Some(out);
            }
        }
        if done_b.is_none() {
            if let Poll::Ready(out) = b.as_mut().poll(&mut context) {
                done_b = Some(out);
            }
        }
    }
    (done_a.expect("done"), done_b.expect("done"))
}

/// #81's race: an uninstall that comes while a load of the same build is past its install, its model not yet in memory.
/// The load holds the engine's files shared until its model is in memory, so the uninstall waits for it, then finds
/// the model in memory and removes nothing: the load keeps its files.
#[cfg(native)]
#[test]
fn an_uninstall_that_lands_inside_a_load_waits_for_it_and_removes_nothing() {
    let fixture = Fixture::new();
    let cancel = Cancel::new();
    let load = fixture.engine.load("ear", Some("ear-1"), &|_| {}, &cancel);
    let mut load = pin!(load);
    let uninstall = fixture.engine.uninstall("ear", Some("ear-1"));
    let mut uninstall = pin!(uninstall);
    let mut context = Context::from_waker(std::task::Waker::noop());
    while fixture.counters.loading.load(Ordering::Relaxed) == 0 {
        assert!(
            load.as_mut().poll(&mut context).is_pending(),
            "it waits once inside the library"
        );
    }
    assert!(
        uninstall.as_mut().poll(&mut context).is_pending(),
        "the uninstall waits for the load"
    );
    let loaded = loop {
        if let Poll::Ready(loaded) = load.as_mut().poll(&mut context) {
            break loaded;
        }
    };
    let ear = loaded.expect("loaded, its files still there");
    let uninstalled = loop {
        if let Poll::Ready(uninstalled) = uninstall.as_mut().poll(&mut context) {
            break uninstalled;
        }
    };
    assert_eq!(
        uninstalled,
        Err(Error::new("model-in-use")),
        "in memory by then"
    );
    assert_eq!(fixture.installed("ear")[0], pair("ear-1", true));
    drop(ear);
    block_on(fixture.engine.uninstall("ear", Some("ear-1"))).expect("uninstalled");
}

#[cfg(native)]
#[test]
fn two_calls_on_one_model_run_one_after_the_other() {
    let fixture = Fixture::new();
    let ear = fixture.load("ear", None).expect("loaded");
    let shared = fixture.load("ear", None).expect("the same model");
    let (stt, other) = (ear.as_stt().expect("stt"), shared.as_stt().expect("stt"));
    let (first, second) = both(
        stt.transcribe(&[0.0; 2], 16_000, None),
        other.transcribe(&[0.0; 3], 16_000, None),
    );
    assert_eq!((first.is_ok(), second.is_ok()), (true, true));
    assert_eq!(
        Fixture::count(&fixture.counters.most_running),
        1,
        "one at a time"
    );
}

/// ENG-02's race: build `other`'s install has found the blob it shares with `ear-1` and links it later, while `ear-1`
/// is uninstalled. The install holds the engine's files shared, so the uninstall waits, removing nothing; once the
/// install has linked the blob, the uninstall removes `ear-1` and keeps the blob `other` links.
#[test]
fn an_uninstall_waits_for_an_install_that_found_a_shared_blob_and_keeps_it() {
    use std::future::Future;
    use std::pin::pin;
    use std::task::{Context, Poll};

    let fixture = Fixture::new();
    fixture.install("ear", Some("ear-1")).expect("installed");
    let shared = sha256(b"ear-1");
    // `other`'s install, after finding `ear-1`'s file stored.
    let installing = block_on(fixture.engine.files.read());

    let uninstall = fixture.engine.uninstall("ear", Some("ear-1"));
    let mut uninstall = pin!(uninstall);
    let mut context = Context::from_waker(std::task::Waker::noop());
    assert!(
        uninstall.as_mut().poll(&mut context).is_pending(),
        "it waits for the install"
    );
    assert_eq!(
        fixture.installed("ear")[0],
        pair("ear-1", true),
        "nothing removed yet"
    );

    // The install links the shared blob into its folder, and is done.
    let storage = fixture.engine.host.storage();
    let mut folder = block_on(storage.create_folder("other-1")).expect("folder");
    block_on(folder.link("ear-1", &shared, None)).expect("linked");
    block_on(folder.commit()).expect("committed");
    drop(installing);

    let uninstalled = loop {
        if let Poll::Ready(uninstalled) = uninstall.as_mut().poll(&mut context) {
            break uninstalled;
        }
    };
    uninstalled.expect("uninstalled");
    assert_eq!(fixture.installed("ear")[0], pair("ear-1", false));
    assert!(
        block_on(storage.find(&shared)).expect("found").is_some(),
        "the blob `other` links stays"
    );
}

/// Installs and loads hold the engine's files shared: one in progress does not hold up another.
#[test]
fn installs_and_loads_run_side_by_side() {
    let fixture = Fixture::new();
    // An install or a load in progress.
    let installing = block_on(fixture.engine.files.read());
    fixture
        .install("ear", Some("ear-2"))
        .expect("installed alongside");
    fixture.load("other", None).expect("loaded alongside");
    drop(installing);
}

/// Polls every future, in turn, until all are done; as [`both`], only for futures that wake themselves.
#[cfg(native)]
fn all(mut futures: Vec<std::pin::Pin<Box<dyn Future<Output = ()> + '_>>>) {
    let mut context = Context::from_waker(std::task::Waker::noop());
    while !futures.is_empty() {
        futures.retain_mut(|future| future.as_mut().poll(&mut context).is_pending());
    }
}

/// Waits, yielding, until `ready` holds.
#[cfg(native)]
async fn until(ready: impl Fn() -> bool) {
    while !ready() {
        YieldOnce(false).await;
    }
}

/// Yields `times` times: lets the other futures [`all`] polls run on.
#[cfg(native)]
async fn yields(times: usize) {
    for _ in 0..times {
        YieldOnce(false).await;
    }
}

#[test]
fn a_record_the_storage_cannot_read_is_left_whole_and_the_load_still_succeeds() {
    let fixture = Fixture::new();
    fixture.host.set_now(1);
    drop(fixture.load("ear", None).expect("loaded"));
    fixture.host.fail_reads("last-used", true);
    fixture.host.set_now(2);
    drop(fixture.load("voice", None).expect("loads all the same"));
    assert_eq!(
        fixture.last_used("ear"),
        None,
        "listed without: it cannot be read"
    );
    fixture.host.fail_reads("last-used", false);
    assert_eq!(
        (fixture.last_used("ear"), fixture.last_used("voice")),
        (Some(1), None),
        "ear's time kept, voice's change skipped"
    );
}

/// Two loads that finish together: the second waits for the first's change, then takes the time and makes its own, so
/// neither is lost and the later time is the one kept.
#[cfg(native)]
#[test]
fn changes_to_the_record_wait_for_one_another_and_take_the_time_in_turn() {
    let fixture = Fixture::new();
    let (host, engine, cancel) = (&fixture.host, &fixture.engine, Cancel::new());
    host.set_now(1);
    drop(fixture.load("ear", None).expect("loaded"));
    host.set_now(10);
    let paused = host.pause_reads("last-used");
    let voice = async {
        let voice = engine.load("voice", None, &|_| {}, &cancel).await;
        drop(voice.expect("loaded"));
    };
    let ear = async {
        // Voice's change holds the record, waiting in its read.
        until(|| host.waiting_reads() >= 1).await;
        host.set_now(20);
        drop(
            engine
                .load("ear", None, &|_| {}, &cancel)
                .await
                .expect("loaded"),
        );
    };
    let release = async {
        until(|| host.waiting_reads() >= 1).await;
        yields(100).await;
        assert_eq!(host.waiting_reads(), 1, "ear's change waits for voice's");
        host.set_now(30);
        drop(paused);
    };
    all(vec![Box::pin(voice), Box::pin(ear), Box::pin(release)]);
    assert_eq!(
        (fixture.last_used("voice"), fixture.last_used("ear")),
        (Some(10), Some(30))
    );
}

/// A load that finishes while another model is uninstalled: the uninstall's forgetting waits for the load's change.
#[cfg(native)]
#[test]
fn forgetting_a_model_waits_for_the_change_of_a_load_in_progress() {
    let fixture = Fixture::new();
    let (host, engine, cancel) = (&fixture.host, &fixture.engine, Cancel::new());
    host.set_now(1);
    drop(fixture.load("ear", None).expect("loaded"));
    drop(fixture.load("voice", None).expect("loaded"));
    host.set_now(10);
    let paused = host.pause_reads("last-used");
    let ear = async {
        drop(
            engine
                .load("ear", None, &|_| {}, &cancel)
                .await
                .expect("loaded"),
        );
    };
    let uninstall = async {
        until(|| host.waiting_reads() >= 1).await;
        engine.uninstall("voice", None).await.expect("uninstalled");
    };
    let release = async {
        until(|| host.waiting_reads() >= 1).await;
        yields(100).await;
        assert_eq!(host.waiting_reads(), 1, "the uninstall's change waits");
        drop(paused);
    };
    all(vec![Box::pin(ear), Box::pin(uninstall), Box::pin(release)]);
    assert_eq!(
        fixture.host.stored().get("last-used").map(Vec::as_slice),
        Some(br#"{"ear":10}"#.as_slice())
    );
}

/// A host that reports a GPU next to the CPU, over [`MemoryHost`]'s storage.
struct GpuHost(MemoryHost);

impl crate::Host for GpuHost {
    fn capabilities(&self) -> crate::Capabilities {
        crate::Capabilities {
            runs: crate::Runs::Native,
            accelerators: vec![Accelerator::Cuda, Accelerator::Cpu],
            ..FakeHost.capabilities()
        }
    }

    fn storage(&self) -> &dyn crate::Storage {
        self.0.storage()
    }

    fn fetcher(&self) -> &dyn crate::Fetcher {
        self.0.fetcher()
    }

    fn http(&self) -> &dyn crate::HttpClient {
        self.0.http()
    }

    fn credentials(&self) -> &dyn crate::Credentials {
        self.0.credentials()
    }
}

/// A catalogue of one model with one build on the GPU backend.
struct GpuModels;

impl CatalogSource for GpuModels {
    fn load(&self) -> Result<CatalogFragment> {
        let build = BuildEntry {
            files: vec![file("gpu-1")],
            ..build("gpu-1", "fake-gpu", 0)
        };
        Ok(CatalogFragment {
            families: vec![family(
                "fake",
                vec![model("gpu-ear", Capability::Stt, vec![build])],
            )],
        })
    }
}

/// What a [`GpuBackend`]'s library is asked for, and how its GPU behaves.
#[derive(Default, Clone)]
struct GpuBehaviour {
    /// The accelerators its `load` was called with, in order.
    asked: Arc<std::sync::Mutex<Vec<Accelerator>>>,
    /// The code loading on the GPU fails with, if it does.
    gpu_fails: Option<&'static str>,
    /// The code loading on the CPU fails with, if it does.
    cpu_fails: Option<&'static str>,
    /// Cancelled when loading on the GPU fails.
    cancel: Option<Cancel>,
    /// Whether the loaded model confirms the accelerator it runs on.
    confirms: bool,
}

struct GpuBackend(GpuBehaviour);

const FAKE_GPU: BackendSpec = BackendSpec {
    id: "fake-gpu",
    name: "Fake GPU",
    description: "A test double that runs on a GPU, then on the CPU.",
    upstream: "https://example.com",
    accelerators: &[Accelerator::Cuda, Accelerator::Cpu],
    requirements: &[],
};

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl Backend for GpuBackend {
    fn spec(&self) -> &BackendSpec {
        &FAKE_GPU
    }

    async fn open(&self, _files: &Installed) -> Result<Box<dyn Library>> {
        Ok(Box::new(GpuLibrary(self.0.clone())))
    }
}

struct GpuLibrary(GpuBehaviour);

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl Library for GpuLibrary {
    async fn load(
        &self,
        _build: &BuildEntry,
        accelerator: Accelerator,
        _files: &Installed,
    ) -> Result<Box<dyn BackendModel>> {
        self.0.asked.lock().expect("asked").push(accelerator);
        let failure = if accelerator == Accelerator::Cuda {
            self.0.gpu_fails
        } else {
            self.0.cpu_fails
        };
        if let Some(code) = failure {
            if let Some(cancel) = &self.0.cancel {
                cancel.cancel();
            }
            return Err(Error::new(code));
        }
        Ok(Box::new(GpuModel(self.0.confirms.then_some(accelerator))))
    }
}

/// A model that says which accelerator it runs on, if it was made to.
struct GpuModel(Option<Accelerator>);

impl BackendModel for GpuModel {
    fn confirmed_accelerator(&self) -> Option<Accelerator> {
        self.0
    }

    fn memory_mb(&self) -> Option<u32> {
        None
    }
}

/// `gpu-ear`, loaded over an engine whose host reports a GPU and the CPU; with what the library was asked for.
fn load_on_gpu_host(behaviour: GpuBehaviour) -> (Result<LocalModel>, Vec<Accelerator>) {
    let cancel = behaviour.cancel.clone().unwrap_or_default();
    let host = MemoryHost::serving(&[("https://models/gpu-1", b"gpu-1")]);
    let engine = Engine::with_backends(
        Box::new(GpuHost(host)),
        vec![Box::new(GpuModels)],
        vec![Box::new(GpuBackend(behaviour.clone()))],
    )
    .expect("engine");
    let loaded = block_on(engine.load("gpu-ear", None, &|_| {}, &cancel));
    let asked = behaviour.asked.lock().expect("asked").clone();
    (loaded, asked)
}

#[test]
fn a_gpu_the_backend_does_not_confirm_is_requested_but_not_reported_as_running() {
    let (loaded, asked) = load_on_gpu_host(GpuBehaviour::default());
    let used = loaded.expect("loaded").accelerator();
    assert_eq!(asked, [Accelerator::Cuda]);
    assert_eq!(used.requested, Accelerator::Cuda);
    assert_eq!(used.actual, None, "the GPU was asked for, not seen running");
    assert!(!used.is_confirmed() && !used.fell_back());
}

#[test]
fn a_gpu_the_backend_confirms_is_reported_as_running() {
    let behaviour = GpuBehaviour {
        confirms: true,
        ..GpuBehaviour::default()
    };
    let (loaded, _) = load_on_gpu_host(behaviour);
    let used = loaded.expect("loaded").accelerator();
    assert_eq!(used.actual, Some(Accelerator::Cuda));
    assert!(used.is_confirmed() && !used.fell_back());
}

#[test]
fn a_gpu_that_fails_to_load_falls_back_to_the_cpu_and_says_so() {
    let behaviour = GpuBehaviour {
        gpu_fails: Some("model-load-failed"),
        ..GpuBehaviour::default()
    };
    let (loaded, asked) = load_on_gpu_host(behaviour);
    let used = loaded.expect("loaded on the CPU").accelerator();
    assert_eq!(asked, [Accelerator::Cuda, Accelerator::Cpu]);
    assert_eq!(used.requested, Accelerator::Cuda);
    assert_eq!(used.actual, Some(Accelerator::Cpu));
    assert!(used.fell_back() && !used.is_confirmed());
}

#[test]
fn a_model_on_the_cpu_is_reported_on_the_cpu() {
    let fixture = Fixture::new();
    let used = fixture.load("ear", None).expect("loaded").accelerator();
    assert_eq!(used.requested, Accelerator::Cpu);
    assert_eq!(used.actual, Some(Accelerator::Cpu));
    assert!(used.is_confirmed());
}

#[test]
fn a_cpu_that_fails_to_load_has_nothing_to_fall_back_to() {
    let fixture = Fixture::new();
    let error = fixture.load("broken", None).expect_err("fails");
    assert_eq!(error.code, "model-load-failed");
}

#[test]
fn when_the_fallback_fails_too_the_error_is_the_first_attempts() {
    let behaviour = GpuBehaviour {
        gpu_fails: Some("model-load-failed"),
        cpu_fails: Some("unsupported-model"),
        ..GpuBehaviour::default()
    };
    let (loaded, asked) = load_on_gpu_host(behaviour);
    assert_eq!(loaded.expect_err("fails").code, "model-load-failed");
    assert_eq!(asked, [Accelerator::Cuda, Accelerator::Cpu]);
}

#[test]
fn a_failure_the_cpu_would_share_is_not_retried() {
    let behaviour = GpuBehaviour {
        gpu_fails: Some("file-not-installed"),
        ..GpuBehaviour::default()
    };
    let (loaded, asked) = load_on_gpu_host(behaviour);
    assert_eq!(loaded.expect_err("fails").code, "file-not-installed");
    assert_eq!(asked, [Accelerator::Cuda]);
}

#[test]
fn a_cancelled_load_does_not_fall_back() {
    let behaviour = GpuBehaviour {
        gpu_fails: Some("model-load-failed"),
        cancel: Some(Cancel::new()),
        ..GpuBehaviour::default()
    };
    let (loaded, asked) = load_on_gpu_host(behaviour);
    assert!(loaded.is_err());
    assert_eq!(asked, [Accelerator::Cuda]);
}
