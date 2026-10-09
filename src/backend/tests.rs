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
    // Remote backends run nothing here: every build has them.
    let mut expected: Vec<&str> = vec!["elevenlabs", "openai"];
    if cfg!(target_arch = "wasm32") {
        expected.push("transformers-js");
    } else {
        if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            expected.push("mlx");
        }
        expected.push("onnxruntime");
        expected.push("sherpa-onnx");
        expected.push("whisper-cpp");
    }
    expected.sort_unstable();
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
        "named by the catalogue, whether this build has its code or not"
    );
    assert!(!is_known("no-such-backend"));
    let mut sorted = KNOWN.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), KNOWN.len(), "no id twice");
}

#[test]
fn only_remote_backends_name_a_provider_and_they_run_on_the_remote_accelerator_alone() {
    for backend in built_in() {
        let spec = backend.spec();
        let remote = spec.provider.is_some();
        assert_eq!(
            remote,
            spec.accelerators == [crate::Accelerator::Remote],
            "{}",
            spec.id
        );
        assert_eq!(super::is_remote(spec.id), remote, "{}", spec.id);
    }
    assert!(super::is_remote("openai") && super::is_remote("elevenlabs"));
    assert!(!super::is_remote("sherpa-onnx"));
}
