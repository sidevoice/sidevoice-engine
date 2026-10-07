//! Idle models leave memory on a clock the test moves, and a backend's library stays open while any of its models is
//! loaded.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

use super::Memory;
use crate::backend::{Library, LoadedModel};
use crate::catalog::Build;
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
        _build: &Build,
        _accelerator: Accelerator,
        _files: &Installed,
    ) -> Result<Box<dyn LoadedModel>> {
        Err(Error::new("not-implemented"))
    }
}

struct NoModel;

impl LoadedModel for NoModel {
    fn memory_mb(&self) -> Option<u32> {
        None
    }
}

const MINUTE: Duration = Duration::from_secs(60);

#[test]
fn a_model_leaves_memory_once_unused_for_the_idle_time_and_its_library_with_the_last_one() {
    static MINUTES: AtomicU64 = AtomicU64::new(0);
    let at = |minutes| MINUTES.store(minutes, Ordering::Relaxed);
    let mut memory = Memory::with_clock(10 * MINUTE, || {
        MINUTE * u32::try_from(MINUTES.load(Ordering::Relaxed)).unwrap()
    });

    let library: Arc<dyn Library> = Arc::new(NoLibrary);
    let closed: Weak<dyn Library> = Arc::downgrade(&library);
    memory.insert("a", "fake", Arc::clone(&library), Box::new(NoModel));
    at(5);
    let shared = memory.library("fake").expect("open while a is loaded");
    memory.insert("b", "fake", shared, Box::new(NoModel));
    drop(library);

    at(9);
    memory.unload_idle();
    assert!(memory.is_loaded("a") && memory.is_loaded("b"));

    at(10);
    memory.unload_idle();
    assert!(!memory.is_loaded("a"), "unused for 10 minutes");
    assert!(memory.is_loaded("b"));
    assert_eq!(memory.open_libraries(), 1);

    at(14);
    assert!(memory.handle("b").is_some(), "using it");
    at(20);
    memory.unload_idle();
    assert!(memory.is_loaded("b"), "used 6 minutes ago");

    at(24);
    memory.unload_idle();
    assert!(!memory.is_loaded("b"));
    assert_eq!(memory.open_libraries(), 0);
    assert!(memory.library("fake").is_none());
    assert!(closed.upgrade().is_none(), "the library is closed");
}

#[test]
fn a_model_taken_out_to_be_used_is_busy_and_stays_until_put_back() {
    let mut memory = Memory::new(Duration::ZERO);
    let handle = memory.insert("a", "fake", Arc::new(NoLibrary), Box::new(NoModel));
    let model = memory.take(handle).map_err(|e| e.code).expect("loaded");
    assert_eq!(
        memory.take(handle).map(|_| ()),
        Err(Error::new("model-busy"))
    );
    memory.unload_idle();
    assert!(memory.is_loaded("a"), "in use, whatever the idle time");

    memory.put_back(handle, model);
    assert!(memory.take(handle).is_ok(), "back");
    memory.put_back(handle, Box::new(NoModel));
    memory.unload_idle();
    assert_eq!(
        memory.take(handle).map(|_| ()),
        Err(Error::new("model-not-loaded"))
    );
}
