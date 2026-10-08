use super::runtime_files;
use super::schema::entries;
use crate::host::Platform;

#[cfg(web)]
use wasm_bindgen_test::wasm_bindgen_test as test;

#[test]
fn a_backend_has_no_runtime_where_backends_json_says_null() {
    // sherpa-onnx is native only, and the placeholders download nothing where they run.
    assert!(runtime_files("sherpa-onnx", Platform::Web).is_none());
    assert_eq!(
        runtime_files("mlx", Platform::MacosAarch64),
        Some(Vec::new())
    );
    assert!(runtime_files("mlx", Platform::LinuxX86_64).is_none());
    assert!(runtime_files("no-such-backend", Platform::LinuxX86_64).is_none());
}

#[test]
fn runtime_files_are_artifacts_keyed_by_their_name_at_the_backends_version() {
    let entry = entries()
        .iter()
        .find(|entry| entry.id == "sherpa-onnx")
        .expect("sherpa-onnx");
    let files = runtime_files("sherpa-onnx", Platform::LinuxX86_64).expect("linux-x86_64");
    let keys: Vec<_> = files.iter().map(|artifact| artifact.key.as_str()).collect();
    assert_eq!(keys, ["library"]);
    let url = &files[0].url;
    assert!(!url.contains("{version}"), "{url}");
    assert!(url.contains(&format!("v{}", entry.version)), "{url}");
}

#[test]
fn a_backend_is_known_if_backends_json_has_it_whether_or_not_it_runs_anywhere() {
    assert!(super::is_known("sherpa-onnx"));
    // Its entry is all `null`: no build of the engine runs it yet, but the catalogue may name it.
    assert!(super::is_known("whisper-cpp"));
    assert!(!super::is_known("no-such-backend"));
}
