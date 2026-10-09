use super::{check, check_folder, check_path, WebStorage};
use crate::{Download, Storage};
use wasm_bindgen_test::wasm_bindgen_test;

const NAME: &str = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
const OTHER: &str = "486ea46224d1bb4fb680f34f7c9ad96a8f24ec88be73ea8e5a6c65260e9cb8a7";
const BUILD: &str = "whisper-tiny/transformers-js-q8";

/// Each test its own OPFS directory, emptied of what a previous run left.
async fn storage(directory: &'static str) -> WebStorage {
    let root = crate::web::opfs::root().await.expect("OPFS");
    root.remove_recursively(directory).await.expect("emptied");
    WebStorage::in_directory(directory)
}

async fn all(download: &mut dyn Download) -> Vec<u8> {
    let mut bytes = Vec::new();
    while let Some(chunk) = download.chunk().await.expect("a part") {
        bytes.extend(chunk);
    }
    bytes
}

async fn store(storage: &WebStorage, name: &str, content: &[u8]) {
    let mut writer = storage.create(name).await.expect("created");
    writer.write(content).await.expect("written");
    writer.commit().await.expect("committed");
}

/// What `location` holds, opened as the transformers.js backend opens it.
async fn opened(location: &str) -> Vec<u8> {
    let blob = crate::web::opfs::open(location)
        .await
        .expect("opened")
        .expect("there");
    let buffer = wasm_bindgen_futures::JsFuture::from(blob.array_buffer())
        .await
        .expect("read");
    js_sys::Uint8Array::new(&buffer).to_vec()
}

/// Waits for what is removed in the background to go: `true` once `directory` (under the OPFS root) is empty.
async fn emptied(directory: &str) -> bool {
    let root = crate::web::opfs::root().await.expect("OPFS");
    let opened = root
        .at(directory, true)
        .await
        .expect("there")
        .expect("made");
    for _ in 0..50 {
        if opened.names().await.expect("listed").is_empty() {
            return true;
        }
        crate::test_support::pause(20).await;
    }
    false
}

#[wasm_bindgen_test]
fn names_folders_and_paths_follow_the_storage_rules() {
    assert_eq!(check(NAME), Ok(()));
    assert_eq!(check("a-b_C9"), Ok(()));
    for name in ["", "a.partial-1", "../a", "a/b", "a b", "ñ"] {
        assert_eq!(
            check(name).map_err(|e| e.code),
            Err("storage-name-invalid"),
            "{name:?}"
        );
    }
    assert_eq!(check_folder(BUILD), Ok(()));
    for name in ["", "a//b", "a/../b", "./a", "a b"] {
        assert!(check_folder(name).is_err(), "{name:?}");
    }
    assert_eq!(check_path("onnx/model_q8.onnx"), Ok(()));
    for path in ["", "/a", "a/", "a/../b", "a\\b"] {
        assert!(check_path(path).is_err(), "{path:?}");
    }
}

#[wasm_bindgen_test]
async fn without_opfs_storage_fails_with_storage_failed() {
    if crate::web::opfs::root().await.is_ok() {
        return; // A page with OPFS: the tests below cover it.
    }
    let storage = WebStorage::new();
    assert_eq!(
        storage.find(NAME).await.err().map(|e| e.code),
        Some("storage-failed")
    );
    assert_eq!(
        storage.find_folder(BUILD).await.err().map(|e| e.code),
        Some("storage-failed")
    );
}

#[wasm_bindgen_test]
#[ignore = "needs OPFS: a browser (cargo xtask test-browser)"]
async fn a_blob_is_stored_whole_only_once_committed() {
    let storage = storage("sidevoice-engine-test-commit").await;
    assert_eq!(storage.find(NAME).await, Ok(None));
    let mut writer = storage.create(NAME).await.expect("created");
    writer.write(b"hello, ").await.expect("written");
    writer.write(b"world").await.expect("written");
    // Written, not committed: not stored.
    assert_eq!(storage.find(NAME).await, Ok(None));
    let location = writer.commit().await.expect("committed");
    assert_eq!(
        location,
        format!("sidevoice-engine-test-commit/blobs/{NAME}")
    );
    assert_eq!(storage.find(NAME).await, Ok(Some(location.clone())));

    let mut read = storage.read(NAME).await.expect("read");
    assert_eq!(read.size(), Some(12));
    assert_eq!(all(read.as_mut()).await, b"hello, world");
    assert_eq!(opened(&location).await, b"hello, world");

    storage.remove(NAME).await.expect("removed");
    assert_eq!(storage.find(NAME).await, Ok(None));
    // Removing what is not there is not an error.
    storage.remove(NAME).await.expect("nothing to remove");
}

#[wasm_bindgen_test]
#[ignore = "needs OPFS: a browser (cargo xtask test-browser)"]
async fn a_blob_writer_dropped_uncommitted_leaves_nothing() {
    let storage = storage("sidevoice-engine-test-drop").await;
    let mut writer = storage.create(NAME).await.expect("created");
    writer.write(b"half a file").await.expect("written");
    drop(writer);
    assert_eq!(storage.find(NAME).await, Ok(None));
    assert!(
        emptied("sidevoice-engine-test-drop/blobs").await,
        "the temporary file was left"
    );
}

