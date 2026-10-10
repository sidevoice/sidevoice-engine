//! ElevenLabs' adapter against a fake provider: what it reads from its spec, what its listings keep (models, the
//! account's voices and their languages), Scribe's form, what speech sends and what comes back; no key, no network.

use std::sync::Arc;

use serde_json::{json, Value};

use super::ElevenLabs;
use crate::backend::BackendModel;
use crate::provider::{Adapter, Facts, RemoteModelInfo};
use crate::test_support::{block_on, contains, header, remote_api, FakeProvider};
use crate::{Capability, Gender, SpeedRange, Voice};

#[cfg(web)]
use wasm_bindgen_test::wasm_bindgen_test as test;

/// `/v1/models` for an account with more text-to-speech models than the spec has requests of its own for, Scribe (which
/// does not speak), and a model that is neither (voice changing).
const MODELS: &[u8] = br#"[
    {"model_id": "eleven_flash_v2_5", "can_do_text_to_speech": true,
     "languages": [{"language_id": "en", "name": "English"}, {"language_id": "es", "name": "Spanish"}]},
    {"model_id": "eleven_turbo_v2_5", "can_do_text_to_speech": true, "languages": [{"language_id": "en", "name": "English"}]},
    {"model_id": "eleven_multilingual_v2", "can_do_text_to_speech": true, "languages": []},
    {"model_id": "eleven_v3", "can_do_text_to_speech": true, "languages": []},
    {"model_id": "eleven_flash_v4", "can_do_text_to_speech": true, "languages": []},
    {"model_id": "eleven_v4", "can_do_text_to_speech": true, "languages": []},
    {"model_id": "scribe_v2", "can_do_text_to_speech": false, "languages": [{"language_id": "en", "name": "English"}]},
    {"model_id": "eleven_english_sts_v2", "can_do_text_to_speech": false, "languages": []}
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

/// The parts of ElevenLabs' spec its facts come from: three per-model requests, a request of another kind with a
/// `const` model, the speech-to-text body and the speech path's formats.
fn spec() -> Value {
    let speed = json!({ "anyOf": [{ "type": "number", "minimum": 0.7, "maximum": 1.2 }, { "type": "null" }] });
    json!({
        "paths": {
            "/v1/text-to-speech/{voice_id}": { "post": {
                "parameters": [
                    { "name": "output_format", "schema": { "type": "string", "enum": ["mp3_44100_128", "pcm_24000"] } },
                ],
                "requestBody": { "content": { "application/json": {
                    "schema": { "$ref": "#/components/schemas/General" },
                }}},
            }},
            "/v1/speech-to-text": { "post": { "requestBody": { "content": { "multipart/form-data": {
                "schema": { "$ref": "#/components/schemas/Body" },
            }}}}},
        },
        "components": { "schemas": {
            "ElevenFlashV2_5Request": { "properties": {
                "text": {}, "voice": {}, "model_id": { "const": "eleven_flash_v2_5" }, "language_code": {},
                "voice_settings": { "anyOf": [{ "$ref": "#/components/schemas/Settings" }, { "type": "null" }] },
            }},
            "ElevenV3Request": { "properties": {
                "text": {}, "voice": {}, "model_id": { "const": "eleven_v3" }, "language_code": {},
                "voice_settings": { "anyOf": [{ "$ref": "#/components/schemas/V3Settings" }, { "type": "null" }] },
            }},
            "ElevenMultilingualV2Request": { "properties": {
                "text": {}, "voice": {}, "model_id": { "const": "eleven_multilingual_v2" },
                "voice_settings": { "anyOf": [{ "$ref": "#/components/schemas/Settings" }, { "type": "null" }] },
            }},
            "MusicRequest": { "properties": { "prompt": {}, "model_id": { "const": "music_v1" } } },
            "V3Settings": { "properties": { "stability": {} } },
            "General": { "properties": {
                "text": {}, "model_id": { "type": "string" }, "language_code": {},
                "voice_settings": { "anyOf": [{ "$ref": "#/components/schemas/StoredSettings" }, { "type": "null" }] },
            }},
            "StoredSettings": { "properties": { "speed": { "anyOf": [{ "type": "number" }, { "type": "null" }] } } },
            "Settings": { "properties": { "speed": speed } },
            "Body": { "properties": {
                "model_id": { "type": "string", "examples": ["scribe_v2", "scribe_v2_medical"] },
                "language_code": { "anyOf": [{ "type": "string" }, { "type": "null" }] },
            }},
        }},
    })
}

fn facts() -> Facts {
    ElevenLabs.facts(&spec()).expect("facts")
}

#[test]
fn its_facts_come_from_its_per_model_requests_and_the_speech_to_text_body() {
    let facts = facts();
    let models: Vec<_> = facts
        .models()
        .map(|(capability, model)| {
            (
                capability,
                model.model.as_str(),
                model.language.as_deref(),
                model.speed,
            )
        })
        .collect();
    assert_eq!(
        models,
        [
            (Capability::Stt, "scribe_v2", Some("language_code"), None),
            (
                Capability::Stt,
                "scribe_v2_medical",
                Some("language_code"),
                None
            ),
            (
                Capability::Tts,
                "eleven_flash_v2_5",
                Some("language_code"),
                Some([0.7, 1.2])
            ),
            (
                Capability::Tts,
                "eleven_multilingual_v2",
                None,
                Some([0.7, 1.2])
            ),
            (Capability::Tts, "eleven_v3", Some("language_code"), None),
        ],
        "not the music model"
    );
    assert!(facts.voices.is_empty(), "the account's, listed live");
    let mut no_pcm = spec();
    no_pcm["paths"]["/v1/text-to-speech/{voice_id}"]["post"]["parameters"][0]["schema"]["enum"] =
        json!(["mp3_44100_128"]);
    assert_eq!(
        ElevenLabs.facts(&no_pcm).map_err(|e| e.code),
        Err("provider-spec-unreadable")
    );
}

