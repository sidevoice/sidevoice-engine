// transformers.js is a JavaScript module of the npm package: it only exists in the web build.
#![cfg(web)]

use async_trait::async_trait;

use crate::backend::LoadedModel;
use crate::backend::{Backend, BackendFactory, BackendSpec};
use crate::catalog::Build;
use crate::host::Accelerator;
use crate::install::Installed;
use crate::{Error, Result};

struct TransformersJs;

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
        // To come: import the transformers.js module at run time, then the model from `files` on `accelerator`.
        Err(Error::new("not-implemented"))
    }
}
