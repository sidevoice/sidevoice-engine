// transformers.js is a JavaScript module of the npm package: it only exists in the web build.
#![cfg(web)]

use async_trait::async_trait;

use crate::backend::registry::BackendFactory;
use crate::backend::{Backend, BackendSpec, Library};
use crate::host::Accelerator;
use crate::install::Installed;
use crate::{Error, Result};

struct TransformersJs;

const SPEC: BackendSpec = BackendSpec {
    id: "transformers-js",
    name: "Transformers.js",
    description: "Models in the browser on ONNX Runtime Web. A stub: it loads no model yet.",
    upstream: "https://github.com/huggingface/transformers.js",
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

    async fn open(&self, _files: &Installed) -> Result<Box<dyn Library>> {
        // To come: import the transformers.js module at run time; its `Library` loads the model from `files` on
        // `accelerator`.
        Err(Error::new("not-implemented"))
    }
}
