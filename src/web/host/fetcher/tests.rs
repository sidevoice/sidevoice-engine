use super::WebFetcher;
use crate::{Download, Fetcher, HttpClient, HttpRequest};
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

fn get(url: &str) -> HttpRequest {
    HttpRequest {
        method: "GET",
        url: url.to_owned(),
        headers: vec![("accept".into(), "*/*".into())],
        body: Vec::new(),
    }
}

#[wasm_bindgen_test]
async fn an_api_call_resolves_with_its_status_and_whole_body() {
    let response = WebFetcher
        .send(get("data:application/json;base64,eyJ0ZXh0IjoiaG9sYSJ9"))
        .await
        .expect("answered");
    assert_eq!(response.status, 200);
    assert_eq!(response.body, br#"{"text":"hola"}"#);
}

#[wasm_bindgen_test]
async fn an_api_call_with_no_answer_fails_with_request_failed() {
    for url in ["http://127.0.0.1:9/nothing", "not a url"] {
        let failed = WebFetcher.send(get(url)).await.err();
        assert_eq!(
            failed.map(|error| error.code),
            Some("request-failed"),
            "{url}"
        );
    }
}

#[wasm_bindgen_test]
#[ignore = "an HTTP error status needs a server: the browser run's (cargo xtask test-browser)"]
async fn an_http_error_status_is_an_answer_for_an_api_call() {
    let response = WebFetcher
        .send(get("/sidevoice-engine-no-such-file"))
        .await
        .expect("answered");
    assert_eq!(response.status, 404);
}
