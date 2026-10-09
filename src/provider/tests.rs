//! The providers of every build, and where their specs are.

use super::built_in;

#[cfg(web)]
use wasm_bindgen_test::wasm_bindgen_test as test;

#[test]
fn every_build_has_the_providers_each_with_its_official_spec() {
    let providers = built_in();
    let ids: Vec<_> = providers
        .iter()
        .map(|provider| (provider.spec().id, provider.spec().spec))
        .collect();
    assert_eq!(
        ids,
        [
            ("elevenlabs", "https://api.elevenlabs.io/openapi.json"),
            (
                "openai",
                "https://raw.githubusercontent.com/openai/openai-openapi/main/openapi.json"
            )
        ],
        "by id, in every build"
    );
}
