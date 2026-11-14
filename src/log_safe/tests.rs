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

#[test]
fn only_a_public_dns_name_is_shown_as_a_host() {
    use super::Origin;
    for (url, address) in [
        ("https://huggingface.co/a/b", "huggingface.co"),
        ("http://192.168.1.5/a", "private"),
        ("http://10.0.0.1:81/a", "private"),
        ("http://127.0.0.1/a", "loopback"),
        ("http://[::1]/a", "loopback"),
        ("http://[fd00::1]/a", "private"),
        ("http://8.8.8.8/a", "public"),
        ("http://localhost:3000/a", "loopback"),
        ("http://nas.local/a", "private"),
        ("http://models/a", "private"),
        ("canary words", "<unparsable url>"),
    ] {
        assert_eq!(Origin::of(url).address, address, "{url}");
    }
}

#[test]
fn an_id_outside_the_grammar_is_invalid() {
    use super::id;
    assert_eq!(id("whisper-small.en_q5"), "whisper-small.en_q5");
    for bad in ["", "Upper", "a b", "x\nforged line", &"a".repeat(65)] {
        assert_eq!(id(bad), "invalid");
    }
}
