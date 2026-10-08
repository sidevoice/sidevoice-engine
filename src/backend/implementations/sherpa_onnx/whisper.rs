//! Whisper through sherpa-onnx's offline recognizer: one whole turn at a time, in the language asked for or the one it
//! detects.

use async_trait::async_trait;
use sherpa_onnx::{OfflineRecognizer, OfflineRecognizerConfig, OfflineWhisperModelConfig};

use super::{num_threads, path};
use crate::backend::loaded_model::SttModel;
use crate::backend::LoadedModel;
use crate::install::Installed;
use crate::{Error, Result};

/// The sample rate [`SttModel::transcribe`] takes, and Whisper's.
const SAMPLE_RATE: i32 = 16_000;

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
        let recognizer = create(&config)?;
        Ok(Self { recognizer, config })
    }

    /// Points it at `language` (a BCP 47 tag, of which Whisper reads the primary language), or at detecting it,
    /// unless it already is. Whisper reads its language from the recognizer's config, which the crate cannot change on
    /// a live recognizer: a new one is made, with the same files, and the old one dropped.
    fn set_language(&mut self, language: Option<&str>) -> Result<()> {
        let code = language
            .and_then(|tag| tag.split(['-', '_']).next())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if self.config.model_config.whisper.language.as_deref() == Some(code.as_str()) {
            return Ok(());
        }
        let mut config = self.config.clone();
        config.model_config.whisper.language = Some(super::text(&code)?);
        self.recognizer = create(&config)?;
        self.config = config;
        Ok(())
    }
}

/// The recognizer `config` describes, or `model-load-failed`.
fn create(config: &OfflineRecognizerConfig) -> Result<OfflineRecognizer> {
    OfflineRecognizer::create(config).ok_or(Error::new("model-load-failed"))
}

impl LoadedModel for Whisper {
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
    /// Decodes on the calling thread: the engine decides where to run it.
    async fn transcribe(&mut self, pcm: &[f32], language: Option<&str>) -> Result<String> {
        self.set_language(language)?;
        let stream = self.recognizer.create_stream();
        stream.accept_waveform(SAMPLE_RATE, pcm);
        self.recognizer.decode(&stream);
        let result = stream
            .get_result()
            .ok_or(Error::new("transcription-failed"))?;
        Ok(result.text.trim().to_owned())
    }
}
