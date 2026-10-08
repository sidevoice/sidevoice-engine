//! The funnel over a fake host and catalogue: what fits is offered, and every other build comes back with why not.
//! Then a build's lifecycle with a fake backend: installed, loaded, unloaded when idle, failed, cancelled, and its
//! backend's library opened with its first model and closed with its last.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use super::{lock, Preparing};
use crate::backend::{Backend, BackendSpec, Library, LoadedModel, SttModel};
use crate::host::Platform;
use crate::install::Installed;
use crate::test_support::{artifact, block_on, FakeCatalog, FakeHost, MemoryHost};
use crate::{
    async_trait, Accelerator, Artifact, Build, BuildState, BundledCatalog, Cancel, Capabilities,
    Capability, Engine, Error, Fetcher, Host, ModelFile, Offer, Preferences, Reason, Rejection,
    Result, Runs, Selection, Storage,
};

#[cfg(web)]
use wasm_bindgen_test::wasm_bindgen_test as test;

#[test]
fn an_engine_with_a_fake_host_offers_what_fits_and_says_why_the_rest_does_not() {
    let engine = Engine::new(Box::new(FakeHost), vec![Box::new(FakeCatalog)]).expect("engine");
    let offers = engine.offers(Capability::Stt);

    let offered: Vec<_> = offers
        .iter()
        .filter_map(|offer| match offer {
            Offer::Offered { model, build, .. } => Some((model.id.as_str(), build.id.as_str())),
            Offer::Rejected { .. } => None,
        })
        .collect();
    let rejected: Vec<_> = offers
        .iter()
        .filter_map(|offer| match offer {
            Offer::Rejected { build, why, .. } => Some((build.id.as_str(), why.clone())),
            Offer::Offered { .. } => None,
        })
        .collect();

    if cfg!(target_arch = "wasm32") {
        assert_eq!(offered, [("whisper-small", "whisper-small-web")]);
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        assert_eq!(offered, [("whisper-small", "whisper-small-mlx")]);
    } else {
        assert_eq!(offered, [("whisper-small", "whisper-small-onnx")]);
    }
    assert!(rejected.contains(&("whisper-small-gguf", Rejection::BackendNotInThisBuild)));
    let large_rejection = if cfg!(target_arch = "wasm32") {
        // Its only build is native: on the web, its backend does not exist.
        Rejection::BackendNotInThisBuild
    } else {
        Rejection::DoesNotFit(Reason::with_numbers("memory", 16_384, 8_192))
    };
    assert!(rejected.contains(&("whisper-large-onnx", large_rejection)));
    assert!(offers
        .iter()
        .all(|offer| !matches!(offer, Offer::Offered { model, .. } if !model.capabilities.contains(&Capability::Stt))));
}

#[cfg(native)]
#[test]
fn a_native_engine_and_its_futures_can_cross_threads() {
    fn shared<T: Send + Sync>(_: &T) {}
    fn sent<T: Send>(_: T) {}

    let engine = Engine::new(Box::new(FakeHost), vec![Box::new(FakeCatalog)]).expect("engine");
    shared(&engine);
    let selection = engine
        .select(Capability::Stt, &crate::Preferences::default())
        .expect("a selection");
    sent(engine.prepare(&selection, &|_| {}, &Cancel::new()));
}

/// A native host on a platform backends.json has no entry for.
struct Elsewhere;

impl Host for Elsewhere {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            runs: Runs::Native,
            os: "plan9".to_owned(),
            ..FakeHost.capabilities()
        }
    }

    fn storage(&self) -> &dyn Storage {
        &FakeHost
    }

    fn fetcher(&self) -> &dyn Fetcher {
        &FakeHost
    }
}

