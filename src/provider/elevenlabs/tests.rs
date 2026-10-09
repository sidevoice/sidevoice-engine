//! ElevenLabs' adapter against a fake provider: what its listings keep (models, the account's voices and their
//! languages), Scribe's form, what speech sends and what comes back; no key, no network.

use std::sync::Arc;

use super::ElevenLabs;
use crate::backend::BackendModel;
use crate::provider::{Adapter, ProviderModel};
use crate::test_support::{block_on, contains, header, remote_api, FakeProvider};
use crate::{Capability, Gender, Voice};

#[cfg(web)]
use wasm_bindgen_test::wasm_bindgen_test as test;

/// `/v1/models`: two text-to-speech models the spec describes, one it does not, and one that does not speak.
const MODELS: &[u8] = br#"[
    {"model_id": "eleven_flash_v2_5", "can_do_text_to_speech": true,
     "languages": [{"language_id": "en", "name": "English"}, {"language_id": "es", "name": "Spanish"}]},
    {"model_id": "eleven_multilingual_v2", "can_do_text_to_speech": true, "languages": []},
    {"model_id": "eleven_turbo_v9", "can_do_text_to_speech": true, "languages": []},
    {"model_id": "eleven_v3", "can_do_text_to_speech": false, "languages": []}
]"#;

/// `/v1/voices`: a default voice, verified in two languages, and a cloned one with a language label.
const VOICES: &[u8] = br#"{"voices": [
    {"voice_id": "21m00Tcm4TlvDq8ikWAM", "name": "Rachel", "category": "premade",
     "labels": {"accent": "american", "gender": "female"},
     "verified_languages": [{"language": "en", "model_id": "eleven_flash_v2_5", "locale": "en-US"},
                            {"language": "es", "model_id": "eleven_flash_v2_5"}]},
    {"voice_id": "EXAVITQu4vr4xnSDxMaL", "name": "Lucia", "category": "cloned",
     "labels": {"language": "es", "gender": "non-binary"}, "verified_languages": []}
]}"#;

fn voice(id: &str) -> Voice {
    Voice {
        id: id.to_owned(),
        name: None,
        languages: Vec::new(),
        gender: None,
    }
}

fn model(id: &str, capability: Capability, speed: Option<[f32; 2]>) -> ProviderModel {
    ProviderModel {
        id: id.to_owned(),
        capabilities: vec![capability],
        languages: Vec::new(),
        voices: vec![voice("21m00Tcm4TlvDq8ikWAM"), voice("EXAVITQu4vr4xnSDxMaL")],
        speed,
    }
}

fn open(model: &ProviderModel) -> (Box<dyn BackendModel>, Arc<FakeProvider>) {
    let (api, provider) = remote_api("elevenlabs");
    provider.key("elevenlabs", "xi-test");
    (ElevenLabs.open(api, model).expect("opened"), provider)
}

#[test]
fn the_listing_keeps_the_spec_s_models_the_key_can_use_with_their_languages() {
    let (api, provider) = remote_api("elevenlabs");
    provider.key("elevenlabs", "xi-test");
    provider.answer("https://api.elevenlabs.io/v1/models", 200, MODELS);
    let models = block_on(ElevenLabs.models(&api)).expect("listed");
    let listed: Vec<_> = models
        .iter()
        .map(|model| {
            (
                model.id.as_str(),
                model.capabilities[0],
                model.languages.clone(),
            )
        })
        .collect();
    assert_eq!(
        listed,
        [
            ("scribe_v2", Capability::Stt, vec![]),
            ("scribe_v2_medical", Capability::Stt, vec![]),
            (
                "eleven_flash_v2_5",
                Capability::Tts,
                vec!["en".to_owned(), "es".to_owned()]
            ),
            ("eleven_multilingual_v2", Capability::Tts, vec![]),
        ],
        "Scribe from the spec; text to speech where the key's listing says it speaks and the spec describes it"
    );
    let request = &provider.requests()[0];
    assert_eq!(
        (request.method, header(request, "xi-api-key")),
        ("GET", Some("xi-test"))
    );
}

#[test]
fn the_voices_are_the_accounts_with_their_own_language_first() {
    let (api, provider) = remote_api("elevenlabs");
    provider.key("elevenlabs", "xi-test");
    provider.answer("https://api.elevenlabs.io/v1/voices", 200, VOICES);
    let voices = block_on(ElevenLabs.voices(&api)).expect("listed");
    assert_eq!(
        voices,
        [
            Voice {
                id: "21m00Tcm4TlvDq8ikWAM".into(),
                name: Some("Rachel".into()),
                languages: vec!["en-US".into(), "es".into()],
                gender: Some(Gender::Female),
            },
            Voice {
                id: "EXAVITQu4vr4xnSDxMaL".into(),
                name: Some("Lucia".into()),
                languages: vec!["es".into()],
                gender: None,
            },
        ]
    );
    let (api, provider) = remote_api("elevenlabs");
    provider.key("elevenlabs", "xi-test");
    provider.answer("https://api.elevenlabs.io/v1/voices", 200, b"not json");
    let listed = block_on(ElevenLabs.voices(&api)).map_err(|e| e.code);
    assert_eq!(listed, Err("listing-failed"));
}

