//! ElevenLabs: speech to text with Scribe and text to speech on ElevenLabs' API, a remote provider. Nothing runs here:
//! a model is the provider's, called through the host's HTTP with the key the host hands over for each call (the
//! `xi-api-key` header).
//!
//! - Its models: `GET /v1/models` lists the key's models, which text-to-speech ones it has (`can_do_text_to_speech`)
//!   and their languages; the models offered are those its spec describes (`elevenlabs/facts.json`, which
//!   `cargo xtask pin-providers` derives from ElevenLabs' OpenAPI spec). The listing has no speech-to-text flag: the
//!   Scribe models are the spec's, offered once the key lists, with the languages the listing gives them, if any.
//! - Its voices: the account's alone (`GET /v1/voices`: the defaults, and those cloned, designed or added), never the
//!   shared library. A voice's languages are its own `labels.language` first, then each of its verified languages
//!   (`verified_languages`: the locale where it states one), each once; its gender is its `labels.gender` where that
//!   says female or male; its name is its `name`.
//! - Speech to text: `POST /v1/speech-to-text`, a multipart form with the model, the turn as a 16-bit WAV at 16 kHz,
//!   and `tag_audio_events: false` (a transcript is what was said, not "(laughter)"); the language goes in the field
//!   the spec gives the request (`language_code`), as its primary subtag. The transcript is the answer's `text`.
//! - Text to speech: `POST /v1/text-to-speech/{voice}?output_format=pcm_24000`, JSON with the text and the model, and
//!   the speed in `voice_settings` (within the model's range) when it is not 1 and the model takes one; the language
//!   goes in `language_code` for a model whose request the spec gives one (Flash v2.5 and v3 do, Multilingual v2 does
//!   not). It answers 16-bit mono samples at 24 kHz.
//!
//! Streaming is not used (sidevoice-engine#35).

use std::sync::LazyLock;

use async_trait::async_trait;
use serde_json::{json, Value};

use super::api::{self, Form};
use super::facts::{Facts, ModelFacts};
use super::registry::ProviderFactory;
use super::{Adapter, Api, ProviderModel, ProviderSpec};
use crate::backend::{BackendModel, SttModel, TtsModel};
use crate::capability::STT_RATE;
use crate::catalog::{Capability, Gender, Voice};
use crate::host::HttpRequest;
use crate::{Error, Result};

#[cfg(test)]
mod tests;

/// Where ElevenLabs' API is.
const API: &str = "https://api.elevenlabs.io/v1";

/// The output format speech is asked in, and its rate.
const SPEECH_FORMAT: &str = "pcm_24000";
const SPEECH_RATE: u32 = 24_000;

/// What ElevenLabs' spec says that its API does not.
static FACTS: LazyLock<Facts> =
    LazyLock::new(|| Facts::parse(include_str!("elevenlabs/facts.json")));

struct ElevenLabs;

const SPEC: ProviderSpec = ProviderSpec {
    id: "elevenlabs",
    name: "ElevenLabs",
    description:
        "Speech to text with Scribe and text to speech on ElevenLabs' API, with the app's key.",
};

inventory::submit! { ProviderFactory(|| Box::new(ElevenLabs)) }

/// A request with the key.
fn request(method: &'static str, url: String, key: String) -> HttpRequest {
    HttpRequest {
        method,
        url,
        headers: vec![("xi-api-key".into(), key)],
        body: Vec::new(),
    }
}

/// The array `field` of the listing `answer`, `listing-failed` when there is none.
fn array<'a>(answer: &'a Value, field: Option<&str>) -> Result<&'a Vec<Value>> {
    let value = field.map_or(answer, |field| &answer[field]);
    value.as_array().ok_or(Error::new("listing-failed"))
}

