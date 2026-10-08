//! Whisper through sherpa-onnx's offline recognizer: one whole turn at a time, in the language asked for or the one it
//! detects.

use async_trait::async_trait;
use sherpa_onnx::{OfflineRecognizer, OfflineRecognizerConfig, OfflineWhisperModelConfig};

use super::{num_threads, path, primary_language, recognizer, text, transcribe, SAMPLE_RATE};
use crate::backend::loaded_model::SttModel;
use crate::backend::BackendModel;
use crate::install::Installed;
use crate::Result;

/// A Whisper model in memory: the recognizer for the language it was made for, and the config to make it again.
pub(super) struct Whisper {
    recognizer: OfflineRecognizer,
    config: OfflineRecognizerConfig,
}

impl Whisper {
    /// Creates the recognizer from the `encoder`, `decoder` and `tokens` in `files`, on `provider`, detecting the
    /// language.
    pub(super) fn load(files: &Installed, provider: &str) -> Result<Self> {
        let mut config = OfflineRecognizerConfig::default();
        config.feat_config.sample_rate = SAMPLE_RATE;
        config.feat_config.feature_dim = 80;
        config.model_config.whisper = OfflineWhisperModelConfig {
            encoder: Some(path(files, "encoder")?),
            decoder: Some(path(files, "decoder")?),
            language: Some(String::new()),
            task: Some("transcribe".to_owned()),
            ..OfflineWhisperModelConfig::default()
        };
        config.model_config.tokens = Some(path(files, "tokens")?);
        config.model_config.provider = Some(provider.to_owned());
        config.model_config.num_threads = num_threads();
        config.decoding_method = Some("greedy_search".to_owned());
        let recognizer = recognizer(&config)?;
        Ok(Self { recognizer, config })
    }

    /// Points it at `language` (a BCP 47 tag, of which Whisper reads the primary language), or at detecting it,
    /// unless it already is. Whisper reads its language from the recognizer's config, which the crate cannot change on
    /// a live recognizer (it has no `set_config`): a new one is made, with the same files, and the old one dropped.
    fn set_language(&mut self, language: Option<&str>) -> Result<()> {
        let code = language.map(primary_language).unwrap_or_default();
        if self.config.model_config.whisper.language.as_deref() == Some(code.as_str()) {
            return Ok(());
        }
        let mut config = self.config.clone();
        config.model_config.whisper.language = Some(text(&code)?);
        self.recognizer = recognizer(&config)?;
        self.config = config;
        Ok(())
    }
}

impl BackendModel for Whisper {
    fn as_stt(&mut self) -> Option<&mut dyn SttModel> {
        Some(self)
    }

    fn memory_mb(&self) -> Option<u32> {
        None
    }
}

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl SttModel for Whisper {
    async fn transcribe(&mut self, pcm: &[f32], language: Option<&str>) -> Result<String> {
        self.set_language(language)?;
        transcribe(&self.recognizer, pcm)
    }
}
