//! OpenAI: speech to text and text to speech on OpenAI's API, a remote backend. Nothing runs here: a model is the
//! provider's (`api_model`), called through the host's HTTP with the key the host hands over for each call.
//!
//! - Speech to text: `POST /v1/audio/transcriptions`, a multipart form with the turn as a 16-bit WAV at 16 kHz, the
//!   model and `response_format: json`; the language goes in the field the build's `call_params` name (`language`), as
//!   its primary subtag. The transcript is the answer's `text`.
//! - Text to speech: `POST /v1/audio/speech`, JSON with the model, the text, the voice and the speed, answered as
//!   `pcm`: 16-bit mono samples at 24 kHz, as OpenAI's text-to-speech guide gives that format. The voices are the
//!   catalogue's (OpenAI has no endpoint that lists them); the model speaks the text's language, and takes none.
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

/// Where OpenAI's API is.
const API: &str = "https://api.openai.com/v1";

/// The rate of what `/v1/audio/speech` answers as `pcm`.
const SPEECH_RATE: u32 = 24_000;

struct OpenAi;

const SPEC: BackendSpec = BackendSpec {
    id: "openai",
    name: "OpenAI",
    description: "Speech to text and text to speech on OpenAI's API, with the app's key.",
    upstream: "https://platform.openai.com/docs/api-reference/audio",
    accelerators: &[Accelerator::Remote],
    requirements: &[],
    provider: Some("openai"),
};

inventory::submit! { BackendFactory(|| Box::new(OpenAi)) }

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl Backend for OpenAi {
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
    /// Makes no call. `unsupported-model` for a build with no `api_model`, or a model that is neither speech to text
    /// nor text to speech.
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
            Ok(Box::new(Transcriber {
                provider,
                model,
                language,
            }))
        } else if capabilities.contains(&Capability::Tts) {
            let voices = load.model.voices.iter().map(|voice| voice.id.clone());
            Ok(Box::new(Speaker {
                provider,
                model,
                language,
                voices: voices.collect(),
            }))
        } else {
            Err(Error::new("unsupported-model"))
        }
    }
}

/// A speech-to-text model of OpenAI's: its id there, and the field a call's language goes in.
struct Transcriber {
    provider: Provider,
    model: String,
    language: Option<String>,
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
        let key = self.provider.key().await?;
        let mut form = Form::new()
            .file("file", "turn.wav", "audio/wav", &remote::wav(pcm, STT_RATE))
            .text("model", &self.model)
            .text("response_format", "json");
        if let (Some(field), Some(language)) = (&self.language, language) {
            form = form.text(field, &remote::primary_subtag(language));
        }
        let (content_type, body) = form.finish();
        let request = HttpRequest {
            method: "POST",
            url: format!("{API}/audio/transcriptions"),
            headers: vec![
                ("Authorization".into(), format!("Bearer {key}")),
                ("Content-Type".into(), content_type),
            ],
            body,
        };
        let answer = self.provider.call(request, "transcription-failed").await?;
        remote::text(&answer, "text", "transcription-failed")
    }
}

/// A text-to-speech model of OpenAI's: its id there, the field a call's language goes in (none for OpenAI's), and its
/// voices, from the catalogue.
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

    /// Fails with `unknown-voice` for a voice the catalogue does not list, `speech-failed`, and the remote codes.
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
        let mut body = json!({
            "model": self.model,
            "input": text,
            "voice": voice.id,
            "response_format": "pcm",
            "speed": speed,
        });
        if let (Some(field), Some(language)) = (&self.language, language) {
            body[field] = remote::primary_subtag(language).into();
        }
        let request = HttpRequest {
            method: "POST",
            url: format!("{API}/audio/speech"),
            headers: vec![
                ("Authorization".into(), format!("Bearer {key}")),
                ("Content-Type".into(), "application/json".into()),
            ],
            body: body.to_string().into_bytes(),
        };
        let answer = self.provider.call(request, "speech-failed").await?;
        Ok(remote::pcm16(&answer))
    }
}
