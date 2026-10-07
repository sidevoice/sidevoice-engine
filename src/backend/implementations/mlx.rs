// MLX only exists on Apple silicon.
#![cfg(apple_silicon)]

use async_trait::async_trait;

use crate::backend::{Backend, BackendFactory, BackendSpec, MinMemoryMb};
use crate::catalog::Build;
use crate::host::Accelerator;
use crate::install::Installed;
use crate::model::LoadedModel;
use crate::{Error, Result};

struct Mlx;

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

    async fn load(
        &self,
        _build: &Build,
        _accelerator: Accelerator,
        _files: &Installed,
    ) -> Result<Box<dyn LoadedModel>> {
        Err(Error::new("not-implemented"))
    }
}
