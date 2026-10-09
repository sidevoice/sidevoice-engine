//! ElevenLabs: speech to text with Scribe and text to speech on ElevenLabs' API, a remote backend. Nothing runs here:
//! a model is the provider's (`api_model`), called through the host's HTTP with the key the host hands over for each
//! call (the `xi-api-key` header).
//!
//! - Speech to text: `POST /v1/speech-to-text`, a multipart form with the model, the turn as a 16-bit WAV at 16 kHz,
//!   and `tag_audio_events: false` (a transcript is what was said, not "(laughter)"); the language goes in the field
//!   the build's `call_params` name (`language_code`), as its primary subtag. The transcript is the answer's `text`.
//! - Text to speech: `POST /v1/text-to-speech/{voice}?output_format=pcm_24000`, JSON with the text and the model, and
//!   the speed in `voice_settings` when it is not 1; the language goes in the field the build names, for a model that
//!   takes one (Flash v2.5 does, Multilingual v2 refuses it). It answers 16-bit mono samples at 24 kHz. The voices are
//!   the account's: loading lists them (`GET /v1/voices`), the one call `load` makes.
//!
//! What a model is follows from its catalogue capability. Streaming is not used (sidevoice-engine#35).

use async_trait::async_trait;
use serde_json::json;

use crate::backend::loaded_model::{SttModel, TtsModel};
use crate::backend::registry::BackendFactory;
use crate::backend::remote::{self, Form, Provider, STT_RATE};
use crate::backend::{Backend, BackendModel, BackendSpec, Library, Load};
use crate::catalog::{Capability, Voice};
use crate::host::{Accelerator, HttpRequest};
use crate::install::Installed;
use crate::{Error, Result};

#[cfg(test)]
mod tests;

/// Where ElevenLabs' API is.
const API: &str = "https://api.elevenlabs.io/v1";

/// The output format speech is asked in, and its rate.
const SPEECH_FORMAT: &str = "pcm_24000";
const SPEECH_RATE: u32 = 24_000;

struct ElevenLabs;

const SPEC: BackendSpec = BackendSpec {
    id: "elevenlabs",
    name: "ElevenLabs",
    description:
        "Speech to text with Scribe and text to speech on ElevenLabs' API, with the app's key.",
    upstream: "https://elevenlabs.io/docs/api-reference",
    accelerators: &[Accelerator::Remote],
    requirements: &[],
    provider: Some("elevenlabs"),
};

inventory::submit! { BackendFactory(|| Box::new(ElevenLabs)) }

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl Backend for ElevenLabs {
    fn spec(&self) -> &BackendSpec {
        &SPEC
    }

    /// The API is the library: there is nothing to open.
    async fn open(&self, _files: &Installed) -> Result<Box<dyn Library>> {
        Ok(Box::new(Api))
    }
}

struct Api;

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl Library for Api {
    /// A text-to-speech model lists the account's voices; a speech-to-text one makes no call. `unsupported-model` for a
    /// build with no `api_model`, or a model that is neither; `model-load-failed` when the voices cannot be read, and
    /// the remote codes.
    async fn load(&self, load: Load<'_>) -> Result<Box<dyn BackendModel>> {
        let model = load
            .build
            .api_model
            .clone()
            .ok_or(Error::new("unsupported-model"))?;
        let provider = Provider::new(load.host, SPEC.id);
        let language = remote::field(load.build, "language").map(str::to_owned);
        let capabilities = &load.model.capabilities;
        if capabilities.contains(&Capability::Stt) {
            Ok(Box::new(Scribe {
                provider,
                model,
                language,
            }))
        } else if capabilities.contains(&Capability::Tts) {
            let voices = voices(&provider).await?;
            Ok(Box::new(Speaker {
                provider,
                model,
                language,
                voices,
            }))
        } else {
            Err(Error::new("unsupported-model"))
        }
    }
}

