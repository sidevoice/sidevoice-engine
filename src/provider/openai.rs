//! OpenAI: speech to text and text to speech on OpenAI's API, a remote provider. Nothing runs here: a model is the
//! provider's, called through the host's HTTP with the key the host hands over for each call.
//!
//! - Its models: those `GET /v1/models` lists for the key that its spec describes (`openai/facts.json`, which
//!   `cargo xtask pin-providers` derives from OpenAI's OpenAPI spec): the API lists ids only, and the spec says which
//!   are speech to text and which text to speech. OpenAI publishes no languages for them: they have none here. Its
//!   voices are the spec's (OpenAI has no endpoint that lists them), each with no languages and no gender.
//! - Speech to text: `POST /v1/audio/transcriptions`, a multipart form with the turn as a 16-bit WAV at 16 kHz, the
//!   model and `response_format: json`; the language goes in the field the spec gives its request (`language`), as its
//!   primary subtag. The transcript is the answer's `text`.
//! - Text to speech: `POST /v1/audio/speech`, JSON with the model, the text, the voice and the speed (within the spec's
//!   range), answered as `pcm`: 16-bit mono samples at 24 kHz, as OpenAI's text-to-speech guide gives that format. The
//!   model speaks the text's language, and its request takes none.
//!
//! Streaming is not used (sidevoice-engine#35).

use std::sync::LazyLock;

use async_trait::async_trait;
use serde_json::json;

use super::api::{self, Form};
use super::facts::{Facts, ModelFacts};
use super::registry::ProviderFactory;
use super::{Adapter, Api, ProviderModel, ProviderSpec};
use crate::backend::{BackendModel, SttModel, TtsModel};
use crate::capability::STT_RATE;
use crate::catalog::{Capability, Voice};
use crate::host::HttpRequest;
use crate::{Error, Result};

#[cfg(test)]
mod tests;

/// Where OpenAI's API is.
const API: &str = "https://api.openai.com/v1";

/// The rate of what `/v1/audio/speech` answers as `pcm`.
const SPEECH_RATE: u32 = 24_000;

/// What OpenAI's spec says that its API does not.
static FACTS: LazyLock<Facts> = LazyLock::new(|| Facts::parse(include_str!("openai/facts.json")));

struct OpenAi;

const SPEC: ProviderSpec = ProviderSpec {
    id: "openai",
    name: "OpenAI",
    description: "Speech to text and text to speech on OpenAI's API, with the app's key.",
};

inventory::submit! { ProviderFactory(|| Box::new(OpenAi)) }

/// A request with the key.
fn request(method: &'static str, url: String, key: &str) -> HttpRequest {
    HttpRequest {
        method,
        url,
        headers: vec![("Authorization".into(), format!("Bearer {key}"))],
        body: Vec::new(),
    }
}

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl Adapter for OpenAi {
    fn spec(&self) -> &ProviderSpec {
        &SPEC
    }

    /// `GET /v1/models`, kept to the ids the spec describes, in the spec's order.
    async fn models(&self, api: &Api) -> Result<Vec<ProviderModel>> {
        let key = api.key().await?;
        let answer = api
            .list(request("GET", format!("{API}/models"), &key))
            .await?;
        let listed = api::listed(&answer)?;
        let ids: Vec<&str> = listed["data"]
            .as_array()
            .ok_or(Error::new("listing-failed"))?
            .iter()
            .filter_map(|model| model["id"].as_str())
            .collect();
        Ok(FACTS
            .models()
            .filter(|(_, facts)| ids.contains(&facts.model.as_str()))
            .map(|(capability, facts)| ProviderModel {
                id: facts.model.clone(),
                capabilities: vec![capability],
                languages: Vec::new(),
                voices: Vec::new(),
                speed: facts.speed,
            })
            .collect())
    }

    /// The spec's voices: no call.
    async fn voices(&self, _api: &Api) -> Result<Vec<Voice>> {
        Ok(FACTS
            .voices
            .iter()
            .map(|id| Voice {
                id: id.clone(),
                name: None,
                languages: Vec::new(),
                gender: None,
            })
            .collect())
    }

    fn open(&self, api: Api, model: &ProviderModel) -> Result<Box<dyn BackendModel>> {
        let (capability, facts) = FACTS
            .model(&model.id)
            .ok_or(Error::new("unsupported-model"))?;
        let facts = facts.clone();
        Ok(match capability {
            Capability::Stt => Box::new(Transcriber { api, facts }),
            _ => Box::new(Speaker {
                api,
                facts,
                voices: model.voices.iter().map(|voice| voice.id.clone()).collect(),
            }),
        })
    }
}

/// A speech-to-text model of OpenAI's, and what its spec says of it.
struct Transcriber {
    api: Api,
    facts: ModelFacts,
}

impl BackendModel for Transcriber {
    fn as_stt(&mut self) -> Option<&mut dyn SttModel> {
        Some(self)
    }

    fn memory_mb(&self) -> Option<u32> {
        Some(0)
    }
}

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl SttModel for Transcriber {
    /// Fails with `transcription-failed`, and the remote codes (`credential-missing`, ...).
    async fn transcribe(&mut self, pcm: &[f32], language: Option<&str>) -> Result<String> {
        let key = self.api.key().await?;
        let mut form = Form::new()
            .file("file", "turn.wav", "audio/wav", &api::wav(pcm, STT_RATE))
            .text("model", &self.facts.model)
            .text("response_format", "json");
        if let (Some(field), Some(language)) = (&self.facts.language, language) {
            form = form.text(field, &api::primary_subtag(language));
        }
        let (content_type, body) = form.finish();
        let mut request = request("POST", format!("{API}/audio/transcriptions"), &key);
        request.headers.push(("Content-Type".into(), content_type));
        request.body = body;
        let answer = self.api.call(request, "transcription-failed").await?;
        api::text(&answer, "text", "transcription-failed")
    }
}

/// A text-to-speech model of OpenAI's, what its spec says of it, and its voices.
struct Speaker {
    api: Api,
    facts: ModelFacts,
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

    /// Fails with `unknown-voice` for a voice the spec does not list, `speech-failed`, and the remote codes.
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
        let key = self.api.key().await?;
        let mut body = json!({
            "model": self.facts.model,
            "input": text,
            "voice": voice.id,
            "response_format": "pcm",
        });
        if let Some(speed) = self.facts.speed(speed) {
            body["speed"] = speed.into();
        }
        if let (Some(field), Some(language)) = (&self.facts.language, language) {
            body[field] = api::primary_subtag(language).into();
        }
        let mut request = request("POST", format!("{API}/audio/speech"), &key);
        request
            .headers
            .push(("Content-Type".into(), "application/json".into()));
        request.body = body.to_string().into_bytes();
        let answer = self.api.call(request, "speech-failed").await?;
        Ok(api::pcm16(&answer))
    }
}
