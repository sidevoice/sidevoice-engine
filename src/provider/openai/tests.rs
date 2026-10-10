//! OpenAI's adapter against a fake provider: what it reads from its spec, what its listing keeps, what each call sends
//! (URL, key, form or JSON), what each answer becomes, and how refusals read; no key, no network.

use serde_json::json;

use super::OpenAi;
use crate::backend::BackendModel;
use crate::provider::{Adapter, Facts, ProviderModel};
use crate::test_support::{block_on, contains, header, openai_spec, remote_api};
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

fn facts() -> Facts {
    OpenAi.facts(&openai_spec()).expect("facts")
}

#[test]
fn its_facts_come_from_its_requests_and_a_spec_without_them_is_unreadable() {
    let facts = facts();
    let ids = |models: &[crate::provider::facts::ModelFacts]| -> Vec<String> {
        models.iter().map(|model| model.model.clone()).collect()
    };
    assert_eq!(
        ids(&facts.speech_to_text),
        [
            "whisper-1",
            "gpt-4o-transcribe",
            "gpt-4o-transcribe-diarize"
        ]
    );
    assert_eq!(facts.transcription.chunking.as_deref(), Some("auto"));
    assert_eq!(facts.speech.chunking, None);
    assert_eq!(ids(&facts.text_to_speech), ["tts-1", "gpt-4o-mini-tts"]);
    assert_eq!(facts.voices, ["alloy", "ash", "fable", "nova"]);
    let mut no_pcm = openai_spec();
    no_pcm["components"]["schemas"]["CreateSpeechRequest"]["properties"]["response_format"]
        ["enum"] = json!(["mp3"]);
    let unreadable = OpenAi.facts(&no_pcm).map_err(|e| e.code);
    assert_eq!(unreadable, Err("provider-spec-unreadable"));
    assert_eq!(
        OpenAi.facts(&json!({})).map_err(|e| e.code),
        Err("provider-spec-unreadable")
    );
}

#[test]
fn what_each_kind_of_model_takes_comes_from_its_request() {
    let facts = facts();
    let stt = facts.of(Capability::Stt, "gpt-4o-transcribe");
    assert_eq!(
        (stt.language.as_deref(), stt.speed),
        (Some("language"), None)
    );
    let tts = facts.of(Capability::Tts, "gpt-4o-mini-tts");
    assert_eq!(
        (tts.language.as_deref(), tts.speed),
        (None, Some([0.25, 4.0]))
    );
    let newer = facts.of(Capability::Tts, "gpt-5-tts");
    assert_eq!(
        (newer.model.as_str(), newer.speed),
        ("gpt-5-tts", Some([0.25, 4.0])),
        "a model the spec does not name follows the general speech request"
    );
    assert!(facts.voices.iter().any(|voice| voice == "alloy"));
}

#[test]
fn the_listing_keeps_the_audio_models_the_spec_describes() {
    let (api, provider) = remote_api("openai");
    assert_eq!(
        block_on(OpenAi.models(&api, &facts())).map_err(|e| e.code),
        Err("credential-missing")
    );
    assert!(provider.requests().is_empty(), "no key, no call");

    provider.key("openai", "sk-test");
    provider.answer(
        "https://api.openai.com/v1/models",
        200,
        br#"{"object": "list", "data": [{"id": "gpt-4o-mini-tts"}, {"id": "gpt-4o"}, {"id": "gpt-4o-transcribe"}]}"#,
    );
    let models = block_on(OpenAi.models(&api, &facts())).expect("listed");
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

    let voices = block_on(OpenAi.voices(&api, &facts())).expect("the spec's");
    assert_eq!(voices.len(), facts().voices.len());
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
        let listed = block_on(OpenAi.models(&api, &facts())).map_err(|e| e.code);
        assert_eq!(listed, Err(expected), "{status}");
    }
    let (api, provider) = remote_api("openai");
    provider.key("openai", "sk-test");
    let listed = block_on(OpenAi.models(&api, &facts())).map_err(|e| e.code);
    assert_eq!(listed, Err("provider-unreachable"), "no answer");
}

fn open(
    model: &ProviderModel,
) -> (
    Box<dyn BackendModel>,
    std::sync::Arc<crate::test_support::FakeProvider>,
) {
    let (api, provider) = remote_api("openai");
    (OpenAi.open(api, model, &facts()).expect("opened"), provider)
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
fn a_model_of_no_kind_is_unsupported() {
    let (api, _) = remote_api("openai");
    let unknown = ProviderModel {
        id: "gpt-4o".into(),
        capabilities: Vec::new(),
        ..transcriber()
    };
    assert_eq!(
        OpenAi
            .open(api, &unknown, &facts())
            .map(drop)
            .unwrap_err()
            .code,
        "unsupported-model"
    );
}

/// ENG-07: a turn longer than 30 seconds asks the provider to cut it, as the spec says `gpt-4o-transcribe-diarize`
/// requires; a shorter one is sent as one block, as before.
#[test]
fn a_turn_longer_than_30_seconds_is_sent_with_the_specs_chunking_strategy() {
    let diarize = ProviderModel {
        id: "gpt-4o-transcribe-diarize".into(),
        ..transcriber()
    };
    let (mut model, provider) = open(&diarize);
    provider.key("openai", "sk-test");
    provider.answer(
        "https://api.openai.com/v1/audio/transcriptions",
        200,
        br#"{"text": "hi"}"#,
    );
    let stt = model.as_stt().expect("speech to text");
    block_on(stt.transcribe(&[0.0; 16_000 * 45], None)).expect("transcribed");
    block_on(stt.transcribe(&[0.0; 16_000 * 5], None)).expect("transcribed");
    let requests = provider.requests();
    let chunked = "name=\"chunking_strategy\"\r\n\r\nauto\r\n";
    assert!(
        contains(&requests[0].body, chunked),
        "45 s: cut by the provider"
    );
    assert!(!contains(&requests[1].body, chunked), "5 s: one block");
}
