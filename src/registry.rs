//! The backends compiled into this build. Each backend submits its factory with `inventory::submit!`; a backend file
//! that cannot compile here does not exist here (its `#![cfg]`), so neither does its entry. Nobody keeps a list.
//!
//! On wasm32 the linker only takes the object files of an rlib that something references, and nothing references
//! these constructors: a crate that links the engine as an rlib (an integration test, say) gets no web backends. The
//! npm package is this crate's own cdylib, which keeps them; for the same reason the tests that count backends are
//! unit tests (src/tests.rs), and `cargo xtask npm-smoke` checks the package.

use crate::backend::Backend;

/// Makes a backend's lazy, empty object: nothing is loaded.
pub struct BackendFactory(pub fn() -> Box<dyn Backend>);

inventory::collect!(BackendFactory);

/// Every backend compiled into this build, in no particular order.
pub fn built_in() -> Vec<Box<dyn Backend>> {
    inventory::iter::<BackendFactory>
        .into_iter()
        .map(|factory| (factory.0)())
        .collect()
}