#[test]
fn scribe_gets_the_turn_as_a_wav_with_the_model_and_the_language_code() {
    let (mut model, provider) = open(&model("scribe_v2", Capability::Stt, None));
    provider.answer(
        "https://api.elevenlabs.io/v1/speech-to-text",
        200,
        br#"{"language_code": "spa", "text": "Hoy hace sol."}"#,
    );
    let stt = model.as_stt().expect("speech to text");
    let text = block_on(stt.transcribe(&[0.0; 1_600], Some("es-ES")));
    assert_eq!(text.as_deref(), Ok("Hoy hace sol."));
    let request = &provider.requests()[0];
    assert_eq!(request.method, "POST");
    assert_eq!(header(request, "xi-api-key"), Some("xi-test"));
    for part in [
        "name=\"model_id\"\r\n\r\nscribe_v2\r\n",
        "name=\"language_code\"\r\n\r\nes\r\n",
        "name=\"tag_audio_events\"\r\n\r\nfalse\r\n",
        "Content-Type: audio/wav\r\n\r\nRIFF",
    ] {
        assert!(contains(&request.body, part), "{part}");
    }
}

#[test]
fn speech_comes_as_pcm_at_24_khz_with_the_language_and_a_speed_in_range() {
    let flash = model("eleven_flash_v2_5", Capability::Tts, Some([0.7, 1.2]));
    let (mut model, provider) = open(&flash);
    provider.answer(
        "https://api.elevenlabs.io/v1/text-to-speech/",
        200,
        &[0x00, 0x40, 0x00, 0xC0],
    );
    let tts = model.as_tts().expect("text to speech");
    assert_eq!(
        tts.voices(),
        ["21m00Tcm4TlvDq8ikWAM", "EXAVITQu4vr4xnSDxMaL"]
    );
    assert_eq!(tts.sample_rate(), 24_000);
    let lucia = voice("EXAVITQu4vr4xnSDxMaL");
    assert_eq!(
        block_on(tts.speak("Hola", &lucia, Some("es-MX"), 1.1)),
        Ok(vec![0.5, -0.5])
    );
    block_on(tts.speak("Hola", &lucia, None, 3.0)).expect("spoken");
    let requests = provider.requests();
    let speech = &requests[0];
    assert_eq!(
        speech.url,
        "https://api.elevenlabs.io/v1/text-to-speech/EXAVITQu4vr4xnSDxMaL?output_format=pcm_24000"
    );
    assert_eq!(header(speech, "xi-api-key"), Some("xi-test"));
    let body: serde_json::Value = serde_json::from_slice(&speech.body).expect("JSON");
    assert_eq!(body["text"], "Hola");
    assert_eq!(body["model_id"], "eleven_flash_v2_5");
    assert_eq!(body["language_code"], "es");
    let speed =
        |body: &serde_json::Value| body["voice_settings"]["speed"].as_f64().unwrap_or_default();
    assert!((speed(&body) - 1.1).abs() < 1e-6);
    let fast: serde_json::Value = serde_json::from_slice(&requests[1].body).expect("JSON");
    assert!(
        (speed(&fast) - 1.2).abs() < 1e-6,
        "kept within the spec's range"
    );

    assert_eq!(
        block_on(tts.speak("Hola", &voice("nobody"), None, 1.0)).map_err(|e| e.code),
        Err("unknown-voice")
    );
}

#[test]
fn a_model_whose_request_takes_no_language_or_no_speed_is_sent_neither() {
    for (id, speed) in [
        ("eleven_multilingual_v2", Some([0.7, 1.2])),
        ("eleven_v3", None),
    ] {
        let (mut model, provider) = open(&model(id, Capability::Tts, speed));
        provider.answer("https://api.elevenlabs.io/v1/text-to-speech/", 200, &[]);
        let tts = model.as_tts().expect("text to speech");
        let rachel = voice("21m00Tcm4TlvDq8ikWAM");
        let language = if id == "eleven_v3" { None } else { Some("en") };
        let speed = if id == "eleven_v3" { 1.1 } else { 1.0 };
        assert_eq!(
            block_on(tts.speak("Hi", &rachel, language, speed)),
            Ok(Vec::new())
        );
        let body: serde_json::Value =
            serde_json::from_slice(&provider.requests()[0].body).expect("JSON");
        assert_eq!(
            body,
            serde_json::json!({ "text": "Hi", "model_id": id }),
            "{id}"
        );
    }
}
