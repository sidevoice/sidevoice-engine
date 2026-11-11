//! The recommender on the catalogue this repository ships, over example devices: which backends their build of the
//! engine has, what their host reports. Each case says which model and build it recommends for speech to text and
//! text to speech, and why. No hardware: the backends are stand-ins with the real ones' ids and accelerators, and
//! nothing is loaded.

use super::{build, Languages, Pick};
use crate::backend::{Backend, BackendSpec, Library};
use crate::install::Installed;
use crate::test_support::{block_on, memory_on_each, FakeHost};
use crate::{
    async_trait, Accelerator, BundledCatalog, Capabilities, Credentials, Engine, Fetcher, Host,
    HttpClient, Result, Runs, Storage,
};

#[cfg(web)]
use wasm_bindgen_test::wasm_bindgen_test as test;

/// A stand-in for a backend: the real one's id and accelerators, and nothing to open (nothing is loaded here).
struct StandIn(BackendSpec);

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl Backend for StandIn {
    fn spec(&self) -> &BackendSpec {
        &self.0
    }

    async fn open(&self, _files: &Installed) -> Result<Box<dyn Library>> {
        unreachable!("the recommender loads nothing")
    }
}

fn stand_in(id: &'static str, accelerators: &'static [Accelerator]) -> Box<dyn Backend> {
    Box::new(StandIn(BackendSpec {
        id,
        name: id,
        description: "A stand-in with the real backend's id and accelerators.",
        upstream: "https://example.com",
        accelerators,
        requirements: &[],
    }))
}

/// The backends of a native build: whisper.cpp (Metal where the host has it, else the CPU), sherpa-onnx and ONNX
/// Runtime on the CPU. MLX is left out: it runs nothing yet.
fn native() -> Vec<Box<dyn Backend>> {
    vec![
        stand_in("whisper-cpp", &[Accelerator::Metal, Accelerator::Cpu]),
        stand_in("sherpa-onnx", &[Accelerator::Cpu]),
        stand_in("onnxruntime", &[Accelerator::Cpu]),
    ]
}

/// The backends of the web build: transformers.js, on WebGPU or in WebAssembly.
fn page() -> Vec<Box<dyn Backend>> {
    vec![stand_in(
        "transformers-js",
        &[Accelerator::WebGpu, Accelerator::Wasm],
    )]
}

/// A host that reports `capabilities` and stores nothing.
struct Device(Capabilities);

impl Host for Device {
    fn capabilities(&self) -> Capabilities {
        self.0.clone()
    }

    fn storage(&self) -> &dyn Storage {
        &FakeHost
    }

    fn fetcher(&self) -> &dyn Fetcher {
        &FakeHost
    }

    fn http(&self) -> &dyn HttpClient {
        &FakeHost
    }

    fn credentials(&self) -> &dyn Credentials {
        &crate::NoCredentials
    }
}

fn caps(accelerators: &[Accelerator], memory_mb: Option<u32>) -> Capabilities {
    Capabilities {
        runs: Runs::Native,
        os: "test".to_owned(),
        arch: "test".to_owned(),
        accelerators: accelerators.to_vec(),
        memory_mb,
        cores: Some(8),
        accelerator_memory: memory_on_each(accelerators, memory_mb),
        gpu_stops_in_background: false,
    }
}

/// A Mac on Apple silicon: Metal, Core ML and the CPU, unified memory.
fn mac(memory_mb: u32) -> (Capabilities, Vec<Box<dyn Backend>>) {
    let accelerators = [Accelerator::Metal, Accelerator::CoreMl, Accelerator::Cpu];
    (caps(&accelerators, Some(memory_mb)), native())
}

/// A PC whose builds all run on the CPU (Windows today: no GPU build in the catalogue for it).
fn cpu_only(memory_mb: u32) -> (Capabilities, Vec<Box<dyn Backend>>) {
    (caps(&[Accelerator::Cpu], Some(memory_mb)), native())
}

