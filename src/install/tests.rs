use super::Installed;

#[cfg(web)]
use wasm_bindgen_test::wasm_bindgen_test as test;

#[test]
fn installed_files_are_found_by_name() {
    let installed = Installed {
        files: [("library".to_owned(), "/data/backends/library".to_owned())].into(),
    };
    assert_eq!(installed.file("library"), Some("/data/backends/library"));
    assert_eq!(installed.file("model.onnx"), None);
}