#[wasm_bindgen_test]
#[ignore = "needs OPFS: a browser (cargo xtask test-browser)"]
async fn committing_a_blob_again_replaces_it() {
    let storage = storage("sidevoice-engine-test-replace").await;
    store(&storage, NAME, b"first").await;
    store(&storage, NAME, b"second").await;
    let mut read = storage.read(NAME).await.expect("read");
    assert_eq!(all(read.as_mut()).await, b"second");
}

#[wasm_bindgen_test]
#[ignore = "needs OPFS: a browser (cargo xtask test-browser)"]
async fn a_folder_holds_copies_at_their_paths_and_is_stored_only_once_committed() {
    let directory = "sidevoice-engine-test-folder";
    let storage = storage(directory).await;
    store(&storage, NAME, b"weights").await;
    store(&storage, OTHER, b"{}").await;

    let mut folder = storage.create_folder(BUILD).await.expect("created");
    folder
        .link("onnx/model_q8.onnx", NAME, None)
        .await
        .expect("linked");
    folder
        .link("config.json", OTHER, None)
        .await
        .expect("linked");
    // Not committed: not stored, and nothing to find in it.
    assert_eq!(storage.find_folder(BUILD).await, Ok(None));
    assert_eq!(storage.find_in_folder(BUILD, "config.json").await, Ok(None));
    assert_eq!(storage.is_linked(NAME).await, Ok(false));

    let location = folder.commit().await.expect("committed");
    assert_eq!(location, format!("{directory}/models/{BUILD}"));
    assert_eq!(storage.find_folder(BUILD).await, Ok(Some(location.clone())));
    let model = storage
        .find_in_folder(BUILD, "onnx/model_q8.onnx")
        .await
        .expect("found")
        .expect("there");
    assert_eq!(model, format!("{location}/onnx/model_q8.onnx"));
    assert_eq!(opened(&model).await, b"weights");
    assert_eq!(
        storage.find_in_folder(BUILD, "onnx").await,
        Ok(Some(format!("{location}/onnx")))
    );
    assert_eq!(
        storage.find_in_folder(BUILD, "missing.onnx").await,
        Ok(None)
    );
    assert_eq!(storage.is_linked(NAME).await, Ok(true));
    assert_eq!(storage.is_linked(OTHER).await, Ok(true));
    assert!(
        emptied(&format!("{directory}/partial")).await,
        "staging left"
    );

    // A second build sharing one blob keeps it linked once the first is removed.
    let mut other = storage.create_folder("other/build").await.expect("created");
    other.link("model.onnx", NAME, None).await.expect("linked");
    other.commit().await.expect("committed");
    storage.remove_folder(BUILD).await.expect("removed");
    assert_eq!(storage.find_folder(BUILD).await, Ok(None));
    assert_eq!(storage.find_in_folder(BUILD, "config.json").await, Ok(None));
    assert_eq!(storage.is_linked(NAME).await, Ok(true));
    assert_eq!(storage.is_linked(OTHER).await, Ok(false));
    // Its directory, and the one above it left empty, are gone; the other build's stays.
    let root = crate::web::opfs::root().await.expect("OPFS");
    let models = root
        .at(&format!("{directory}/models"), false)
        .await
        .expect("listed")
        .expect("there");
    assert_eq!(models.names().await.expect("listed"), ["other"]);
    storage
        .remove_folder(BUILD)
        .await
        .expect("nothing to remove");
}

#[wasm_bindgen_test]
#[ignore = "needs OPFS: a browser (cargo xtask test-browser)"]
async fn a_folder_dropped_uncommitted_leaves_nothing_and_needs_its_blobs() {
    let directory = "sidevoice-engine-test-folder-drop";
    let storage = storage(directory).await;
    store(&storage, NAME, b"weights").await;
    let mut folder = storage.create_folder(BUILD).await.expect("created");
    assert_eq!(
        folder
            .link("a.onnx", OTHER, None)
            .await
            .err()
            .map(|e| e.code),
        Some("storage-failed"),
        "a blob that is not stored"
    );
    assert_eq!(
        folder
            .link("b", NAME, Some("member"))
            .await
            .err()
            .map(|e| e.code),
        Some("archive-unsupported")
    );
    folder.link("a.onnx", NAME, None).await.expect("linked");
    drop(folder);
    assert_eq!(storage.find_folder(BUILD).await, Ok(None));
    assert!(
        emptied(&format!("{directory}/partial")).await,
        "the staging directory was left"
    );
}

#[wasm_bindgen_test]
async fn trees_are_not_stored_on_the_web() {
    let storage = WebStorage::new();
    let found = storage.find_member("tree", "a/b").await;
    assert_eq!(found.err().map(|e| e.code), Some("archive-unsupported"));
}