/// Linux with an NVIDIA GPU: no backend runs on CUDA yet, so it is a CPU machine for the catalogue.
fn linux_cuda() -> (Capabilities, Vec<Box<dyn Backend>>) {
    (
        caps(&[Accelerator::Cuda, Accelerator::Cpu], Some(16_384)),
        native(),
    )
}

/// An iPhone: Metal next to the CPU, which iOS stops serving to an app in the background; about 3 GB for the app.
fn iphone() -> (Capabilities, Vec<Box<dyn Backend>>) {
    let accelerators = [Accelerator::Metal, Accelerator::CoreMl, Accelerator::Cpu];
    let mut caps = caps(&accelerators, Some(3_072));
    caps.gpu_stops_in_background = true;
    (caps, native())
}

/// A browser tab with WebGPU, whose memory the page cannot tell.
fn browser() -> (Capabilities, Vec<Box<dyn Backend>>) {
    let caps = Capabilities {
        runs: Runs::Page,
        os: "web".to_owned(),
        arch: "wasm32".to_owned(),
        ..caps(&[Accelerator::WebGpu, Accelerator::Wasm], None)
    };
    (caps, page())
}

fn engine((caps, backends): (Capabilities, Vec<Box<dyn Backend>>)) -> Engine {
    Engine::with_backends(
        Box::new(Device(caps)),
        vec![Box::new(BundledCatalog)],
        backends,
    )
    .expect("engine")
}

fn speaking(stt: &str, tts: &str) -> Languages {
    Languages {
        stt: Some(stt.to_owned()),
        tts: Some(tts.to_owned()),
    }
}

fn pick(model: &str, build: &str) -> Option<Pick> {
    Some(Pick {
        model: model.to_owned(),
        build: format!("{model}/{build}"),
    })
}

fn ranked(picks: &[Pick]) -> Vec<&str> {
    picks.iter().map(|pick| pick.model.as_str()).collect()
}

#[test]
fn each_device_gets_the_best_pair_of_the_lists_that_runs_well_there() {
    let es = speaking("es", "es");
    let cases = [
        // Metal runs Whisper large-v3 turbo (whisper.cpp), and the list's first voice speaks Spanish.
        (
            "mac 16 GB",
            mac(16_384),
            &es,
            pick("whisper-large-v3-turbo", "whisper-cpp-q5_0"),
            pick("supertonic-3", "sherpa-onnx-int8"),
        ),
        // 8 GB: 60 % is 4.8 GB, the whole set takes under 1 GB.
        (
            "mac 8 GB",
            mac(8_192),
            &es,
            pick("whisper-large-v3-turbo", "whisper-cpp-q5_0"),
            pick("supertonic-3", "sherpa-onnx-int8"),
        ),
        // Turbo would run on the CPU only, which its entry does not take (too slow there): Parakeet.
        (
            "windows 8 GB",
            cpu_only(8_192),
            &es,
            pick("parakeet-tdt-0.6b-v3", "sherpa-onnx-int8"),
            pick("supertonic-3", "sherpa-onnx-int8"),
        ),
        // An NVIDIA GPU no build of the catalogue runs on is no GPU for it.
        (
            "linux + cuda",
            linux_cuda(),
            &es,
            pick("parakeet-tdt-0.6b-v3", "sherpa-onnx-int8"),
            pick("supertonic-3", "sherpa-onnx-int8"),
        ),
        // Turbo's build on Metal stops in the background, and its CPU build is not on the GPU its entry asks for.
        (
            "iphone",
            iphone(),
            &es,
            pick("parakeet-tdt-0.6b-v3", "sherpa-onnx-int8"),
            pick("supertonic-3", "sherpa-onnx-int8"),
        ),
        // 1.5 GB: Parakeet (840 MB) fits alone but with no voice and the end of turn inside 921 MB; Whisper small with
        // the first voice does, and its places (2nd, 1st) are as close to the top as Parakeet's with the 2nd voice.
        (
            "1.5 GB laptop",
            cpu_only(1_536),
            &es,
            pick("whisper-small", "sherpa-onnx-int8"),
            pick("supertonic-3", "sherpa-onnx-int8"),
        ),
        // A page: turbo on WebGPU, as its q8 build (its fp16 one needs `shader-f16`, which nothing here checks); Supertonic
        // 3 has no build for the page, Supertonic 2 does.
        (
            "browser",
            browser(),
            &es,
            pick("whisper-large-v3-turbo", "transformers-js-q8"),
            pick("supertonic-2", "transformers-js-fp32"),
        ),
    ];
    for (name, device, languages, stt, tts) in cases {
        let recommendation =
            block_on(engine(device).local_catalog().recommend(languages)).expect("recommend");
        assert_eq!(recommendation.stt.choice, stt, "{name}: speech to text");
        assert_eq!(recommendation.tts.choice, tts, "{name}: text to speech");
    }
}

