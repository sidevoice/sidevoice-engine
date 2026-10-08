//! What every text-to-speech model here shares on top of sherpa-onnx's offline TTS: its voices named, a voice found by
//! name, and a language told to the model in the generation's extra options. Kokoro, VITS (Piper) and Supertonic
//! differ in the config they make it from, and in how a language reaches them.

use std::collections::HashMap;
use std::path::Path;

use async_trait::async_trait;
use sherpa_onnx::{GenerationConfig, OfflineTts, OfflineTtsConfig};

use super::{model_metadata, num_threads, text};
use crate::backend::loaded_model::TtsModel;
use crate::backend::LoadedModel;
use crate::{Error, Result};

/// What a model is told of the language it speaks, from a BCP 47 tag: the `lang` of the generation's extra options,
/// or `None` to tell it nothing.
pub(super) type Language = fn(&str) -> Option<String>;

/// A TTS model in memory: the TTS, its sample rate, its voices by speaker id, and how it is told a language.
pub(super) struct Synthesizer {
    tts: OfflineTts,
    sample_rate: u32,
    voices: Vec<String>,
    language: Language,
}

impl Synthesizer {
    /// Creates it from `config`, after setting the threads. Its voices are named by the `speaker_names` metadata of
    /// the ONNX model at `named_by`, if any, when it has as many as the model has speakers; otherwise by speaker id,
    /// `"0"`, `"1"`, .... Fails with `model-load-failed`.
    pub(super) fn new(
        mut config: OfflineTtsConfig,
        named_by: Option<&str>,
        language: Language,
    ) -> Result<Self> {
        config.model.num_threads = num_threads();
        let failed = Error::new("model-load-failed");
        let tts = OfflineTts::create(&config).ok_or(failed)?;
        let sample_rate = u32::try_from(tts.sample_rate()).map_err(|_| failed)?;
        let speakers = usize::try_from(tts.num_speakers()).unwrap_or_default();
        let named = named_by
            .and_then(|model| model_metadata::read(Path::new(model), "speaker_names"))
            .map(|names| names.split(',').map(str::to_owned).collect::<Vec<_>>())
            .filter(|names| names.len() == speakers);
        let voices = named.unwrap_or_else(|| (0..speakers).map(|id| id.to_string()).collect());
        Ok(Self {
            tts,
            sample_rate,
            voices,
            language,
        })
    }
}

impl LoadedModel for Synthesizer {
    fn as_tts(&mut self) -> Option<&mut dyn TtsModel> {
        Some(self)
    }

    fn memory_mb(&self) -> Option<u32> {
        None
    }
}

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl TtsModel for Synthesizer {
    fn voices(&self) -> Vec<String> {
        self.voices.clone()
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Synthesises on the calling thread: the engine decides where to run it. `voice` is one of
    /// [`TtsModel::voices`], or fails with `unknown-voice`.
    async fn speak(
        &mut self,
        words: &str,
        voice: &str,
        language: Option<&str>,
        speed: f32,
    ) -> Result<Vec<f32>> {
        let failed = Error::new("speech-failed");
        let sid = self
            .voices
            .iter()
            .position(|name| name == voice)
            .ok_or(Error::new("unknown-voice"))?;
        let extra = language
            .and_then(self.language)
            .map(|lang| HashMap::from([("lang".to_owned(), serde_json::Value::String(lang))]));
        let config = GenerationConfig {
            sid: i32::try_from(sid).map_err(|_| failed)?,
            speed,
            extra,
            ..GenerationConfig::default()
        };
        let audio = self
            .tts
            .generate_with_config(&text(words)?, &config, None::<fn(&[f32], f32) -> bool>)
            .ok_or(failed)?;
        Ok(audio.samples().to_vec())
    }
}
