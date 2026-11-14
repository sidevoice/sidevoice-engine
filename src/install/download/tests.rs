//! One file's download over a host with storage in memory: stored under its digest only when it is the file expected,
//! and nothing stored otherwise.

use std::sync::Mutex;

use super::download;
use crate::install::{Cancel, Progress};
use crate::test_support::{block_on, logged, sha256, MemoryHost};
use crate::Result;

#[cfg(web)]
use wasm_bindgen_test::wasm_bindgen_test as test;

const URL: &str = "https://models/model.onnx";
const MODEL: &[u8] = b"the model's weights";

fn fetch(host: &MemoryHost, digest: &str, cancel: &Cancel) -> (Result<()>, Vec<Progress>) {
    let reported = Mutex::new(Vec::new());
    let progress = |progress: Progress| reported.lock().unwrap().push(progress);
    let start = Progress {
        files: 1,
        done: 0,
        received: 0,
        size: None,
    };
    let result = block_on(download(URL, digest, host, &progress, cancel, start));
    (result, reported.into_inner().unwrap())
}

#[test]
fn a_file_is_stored_under_its_digest_with_its_progress_reported() {
    let host = MemoryHost::serving(&[(URL, MODEL)]);
    let (result, reported) = fetch(&host, &sha256(MODEL), &Cancel::new());
    assert_eq!(result, Ok(()));
    assert_eq!(host.stored()[&sha256(MODEL)], MODEL);
    let last = reported.last().expect("reported");
    assert_eq!(
        (last.received, last.size),
        (MODEL.len() as u64, Some(MODEL.len() as u64))
    );
}

#[test]
fn a_file_whose_bytes_do_not_match_its_digest_is_not_stored() {
    let host = MemoryHost::serving(&[(URL, MODEL)]);
    let (result, _) = fetch(&host, &sha256(b"something else"), &Cancel::new());
    assert_eq!(result.expect_err("mismatch").code, "digest-mismatch");
    assert!(host.stored().is_empty());
}

#[test]
fn a_failed_download_fails_with_the_fetchers_code() {
    let host = MemoryHost::default();
    let (result, _) = fetch(&host, &sha256(MODEL), &Cancel::new());
    assert_eq!(result.expect_err("not served").code, "download-failed");
    assert!(host.stored().is_empty());
}

#[test]
fn cancelling_in_the_middle_of_a_file_stores_nothing_of_it() {
    let host = MemoryHost::serving(&[(URL, MODEL)]);
    let cancel = Cancel::new();
    let progress = |progress: Progress| {
        if progress.received > 0 {
            cancel.cancel();
        }
    };
    let start = Progress {
        files: 1,
        done: 0,
        received: 0,
        size: None,
    };
    let result = block_on(download(
        URL,
        &sha256(MODEL),
        &host,
        &progress,
        &cancel,
        start,
    ));
    assert_eq!(result.expect_err("cancelled").code, "cancelled");
    assert!(host.stored().is_empty());
}

/// The first bytes of `MODEL`, as an interrupted download left them.
const KEPT: usize = 7;

#[test]
fn a_download_goes_on_after_the_bytes_an_earlier_attempt_kept() {
    let host = MemoryHost::serving(&[(URL, MODEL)]).with_ranges();
    host.keep_partial(&sha256(MODEL), &MODEL[..KEPT]);
    let (result, reported) = fetch(&host, &sha256(MODEL), &Cancel::new());
    assert_eq!(result, Ok(()));
    assert_eq!(host.offsets(), [KEPT as u64]);
    assert_eq!(host.stored()[&sha256(MODEL)], MODEL);
    assert_eq!(host.partial(&sha256(MODEL)), None);
    let last = reported.last().expect("reported");
    assert_eq!(
        (last.received, last.size),
        (MODEL.len() as u64, Some(MODEL.len() as u64))
    );
}

#[test]
fn a_server_without_ranges_sends_the_whole_file_and_the_kept_bytes_go() {
    let host = MemoryHost::serving(&[(URL, MODEL)]);
    host.keep_partial(&sha256(MODEL), &MODEL[..KEPT]);
    let (result, reported) = fetch(&host, &sha256(MODEL), &Cancel::new());
    assert_eq!(result, Ok(()));
    assert!(host.offsets().is_empty());
    assert_eq!(host.stored()[&sha256(MODEL)], MODEL);
    assert_eq!(host.partial(&sha256(MODEL)), None);
    let last = reported.last().expect("reported");
    assert_eq!(last.received, MODEL.len() as u64);
}

