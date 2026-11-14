use super::{HttpRequest, HttpResponse};

#[cfg(web)]
use wasm_bindgen_test::wasm_bindgen_test as test;

#[test]
fn a_request_shows_no_query_header_value_or_body() {
    let request = HttpRequest {
        method: "POST",
        url: "https://u:canary-pass@x.test/v1/a?key=canary-secret-7f3a".into(),
        headers: vec![("Authorization".into(), "Bearer canary-secret-7f3a".into())],
        body: b"canary words 7f3a".to_vec(),
    };
    let shown = format!("{request:?}");
    assert!(shown.contains("https://x.test/v1/a"), "{shown}");
    for canary in ["canary-secret", "canary-pass", "canary words", "key="] {
        assert!(!shown.contains(canary), "{shown}");
    }
}

#[test]
fn a_response_shows_its_size_and_not_its_body() {
    let response = HttpResponse {
        status: 200,
        body: b"canary words 7f3a".to_vec(),
    };
    let shown = format!("{response:?}");
    assert!(shown.contains("<17 bytes>"), "{shown}");
    assert!(!shown.contains("canary"), "{shown}");
}
