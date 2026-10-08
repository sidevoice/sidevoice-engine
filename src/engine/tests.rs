//! The engine over fake hosts, catalogues and a fake backend. What [`Engine::models`] says: builds ranked, those that
//! run here first, the rest with why not, and what is installed. Then the interface: install, load (installing first,
//! one model in memory per build, one library per backend, closed with its last model), the build `load` chooses,
//! what fails and why, uninstalling, and a loaded model's calls: resampled audio, voices described by the catalogue,
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
    async_trait, Accelerator, BuildEntry, BundledCatalog, Cancel, Capability, Engine, Error,
    Gender, LoadedModel, ModelFile, Reason, Result, Voice,
};

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

    let recommended = if cfg!(target_arch = "wasm32") {
        "whisper-small-web"
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "whisper-small-mlx"
    } else {
        "whisper-small-onnx"
    };
    assert_eq!(small.recommended_build.as_deref(), Some(recommended));
    assert_eq!(small.builds[0].id, recommended);
    assert!(small.builds[0].available && small.builds[0].accelerator.is_some());
    assert!(small.builds[0].reasons.is_empty());
    let gguf = small
        .builds
        .iter()
        .find(|b| b.id == "whisper-small-gguf")
        .expect("gguf");
    assert!(!gguf.available && gguf.accelerator.is_none());
    assert_eq!(gguf.reasons, [Reason::new("backend-not-in-this-build")]);
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

impl DownloadBytes for crate::Model {
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
    shared_type::<LoadedModel>();
}

/// What each CI platform offers with the catalogue this repository ships: every bundled model of a capability, each
/// on a backend this platform compiles. Which of a model's builds wins is the ranking's (sidevoice-engine#4).
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
        &["mlx", "sherpa-onnx"]
    } else {
        &["sherpa-onnx"]
    };

    let stt = offered(Capability::Stt);
    let models: Vec<_> = stt.iter().map(|(model, _)| model.as_str()).collect();
    if cfg!(target_arch = "wasm32") {
        assert_eq!(models, ["whisper-base", "whisper-small", "whisper-tiny"]);
    } else {
        assert_eq!(
            models,
            [
                "fastconformer-es-large",
                "parakeet-tdt-0.6b-v3",
                "whisper-base",
                "whisper-small",
                "whisper-tiny"
            ]
        );
    }
    for (model, backend) in &stt {
        assert!(backends.contains(&backend.as_str()), "{model} on {backend}");
    }

    let tts = offered(Capability::Tts);
    if cfg!(target_arch = "wasm32") {
        let web = |model: &str| (model.to_owned(), "transformers-js".to_owned());
        assert_eq!(tts, [web("kokoro-82m-v0.19"), web("kokoro-82m-v1.0")]);
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
        voice: &str,
        _language: Option<&str>,
        speed: f32,
    ) -> Result<Vec<f32>> {
        if voice != "a" && voice != "b" {
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
}

impl Fixture {
    fn new() -> Self {
        let counters = Counters::default();
        let backend = FakeBackend(counters.clone());
        let engine = Engine::with_backends(
            Box::new(served()),
            vec![Box::new(FakeModels)],
            vec![Box::new(backend)],
        )
        .expect("engine");
        Self { engine, counters }
    }

    fn load(&self, model: &str, build: Option<&str>) -> Result<LoadedModel> {
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
    let code = |result: Result<LoadedModel>| result.map(drop).expect_err("refused").code;
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
    let uninstall = |model| block_on(fixture.engine.uninstall(model));
    assert_eq!(uninstall("ear"), Err(Error::new("model-in-use")));
    drop(ear);
    uninstall("ear").expect("uninstalled");
    // ear-2's file goes; ear-1's only file is one `other` uses too, so it stays, and so ear-1 is still whole.
    assert_eq!(
        fixture.installed("ear"),
        [pair("ear-1", true), pair("ear-2", false)]
    );
    assert_eq!(fixture.installed("other"), [pair("other-1", true)]);
    assert_eq!(uninstall("nobody"), Err(Error::new("model-not-found")));
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
                languages: vec!["es".to_owned()],
                gender: Some(Gender::Female),
            },
            // Not in the catalogue: the model's languages, and no gender.
            Voice {
                id: "b".to_owned(),
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

/// The resampling the engine does for speech to text: the length scales with the rates, and a constant stays constant.
#[test]
fn audio_is_resampled_linearly() {
    let up = super::audio::resample(&[0.5; 100], 8_000, 16_000);
    assert_eq!(up.len(), 200);
    assert!(up.iter().all(|sample| (sample - 0.5).abs() < 1e-6));
    assert_eq!(
        super::audio::resample(&[0.1, 0.2], 16_000, 16_000),
        [0.1, 0.2]
    );
    assert_eq!(
        super::audio::resample(&[0.0; 48_000], 48_000, 16_000).len(),
        16_000
    );
}
