//! Which backends each build contains. The expected lists use the raw target conditions, not the engine's aliases:
//! these tests are what checks the aliases.

use super::{built_in, is_known, KNOWN};
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
        if cfg!(feature = "sherpa-onnx") {
            &["mlx", "sherpa-onnx"]
        } else {
            &["mlx"]
        }
    } else if cfg!(feature = "sherpa-onnx") {
        &["sherpa-onnx"]
    } else {
        &[]
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
fn every_backend_of_this_build_is_known_and_describes_itself() {
    for backend in built_in() {
        let spec = backend.spec();
        assert!(is_known(spec.id), "{} is not in KNOWN", spec.id);
        assert!(
            !spec.name.is_empty() && !spec.description.is_empty(),
            "{}",
            spec.id
        );
        assert!(spec.upstream.starts_with("https://"), "{}", spec.id);
    }
    assert!(
        is_known("whisper-cpp"),
        "named by the catalogue before its code exists"
    );
    assert!(!is_known("no-such-backend"));
    let mut sorted = KNOWN.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), KNOWN.len(), "no id twice");
}
