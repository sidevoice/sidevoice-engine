//! Kokoro through sherpa-onnx's offline TTS: a whole utterance at a time.

use std::path::Path;

use async_trait::async_trait;
use sherpa_onnx::{GenerationConfig, OfflineTts, OfflineTtsConfig};

use super::{model_metadata, text};
use crate::backend::loaded_model::TtsModel;
use crate::backend::LoadedModel;
use crate::{Error, Result};

/// A Kokoro model in memory: the TTS, its sample rate and its voices, by speaker id.
pub(super) struct Kokoro {
    tts: OfflineTts,
    sample_rate: u32,
    voices: Vec<String>,
}

impl Kokoro {
    /// Creates the TTS from `config`, the build's files in it (`config.rs`). The voices are named by the model's
    /// `speaker_names` metadata; a model without it has its speaker ids, `"0"`, `"1"`, ..., as names.
    pub(super) fn load(config: OfflineTtsConfig) -> Result<Self> {
        let model = config
            .model
            .kokoro
            .model
            .clone()
            .ok_or(Error::new("file-not-installed"))?;
        let tts = OfflineTts::create(&config).ok_or(Error::new("model-load-failed"))?;
        let sample_rate =
            u32::try_from(tts.sample_rate()).map_err(|_| Error::new("model-load-failed"))?;
        let speakers = usize::try_from(tts.num_speakers()).unwrap_or_default();
        let named = model_metadata::read(Path::new(&model), "speaker_names")
            .map(|names| names.split(',').map(str::to_owned).collect::<Vec<_>>())
            .filter(|names| names.len() == speakers);
        let voices = named.unwrap_or_else(|| (0..speakers).map(|id| id.to_string()).collect());
        Ok(Self {
            tts,
            sample_rate,
            voices,
        })
    }
}

impl LoadedModel for Kokoro {
    fn as_tts(&mut self) -> Option<&mut dyn TtsModel> {
        Some(self)
    }

    fn memory_mb(&self) -> Option<u32> {
        None
    }
}

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl TtsModel for Kokoro {
    fn voices(&self) -> Vec<String> {
        self.voices.clone()
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Synthesises on the calling thread: the engine decides where to run it. `voice` is one of
    /// [`TtsModel::voices`], or fails with `unknown-voice`.
    async fn speak(&mut self, words: &str, voice: &str, speed: f32) -> Result<Vec<f32>> {
        let failed = Error::new("speech-failed");
        let sid = self
            .voices
            .iter()
            .position(|name| name == voice)
            .ok_or(Error::new("unknown-voice"))?;
        let config = GenerationConfig {
            sid: i32::try_from(sid).map_err(|_| failed)?,
            speed,
            ..GenerationConfig::default()
        };
        let audio = self
            .tts
            .generate_with_config(&text(words)?, &config, None::<fn(&[f32], f32) -> bool>)
            .ok_or(failed)?;
        Ok(audio.samples().to_vec())
    }
}
