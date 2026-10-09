//! ElevenLabs' backend against a fake provider: Scribe's form, the voices listed when a speaker loads, what speech
//! sends and what comes back; no key, no network.

use std::sync::Arc;

use super::{Api, SPEC};
use crate::backend::{BackendModel, Library, Load};
use crate::catalog::{BuildEntry, Capability, ModelEntry, Voice};
use crate::host::{Accelerator, Host, HttpRequest};
use crate::install::Installed;
use crate::test_support::{block_on, build, model, FakeProvider, MemoryHost};
use crate::Result;

#[cfg(web)]
use wasm_bindgen_test::wasm_bindgen_test as test;

const VOICES: &[u8] = br#"{"voices": [{"voice_id": "21m00Tcm4TlvDq8ikWAM", "name": "Rachel"},
    {"voice_id": "EXAVITQu4vr4xnSDxMaL", "name": "Sarah"}]}"#;

/// A remote build of ElevenLabs' `api_model`, mapping the language to `language_code` when `language` says so.
fn remote(api_model: &str, language: bool) -> BuildEntry {
    let mut build = build(&format!("{api_model}/elevenlabs"), "elevenlabs", 0);
    build.files.clear();
    build.api_model = Some(api_model.to_owned());
    if language {
        build
            .call_params
            .insert("language".into(), vec!["language_code".into()]);
    }
    build
}

/// A host with the key, whose provider side `prepare` sets up before `model` loads; the loaded model and that side.
fn load(
    model: &ModelEntry,
    prepare: impl FnOnce(&FakeProvider),
) -> (Result<Box<dyn BackendModel>>, Arc<FakeProvider>) {
    let host = MemoryHost::default();
    let provider = host.remote();
    prepare(&provider);
    let host: Arc<dyn Host> = Arc::new(host);
    let files = Installed::default();
    let load = Load {
        model,
        build: &model.builds[0],
        accelerator: Accelerator::Remote,
        files: &files,
        host: &host,
    };
    (block_on(Api.load(load)), provider)
}

fn header<'a>(request: &'a HttpRequest, name: &str) -> Option<&'a str> {
    let found = request
        .headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name));
    found.map(|(_, value)| value.as_str())
}

fn contains(body: &[u8], part: &str) -> bool {
    body.windows(part.len())
        .any(|window| window == part.as_bytes())
}

fn voice(id: &str) -> Voice {
    Voice {
        id: id.to_owned(),
        languages: Vec::new(),
        gender: None,
    }
}

#[test]
fn it_is_remote_and_calls_with_the_hosts_elevenlabs_key() {
    assert_eq!(SPEC.provider, Some("elevenlabs"));
    assert_eq!(SPEC.accelerators, [Accelerator::Remote]);
}

#[test]
fn scribe_gets_the_turn_as_a_wav_with_the_model_and_the_language_code() {
    let model = model(
        "scribe_v2",
        Capability::Stt,
        vec![remote("scribe_v2", true)],
    );
    let (loaded, provider) = load(&model, |provider| {
        provider.key("elevenlabs", "xi-test");
        provider.answer(
            "https://api.elevenlabs.io/v1/speech-to-text",
            200,
            br#"{"language_code": "spa", "text": "Hoy hace sol."}"#,
        );
    });
    let mut loaded = loaded.expect("loaded");
    assert!(
        provider.requests().is_empty(),
        "loading Scribe makes no call"
    );
    let stt = loaded.as_stt().expect("speech to text");
    let text = block_on(stt.transcribe(&[0.0; 1_600], Some("es-ES")));
    assert_eq!(text.as_deref(), Ok("Hoy hace sol."));
    let request = &provider.requests()[0];
    assert_eq!(request.method, "POST");
    assert_eq!(header(request, "xi-api-key"), Some("xi-test"));
    assert!(contains(
        &request.body,
        "name=\"model_id\"\r\n\r\nscribe_v2\r\n"
    ));
    assert!(contains(
        &request.body,
        "name=\"language_code\"\r\n\r\nes\r\n"
    ));
    assert!(contains(
        &request.body,
        "name=\"tag_audio_events\"\r\n\r\nfalse\r\n"
    ));
    assert!(contains(
        &request.body,
        "Content-Type: audio/wav\r\n\r\nRIFF"
    ));
}

