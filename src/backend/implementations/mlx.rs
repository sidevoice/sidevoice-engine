// MLX only exists on Apple silicon.
#![cfg(apple_silicon)]

use async_trait::async_trait;

use crate::backend::registry::BackendFactory;
use crate::backend::{Backend, BackendSpec, Library, MinMemoryMb};
use crate::host::Accelerator;
use crate::install::Installed;
use crate::resolver::Reason;
use crate::{Error, Result};

struct Mlx;

const SPEC: BackendSpec = BackendSpec {
    id: "mlx",
    name: "MLX",
    description: "Apple's array framework for Apple silicon. A stub: it loads no model yet.",
    upstream: "https://github.com/ml-explore/mlx",
    accelerators: &[Accelerator::Metal],
    requirements: &[&MinMemoryMb(8_192)],
};

inventory::submit! { BackendFactory(|| Box::new(Mlx)) }

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl Backend for Mlx {
    fn spec(&self) -> &BackendSpec {
        &SPEC
    }

    /// A stub: it loads no model yet, so none of its builds is offered, and none is downloaded.
    fn unavailable(&self) -> Option<Reason> {
        Some(Reason::new("not-implemented"))
    }

    async fn open(&self, _files: &Installed) -> Result<Box<dyn Library>> {
        // To come: open the MLX library at run time; its `Library` loads the model from `files` on Metal.
        Err(Error::new("not-implemented"))
    }
}
