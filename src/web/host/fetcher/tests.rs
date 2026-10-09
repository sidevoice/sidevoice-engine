use super::WebFetcher;
use crate::{Download, Fetcher};
use wasm_bindgen_test::wasm_bindgen_test;

/// Every part of `download`, in order.
async fn all(download: &mut dyn Download) -> Vec<u8> {
    let mut bytes = Vec::new();
    while let Some(chunk) = download.chunk().await.expect("a part") {
        assert!(!chunk.is_empty(), "a part is never empty");
        bytes.extend(chunk);
    }
    bytes
}

#[wasm_bindgen_test]
async fn a_download_streams_the_body_it_fetched() {
    let mut download = WebFetcher
        .fetch("data:application/octet-stream;base64,aGVsbG8sIHdvcmxk")
        .await
        .expect("fetched");
    assert_eq!(all(download.as_mut()).await, b"hello, world");
    // Read to the end, it stays there.
    assert_eq!(download.chunk().await, Ok(None));
}

#[wasm_bindgen_test]
async fn what_cannot_be_fetched_fails_with_download_failed() {
    // Nothing listens on port 9 (discard) of this machine, and a malformed URL never gets that far.
    for url in ["http://127.0.0.1:9/nothing", "not a url"] {
        let failed = WebFetcher.fetch(url).await.err();
        assert_eq!(
            failed.map(|error| error.code),
            Some("download-failed"),
            "{url}"
        );
    }
}

#[wasm_bindgen_test]
#[ignore = "an HTTP error status needs a server: the browser run's (cargo xtask test-browser)"]
async fn an_http_error_status_fails_with_download_failed() {
    // wasm-bindgen-test-runner's server answers 404 for a path it does not serve.
    let failed = WebFetcher
        .fetch("/sidevoice-engine-no-such-file")
        .await
        .err();
    assert_eq!(failed.map(|error| error.code), Some("download-failed"));
}

#[wasm_bindgen_test]
async fn dropping_a_download_part_way_stops_it() {
    let download = WebFetcher
        .fetch("data:application/octet-stream;base64,aGVsbG8sIHdvcmxk")
        .await
        .expect("fetched");
    // Dropped unread: the stream is cancelled and the request aborted, without a panic or an unhandled rejection.
    download.size();
    drop(download);
}
