//! OpenAI's backend against a fake provider: what each call sends (URL, key, form or JSON), what each answer becomes,
//! and how refusals read; no key, no network.

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

/// A remote build of OpenAI's `api_model`, mapping the language to `language` when `language` says so.
fn remote(api_model: &str, language: bool) -> BuildEntry {
    let mut build = build(&format!("{api_model}/openai"), "openai", 0);
    build.files.clear();
    build.api_model = Some(api_model.to_owned());
    if language {
        build
            .call_params
            .insert("language".into(), vec!["language".into()]);
    }
    build
}

/// `model` loaded on a host whose provider side is returned too.
fn load(model: &ModelEntry) -> (Result<Box<dyn BackendModel>>, Arc<FakeProvider>) {
    let host = MemoryHost::default();
    let provider = host.remote();
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

fn transcriber() -> ModelEntry {
    model(
        "gpt-4o-transcribe",
        Capability::Stt,
        vec![remote("gpt-4o-transcribe", true)],
    )
}

fn speaker() -> ModelEntry {
    let mut speaker = model(
        "gpt-4o-mini-tts",
        Capability::Tts,
        vec![remote("gpt-4o-mini-tts", false)],
    );
    speaker.voices = ["alloy", "nova"]
        .map(|id| Voice {
            id: id.to_owned(),
            languages: Vec::new(),
            gender: None,
        })
        .to_vec();
    speaker
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

#[test]
fn it_is_remote_and_calls_with_the_hosts_openai_key() {
    assert_eq!(SPEC.provider, Some("openai"));
    assert_eq!(SPEC.accelerators, [Accelerator::Remote]);
}

#[test]
fn a_turn_is_sent_as_a_wav_in_a_form_with_the_model_and_the_language() {
    let model = transcriber();
    let (loaded, provider) = load(&model);
    let mut loaded = loaded.expect("loaded");
    provider.key("openai", "sk-test");
    provider.answer(
        "https://api.openai.com/v1/audio/transcriptions",
        200,
        r#"{"text": " Hola, ¿qué tal? "}"#.as_bytes(),
    );
    let stt = loaded.as_stt().expect("speech to text");
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
    let model = transcriber();
    let (loaded, provider) = load(&model);
    let mut loaded = loaded.expect("loaded: loading makes no call");
    let stt = loaded.as_stt().expect("speech to text");
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
        (429, "rate-limited"),
        (500, "transcription-failed"),
    ] {
        let model = transcriber();
        let (loaded, provider) = load(&model);
        let mut loaded = loaded.expect("loaded");
        provider.key("openai", "sk-test");
        provider.answer("https://api.openai.com/", status, b"{}");
        assert_eq!(code(loaded.as_stt().expect("stt")), expected, "{status}");
    }
}

#[test]
fn speech_is_asked_as_pcm_at_24_khz_with_a_voice_of_the_catalogue() {
    let model = speaker();
    let (loaded, provider) = load(&model);
    let mut loaded = loaded.expect("loaded");
    provider.key("openai", "sk-test");
    provider.answer(
        "https://api.openai.com/v1/audio/speech",
        200,
        &[0x00, 0x40, 0x00, 0xC0],
    );
    let tts = loaded.as_tts().expect("text to speech");
    assert_eq!(tts.voices(), ["alloy", "nova"]);
    assert_eq!(tts.sample_rate(), 24_000);
    let nova = Voice {
        id: "nova".into(),
        languages: Vec::new(),
        gender: None,
    };
    let samples = block_on(tts.speak("Hola", &nova, Some("es"), 1.25));
    assert_eq!(samples, Ok(vec![0.5, -0.5]));

    let request = &provider.requests()[0];
    let body: serde_json::Value = serde_json::from_slice(&request.body).expect("JSON");
    assert_eq!(
        body,
        serde_json::json!({
            "model": "gpt-4o-mini-tts",
            "input": "Hola",
            "voice": "nova",
            "response_format": "pcm",
            "speed": 1.25,
        }),
        "no language: the build maps none"
    );
    let other = Voice {
        id: "ash".into(),
        ..nova
    };
    let refused = block_on(tts.speak("Hola", &other, None, 1.0));
    assert_eq!(refused.map_err(|e| e.code), Err("unknown-voice"));
}

#[test]
fn a_build_without_its_model_or_a_model_of_another_kind_is_unsupported() {
    let mut model = transcriber();
    model.builds[0].api_model = None;
    assert_eq!(
        load(&model).0.map(drop).unwrap_err().code,
        "unsupported-model"
    );
    let detector = ModelEntry {
        capabilities: vec![Capability::Vad],
        ..transcriber()
    };
    assert_eq!(
        load(&detector).0.map(drop).unwrap_err().code,
        "unsupported-model"
    );
}
