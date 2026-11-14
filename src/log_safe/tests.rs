#[cfg(web)]
use wasm_bindgen_test::wasm_bindgen_test as test;

#[test]
fn only_a_known_host_is_shown_as_a_host() {
    use super::Origin;
    for (url, address) in [
        ("https://huggingface.co/a/b", "huggingface.co"),
        ("https://cdn-lfs.huggingface.co/a", "cdn-lfs.huggingface.co"),
        (
            "https://objects.githubusercontent.com/a",
            "objects.githubusercontent.com",
        ),
        ("https://ruben-nas.tailnet.ts.net/a", "public"),
        ("https://alice.duckdns.org/a", "public"),
        ("https://fileserver.corp.acme.com/a", "public"),
        ("https://huggingface.co.evil.test/a", "public"),
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
    assert_eq!(
        id("whisper-small/sherpa-onnx-int8"),
        "whisper-small/sherpa-onnx-int8"
    );
    assert_eq!(
        id("parakeet-tdt-0.6b-v3/sherpa-onnx-int8"),
        "parakeet-tdt-0.6b-v3/sherpa-onnx-int8"
    );
    for bad in [
        "",
        "/",
        "a//b",
        "a/",
        "/a",
        &format!("{0}/{0}/{0}", "a".repeat(50)),
        "Upper",
        "a b",
        "x\nforged line",
        &"a".repeat(65),
    ] {
        assert_eq!(id(bad), "invalid");
    }
}
