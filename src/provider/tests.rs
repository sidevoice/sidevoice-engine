//! The providers of every build, and their facts as the engine reads them.

use super::built_in;
use crate::Capability;

#[cfg(web)]
use wasm_bindgen_test::wasm_bindgen_test as test;

#[test]
fn every_build_has_the_providers_and_each_its_spec_s_models() {
    let providers = built_in();
    let ids: Vec<_> = providers
        .iter()
        .map(|provider| provider.spec().id)
        .collect();
    assert_eq!(ids, ["elevenlabs", "openai"], "by id, in every build");
}

#[test]
fn the_facts_files_parse_and_describe_both_kinds_of_model() {
    for (provider, json) in [
        ("openai", include_str!("openai/facts.json")),
        ("elevenlabs", include_str!("elevenlabs/facts.json")),
    ] {
        let facts = super::facts::Facts::parse(json);
        let kinds: Vec<_> = facts.models().map(|(capability, _)| capability).collect();
        assert!(kinds.contains(&Capability::Stt), "{provider}");
        assert!(kinds.contains(&Capability::Tts), "{provider}");
        for (_, model) in facts.models() {
            if let Some([low, high]) = model.speed {
                assert!(low < 1.0 && 1.0 < high, "{provider} {}", model.model);
            }
        }
    }
}
