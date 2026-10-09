//! The engine's remote side over a host whose provider side is fake: the providers listed with their status, a
//! refresh, and a remote model made from the listing that transcribes through the same interface as a local one.

use crate::test_support::{block_on, MemoryHost};
use crate::Engine;

#[cfg(web)]
use wasm_bindgen_test::wasm_bindgen_test as test;

#[test]
fn providers_are_listed_beside_the_catalogue_and_a_remote_model_transcribes_like_a_local_one() {
    let host = MemoryHost::default();
    let remote = host.remote();
    let engine = Engine::new(Box::new(host), Vec::new()).expect("engine");

    let providers = block_on(engine.providers());
    let listed: Vec<_> = providers
        .iter()
        .map(|provider| (provider.id, provider.status.map(|reason| reason.code)))
        .collect();
    assert_eq!(
        listed,
        [
            ("elevenlabs", Some("credential-missing")),
            ("openai", Some("credential-missing"))
        ]
    );
    assert!(remote.requests().is_empty(), "no key, no call");
    let missing = block_on(engine.remote("openai", "gpt-4o-transcribe")).map(drop);
    assert_eq!(missing.map_err(|e| e.code), Err("credential-missing"));

    remote.key("openai", "sk-test");
    remote.answer(
        "https://api.openai.com/v1/models",
        200,
        br#"{"data": [{"id": "gpt-4o-transcribe"}]}"#,
    );
    remote.answer(
        "https://api.openai.com/v1/audio/transcriptions",
        200,
        br#"{"text": "hi"}"#,
    );
    let openai = block_on(engine.refresh("openai")).expect("refreshed");
    assert_eq!(openai.status, None);
    let ids: Vec<_> = openai
        .models
        .iter()
        .map(|model| model.id.as_str())
        .collect();
    assert_eq!(ids, ["gpt-4o-transcribe"]);

    let model = block_on(engine.remote("openai", "gpt-4o-transcribe")).expect("made");
    assert_eq!(
        (model.provider(), model.id()),
        ("openai", "gpt-4o-transcribe")
    );
    assert!(model.as_tts().is_none() && model.as_vad().is_none());
    let stt = model.as_stt().expect("speech to text");
    assert_eq!(
        block_on(stt.transcribe(&[0.0; 800], 8_000, Some("en"))).as_deref(),
        Ok("hi")
    );
    assert_eq!(remote.requests().len(), 2, "one listing, one transcription");

    let absent = block_on(engine.remote("openai", "whisper-1")).map(drop);
    assert_eq!(absent.map_err(|e| e.code), Err("model-not-found"));
    let unknown = block_on(engine.refresh("nobody")).map(drop);
    assert_eq!(unknown.map_err(|e| e.code), Err("provider-not-found"));
}