#[test]
fn a_platform_with_no_runtime_rejects_every_build_of_this_engine_in_the_funnel() {
    let engine = Engine::new(Box::new(Elsewhere), vec![Box::new(FakeCatalog)]).expect("engine");
    let offers = engine.offers(Capability::Stt);
    assert!(!offers.is_empty());
    for offer in offers {
        let Offer::Rejected { build, why, .. } = offer else {
            panic!("nothing runs on plan9: {offer:?}");
        };
        if engine.backends().contains(&build.backend.as_str()) {
            let no_runtime = Reason::new("no-runtime-for-platform");
            assert_eq!(
                why,
                Rejection::BackendUnavailable(no_runtime),
                "{}",
                build.id
            );
        } else {
            assert_eq!(why, Rejection::BackendNotInThisBuild, "{}", build.id);
        }
    }
}

/// What each CI platform offers with the catalogue this repository ships: every bundled model of a capability, each
/// on a backend this platform compiles. Which of a model's builds wins is the ranking's (sidevoice-engine#4).
#[test]
fn the_bundled_catalogue_offers_every_model_on_this_platforms_backends() {
    let engine = Engine::new(Box::new(FakeHost), vec![Box::new(BundledCatalog)]).expect("engine");
    let offered = |capability| {
        let mut offered: Vec<_> = engine
            .offers(capability)
            .into_iter()
            .filter_map(|offer| match offer {
                Offer::Offered { model, build, .. } => Some((model.id, build.backend)),
                Offer::Rejected { .. } => None,
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

const MODEL_A: &[u8] = b"model a";
const MODEL_B: &[u8] = b"model b";
const LIBRARY: &[u8] = b"the fake backend's library";

/// A backend whose library counts how many times it was opened, and how many are open.
struct FakeBackend {
    opened: Arc<AtomicUsize>,
    open: Arc<AtomicUsize>,
}

const FAKE: BackendSpec = BackendSpec {
    id: "fake",
    accelerators: &[Accelerator::Cpu],
    requirements: &[],
};

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl Backend for FakeBackend {
    fn spec(&self) -> &BackendSpec {
        &FAKE
    }

    async fn open(&self, files: &Installed) -> Result<Box<dyn Library>> {
        assert_eq!(
            files.file("library"),
            Some(format!("memory:{}", crate::test_support::sha256(LIBRARY)).as_str())
        );
        self.opened.fetch_add(1, Ordering::Relaxed);
        self.open.fetch_add(1, Ordering::Relaxed);
        Ok(Box::new(FakeLibrary {
            open: Arc::clone(&self.open),
        }))
    }
}

struct FakeLibrary {
    open: Arc<AtomicUsize>,
}

impl Drop for FakeLibrary {
    fn drop(&mut self) {
        self.open.fetch_sub(1, Ordering::Relaxed);
    }
}

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl Library for FakeLibrary {
    async fn load(
        &self,
        build: &Build,
        _accelerator: Accelerator,
        files: &Installed,
    ) -> Result<Box<dyn LoadedModel>> {
        if build.id == "broken" {
            return Err(Error::new("model-did-not-load"));
        }
        assert!(files.file("model.onnx").is_some());
        Ok(Box::new(FakeModel))
    }
}

/// A speech-to-text model that says how many samples it heard, and in which language.
struct FakeModel;

impl LoadedModel for FakeModel {
    fn as_stt(&mut self) -> Option<&mut dyn SttModel> {
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
        Ok(format!("{} samples in {language:?}", pcm.len()))
    }
}

/// The fake backend's library, on any platform.
fn fake_runtime(_backend: &str, _platform: Platform) -> Option<Vec<Artifact>> {
    Some(vec![artifact(
        "library",
        "https://backends/library",
        LIBRARY,
    )])
}

fn served() -> MemoryHost {
    MemoryHost::serving(&[
        ("https://models/a", MODEL_A),
        ("https://models/b", MODEL_B),
        ("https://models/broken", MODEL_A),
        ("https://backends/library", LIBRARY),
    ])
}

struct Fixture {
    engine: Engine,
    opened: Arc<AtomicUsize>,
    open: Arc<AtomicUsize>,
}

impl Fixture {
    fn new(host: MemoryHost) -> Self {
        let opened = Arc::new(AtomicUsize::new(0));
        let open = Arc::new(AtomicUsize::new(0));
        let backend = FakeBackend {
            opened: Arc::clone(&opened),
            open: Arc::clone(&open),
        };
        let engine = Engine::with_backends(
            Box::new(host),
            vec![],
            vec![Box::new(backend)],
            fake_runtime,
        )
        .expect("engine");
        Self {
            engine,
            opened,
            open,
        }
    }

    fn prepare(&self, selection: &Selection) -> Result<super::Handle> {
        block_on(self.engine.prepare(selection, &|_| {}, &Cancel::new()))
    }

    fn state(&self, selection: &Selection) -> Result<BuildState> {
        block_on(self.engine.state(&selection.build))
    }

    fn opened(&self) -> usize {
        self.opened.load(Ordering::Relaxed)
    }

    fn open(&self) -> usize {
        self.open.load(Ordering::Relaxed)
    }
}

/// Build `id` of the fake backend, whose model file is `https://models/<id>` and should be `bytes`.
fn selection(id: &str, bytes: &[u8]) -> Selection {
    let mut build = crate::test_support::build(id, "fake", 0);
    build.files = vec![ModelFile {
        key: "model.onnx".to_owned(),
        url: format!("https://models/{id}"),
        sha256: crate::test_support::sha256(bytes),
        bytes: bytes.len() as u64,
        archive_path: None,
        mutable: false,
    }];
    Selection {
        model: crate::test_support::model(id, Capability::Stt, vec![build.clone()]),
        build,
        accelerator: Accelerator::Cpu,
    }
}

#[test]
fn preparing_installs_and_loads_a_build_once_and_its_state_follows() {
    let fixture = Fixture::new(served());
    let a = selection("a", MODEL_A);
    assert_eq!(fixture.state(&a), Ok(BuildState::Absent));

    let handle = fixture.prepare(&a).expect("prepared");
    assert_eq!(fixture.state(&a), Ok(BuildState::Ready));
    assert_eq!(fixture.prepare(&a), Ok(handle), "already loaded");
    assert_eq!((fixture.opened(), fixture.open()), (1, 1));
}

#[test]
fn a_backends_library_opens_with_its_first_model_and_closes_with_its_last() {
    let fixture = Fixture::new(served());
    let a = selection("a", MODEL_A);
    let b = selection("b", MODEL_B);
    let first = fixture.prepare(&a).expect("a");
    fixture.prepare(&b).expect("b");
    assert_eq!((fixture.opened(), fixture.open()), (1, 1), "one library");

    fixture.engine.unload_idle();
    assert_eq!(fixture.state(&a), Ok(BuildState::Ready), "not idle yet");

    lock(&fixture.engine.memory).set_idle(Duration::ZERO);
    fixture.engine.unload_idle();
    assert_eq!(fixture.open(), 0, "closed with the last model");
    assert_eq!(fixture.state(&a), Ok(BuildState::Installed));
    assert_eq!(fixture.state(&b), Ok(BuildState::Installed));

    let again = Engine::with_idle_unload(fixture.engine, super::DEFAULT_IDLE_UNLOAD);
    let reloaded = block_on(again.prepare(&a, &|_| {}, &Cancel::new())).expect("a again");
    assert_ne!(reloaded, first);
    assert_eq!(fixture.opened.load(Ordering::Relaxed), 2);
    assert_eq!(fixture.open.load(Ordering::Relaxed), 1);
}

#[test]
fn a_build_that_fails_to_install_or_load_is_failed_until_prepared_again() {
    let fixture = Fixture::new(MemoryHost::serving(&[
        ("https://models/a", b"tampered"),
        ("https://models/broken", MODEL_A),
        ("https://backends/library", LIBRARY),
    ]));
    let a = selection("a", MODEL_A);
    let digest_mismatch = Error::new("digest-mismatch");
    assert_eq!(fixture.prepare(&a), Err(digest_mismatch));
    assert_eq!(fixture.state(&a), Ok(BuildState::Failed(digest_mismatch)));
    assert_eq!(fixture.opened(), 0, "nothing to open");

    let broken = selection("broken", MODEL_A);
    let did_not_load = Error::new("model-did-not-load");
    assert_eq!(fixture.prepare(&broken), Err(did_not_load));
    assert_eq!(fixture.state(&broken), Ok(BuildState::Failed(did_not_load)));
    assert_eq!(
        (fixture.opened(), fixture.open()),
        (1, 0),
        "a library no model holds is closed"
    );
}

#[test]
fn a_cancelled_prepare_leaves_the_build_as_storage_has_it() {
    let fixture = Fixture::new(served());
    let a = selection("a", MODEL_A);
    let cancel = Cancel::new();
    cancel.cancel();
    let prepared = block_on(fixture.engine.prepare(&a, &|_| {}, &cancel));

    assert_eq!(prepared, Err(Error::new("cancelled")));
    assert_eq!(fixture.state(&a), Ok(BuildState::Absent));
}

#[test]
fn a_build_being_prepared_cannot_be_prepared_again_until_that_ends() {
    let states = std::sync::Mutex::default();
    let preparing = Preparing::start(&states, "a").expect("first");
    let busy = Preparing::start(&states, "a").map(drop);
    assert_eq!(busy, Err(Error::new("already-preparing")));
    preparing.set(BuildState::Loading);
    assert!(Preparing::start(&states, "a").is_err());
    assert!(Preparing::start(&states, "b").is_ok(), "another build");

    drop(preparing);
    assert!(lock(&states).is_empty());
    assert!(Preparing::start(&states, "a").is_ok());
}

#[test]
fn a_prepared_model_is_used_through_its_handle_while_it_is_in_memory() {
    let fixture = Fixture::new(served());
    let handle = fixture.prepare(&selection("a", MODEL_A)).expect("a");
    let heard = block_on(fixture.engine.transcribe(handle, &[0.0; 3], Some("es")));
    assert_eq!(heard.as_deref(), Ok("3 samples in Some(\"es\")"));
    let again = block_on(fixture.engine.transcribe(handle, &[0.0; 2], None));
    assert_eq!(again.as_deref(), Ok("2 samples in None"), "still there");

    let spoken = block_on(fixture.engine.speak(handle, "hola", "0", None, 1.0));
    assert_eq!(spoken, Err(Error::new("model-cannot-speak")));
    assert_eq!(
        fixture.engine.voices(handle),
        Err(Error::new("model-cannot-speak"))
    );

    lock(&fixture.engine.memory).set_idle(Duration::ZERO);
    fixture.engine.unload_idle();
    let gone = block_on(fixture.engine.transcribe(handle, &[0.0; 3], None));
    assert_eq!(gone, Err(Error::new("model-not-loaded")));
}

#[test]
fn a_backend_asked_for_is_the_only_one_whose_builds_are_considered() {
    let engine = Engine::new(Box::new(FakeHost), vec![Box::new(FakeCatalog)]).expect("engine");
    let on = |backend: &str| {
        let preferences = Preferences {
            model: Some("whisper-small".to_owned()),
            backend: Some(backend.to_owned()),
            ..Preferences::default()
        };
        engine
            .select(Capability::Stt, &preferences)
            .map(|selection| selection.build.id)
    };
    if cfg!(target_arch = "wasm32") {
        assert_eq!(on("sherpa-onnx"), None, "not in this build");
        assert_eq!(on("transformers-js").as_deref(), Some("whisper-small-web"));
    } else {
        assert_eq!(on("sherpa-onnx").as_deref(), Some("whisper-small-onnx"));
        assert_eq!(on("transformers-js"), None, "not in this build");
    }
    assert_eq!(on("whisper-cpp"), None, "compiled nowhere");
}
