//! A model in memory, as the app holds it: [`LoadedModel`], and what it can do, [`Stt`], [`Tts`], [`Vad`] (in `vad`)
//! and [`EndOfTurn`] (in `end_of_turn`). Every `LoadedModel` of one build shares one model in memory (`Resident`), and
//! that model holds its backend's library: so the model is unloaded when the last `LoadedModel` of its build (or a
//! `VadStream` of it) is dropped, and the library when the last model of its backend is. Calls on one model are one at
//! a time: a second waits for the first to end; a voice activity stream has a state of its own and runs on its own.

use std::sync::Arc;

use async_lock::Mutex;

use super::audio::{self, Audio};
use crate::backend::{BackendModel, Library};
use crate::catalog::{Capability, Voice};
use crate::{Error, Result};

mod end_of_turn;
mod vad;

pub use end_of_turn::EndOfTurn;
pub use vad::{Vad, VadEvent, VadFrame, VadOptions, VadOutput, VadStream};

/// The sample rate every speech-to-text backend takes (`SttModel::transcribe`).
const STT_RATE: u32 = 16_000;

/// One build's model in memory. Fields drop in order: the model before the library it was loaded with.
pub(super) struct Resident {
    model: Mutex<Box<dyn BackendModel>>,
    #[allow(
        dead_code,
        reason = "held: it keeps the library open while the model is in memory"
    )]
    library: Arc<dyn Library>,
    capabilities: Vec<Capability>,
    /// How many seconds of a turn it hears, for an end-of-turn classifier.
    end_of_turn_seconds: Option<u32>,
    /// The model's languages and declared voices, from the catalogue: what describes the voices the backend has.
    languages: Vec<String>,
    voices: Vec<Voice>,
}

impl Resident {
    /// `model`, loaded with `library`, which keeps the library open; its capabilities are what it answers to. Shared:
    /// every [`LoadedModel`] of it holds it, and memory remembers it weakly.
    #[cfg_attr(
        web,
        allow(
            clippy::arc_with_non_send_sync,
            reason = "the web build has one thread: the model is shared, never sent"
        )
    )]
    pub(super) fn new(
        mut model: Box<dyn BackendModel>,
        library: Arc<dyn Library>,
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

    /// The voice `id` as the catalogue declares it, or, when it does not describe it, with the model's languages and
    /// no gender.
    fn voice(&self, id: String) -> Voice {
        self.voices
            .iter()
            .find(|voice| voice.id == id)
            .cloned()
            .unwrap_or_else(|| Voice {
                id,
                languages: self.languages.clone(),
                gender: None,
            })
    }
}

/// A model loaded by [`Engine::load`](crate::Engine::load). It stays in memory while any `LoadedModel` of its build
/// lives: dropping the last one unloads it. Calls on it wait for one another.
#[derive(Clone)]
pub struct LoadedModel {
    id: String,
    build: String,
    resident: Arc<Resident>,
}

impl std::fmt::Debug for LoadedModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoadedModel")
            .field("id", &self.id)
            .field("build", &self.build)
            .finish_non_exhaustive()
    }
}

impl LoadedModel {
    pub(super) fn new(id: &str, build: &str, resident: Arc<Resident>) -> Self {
        Self {
            id: id.to_owned(),
            build: build.to_owned(),
            resident,
        }
    }

    /// The model's id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The id of the build that was loaded.
    #[must_use]
    pub fn build(&self) -> &str {
        &self.build
    }

    /// What it can do.
    #[must_use]
    pub fn capabilities(&self) -> &[Capability] {
        &self.resident.capabilities
    }

    /// The model as speech to text, if it is one.
    #[must_use]
    pub fn as_stt(&self) -> Option<Stt<'_>> {
        self.capabilities()
            .contains(&Capability::Stt)
            .then_some(Stt(self))
    }

    /// The model as text to speech, if it is one.
    #[must_use]
    pub fn as_tts(&self) -> Option<Tts<'_>> {
        self.capabilities()
            .contains(&Capability::Tts)
            .then_some(Tts(self))
    }

    /// The model as a voice activity detector, if it is one.
    #[must_use]
    pub fn as_vad(&self) -> Option<Vad<'_>> {
        self.capabilities()
            .contains(&Capability::Vad)
            .then_some(Vad(self))
    }

    /// The model as an end-of-turn classifier, if it is one.
    #[must_use]
    pub fn as_end_of_turn(&self) -> Option<EndOfTurn<'_>> {
        self.capabilities()
            .contains(&Capability::EndOfTurn)
            .then_some(EndOfTurn(self))
    }
}

/// A loaded model, as speech to text.
#[derive(Debug, Clone, Copy)]
pub struct Stt<'a>(&'a LoadedModel);

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
        let mut model = self.0.resident.model.lock().await;
        let stt = model
            .as_stt()
            .ok_or(Error::new("model-cannot-transcribe"))?;
        stt.transcribe(&pcm, language).await
    }
}

/// A loaded model, as text to speech.
#[derive(Debug, Clone, Copy)]
pub struct Tts<'a>(&'a LoadedModel);

impl Tts<'_> {
    /// The voices it speaks with, each a `voice` for [`Tts::speak`]: as the catalogue declares them, or, for a voice
    /// it does not describe, with the model's languages and no gender.
    pub async fn voices(&self) -> Vec<Voice> {
        let resident = &self.0.resident;
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
        let mut model = self.0.resident.model.lock().await;
        let tts = model.as_tts().ok_or(Error::new("model-cannot-speak"))?;
        let voice = self.0.resident.voice(voice.to_owned());
        let samples = tts
            .speak(text, &voice, language, speed.unwrap_or(1.0))
            .await?;
        Ok(Audio {
            samples,
            sample_rate: tts.sample_rate(),
        })
    }
}
