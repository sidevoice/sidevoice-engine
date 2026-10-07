// transformers.js is a JavaScript module of the npm package: it only exists in the web build.
#![cfg(web)]

use async_trait::async_trait;

use crate::{Accelerator, Backend, BackendFactory, BackendSpec, Build, LoadedModel};
use crate::{Error, Installed, Result};

pub(crate) struct TransformersJs;

const SPEC: BackendSpec = BackendSpec {
    id: "transformers-js",
    accelerators: &[Accelerator::WebGpu, Accelerator::Wasm],
    requirements: &[],
};

inventory::submit! { BackendFactory(|| Box::new(TransformersJs)) }

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl Backend for TransformersJs {
    fn spec(&self) -> &BackendSpec {
        &SPEC
    }

    async fn load(
        &self,
        _build: &Build,
        _accelerator: Accelerator,
        _files: &Installed,
    ) -> Result<Box<dyn LoadedModel>> {
        Err(Error::new("not-implemented"))
    }
}
