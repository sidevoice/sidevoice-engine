use super::{check, WebStorage};
use crate::{Download, Storage};
use wasm_bindgen_test::wasm_bindgen_test;

const NAME: &str = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";

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

#[wasm_bindgen_test]
fn names_are_letters_digits_dashes_and_underscores() {
    assert_eq!(check(NAME), Ok(()));
    assert_eq!(check("a-b_C9"), Ok(()));
    for name in ["", "a.partial-1", "../a", "a/b", "a b", "ñ"] {
        assert_eq!(
            check(name).map_err(|e| e.code),
            Err("storage-name-invalid"),
            "{name:?}"
        );
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
}

#[wasm_bindgen_test]
#[ignore = "needs OPFS: a browser (cargo xtask test-browser)"]
async fn a_file_is_stored_whole_only_once_committed() {
    let storage = storage("sidevoice-engine-test-commit").await;
    assert_eq!(storage.find(NAME).await, Ok(None));
    let mut writer = storage.create(NAME).await.expect("created");
    writer.write(b"hello, ").await.expect("written");
    writer.write(b"world").await.expect("written");
    // Written, not committed: not stored.
    assert_eq!(storage.find(NAME).await, Ok(None));
    let location = writer.commit().await.expect("committed");
    assert_eq!(location, format!("sidevoice-engine-test-commit/{NAME}"));
    assert_eq!(storage.find(NAME).await, Ok(Some(location.clone())));

    let mut read = storage.read(NAME).await.expect("read");
    assert_eq!(read.size(), Some(12));
    assert_eq!(all(read.as_mut()).await, b"hello, world");
    // What the transformers.js backend opens a file by.
    let blob = crate::web::opfs::open(&location)
        .await
        .expect("opened")
        .expect("there");
    assert_eq!(blob.size() as u64, 12);

    storage.remove(NAME).await.expect("removed");
    assert_eq!(storage.find(NAME).await, Ok(None));
    // Removing what is not there is not an error.
    storage.remove(NAME).await.expect("nothing to remove");
}

#[wasm_bindgen_test]
#[ignore = "needs OPFS: a browser (cargo xtask test-browser)"]
async fn a_writer_dropped_uncommitted_leaves_nothing() {
    let directory = "sidevoice-engine-test-drop";
    let storage = storage(directory).await;
    let mut writer = storage.create(NAME).await.expect("created");
    writer.write(b"half a file").await.expect("written");
    drop(writer);
    assert_eq!(storage.find(NAME).await, Ok(None));
    // The temporary file goes in the background: wait for the page to get to it.
    let root = crate::web::opfs::root().await.expect("OPFS");
    let opened = root.directory(directory).await.expect("there");
    for _ in 0..50 {
        if opened.names().await.expect("listed").is_empty() {
            return;
        }
        crate::test_support::pause(20).await;
    }
    panic!("the temporary file was left: {:?}", opened.names().await);
}

#[wasm_bindgen_test]
#[ignore = "needs OPFS: a browser (cargo xtask test-browser)"]
async fn committing_again_replaces_the_file() {
    let storage = storage("sidevoice-engine-test-replace").await;
    for content in [&b"first"[..], b"second"] {
        let mut writer = storage.create(NAME).await.expect("created");
        writer.write(content).await.expect("written");
        writer.commit().await.expect("committed");
    }
    let mut read = storage.read(NAME).await.expect("read");
    assert_eq!(all(read.as_mut()).await, b"second");
}

#[wasm_bindgen_test]
async fn trees_are_not_stored_on_the_web() {
    let storage = WebStorage::new();
    let found = storage.find_member("tree", "a/b").await;
    assert_eq!(found.err().map(|e| e.code), Some("archive-unsupported"));
}
