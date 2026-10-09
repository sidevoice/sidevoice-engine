//! OpenAI's adapter against a fake provider: what its listing keeps, what each call sends (URL, key, form or JSON),
//! what each answer becomes, and how refusals read; no key, no network.

use super::{OpenAi, FACTS};
use crate::backend::BackendModel;
use crate::provider::{Adapter, ProviderModel};
use crate::test_support::{block_on, contains, header, remote_api};
use crate::{Capability, Voice};

#[cfg(web)]
use wasm_bindgen_test::wasm_bindgen_test as test;

fn transcriber() -> ProviderModel {
    ProviderModel {
        id: "gpt-4o-transcribe".into(),
        capabilities: vec![Capability::Stt],
        languages: Vec::new(),
        voices: Vec::new(),
        speed: None,
    }
}

fn voice(id: &str) -> Voice {
    Voice {
        id: id.into(),
        name: None,
        languages: Vec::new(),
        gender: None,
    }
}

fn speaker() -> ProviderModel {
    ProviderModel {
        id: "gpt-4o-mini-tts".into(),
        capabilities: vec![Capability::Tts],
        languages: Vec::new(),
        voices: vec![voice("alloy"), voice("nova")],
        speed: Some([0.25, 4.0]),
    }
}

#[test]
fn its_facts_say_which_models_transcribe_and_which_speak() {
    let (capability, facts) = FACTS.model("gpt-4o-transcribe").expect("described");
    assert_eq!(capability, Capability::Stt);
    assert_eq!(facts.language.as_deref(), Some("language"));
    let (capability, facts) = FACTS.model("gpt-4o-mini-tts").expect("described");
    assert_eq!(capability, Capability::Tts);
    assert_eq!(
        (facts.language.as_deref(), facts.speed),
        (None, Some([0.25, 4.0]))
    );
    assert!(FACTS.voices.iter().any(|voice| voice == "alloy"));
}

#[test]
fn the_listing_keeps_the_audio_models_the_spec_describes() {
    let (api, provider) = remote_api("openai");
    assert_eq!(
        block_on(OpenAi.models(&api)).map_err(|e| e.code),
        Err("credential-missing")
    );
    assert!(provider.requests().is_empty(), "no key, no call");

    provider.key("openai", "sk-test");
    provider.answer(
        "https://api.openai.com/v1/models",
        200,
        br#"{"object": "list", "data": [{"id": "gpt-4o-mini-tts"}, {"id": "gpt-4o"}, {"id": "gpt-4o-transcribe"}]}"#,
    );
    let models = block_on(OpenAi.models(&api)).expect("listed");
    let listed: Vec<_> = models
        .iter()
        .map(|model| (model.id.as_str(), model.capabilities.clone(), model.speed))
        .collect();
    assert_eq!(
        listed,
        [
            ("gpt-4o-transcribe", vec![Capability::Stt], None),
            ("gpt-4o-mini-tts", vec![Capability::Tts], Some([0.25, 4.0])),
        ],
        "in the spec's order, without the models that are not audio"
    );
    let request = &provider.requests()[0];
    assert_eq!(
        (request.method, header(request, "authorization")),
        ("GET", Some("Bearer sk-test"))
    );

    let voices = block_on(OpenAi.voices(&api)).expect("the spec's");
    assert_eq!(voices.len(), FACTS.voices.len());
    assert_eq!(provider.requests().len(), 1, "voices make no call");
}

#[test]
fn a_listing_refused_reads_as_the_providers_status() {
    for (status, expected) in [
        (401, "credential-rejected"),
        (403, "listing-not-permitted"),
        (429, "provider-quota"),
        (503, "provider-unreachable"),
        (404, "listing-failed"),
    ] {
        let (api, provider) = remote_api("openai");
        provider.key("openai", "sk-test");
        provider.answer("https://api.openai.com/", status, b"{}");
        let listed = block_on(OpenAi.models(&api)).map_err(|e| e.code);
        assert_eq!(listed, Err(expected), "{status}");
    }
    let (api, provider) = remote_api("openai");
    provider.key("openai", "sk-test");
    let listed = block_on(OpenAi.models(&api)).map_err(|e| e.code);
    assert_eq!(listed, Err("provider-unreachable"), "no answer");
}

fn open(
    model: &ProviderModel,
) -> (
    Box<dyn BackendModel>,
    std::sync::Arc<crate::test_support::FakeProvider>,
) {
    let (api, provider) = remote_api("openai");
    (OpenAi.open(api, model).expect("opened"), provider)
}

