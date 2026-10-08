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
fn sherpa_onnx_is_linked_so_it_downloads_nothing_and_its_version_is_the_crates() {
    for platform in [
        Platform::MacosAarch64,
        Platform::MacosX86_64,
        Platform::LinuxX86_64,
        Platform::LinuxAarch64,
        Platform::WindowsX86_64,
    ] {
        assert_eq!(
            runtime_files("sherpa-onnx", platform),
            Some(Vec::new()),
            "{platform:?}"
        );
    }
    let version = &entries()
        .iter()
        .find(|entry| entry.id == "sherpa-onnx")
        .expect("sherpa-onnx")
        .version;
    let pinned = format!("sherpa-onnx = {{ version = \"={version}\"");
    assert!(
        include_str!("../../../Cargo.toml").contains(&pinned),
        "backends.json says {version}; Cargo.toml must pin the crate to it"
    );
}