/// The ids of the account's voices.
async fn voices(provider: &Provider) -> Result<Vec<String>> {
    let key = provider.key().await?;
    let request = HttpRequest {
        method: "GET",
        url: format!("{API}/voices"),
        headers: vec![("xi-api-key".into(), key)],
        body: Vec::new(),
    };
    let answer = provider.call(request, "model-load-failed").await?;
    let listed: serde_json::Value =
        serde_json::from_slice(&answer).map_err(|_| Error::new("model-load-failed"))?;
    let voices = listed["voices"]
        .as_array()
        .ok_or(Error::new("model-load-failed"))?;
    Ok(voices
        .iter()
        .filter_map(|voice| voice["voice_id"].as_str().map(str::to_owned))
        .collect())
}

/// A Scribe model: its id there, and the field a call's language goes in.
struct Scribe {
    provider: Provider,
    model: String,
    language: Option<String>,
}

impl BackendModel for Scribe {
    fn as_stt(&mut self) -> Option<&mut dyn SttModel> {
        Some(self)
    }

    fn memory_mb(&self) -> Option<u32> {
        Some(0)
    }
}

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl SttModel for Scribe {
    /// Fails with `transcription-failed`, and the remote codes (`credential-missing`, ...).
    async fn transcribe(&mut self, pcm: &[f32], language: Option<&str>) -> Result<String> {
        let key = self.provider.key().await?;
        let mut form = Form::new()
            .text("model_id", &self.model)
            .file("file", "turn.wav", "audio/wav", &remote::wav(pcm, STT_RATE))
            .text("tag_audio_events", "false");
        if let (Some(field), Some(language)) = (&self.language, language) {
            form = form.text(field, &remote::primary_subtag(language));
        }
        let (content_type, body) = form.finish();
        let request = HttpRequest {
            method: "POST",
            url: format!("{API}/speech-to-text"),
            headers: vec![
                ("xi-api-key".into(), key),
                ("Content-Type".into(), content_type),
            ],
            body,
        };
        let answer = self.provider.call(request, "transcription-failed").await?;
        remote::text(&answer, "text", "transcription-failed")
    }
}

/// A text-to-speech model of ElevenLabs': its id there, the field a call's language goes in, and the account's voices.
struct Speaker {
    provider: Provider,
    model: String,
    language: Option<String>,
    voices: Vec<String>,
}

impl BackendModel for Speaker {
    fn as_tts(&mut self) -> Option<&mut dyn TtsModel> {
        Some(self)
    }

    fn memory_mb(&self) -> Option<u32> {
        Some(0)
    }
}

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl TtsModel for Speaker {
    fn voices(&self) -> Vec<String> {
        self.voices.clone()
    }

    fn sample_rate(&self) -> u32 {
        SPEECH_RATE
    }

    /// Fails with `unknown-voice` for a voice the account does not have (as listed when loaded), `speech-failed`, and
    /// the remote codes.
    async fn speak(
        &mut self,
        text: &str,
        voice: &Voice,
        language: Option<&str>,
        speed: f32,
    ) -> Result<Vec<f32>> {
        if !self.voices.contains(&voice.id) {
            return Err(Error::new("unknown-voice"));
        }
        let key = self.provider.key().await?;
        let mut body = json!({ "text": text, "model_id": self.model });
        if (speed - 1.0).abs() > f32::EPSILON {
            body["voice_settings"] = json!({ "speed": speed });
        }
        if let (Some(field), Some(language)) = (&self.language, language) {
            body[field] = remote::primary_subtag(language).into();
        }
        let request = HttpRequest {
            method: "POST",
            url: format!(
                "{API}/text-to-speech/{}?output_format={SPEECH_FORMAT}",
                voice.id
            ),
            headers: vec![
                ("xi-api-key".into(), key),
                ("Content-Type".into(), "application/json".into()),
            ],
            body: body.to_string().into_bytes(),
        };
        let answer = self.provider.call(request, "speech-failed").await?;
        Ok(remote::pcm16(&answer))
    }
}