fn voice(id: &str) -> Voice {
    Voice {
        id: id.to_owned(),
        name: None,
        languages: Vec::new(),
        gender: None,
    }
}

fn model(id: &str, capability: Capability, speed: Option<[f32; 2]>) -> RemoteModelInfo {
    RemoteModelInfo {
        id: id.to_owned(),
        capabilities: vec![capability],
        languages: Vec::new(),
        voices: vec![voice("21m00Tcm4TlvDq8ikWAM"), voice("EXAVITQu4vr4xnSDxMaL")],
        speed: speed.map(|[min, max]| SpeedRange { min, max }),
    }
}

fn open(model: &RemoteModelInfo) -> (Box<dyn BackendModel>, Arc<FakeProvider>) {
    let (api, provider) = remote_api("elevenlabs");
    provider.key("elevenlabs", "xi-test");
    (
        ElevenLabs.open(api, model, &facts()).expect("opened"),
        provider,
    )
}

#[test]
fn every_model_the_listing_says_speaks_is_offered_and_the_spec_only_enriches_it() {
    let (api, provider) = remote_api("elevenlabs");
    provider.key("elevenlabs", "xi-test");
    provider.answer("https://api.elevenlabs.io/v1/models", 200, MODELS);
    let models = block_on(ElevenLabs.models(&api, &facts())).expect("listed");
    let listed: Vec<_> = models
        .iter()
        .map(|model| {
            (
                model.id.as_str(),
                model.capabilities[0],
                model.languages.len(),
                model.speed.map(|speed| [speed.min, speed.max]),
            )
        })
        .collect();
    let speed = Some([0.7, 1.2]);
    assert_eq!(
        listed,
        [
            ("eleven_flash_v2_5", Capability::Tts, 2, speed),
            ("eleven_turbo_v2_5", Capability::Tts, 1, None),
            ("eleven_multilingual_v2", Capability::Tts, 0, speed),
            ("eleven_v3", Capability::Tts, 0, None),
            ("eleven_flash_v4", Capability::Tts, 0, None),
            ("eleven_v4", Capability::Tts, 0, None),
            ("scribe_v2", Capability::Stt, 1, None),
            ("scribe_v2_medical", Capability::Stt, 0, None),
        ],
        "every model the listing says speaks, in its order, more than the spec has requests for; then the spec's \
         speech-to-text models, with the languages the listing gives one it lists; a speed range only where the spec \
         gives one"
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
    let voices = block_on(ElevenLabs.voices(&api, &facts())).expect("listed");
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
    let listed = block_on(ElevenLabs.voices(&api, &facts())).map_err(|e| e.code);
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
fn a_model_the_spec_has_no_request_for_follows_the_general_one() {
    let (mut model, provider) = open(&model("eleven_flash_v4", Capability::Tts, None));
    provider.answer("https://api.elevenlabs.io/v1/text-to-speech/", 200, &[]);
    let tts = model.as_tts().expect("text to speech");
    block_on(tts.speak("Hola", &voice("21m00Tcm4TlvDq8ikWAM"), Some("es"), 1.1)).expect("spoken");
    let body: serde_json::Value =
        serde_json::from_slice(&provider.requests()[0].body).expect("JSON");
    assert_eq!(
        body,
        json!({ "text": "Hola", "model_id": "eleven_flash_v4", "language_code": "es" }),
        "the general request takes a language; its stored settings give no speed range, so no speed is sent"
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

/// What `/v1/models` answers for a real key: text-to-speech models only, no Scribe (remote-live, engine 0.3.0). The
/// listing never reports speech to text, so the spec's Scribe models are offered all the same.
#[test]
fn a_listing_without_scribe_still_offers_the_specs_speech_to_text() {
    let (api, provider) = remote_api("elevenlabs");
    provider.key("elevenlabs", "xi-test");
    provider.answer(
        "https://api.elevenlabs.io/v1/models",
        200,
        br#"[
            {"model_id": "eleven_flash_v2_5", "can_do_text_to_speech": true, "languages": [{"language_id": "en", "name": "English"}]},
            {"model_id": "eleven_multilingual_v2", "can_do_text_to_speech": true, "languages": []}
        ]"#,
    );
    let models = block_on(ElevenLabs.models(&api, &facts())).expect("listed");
    let of = |capability| -> Vec<&str> {
        models
            .iter()
            .filter(|model| model.capabilities.contains(&capability))
            .map(|model| model.id.as_str())
            .collect()
    };
    assert_eq!(
        of(Capability::Tts),
        ["eleven_flash_v2_5", "eleven_multilingual_v2"]
    );
    assert_eq!(
        of(Capability::Stt),
        ["scribe_v2", "scribe_v2_medical"],
        "from the spec: the listing reports no speech to text"
    );
}
