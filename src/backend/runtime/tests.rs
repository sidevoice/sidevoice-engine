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
fn the_sherpa_onnx_library_is_the_lib_directory_of_its_archive_at_the_backends_version() {
    let version = &entries()
        .iter()
        .find(|entry| entry.id == "sherpa-onnx")
        .expect("sherpa-onnx")
        .version;
    for platform in [
        Platform::MacosAarch64,
        Platform::MacosX86_64,
        Platform::LinuxX86_64,
        Platform::LinuxAarch64,
        Platform::WindowsX86_64,
    ] {
        let files = runtime_files("sherpa-onnx", platform).expect("native");
        let path = files[0]
            .archive_path
            .as_deref()
            .expect("a member of the archive");
        assert!(
            path.starts_with(&format!("sherpa-onnx-v{version}-")),
            "{path}"
        );
        assert!(path.ends_with("/lib"), "{path}");
        let archive = files[0].url.rsplit('/').next().expect("a file name");
        assert_eq!(
            archive.strip_suffix(".tar.bz2"),
            path.strip_suffix("/lib"),
            "{platform:?}"
        );
    }
}
