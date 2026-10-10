//! ElevenLabs: speech to text with Scribe and text to speech on ElevenLabs' API, a remote provider. Nothing runs here:
//! a model is the provider's, called through the host's HTTP with the key the host hands over for each call (the
//! `xi-api-key` header).
//!
//! - Its models: the listing decides for what it reports, and the spec for what it never does. `GET /v1/models` says
//!   which models speak (`can_do_text_to_speech`): each is offered as text to speech, and no other. It says nothing of
//!   speech to text, and does not list Scribe: the speech-to-text models are the spec's, each offered once the key
//!   lists, with the languages the listing gives it if it lists it.
//! - Its facts, from ElevenLabs' OpenAPI spec (`api.elevenlabs.io/openapi.json`), read at run time, only enrich them. A
//!   text-to-speech model with a generation request of its own in the spec (a schema with `text`, `voice` and a `const`
//!   `model_id`: `ElevenFlashV2_5Request`, ...) takes the `language_code` field if that request has one, and the speed
//!   range of its `voice_settings`; any other follows the general request of `/v1/text-to-speech/{voice}`, which takes
//!   `language_code`, and a speed only where its `voice_settings` give a range. The speech-to-text models are the ids
//!   the speech-to-text request gives as examples (the spec enumerates none), with its `language_code` field. And the
//!   speech path must still offer `pcm_24000`.
//! - Its voices: the account's alone (`GET /v1/voices`: the defaults, and those cloned, designed or added), never the
//!   shared library. A voice's languages are its own `labels.language` first, then each of its verified languages
//!   (`verified_languages`: the locale where it states one), each once; its gender is its `labels.gender` where that
//!   says female or male; its name is its `name`.
//! - Speech to text: `POST /v1/speech-to-text`, a multipart form with the model, the turn as a 16-bit WAV at 16 kHz,
//!   and `tag_audio_events: false` (a transcript is what was said, not "(laughter)"); the language goes in the field
//!   the spec gives the request (`language_code`), as its primary subtag. The transcript is the answer's `text`.
//! - Text to speech: `POST /v1/text-to-speech/{voice}?output_format=pcm_24000`, JSON with the text and the model, and
//!   the speed in `voice_settings` (within the model's range) when it is not 1 and the model takes one; the language
//!   goes in `language_code` where the model's request takes it (Flash v2.5 and v3 do, Multilingual v2 does not). It
//!   answers 16-bit mono samples at 24 kHz.
//!
//! Streaming is not used (sidevoice-engine#35).

use async_trait::async_trait;
use serde_json::{json, Value};

use super::api::{self, Form};
use super::facts::{self, Facts, ModelFacts};
use super::registry::ProviderFactory;
use super::{Adapter, Api, ProviderSpec, RemoteModelInfo};
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

struct ElevenLabs;

const SPEC: ProviderSpec = ProviderSpec {
    id: "elevenlabs",
    name: "ElevenLabs",
    spec: "https://api.elevenlabs.io/openapi.json",
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

    fn facts(&self, spec: &Value) -> Result<Facts> {
        let path = &spec["paths"]["/v1/text-to-speech/{voice_id}"]["post"];
        let format = path["parameters"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|parameter| parameter["name"] == "output_format")
            .map(|parameter| facts::strings(spec, &parameter["schema"]))
            .unwrap_or_default();
        if !format.iter().any(|format| format == SPEECH_FORMAT) {
            return Err(facts::unreadable());
        }
        let schemas = spec["components"]["schemas"]
            .as_object()
            .ok_or_else(facts::unreadable)?;
        let mut speakers = Vec::new();
        for request in schemas.values() {
            let properties = &request["properties"];
            let (Some(id), true, true) = (
                properties["model_id"]["const"].as_str(),
                properties.get("text").is_some(),
                properties.get("voice").is_some(),
            ) else {
                continue;
            };
            let language = properties.get("language_code").map(|_| "language_code");
            let settings = facts::object(spec, &properties["voice_settings"]);
            let speed = facts::range(spec, &settings["properties"]["speed"]);
            speakers.push(ModelFacts::new(id, language, speed));
        }
        speakers.sort_by(|a, b| a.model.cmp(&b.model));
        // The general request, for a model the spec has no request of its own for.
        let general = path["requestBody"]["content"]
            .as_object()
            .and_then(|content| content.values().next())
            .map(|content| facts::object(spec, &content["schema"]))
            .ok_or_else(facts::unreadable)?;
        let general_language = general["properties"]
            .get("language_code")
            .map(|_| "language_code");
        let general_settings = facts::object(spec, &general["properties"]["voice_settings"]);
        let general_speed = facts::range(spec, &general_settings["properties"]["speed"]);
        let body = &spec["paths"]["/v1/speech-to-text"]["post"]["requestBody"]["content"];
        let body = body
            .as_object()
            .and_then(|content| content.values().next())
            .map(|content| facts::resolve(spec, &content["schema"]))
            .ok_or_else(facts::unreadable)?;
        let language = body["properties"]
            .get("language_code")
            .map(|_| "language_code");
        let scribes: Vec<_> = facts::strings(spec, &body["properties"]["model_id"]["examples"])
            .iter()
            .map(|id| ModelFacts::new(id, language, None))
            .collect();
        if scribes.is_empty() {
            return Err(facts::unreadable());
        }
        Ok(Facts {
            speech_to_text: scribes,
            text_to_speech: speakers,
            transcription: ModelFacts::new("", language, None),
            speech: ModelFacts::new("", general_language, general_speed),
            voices: Vec::new(),
        })
    }

    /// `GET /v1/models`, in its order, for text to speech: every model it says speaks (`can_do_text_to_speech`). Then,
    /// for speech to text, which the listing never reports, the spec's models. Each with the languages the listing gives
    /// it, and the speed range its facts, or the general request's, give.
    async fn models(&self, api: &Api, facts: &Facts) -> Result<Vec<RemoteModelInfo>> {
        let key = api.key().await?;
        let answer = api
            .list(request("GET", format!("{API}/models"), key))
            .await?;
        let listed = api::listed(&answer)?;
        let listed = array(&listed, None)?;
        let languages = |id: &str| {
            listed
                .iter()
                .find(|model| model["model_id"] == id)
                .map(|model| strings(&model["languages"], "language_id"))
                .unwrap_or_default()
        };
        let model = |capability, id: &str| RemoteModelInfo {
            id: id.to_owned(),
            capabilities: vec![capability],
            languages: languages(id),
            voices: Vec::new(),
            speed: facts.of(capability, id).range(),
        };
        let speakers = listed
            .iter()
            .filter(|listed| listed["can_do_text_to_speech"] == true)
            .filter_map(|listed| listed["model_id"].as_str())
            .map(|id| model(Capability::Tts, id));
        let scribes = facts
            .speech_to_text
            .iter()
            .map(|facts| model(Capability::Stt, &facts.model));
        Ok(speakers.chain(scribes).collect())
    }

    /// `GET /v1/voices`: the account's voices.
    async fn voices(&self, api: &Api, _facts: &Facts) -> Result<Vec<Voice>> {
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

    fn open(
        &self,
        api: Api,
        model: &RemoteModelInfo,
        facts: &Facts,
    ) -> Result<Box<dyn BackendModel>> {
        let capability = *model
            .capabilities
            .first()
            .ok_or(Error::new("unsupported-model"))?;
        let facts = facts.of(capability, &model.id);
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