#[test]
fn a_model_that_does_not_speak_the_language_is_not_recommended_unless_none_does() {
    // Italian: Parakeet and Qwen3-ASR hear it; Supertonic 3 speaks it, and Kokoro has voices in it (a voice's language
    // counts as the model's).
    let italian = block_on(
        engine(cpu_only(8_192))
            .local_catalog()
            .recommend(&speaking("it", "it")),
    )
    .expect("it");
    assert_eq!(
        ranked(&italian.stt.ranked),
        ["parakeet-tdt-0.6b-v3", "qwen3-asr-0.6b", "whisper-small"]
    );
    assert_eq!(
        ranked(&italian.tts.ranked),
        ["supertonic-3", "kokoro-82m-v1.0"]
    );
    // Spanish leaves out the English Piper voice, and keeps the Spanish one.
    let spanish = block_on(
        engine(cpu_only(8_192))
            .local_catalog()
            .recommend(&speaking("es-ES", "es-ES")),
    )
    .expect("es");
    assert_eq!(
        ranked(&spanish.tts.ranked),
        [
            "supertonic-3",
            "supertonic-2",
            "kokoro-82m-v1.0",
            "piper-es_ES-carlfm-x_low"
        ]
    );
    // Basque: no voice speaks it, so no voice is recommended (all stay selectable); Whisper hears it, and is.
    let basque = block_on(
        engine(cpu_only(8_192))
            .local_catalog()
            .recommend(&speaking("eu", "eu")),
    )
    .expect("eu");
    assert_eq!(basque.tts.choice, None);
    assert!(basque.tts.ranked.is_empty());
    assert_eq!(ranked(&basque.stt.ranked), ["whisper-small"]);
    assert_eq!(basque.stt.choice, pick("whisper-small", "sherpa-onnx-int8"));
    // No language: nothing is left out for it.
    let any = block_on(
        engine(cpu_only(8_192))
            .local_catalog()
            .recommend(&Languages::default()),
    )
    .expect("any");
    assert_eq!(ranked(&any.tts.ranked).len(), 5);
}

#[test]
fn what_does_not_fit_the_budget_on_its_own_is_left_out_of_the_list() {
    // 1.5 GB: 921 MB for the set; Qwen3-ASR takes 1090 MB.
    let small = block_on(
        engine(cpu_only(1_536))
            .local_catalog()
            .recommend(&speaking("es", "es")),
    )
    .expect("small");
    assert_eq!(
        ranked(&small.stt.ranked),
        ["parakeet-tdt-0.6b-v3", "whisper-small"]
    );
}

#[test]
fn on_a_phone_a_models_build_is_one_that_keeps_working_in_the_background() {
    let models = block_on(engine(iphone()).models()).expect("models");
    let recommended = |id: &str| {
        let model = models.iter().find(|model| model.id == id).expect("listed");
        model.recommended_build.clone().expect("a build runs here")
    };
    // whisper.cpp on Metal runs here, and is ranked first, but stops in the background: sherpa-onnx on the CPU instead.
    assert_eq!(
        recommended("whisper-small"),
        "whisper-small/sherpa-onnx-int8"
    );
    let mac = block_on(engine(mac(16_384)).models()).expect("models");
    let small = mac
        .iter()
        .find(|model| model.id == "whisper-small")
        .expect("listed");
    assert_eq!(
        small.recommended_build.as_deref(),
        Some("whisper-small/whisper-cpp-q5_1")
    );
}