#[test]
fn a_turn_is_sent_as_a_wav_in_a_form_with_the_model_and_the_language() {
    let (mut model, provider) = open(&transcriber());
    provider.key("openai", "sk-test");
    provider.answer(
        "https://api.openai.com/v1/audio/transcriptions",
        200,
        r#"{"text": " Hola, ¿qué tal? "}"#.as_bytes(),
    );
    let stt = model.as_stt().expect("speech to text");
    let text = block_on(stt.transcribe(&[0.0; 1_600], Some("es-ES")));
    assert_eq!(text.as_deref(), Ok("Hola, ¿qué tal?"));

    let requests = provider.requests();
    let request = &requests[0];
    assert_eq!(request.method, "POST");
    assert_eq!(header(request, "authorization"), Some("Bearer sk-test"));
    let content_type = header(request, "content-type").unwrap_or_default();
    assert!(content_type.starts_with("multipart/form-data; boundary="));
    let body = &request.body;
    assert!(contains(
        body,
        "name=\"model\"\r\n\r\ngpt-4o-transcribe\r\n"
    ));
    assert!(contains(body, "name=\"language\"\r\n\r\nes\r\n"));
    assert!(contains(body, "name=\"response_format\"\r\n\r\njson\r\n"));
    assert!(contains(
        body,
        "filename=\"turn.wav\"\r\nContent-Type: audio/wav\r\n\r\nRIFF"
    ));
}

#[test]
fn without_a_key_nothing_is_sent_and_refusals_have_their_codes() {
    let (mut model, provider) = open(&transcriber());
    let stt = model.as_stt().expect("speech to text");
    let code = |stt: &mut dyn crate::backend::SttModel| {
        block_on(stt.transcribe(&[0.0; 160], None))
            .unwrap_err()
            .code
    };
    assert_eq!(code(stt), "credential-missing");
    assert!(provider.requests().is_empty());

    provider.key("openai", "sk-test");
    assert_eq!(code(stt), "request-failed", "no answer");
    for (status, expected) in [
        (401, "credential-rejected"),
        (402, "provider-quota"),
        (429, "rate-limited"),
        (500, "transcription-failed"),
    ] {
        let (mut model, provider) = open(&transcriber());
        provider.key("openai", "sk-test");
        provider.answer("https://api.openai.com/", status, b"{}");
        assert_eq!(code(model.as_stt().expect("stt")), expected, "{status}");
    }
}

#[test]
fn speech_is_asked_as_pcm_at_24_khz_with_a_listed_voice_and_a_speed_in_range() {
    let (mut model, provider) = open(&speaker());
    provider.key("openai", "sk-test");
    provider.answer(
        "https://api.openai.com/v1/audio/speech",
        200,
        &[0x00, 0x40, 0x00, 0xC0],
    );
    let tts = model.as_tts().expect("text to speech");
    assert_eq!(tts.voices(), ["alloy", "nova"]);
    assert_eq!(tts.sample_rate(), 24_000);
    let samples = block_on(tts.speak("Hola", &voice("nova"), Some("es"), 1.25));
    assert_eq!(samples, Ok(vec![0.5, -0.5]));
    block_on(tts.speak("Hola", &voice("nova"), None, 9.0)).expect("spoken");

    let requests = provider.requests();
    let body = |at: usize| -> serde_json::Value {
        serde_json::from_slice(&requests[at].body).expect("JSON")
    };
    assert_eq!(
        body(0),
        serde_json::json!({
            "model": "gpt-4o-mini-tts",
            "input": "Hola",
            "voice": "nova",
            "response_format": "pcm",
            "speed": 1.25,
        }),
        "no language: the spec gives its request none"
    );
    assert_eq!(body(1)["speed"], 4.0, "kept within the spec's range");
    let refused = block_on(tts.speak("Hola", &voice("ash"), None, 1.0));
    assert_eq!(refused.map_err(|e| e.code), Err("unknown-voice"));
}

#[test]
fn a_model_the_spec_does_not_describe_is_unsupported() {
    let (api, _) = remote_api("openai");
    let unknown = ProviderModel {
        id: "gpt-4o".into(),
        ..transcriber()
    };
    assert_eq!(
        OpenAi.open(api, &unknown).map(drop).unwrap_err().code,
        "unsupported-model"
    );
}