#[test]
fn a_speaker_loads_the_accounts_voices_and_speech_comes_as_pcm_at_24_khz() {
    let model = model(
        "eleven_flash_v2_5",
        Capability::Tts,
        vec![remote("eleven_flash_v2_5", true)],
    );
    let (loaded, provider) = load(&model, |provider| {
        provider.key("elevenlabs", "xi-test");
        provider.answer("https://api.elevenlabs.io/v1/voices", 200, VOICES);
        provider.answer(
            "https://api.elevenlabs.io/v1/text-to-speech/",
            200,
            &[0x00, 0x40, 0x00, 0xC0],
        );
    });
    let mut loaded = loaded.expect("loaded");
    let tts = loaded.as_tts().expect("text to speech");
    assert_eq!(
        tts.voices(),
        ["21m00Tcm4TlvDq8ikWAM", "EXAVITQu4vr4xnSDxMaL"]
    );
    assert_eq!(tts.sample_rate(), 24_000);
    let sarah = voice("EXAVITQu4vr4xnSDxMaL");
    assert_eq!(
        block_on(tts.speak("Hola", &sarah, Some("es-MX"), 1.1)),
        Ok(vec![0.5, -0.5])
    );
    let requests = provider.requests();
    assert_eq!(requests[0].method, "GET");
    let speech = &requests[1];
    assert_eq!(
        speech.url,
        "https://api.elevenlabs.io/v1/text-to-speech/EXAVITQu4vr4xnSDxMaL?output_format=pcm_24000"
    );
    assert_eq!(header(speech, "xi-api-key"), Some("xi-test"));
    let body: serde_json::Value = serde_json::from_slice(&speech.body).expect("JSON");
    assert_eq!(body["text"], "Hola");
    assert_eq!(body["model_id"], "eleven_flash_v2_5");
    assert_eq!(body["language_code"], "es");
    assert!((body["voice_settings"]["speed"].as_f64().unwrap_or_default() - 1.1).abs() < 1e-6);

    assert_eq!(
        block_on(tts.speak("Hola", &voice("nobody"), None, 1.0)).map_err(|e| e.code),
        Err("unknown-voice")
    );
}

#[test]
fn speech_at_normal_speed_in_a_model_that_takes_no_language_sends_neither() {
    let model = model(
        "eleven_multilingual_v2",
        Capability::Tts,
        vec![remote("eleven_multilingual_v2", false)],
    );
    let (loaded, provider) = load(&model, |provider| {
        provider.key("elevenlabs", "xi-test");
        provider.answer("https://api.elevenlabs.io/v1/voices", 200, VOICES);
        provider.answer("https://api.elevenlabs.io/v1/text-to-speech/", 200, &[]);
    });
    let mut loaded = loaded.expect("loaded");
    let tts = loaded.as_tts().expect("text to speech");
    let rachel = voice("21m00Tcm4TlvDq8ikWAM");
    assert_eq!(
        block_on(tts.speak("Hi", &rachel, Some("en"), 1.0)),
        Ok(Vec::new())
    );
    let body: serde_json::Value =
        serde_json::from_slice(&provider.requests()[1].body).expect("JSON");
    assert_eq!(
        body,
        serde_json::json!({ "text": "Hi", "model_id": "eleven_multilingual_v2" })
    );
}

#[test]
fn a_speaker_without_a_key_or_whose_voices_cannot_be_read_does_not_load() {
    let model = model(
        "eleven_flash_v2_5",
        Capability::Tts,
        vec![remote("eleven_flash_v2_5", true)],
    );
    let code = |prepare: &dyn Fn(&FakeProvider)| {
        load(&model, |provider| prepare(provider))
            .0
            .map(drop)
            .unwrap_err()
            .code
    };
    assert_eq!(code(&|_| {}), "credential-missing");
    assert_eq!(
        code(&|provider| {
            provider.key("elevenlabs", "xi-bad");
            provider.answer("https://api.elevenlabs.io/", 401, b"{}");
        }),
        "credential-rejected"
    );
    assert_eq!(
        code(&|provider| {
            provider.key("elevenlabs", "xi-test");
            provider.answer("https://api.elevenlabs.io/v1/voices", 200, b"not json");
        }),
        "model-load-failed"
    );
}
