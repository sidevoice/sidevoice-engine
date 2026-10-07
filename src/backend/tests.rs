//! Which backends each build contains. The expected lists use the raw target conditions, not the engine's aliases:
//! these tests are what checks the aliases.

use super::{built_in, downloads, Platform};
use crate::host::Host;
use crate::test_support::FakeHost;

#[cfg(web)]
use wasm_bindgen_test::wasm_bindgen_test as test;

#[test]
fn built_in_is_exactly_this_platforms_backends() {
    let mut ids: Vec<_> = built_in().iter().map(|backend| backend.spec().id).collect();
    ids.sort_unstable();
    let expected: &[&str] = if cfg!(target_arch = "wasm32") {
        &["transformers-js"]
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        &["mlx", "sherpa-onnx"]
    } else {
        &["sherpa-onnx"]
    };
    assert_eq!(ids, expected);
}

#[test]
fn backends_probe_and_fit_without_loading_anything() {
    let caps = FakeHost.capabilities();
    for backend in built_in() {
        assert!(
            !backend.probe(&caps).is_empty(),
            "{} should work on the fake host",
            backend.spec().id
        );
    }
}

#[test]
fn every_backend_of_this_build_has_downloads_for_this_platform() {
    let platform =
        Platform::of(&FakeHost.capabilities()).expect("backends.json knows this platform");
    for backend in built_in() {
        let id = backend.spec().id;
        assert!(
            downloads(id, platform).is_some(),
            "backends.json has no {platform:?} entry for {id}, which this platform compiles"
        );
    }
}
