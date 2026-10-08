// The sherpa-onnx library exists on every native platform.
#![cfg(native)]

use async_trait::async_trait;

use crate::backend::{Backend, BackendFactory, BackendSpec, Library};
use crate::host::Accelerator;
use crate::install::Installed;
use crate::{Error, Result};

struct SherpaOnnx;

const SPEC: BackendSpec = BackendSpec {
    id: "sherpa-onnx",
    accelerators: &[Accelerator::CoreMl, Accelerator::Cpu],
    requirements: &[],
};

inventory::submit! { BackendFactory(|| Box::new(SherpaOnnx)) }

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl Backend for SherpaOnnx {
    fn spec(&self) -> &BackendSpec {
        &SPEC
    }

    async fn open(&self, _files: &Installed) -> Result<Box<dyn Library>> {
        // To come: open the sherpa-onnx C API from `files` with `libloading`; its `Library` loads the model on
        // `accelerator`.
        Err(Error::new("not-implemented"))
    }
}
