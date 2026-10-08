//! Transducers through sherpa-onnx's offline recognizer: the FastConformer and Parakeet models NeMo exports (sherpa-onnx
//! reads which from the model's metadata, and its feature size too). They take no language: a multilingual one hears
//! whichever is spoken.

use async_trait::async_trait;
use sherpa_onnx::{OfflineRecognizer, OfflineRecognizerConfig, OfflineTransducerModelConfig};

use super::{num_threads, path, recognizer, transcribe, SAMPLE_RATE};
use crate::backend::loaded_model::SttModel;
use crate::backend::BackendModel;
use crate::install::Installed;
use crate::Result;

/// A transducer in memory.
pub(super) struct Transducer(OfflineRecognizer);

impl Transducer {
    /// Creates the recognizer from the `encoder`, `decoder`, `joiner` and `tokens` in `files`, on `provider`.
    pub(super) fn load(files: &Installed, provider: &str) -> Result<Self> {
        let mut config = OfflineRecognizerConfig::default();
        config.feat_config.sample_rate = SAMPLE_RATE;
        config.model_config.transducer = OfflineTransducerModelConfig {
            encoder: Some(path(files, "encoder")?),
            decoder: Some(path(files, "decoder")?),
            joiner: Some(path(files, "joiner")?),
        };
        config.model_config.tokens = Some(path(files, "tokens")?);
        config.model_config.provider = Some(provider.to_owned());
        config.model_config.num_threads = num_threads();
        config.decoding_method = Some("greedy_search".to_owned());
        Ok(Self(recognizer(&config)?))
    }
}

impl BackendModel for Transducer {
    fn as_stt(&mut self) -> Option<&mut dyn SttModel> {
        Some(self)
    }

    fn memory_mb(&self) -> Option<u32> {
        None
    }
}

#[cfg_attr(native, async_trait)]
#[cfg_attr(web, async_trait(?Send))]
impl SttModel for Transducer {
    /// `language` is not asked of the model: it is not one that takes it.
    async fn transcribe(&mut self, pcm: &[f32], _language: Option<&str>) -> Result<String> {
        transcribe(&self.0, pcm)
    }
}
