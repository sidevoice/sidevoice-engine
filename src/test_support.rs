//! Test doubles shared by the tests of every module: a host (CPU and Wasm everywhere, Metal on Apple silicon, 8 GB,
//! 8 cores), a small catalogue with a build for each backend, one that needs too much memory and one for a backend
//! no build has, and the builders it is made with.

use crate::{
    async_trait, Accelerator, Build, Capabilities, Capability, CatalogFragment, CatalogSource,
    Family, Fetcher, Host, Memory, MemorySource, Model, ModelFile, Precision, Requires, Result,
    Runs, Storage,
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
    async fn contains(&self, _key: &str) -> Result<bool> {
        Ok(false)
    }
}

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl Fetcher for FakeHost {
    async fn fetch(&self, _url: &str, _sha256: &str, _key: &str) -> Result<()> {
        unreachable!("offers never download")
    }
}

pub(crate) struct FakeCatalog;

/// A build of `backend` that needs `memory_mb`, with one file.
pub(crate) fn build(id: &str, backend: &str, memory_mb: u32) -> Build {
    Build {
        id: id.to_owned(),
        backend: backend.to_owned(),
        precision: Precision::Int8,
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
