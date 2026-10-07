// MLX only exists on Apple silicon.
#![cfg(apple_silicon)]

use async_trait::async_trait;

use crate::{
    Accelerator, Backend, BackendFactory, BackendSpec, Build, Capabilities, LoadedModel,
    MinMemoryMb,
};
use crate::{Error, Installed, Result};

pub(crate) struct Mlx;

const SPEC: BackendSpec = BackendSpec {
    id: "mlx",
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

    fn probe(&self, _caps: &Capabilities) -> Vec<Accelerator> {
        // A stub: Apple silicon always has Metal.
        vec![Accelerator::Metal]
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