#[test]
fn a_build_is_the_first_that_runs_keeps_working_and_fits_then_the_first_that_keeps_working_then_the_first(
) {
    // (id, runs on, stops in the background, memory)
    let builds = [
        ("gpu", Some(Accelerator::Metal), true, 100),
        ("big", Some(Accelerator::Cpu), false, 900),
        ("cpu", Some(Accelerator::Cpu), false, 200),
        ("absent", None, false, 10),
    ];
    let chosen = |budget: Option<u32>, of: &[(&'static str, Option<Accelerator>, bool, u32)]| {
        build(of.iter(), |b| b.1, |b| b.2, |b| b.3, budget).map(|b| b.0)
    };
    assert_eq!(chosen(Some(500), &builds), Some("cpu"));
    assert_eq!(chosen(None, &builds), Some("big"));
    assert_eq!(
        chosen(Some(50), &builds),
        Some("big"),
        "none fits: the first that keeps working"
    );
    assert_eq!(
        chosen(Some(500), &builds[..1]),
        Some("gpu"),
        "only one that stops: it still runs"
    );
    assert_eq!(chosen(Some(500), &builds[3..]), None, "nothing runs here");
}

#[test]
fn the_end_of_turn_model_is_counted_in_the_set() {
    // 1750 MB: 1050 MB for the set. Parakeet (840) and Supertonic 3 (190) take 1030 alone, 1080 with smart-turn (50):
    // over, so Whisper small with Supertonic 3 (710) is the pair.
    let tight = block_on(
        engine(cpu_only(1_750))
            .local_catalog()
            .recommend(&speaking("es", "es")),
    )
    .expect("tight");
    assert_eq!(ranked(&tight.stt.ranked)[0], "parakeet-tdt-0.6b-v3");
    assert_eq!(tight.stt.choice, pick("whisper-small", "sherpa-onnx-int8"));
    assert_eq!(tight.tts.choice, pick("supertonic-3", "sherpa-onnx-int8"));
}

#[test]
fn when_no_pair_fits_the_lightest_is_recommended() {
    // 850 MB: 510 MB for the set. Whisper small (470) fits alone, as every voice does, but with a voice and the end of
    // turn none does: the lightest pair, Whisper small with the Spanish Piper voice (20), is recommended.
    let small = block_on(
        engine(cpu_only(850))
            .local_catalog()
            .recommend(&speaking("es", "es")),
    )
    .expect("small");
    assert_eq!(ranked(&small.stt.ranked), ["whisper-small"]);
    assert_eq!(small.stt.choice, pick("whisper-small", "sherpa-onnx-int8"));
    assert_eq!(
        small.tts.choice,
        pick("piper-es_ES-carlfm-x_low", "sherpa-onnx-int8")
    );
}

#[test]
fn a_model_two_lists_name_counts_once_at_its_first_place() {
    struct Again;
    impl crate::CatalogSource for Again {
        fn load(&self) -> Result<crate::CatalogFragment> {
            let entry = |model: &str| crate::ListEntry {
                model: model.to_owned(),
                on: None,
            };
            Ok(crate::CatalogFragment {
                families: Vec::new(),
                recommended: crate::RecommendedLists {
                    basis: "a test".to_owned(),
                    stt: vec![entry("parakeet-tdt-0.6b-v3")],
                    tts: Vec::new(),
                },
            })
        }
    }
    let (caps, backends) = cpu_only(8_192);
    let engine = Engine::with_backends(
        Box::new(Device(caps)),
        vec![Box::new(BundledCatalog), Box::new(Again)],
        backends,
    )
    .expect("engine");
    let twice = block_on(engine.local_catalog().recommend(&speaking("es", "es"))).expect("twice");
    assert_eq!(
        ranked(&twice.stt.ranked),
        ["parakeet-tdt-0.6b-v3", "qwen3-asr-0.6b", "whisper-small"]
    );
}
