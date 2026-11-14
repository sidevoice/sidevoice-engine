use super::UrlPath;

#[cfg(web)]
use wasm_bindgen_test::wasm_bindgen_test as test;

#[test]
fn a_url_shows_no_userinfo_query_or_fragment() {
    let shown = UrlPath("https://u:canary-pass@x.test:8443/a/b?canary-q=1#canary-f").to_string();
    assert_eq!(shown, "https://x.test:8443/a/b");
}

#[test]
fn what_does_not_parse_is_not_shown() {
    for raw in ["canary words 7f3a", "data:text/plain,canary", ""] {
        assert_eq!(UrlPath(raw).to_string(), "<unparsable url>");
    }
}
