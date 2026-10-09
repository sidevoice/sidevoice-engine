//! What a model can do, whatever runs it: the engine's capability interfaces, [`Stt`], [`Tts`], [`Vad`] (in `vad`) and
//! [`EndOfTurn`] (in `end_of_turn`), with [`Audio`] and resampling (in `audio`). A local model
//! ([`LocalModel`](crate::LocalModel)) and a remote one ([`RemoteModel`](crate::RemoteModel)) both hand them out
//! (`as_stt`, `as_tts`, `as_vad`, `as_end_of_turn`), and a call is the same call, with the same answer, on either:
//! transcribing with Whisper, OpenAI or Scribe is one `transcribe`.
//!
//! Behind them is a model in memory (`Resident`), shared by every handle of it: a local one holds its backend's
//! library, so a local model is unloaded when the last handle of its build (or a `VadStream` of it) is dropped, and the
//! library when the last model of its backend is. Calls on one model are one at a time: a second waits for the first
//! to end; a voice activity stream has a state of its own and runs on its own.

use std::fmt;
use std::sync::Arc;

use async_lock::Mutex;

use crate::backend::{BackendModel, Library};
use crate::catalog::{Capability, Voice};
use crate::{Error, Result};

pub(crate) mod audio;
mod end_of_turn;
mod vad;

pub use audio::Audio;
pub use end_of_turn::EndOfTurn;
pub use vad::{Vad, VadEvent, VadFrame, VadOptions, VadOutput, VadStream};

/// The sample rate every speech-to-text model takes (`SttModel::transcribe`).
pub(crate) const STT_RATE: u32 = 16_000;

/// One model in memory, local or remote. Fields drop in order: the model before the library it was loaded with.
pub(crate) struct Resident {
    model: Mutex<Box<dyn BackendModel>>,
    #[allow(
        dead_code,
        reason = "held: it keeps a local model's library open while the model is in memory"
    )]
    library: Option<Arc<dyn Library>>,
    capabilities: Vec<Capability>,
    /// How many seconds of a turn it hears, for an end-of-turn classifier.
    end_of_turn_seconds: Option<u32>,
    /// The model's languages and the voices its source describes: the catalogue's for a local model, its provider's
    /// listing for a remote one.
    languages: Vec<String>,
    voices: Vec<Voice>,
}

impl fmt::Debug for Resident {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Resident")
            .field("capabilities", &self.capabilities)
            .finish_non_exhaustive()
    }
}

impl Resident {
    /// `model`, with `library` for a local model (which it keeps open; `None` for a remote one); its capabilities are
    /// what it answers to. Shared: every handle of it holds it, and the engine's memory remembers a local one weakly.
    #[cfg_attr(
        web,
        allow(
            clippy::arc_with_non_send_sync,
            reason = "the web build has one thread: the model is shared, never sent"
        )
    )]
    pub(crate) fn new(
        mut model: Box<dyn BackendModel>,
        library: Option<Arc<dyn Library>>,
        languages: Vec<String>,
        voices: Vec<Voice>,
    ) -> Arc<Self> {
        let mut capabilities = Vec::new();
        if model.as_stt().is_some() {
            capabilities.push(Capability::Stt);
        }
        if model.as_tts().is_some() {
            capabilities.push(Capability::Tts);
        }
        if model.as_vad().is_some() {
            capabilities.push(Capability::Vad);
        }
        let end_of_turn_seconds = model.as_end_of_turn().map(|model| model.seconds());
        if end_of_turn_seconds.is_some() {
            capabilities.push(Capability::EndOfTurn);
        }
        Arc::new(Self {
            model: Mutex::new(model),
            library,
            capabilities,
            end_of_turn_seconds,
            languages,
            voices,
        })
    }

    /// What it can do.
    pub(crate) fn capabilities(&self) -> &[Capability] {
        &self.capabilities
    }

    /// The model as speech to text, if it is one.
    pub(crate) fn as_stt(self: &Arc<Self>) -> Option<Stt<'_>> {
        self.capabilities
            .contains(&Capability::Stt)
            .then_some(Stt(self))
    }

    /// The model as text to speech, if it is one.
    pub(crate) fn as_tts(self: &Arc<Self>) -> Option<Tts<'_>> {
        self.capabilities
            .contains(&Capability::Tts)
            .then_some(Tts(self))
    }

    /// The model as a voice activity detector, if it is one.
    pub(crate) fn as_vad(self: &Arc<Self>) -> Option<Vad<'_>> {
        self.capabilities
            .contains(&Capability::Vad)
            .then_some(Vad(self))
    }

    /// The model as an end-of-turn classifier, if it is one.
    pub(crate) fn as_end_of_turn(self: &Arc<Self>) -> Option<EndOfTurn<'_>> {
        self.capabilities
            .contains(&Capability::EndOfTurn)
            .then_some(EndOfTurn(self))
    }

    /// The voice `id` as its source describes it, or, when it does not, with the model's languages and no gender.
    fn voice(&self, id: String) -> Voice {
        self.voices
            .iter()
            .find(|voice| voice.id == id)
            .cloned()
            .unwrap_or_else(|| Voice {
                id,
                name: None,
                languages: self.languages.clone(),
                gender: None,
            })
    }
}