#[test]
fn kept_bytes_that_are_wrong_start_the_download_over() {
    let host = MemoryHost::serving(&[(URL, MODEL)]).with_ranges();
    host.keep_partial(&sha256(MODEL), &[b'x'; KEPT]);
    let (result, _) = fetch(&host, &sha256(MODEL), &Cancel::new());
    assert_eq!(result, Ok(()));
    // Once from the kept bytes, once from the start.
    assert_eq!(host.fetches(), 2);
    assert_eq!(host.stored()[&sha256(MODEL)], MODEL);
    assert_eq!(host.partial(&sha256(MODEL)), None);
}

#[test]
fn a_file_that_is_wrong_from_the_start_is_downloaded_once_and_leaves_nothing_kept() {
    let host = MemoryHost::serving(&[(URL, MODEL)]).with_ranges();
    let (result, _) = fetch(&host, &sha256(b"something else"), &Cancel::new());
    assert_eq!(result.expect_err("mismatch").code, "digest-mismatch");
    assert_eq!(host.fetches(), 1);
    assert_eq!(host.partial(&sha256(b"something else")), None);
}

#[test]
fn a_cancelled_download_leaves_nothing_kept_for_the_next_one() {
    let host = MemoryHost::serving(&[(URL, MODEL)]).with_ranges();
    host.keep_partial(&sha256(MODEL), &MODEL[..KEPT]);
    let cancel = Cancel::new();
    let progress = |progress: Progress| {
        if progress.received > KEPT as u64 {
            cancel.cancel();
        }
    };
    let start = Progress {
        files: 1,
        done: 0,
        received: 0,
        size: None,
    };
    let result = block_on(download(
        URL,
        &sha256(MODEL),
        &host,
        &progress,
        &cancel,
        start,
    ));
    assert_eq!(result.expect_err("cancelled").code, "cancelled");
    assert_eq!(host.partial(&sha256(MODEL)), None);
    assert!(host.stored().is_empty());
}

/// A download's log line names the file by scheme, host and port, never its path: the URL's userinfo and query (a
/// signed link's token) never reach it either (the Sidevoice logging guideline, sidevoice/.github-private docs/logging.md).
#[test]
fn a_download_logs_its_url_without_userinfo_or_query() {
    let url = "https://u:canary-pass@huggingface.co:8443/model.onnx?canary-q=1#canary-f";
    let (result, log) = logged_download(url);
    assert_eq!(result.expect_err("mismatch").code, "digest-mismatch");
    for expected in [
        "download",
        "url.scheme=\"https\"",
        "server.address=\"huggingface.co\"",
        "server.port=8443",
        "download started",
    ] {
        assert!(log.contains(expected), "{expected} in {log}");
    }
    for canary in ["canary-pass", "canary-q", "canary-f", "model.onnx"] {
        assert!(!log.contains(canary), "{canary} in {log}");
    }
}

/// A model URL of the user's own (a LAN server, a private path) shows its host class, never the address or the path.
#[test]
fn a_download_from_a_lan_address_logs_only_its_class() {
    let url = "http://192.168.7.23:9000/alice-private/secret-ft/model.onnx";
    let (_, log) = logged_download(url);
    for expected in [
        "url.scheme=\"http\"",
        "server.address=\"private\"",
        "server.port=9000",
    ] {
        assert!(log.contains(expected), "{expected} in {log}");
    }
    for canary in ["192.168", "alice-private", "secret-ft"] {
        assert!(!log.contains(canary), "{canary} in {log}");
    }
}

fn logged_download(url: &str) -> (Result<()>, String) {
    let host = MemoryHost::serving(&[(url, MODEL)]);
    let progress = |_: Progress| {};
    let start = Progress {
        files: 1,
        done: 0,
        received: 0,
        size: None,
    };
    logged(|| {
        block_on(download(
            url,
            &sha256(b"something else"),
            &host,
            &progress,
            &Cancel::new(),
            start,
        ))
    })
}
