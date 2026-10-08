//! Test doubles shared by the tests of every module: a host (CPU and Wasm everywhere, Metal on Apple silicon, 8 GB,
//! 8 cores) and a small catalogue with a build for each backend, one that needs too much memory and one for a backend
//! no build has.

use crate::{
    async_trait, Accelerator, Build, Capabilities, CatalogFragment, CatalogSource, Fetcher, Host,
    Model, Result, Runs, Storage, Task,
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

/// The output of `future`, which must be ready on its first poll: the engine's futures that do not wait on a host
/// (a backend's `load`, a loaded model's work) are, and the tests have no executor.
#[cfg(sherpa_onnx)]
pub(crate) fn ready<F: std::future::Future>(future: F) -> F::Output {
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    match std::pin::pin!(future).poll(&mut context) {
        std::task::Poll::Ready(output) => output,
        std::task::Poll::Pending => panic!("the future waits on something"),
    }
}