/// A model, local or remote, as speech to text.
#[derive(Debug, Clone, Copy)]
pub struct Stt<'a>(&'a Arc<Resident>);

impl Stt<'_> {
    /// What is said in `audio` (mono samples at `sample_rate` Hz, one whole turn; the engine brings them to the
    /// model's rate), in `language`, a BCP 47 tag, or in the one the model detects with `None`. A model that takes no
    /// language ignores it. It runs on the calling task: the app decides where.
    ///
    /// # Errors
    ///
    /// The backend's `transcription-failed`.
    pub async fn transcribe(
        &self,
        audio: &[f32],
        sample_rate: u32,
        language: Option<&str>,
    ) -> Result<String> {
        let pcm = audio::resample(audio, sample_rate, STT_RATE);
        let mut model = self.0.model.lock().await;
        let stt = model
            .as_stt()
            .ok_or(Error::new("model-cannot-transcribe"))?;
        stt.transcribe(&pcm, language).await
    }
}

/// A model, local or remote, as text to speech.
#[derive(Debug, Clone, Copy)]
pub struct Tts<'a>(&'a Arc<Resident>);

impl Tts<'_> {
    /// The voices it speaks with, each a `voice` for [`Tts::speak`]: as the catalogue declares them, or, for a voice
    /// it does not describe, with the model's languages and no gender.
    pub async fn voices(&self) -> Vec<Voice> {
        let resident = self.0;
        let mut model = resident.model.lock().await;
        let Some(tts) = model.as_tts() else {
            return Vec::new();
        };
        tts.voices()
            .into_iter()
            .map(|id| resident.voice(id))
            .collect()
    }

    /// `text` spoken with `voice` (one of [`Tts::voices`]) at `speed` (1.0, normal, when `None`), at the model's own
    /// sample rate. `language`, a BCP 47 tag, tells a model that speaks several which `text` is in; `None` leaves it to
    /// the model, and a model of one language ignores it. It runs on the calling task: the app decides where.
    ///
    /// # Errors
    ///
    /// The backend's `unknown-voice`, `invalid-text` and `speech-failed`.
    pub async fn speak(
        &self,
        text: &str,
        voice: &str,
        language: Option<&str>,
        speed: Option<f32>,
    ) -> Result<Audio> {
        let mut model = self.0.model.lock().await;
        let tts = model.as_tts().ok_or(Error::new("model-cannot-speak"))?;
        let voice = self.0.voice(voice.to_owned());
        let samples = tts
            .speak(text, &voice, language, speed.unwrap_or(1.0))
            .await?;
        Ok(Audio {
            samples,
            sample_rate: tts.sample_rate(),
        })
    }
}
