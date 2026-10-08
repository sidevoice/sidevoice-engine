//! Memory only finds what something else holds: a library and a model are found while held, and forgotten once not.

use std::sync::Arc;

use super::Memory;
use crate::backend::{BackendModel, Library};
use crate::catalog::BuildEntry;
use crate::engine::loaded::Resident;
use crate::host::Accelerator;
use crate::install::Installed;
use crate::{async_trait, Error, Result};

#[cfg(web)]
use wasm_bindgen_test::wasm_bindgen_test as test;

struct NoLibrary;

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl Library for NoLibrary {
    async fn load(
        &self,
        _build: &BuildEntry,
        _accelerator: Accelerator,
        _files: &Installed,
    ) -> Result<Box<dyn BackendModel>> {
        Err(Error::new("not-implemented"))
    }
}

struct NoModel;

impl BackendModel for NoModel {
    fn memory_mb(&self) -> Option<u32> {
        None
    }
}

fn resident(library: &Arc<dyn Library>) -> Arc<Resident> {
    Resident::new(
        Box::new(NoModel),
        Arc::clone(library),
        Vec::new(),
        Vec::new(),
    )
}

#[test]
fn a_library_and_a_model_are_found_while_held_and_forgotten_once_not() {
    let mut memory = Memory::default();
    let library: Arc<dyn Library> = Arc::new(NoLibrary);
    let model = resident(&library);
    memory.remember("fake", &library, "a", &model);
    drop(library);

    assert!(
        memory.library("fake").is_some(),
        "the model holds its library"
    );
    assert!(memory.model("a").is_some());
    assert!(memory.model("b").is_none());

    drop(model);
    assert!(memory.model("a").is_none(), "nothing holds it");
    assert!(memory.library("fake").is_none(), "nor its library");
}