/// The strings of `values` at `field`.
fn strings(values: &Value, field: &str) -> Vec<String> {
    values
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|value| value[field].as_str().map(str::to_owned))
        .collect()
}

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl Adapter for ElevenLabs {
    fn spec(&self) -> &ProviderSpec {
        &SPEC
    }

    /// `GET /v1/models`, kept to what the spec describes, in the spec's order.
    async fn models(&self, api: &Api) -> Result<Vec<ProviderModel>> {
        let key = api.key().await?;
        let answer = api
            .list(request("GET", format!("{API}/models"), key))
            .await?;
        let listed = api::listed(&answer)?;
        let listed = array(&listed, None)?;
        let find = |id: &str| listed.iter().find(|model| model["model_id"] == id);
        Ok(FACTS
            .models()
            .filter_map(|(capability, facts)| {
                let found = find(&facts.model);
                let speaks = found.is_some_and(|model| model["can_do_text_to_speech"] == true);
                if capability == Capability::Tts && !speaks {
                    return None;
                }
                Some(ProviderModel {
                    id: facts.model.clone(),
                    capabilities: vec![capability],
                    languages: found
                        .map(|model| strings(&model["languages"], "language_id"))
                        .unwrap_or_default(),
                    voices: Vec::new(),
                    speed: facts.speed,
                })
            })
            .collect())
    }

    /// `GET /v1/voices`: the account's voices.
    async fn voices(&self, api: &Api) -> Result<Vec<Voice>> {
        let key = api.key().await?;
        let answer = api
            .list(request("GET", format!("{API}/voices"), key))
            .await?;
        let listed = api::listed(&answer)?;
        Ok(array(&listed, Some("voices"))?
            .iter()
            .filter_map(voice)
            .collect())
    }

    fn open(&self, api: Api, model: &ProviderModel) -> Result<Box<dyn BackendModel>> {
        let (capability, facts) = FACTS
            .model(&model.id)
            .ok_or(Error::new("unsupported-model"))?;
        let facts = facts.clone();
        Ok(match capability {
            Capability::Stt => Box::new(Scribe { api, facts }),
            _ => Box::new(Speaker {
                api,
                facts,
                voices: model.voices.iter().map(|voice| voice.id.clone()).collect(),
            }),
        })
    }
}

/// A voice of `/v1/voices`, as the engine describes it; `None` without an id.
fn voice(listed: &Value) -> Option<Voice> {
    let id = listed["voice_id"].as_str()?;
    let labels = &listed["labels"];
    let mut languages: Vec<String> = labels["language"]
        .as_str()
        .map(str::to_owned)
        .into_iter()
        .collect();
    for verified in listed["verified_languages"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let tag = verified["locale"]
            .as_str()
            .or(verified["language"].as_str());
        if let Some(tag) = tag {
            if !languages.iter().any(|known| known == tag) {
                languages.push(tag.to_owned());
            }
        }
    }
    let gender = match labels["gender"].as_str() {
        Some("female") => Some(Gender::Female),
        Some("male") => Some(Gender::Male),
        _ => None,
    };
    Some(Voice {
        id: id.to_owned(),
        name: listed["name"].as_str().map(str::to_owned),
        languages,
        gender,
    })
}

/// A Scribe model, and what the spec says of it.
struct Scribe {
    api: Api,
    facts: ModelFacts,
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
        let key = self.api.key().await?;
        let mut form = Form::new()
            .text("model_id", &self.facts.model)
            .file("file", "turn.wav", "audio/wav", &api::wav(pcm, STT_RATE))
            .text("tag_audio_events", "false");
        if let (Some(field), Some(language)) = (&self.facts.language, language) {
            form = form.text(field, &api::primary_subtag(language));
        }
        let (content_type, body) = form.finish();
        let mut request = request("POST", format!("{API}/speech-to-text"), key);
        request.headers.push(("Content-Type".into(), content_type));
        request.body = body;
        let answer = self.api.call(request, "transcription-failed").await?;
        api::text(&answer, "text", "transcription-failed")
    }
}

/// A text-to-speech model of ElevenLabs', what the spec says of it, and the account's voices as listed.
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

    /// Fails with `unknown-voice` for a voice the account does not have (as listed when the model was made),
    /// `speech-failed`, and the remote codes.
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
        let mut body = json!({ "text": text, "model_id": self.facts.model });
        if let Some(speed) = self.facts.speed(speed) {
            if (speed - 1.0).abs() > f32::EPSILON {
                body["voice_settings"] = json!({ "speed": speed });
            }
        }
        if let (Some(field), Some(language)) = (&self.facts.language, language) {
            body[field] = api::primary_subtag(language).into();
        }
        let url = format!(
            "{API}/text-to-speech/{}?output_format={SPEECH_FORMAT}",
            voice.id
        );
        let mut request = request("POST", url, key);
        request
            .headers
            .push(("Content-Type".into(), "application/json".into()));
        request.body = body.to_string().into_bytes();
        let answer = self.api.call(request, "speech-failed").await?;
        Ok(api::pcm16(&answer))
    }
}
