//! Which backends each build contains, and the funnel over a fake host. The expected lists use the raw target
//! conditions, not the engine's aliases: these tests are what checks the aliases.

use crate::{
    async_trait, built_in, Accelerator, Build, Capabilities, CatalogFragment, CatalogSource,
    Engine, Fetcher, Host, Model, Offer, Reason, Rejection, Result, Runs, Storage, Task,
};

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

#[test]
fn built_in_is_exactly_this_platforms_backends() {
    let mut ids: Vec<_> = built_in().iter().map(|backend| backend.spec().id).collect();
    ids.sort_unstable();
    let expected: &[&str] = if cfg!(target_arch = "wasm32") {
        &["transformers-js"]
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        &["mlx", "sherpa-onnx"]
    } else {
        &["sherpa-onnx"]
    };
    assert_eq!(ids, expected);
}

#[test]
fn an_engine_with_a_fake_host_offers_what_fits_and_says_why_the_rest_does_not() {
    let engine = Engine::new(Box::new(FakeHost), vec![Box::new(FakeCatalog)]).expect("engine");
    let offers = engine.offers(Task::Stt);

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
        Rejection::DoesNotFit(Reason::numbers("memory", 16_384, 8_192))
    };
    assert!(rejected.contains(&("whisper-large-onnx", large_rejection)));
    assert!(offers
        .iter()
        .all(|offer| !matches!(offer, Offer::Offered { model, .. } if model.task != Task::Stt)));
}

#[test]
fn backends_probe_and_fit_without_loading_anything() {
    let caps = FakeHost.capabilities();
    for backend in built_in() {
        assert!(
            !backend.probe(&caps).is_empty(),
            "{} should work on the fake host",
            backend.spec().id
        );
    }
}

struct FakeHost;

impl Host for FakeHost {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            runs: if cfg!(target_arch = "wasm32") {
                Runs::Page
            } else {
                Runs::Native
            },
            os: std::env::consts::OS.to_owned(),
            arch: std::env::consts::ARCH.to_owned(),
            accelerators: vec![Accelerator::Cpu, Accelerator::Wasm],
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

#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
impl Storage for FakeHost {
    async fn contains(&self, _key: &str) -> Result<bool> {
        Ok(false)
    }
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
impl Fetcher for FakeHost {
    async fn fetch(&self, _url: &str, _sha256: &str, _key: &str) -> Result<()> {
        unreachable!("offers never download")
    }
}

struct FakeCatalog;

impl CatalogSource for FakeCatalog {
    fn load(&self) -> Result<CatalogFragment> {
        let build = |id: &str, backend, format: &str, memory_mb| Build {
            id: id.to_owned(),
            backend,
            format: format.to_owned(),
            memory_mb,
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
